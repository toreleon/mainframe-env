//! Fixture consumer: emits originals, does not implement queue semantics.
use super::*;
use mainframe_env_diagnostics::{
    DiagnosticCode, DiagnosticLimits, ExecutionProblem, FailureCategory, Phase,
};
use mainframe_env_host_api::mq_md_value::*;
use mainframe_env_host_api::mq_object_route::*;

pub(super) struct Consumer {
    port: Arc<Port>,
    step: usize,
    connection: Option<MqHconn>,
    output: Option<MqHobj>,
    input: Option<MqHobj>,
}
impl Consumer {
    pub(super) fn new(port: Arc<Port>) -> Self {
        Self {
            port,
            step: 0,
            connection: None,
            output: None,
            input: None,
        }
    }
    fn next(
        &mut self,
        resume: MachineResume<EffectResult>,
    ) -> Result<MachineDrive<EffectRequest>, String> {
        if let MachineResume::HostResult(r) = resume {
            let r = match r.outcome {
                Ok(HostResult::MqMqi(r)) => r,
                other => return Err(format!("host result missing at {}: {other:?}", self.step)),
            };
            let output = match r.result.outcome {
                MqMqiOutcome::Completed { output, .. }
                | MqMqiOutcome::ReviewedOutput { output, .. }
                | MqMqiOutcome::StatusPending { output } => output,
                _ => return Err("host uncertainty".into()),
            };
            match output {
                MqMqiOutput::Connected(c) => self.connection = Some(c),
                MqMqiOutput::Opened { object, .. } if self.step == 2 => self.output = Some(object),
                MqMqiOutput::Opened { object, .. } if self.step == 3 => self.input = Some(object),
                _ => {}
            }
        }
        if self.step == 15 {
            return Ok(MachineDrive::Completed(Completion {
                return_code: 0,
                output: BoundedPayload::new(
                    "mq-ir-completion@1",
                    b"complete".to_vec(),
                    Default::default(),
                )
                .unwrap(),
            }));
        }
        if self.step == 0 {
            let root = self
                .port
                .runtime
                .admit_root(self.port.invocation.clone())
                .map_err(|e| e.to_string())?;
            *self.port.frame.lock().unwrap() = Some(root.frame());
        }
        let guard = self.port.frame.lock().unwrap();
        let frame = guard.as_ref().ok_or("frame")?;
        let c = self.connection;
        let current = || frame.current_unit(c.unwrap()).map_err(|e| e.to_string());
        let request = match self.step {
            0 => MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
            1 | 2 => MqMqiRequest::Open(
                MqObjectOpenRequest::new(
                    c.unwrap(),
                    lookup(),
                    &[if self.step == 1 {
                        MqRouteOpenAccess::Output
                    } else {
                        MqRouteOpenAccess::InputShared
                    }],
                    Default::default(),
                )
                .unwrap(),
            ),
            3 | 9 => MqMqiRequest::FullPut {
                connection: c.unwrap(),
                object: self.output.unwrap(),
                put: MqMqiFullPut {
                    message: MqFullMessage {
                        descriptor: descriptor([7; 24]),
                        body: b"HELLO".to_vec(),
                        properties: Vec::new(),
                    },
                    message_handle: None,
                    context: MqMqiMessageContext::NoContext,
                    options: MqMqiOptions::PutV1Synchronous,
                    unit: if self.step == 3 {
                        current()?
                    } else {
                        MqMqiUnitOfWork::NoSyncpoint
                    },
                },
            },
            4 | 6 | 8 | 10 | 11 => MqMqiRequest::QualifiedFullGet(MqMqiFullGet {
                connection: c.unwrap(),
                object: self.input.unwrap(),
                descriptor: descriptor([0; 24]),
                mode: MqGetMode::Remove,
                wait: MqWait::NoWait,
                truncation: if self.step == 11 {
                    MqTruncation::Accept
                } else {
                    MqTruncation::Reject
                },
                buffer_capacity: if self.step >= 10 { 2 } else { 64 },
                message_handle: None,
                options: MqMqiOptions::ContractDefault,
                unit: if self.step == 6 {
                    current()?
                } else {
                    MqMqiUnitOfWork::NoSyncpoint
                },
            }),
            5 | 7 => {
                let MqMqiUnitOfWork::Local { unit } = current()? else {
                    return Err("local unit".into());
                };
                if self.step == 5 {
                    MqMqiRequest::Commit {
                        connection: c.unwrap(),
                        unit,
                    }
                } else {
                    MqMqiRequest::Back {
                        connection: c.unwrap(),
                        unit,
                    }
                }
            }
            12 | 13 => MqMqiRequest::Close(
                MqObjectCloseRequest::new(
                    c.unwrap(),
                    MqRouteCloseTarget::Object {
                        handle: if self.step == 12 {
                            self.output.unwrap()
                        } else {
                            self.input.unwrap()
                        },
                        lifecycle: MqRouteCloseLifecycle::Predefined,
                    },
                    MqRouteCloseMode::None,
                )
                .unwrap(),
            ),
            14 => MqMqiRequest::Disconnect {
                connection: c.unwrap(),
            },
            _ => return Err("consumer step".into()),
        };
        self.step += 1;
        let sequence = self.step as u64;
        let key = setup::key(sequence);
        Ok(MachineDrive::HostCall(EffectRequest {
            run_unit: self.port.invocation.run_unit_id.clone(),
            sequence,
            deadline_tick: self.port.invocation.deadline_tick - 1,
            idempotency_key: Some(key.clone()),
            request: HostRequest::MqMqi(Box::new(MqMqiHostRequest {
                mutation: Mutation {
                    sequence,
                    idempotency_key: key,
                    transaction: None,
                },
                envelope: MqMqiRequestEnvelope {
                    context: frame.context().map_err(|e| e.to_string())?,
                    limits: frame.limits(),
                    request,
                },
            })),
        }))
    }
}
impl Machine for Consumer {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        self.next(resume).unwrap_or_else(|e| {
            MachineDrive::Failed(
                ExecutionProblem::new(
                    DiagnosticCode::new("MQIR0001").unwrap(),
                    FailureCategory::ProviderFailure,
                    Phase::Execute,
                    e,
                    false,
                    false,
                    DiagnosticLimits::default(),
                )
                .unwrap(),
            )
        })
    }
    fn effect_sequence(&self) -> u64 {
        self.step as u64
    }
}
fn lookup() -> MqRouteLookup {
    MqRouteLookup::Queue {
        name: MqRouteName::new("Q").unwrap(),
        manager: None,
        dynamic_pattern: None,
    }
}
fn descriptor(id: [u8; 24]) -> MqMdValue {
    MqMdValue::V1 {
        characters: MqMdCharacterEncoding::AsciiCompatible,
        fields: MqMdFields {
            struc_id: *b"MD  ",
            report: 0,
            msg_type: 8,
            expiry: -1,
            feedback: 0,
            encoding: 273,
            coded_char_set_id: 819,
            format: [b' '; 8],
            priority: 0,
            persistence: 1,
            msg_id: id,
            correl_id: [0; 24],
            backout_count: 0,
            reply_to_q: [b' '; 48],
            reply_to_q_mgr: [b' '; 48],
            user_identifier: [b' '; 12],
            accounting_token: [0; 32],
            appl_identity_data: [b' '; 32],
            put_appl_type: 0,
            put_appl_name: [b' '; 28],
            put_date: [b' '; 8],
            put_time: [b' '; 8],
            appl_origin_data: [b' '; 4],
        },
    }
}
