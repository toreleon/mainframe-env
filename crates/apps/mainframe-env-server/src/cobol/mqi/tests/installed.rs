//! Actual compiler/artifact/program/machine/core route with a fixture MQ provider.
//! This proves the handoff and original journal identity, not MQ queue/SAF acceptance.

use super::*;
use mainframe_env_execution_api::{CapabilityId, Principal};
use mainframe_env_execution_api::{Completion, Machine, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::mq_mqi::{
    MqMqiOutcome, MqMqiOutput, MqMqiRequest, MqMqiResult, MqMqiStatus,
};
use mainframe_env_host_api::{
    MqHandleRegistry, MqHandleSharing, MqMqiHostResult, RegistrySnapshot, canonical_request_digest,
};
use mainframe_env_store::{LocalArtifactStore, SqliteStateStore};
use mainframe_env_store_api::{ArtifactStore, EffectState, IdempotencyStore};
mod boundary;

struct WeakProgramRouter {
    router: std::sync::Weak<DefaultProgramRouter>,
    descriptor: CapabilityDescriptor,
}
impl WeakProgramRouter {
    fn new(router: &Arc<DefaultProgramRouter>) -> Self {
        Self {
            router: Arc::downgrade(router),
            descriptor: router.descriptor().clone(),
        }
    }
}
impl HostProvider for WeakProgramRouter {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        self.router.upgrade().unwrap().invoke(invocation, effect)
    }
}

// The parent test driver uses the REAL coordinator admission/intent protocol.
// It is not claimed as a compiled batch scheduler or selected MQ producer.
struct OriginalCaller {
    original: Option<EffectRequest>,
    reply: Option<EffectResult>,
}
impl Machine for OriginalCaller {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.original.take().unwrap()),
            MachineResume::HostResult(reply) => {
                let output = match &reply.outcome {
                    Ok(HostResult::Program(output)) => output.clone(),
                    _ => {
                        BoundedPayload::new("test@1", vec![], InvocationLimits::default()).unwrap()
                    }
                };
                self.reply = Some(reply);
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output,
                })
            }
            _ => panic!("unexpected caller continuation"),
        }
    }
}

struct Provider {
    descriptor: CapabilityDescriptor,
    store: Arc<dyn PlatformStore>,
    registry: Mutex<MqHandleRegistry>,
    keys: Mutex<Vec<IdempotencyKey>>,
    failure: Mutex<Option<HostProblem>>,
    cancel_after_connect: std::sync::atomic::AtomicBool,
}
impl HostProvider for Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let occurrence = effect
            .mq_mqi_occurrence(Default::default())
            .unwrap()
            .unwrap();
        let key = effect.idempotency_key.as_ref().unwrap();
        let intent = self.store.effect(key).unwrap().unwrap();
        assert_eq!(intent.state, EffectState::Intent);
        assert_eq!(intent.execution_id, invocation.execution_id);
        assert_eq!(
            intent.request_digest,
            canonical_request_digest(&effect.request).unwrap()
        );
        self.keys.lock().unwrap().push(key.clone());
        if let Some(problem) = self.failure.lock().unwrap().as_ref() {
            return EffectResult {
                sequence: effect.sequence,
                outcome: Err(problem.clone()),
            };
        }
        let envelope = occurrence.envelope();
        let output = match envelope.request {
            MqMqiRequest::Connect(_) => MqMqiOutput::Connected(
                self.registry
                    .lock()
                    .unwrap()
                    .connect(envelope.context.owner, MqHandleSharing::NonShared)
                    .unwrap(),
            ),
            MqMqiRequest::Disconnect { connection } => {
                self.registry
                    .lock()
                    .unwrap()
                    .disconnect(envelope.context.owner, connection)
                    .unwrap();
                MqMqiOutput::NoOutput
            }
            _ => panic!("fixture handles only the tested connection flow"),
        };
        if self.cancel_after_connect.load(Ordering::SeqCst)
            && matches!(output, MqMqiOutput::Connected(_))
        {
            invocation.cancellation_probe.as_ref().unwrap().request();
        }
        EffectResult {
            sequence: effect.sequence,
            outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                limits: envelope.limits,
                result: MqMqiResult {
                    call: envelope.request.call(),
                    outcome: MqMqiOutcome::Completed {
                        status: MqMqiStatus::OkNone,
                        output,
                    },
                },
            })),
        }
    }
}

#[test]
fn compiled_installed_batch_emits_original_typed_effects_and_real_durable_intents() {
    for sqlite in [false, true] {
        let root = super::super::super::hardening::TestRoot::new();
        let url = format!("sqlite://{}?mode=rwc", root.0.join("mqi.db").display());
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap())
        } else {
            Arc::new(MemoryStore::new(Default::default()))
        };
        let router = default_program_router();
        let factory = admission(false);
        router.bind_mqi_program_host(factory.clone()).unwrap();
        router
            .bind_execution_control(Arc::new(|_: &Invocation| {
                Ok(ExecutionControl {
                    now_tick: 1,
                    cancellation_requested: false,
                })
            }))
            .unwrap();
        let limits = InvocationLimits::default();
        let provider = Arc::new(Provider {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.mq.write", limits).unwrap(),
                provider_id: "mqi-host-fixture".into(),
                generation: "fixture@1".into(),
                request_schema: "mainframe-env.host-request@1".into(),
                result_schema: "mainframe-env.host-result@1".into(),
                max_request_bytes: 8 * 1024 * 1024,
                max_result_bytes: 8 * 1024 * 1024,
                ready: true,
            },
            store: store.clone(),
            registry: Mutex::new(MqHandleRegistry::new(1, 4).unwrap()),
            keys: Mutex::new(vec![]),
            failure: Mutex::new(None),
            cancel_after_connect: std::sync::atomic::AtomicBool::new(false),
        });
        let artifacts = Arc::new(LocalArtifactStore::open(&root.0, 64 * 1024 * 1024).unwrap());
        let host = Arc::new(ScopedHostService::new(
            Arc::new(
                RegistrySnapshot::new(
                    1,
                    vec![
                        provider.clone() as Arc<dyn HostProvider>,
                        Arc::new(WeakProgramRouter::new(&router)),
                    ],
                    limits,
                )
                .unwrap(),
            ),
            Default::default(),
        ));
        router
            .bind_runtime(host.clone(), store.clone(), artifacts.clone())
            .unwrap();
        let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MQFLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. PROCEDURE DIVISION. CALL 'MQCONN' USING QM HC CC RC. CALL 'MQDISC' USING HC CC RC. DISPLAY 'DONE'. STOP RUN.";
        let input = super::super::super::hardening::input(source);
        let CompilerResult::Published { artifact, .. } = CobolCompiler::default()
            .compile(CompilerRequest {
                source: source_bundle(&input).unwrap(),
                mode: CompilationMode::Executable,
                target: CompileTarget::new("reference").unwrap(),
                options: CompileOptions::new(BTreeMap::new()).unwrap(),
            })
            .unwrap()
        else {
            panic!("compiled artifact")
        };
        let record = artifact::published_artifact_record(&artifact).unwrap();
        let id = record.artifact.clone();
        artifacts.put_artifact(record).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "batch-program".into(),
                    key: "MQFLOW".into(),
                    version: 1,
                    payload: id.as_str().as_bytes().to_vec(),
                },
                None,
            )
            .unwrap();
        let mut parent = super::super::super::hardening::parent();
        parent.deadline_tick = 1000;
        let mut grants = parent.principal.grants().clone();
        grants.insert(CapabilityId::new("host.mq.write", limits).unwrap());
        parent.principal = Principal::new(parent.principal.id().clone(), grants, limits).unwrap();
        let payload = BoundedPayload::new(
            "mainframe-env.program.input@1",
            serde_json::to_vec(&super::super::super::hardening::input("")).unwrap(),
            limits,
        )
        .unwrap();
        let original = EffectRequest {
            run_unit: parent.run_unit_id.clone(),
            sequence: 1,
            deadline_tick: parent.deadline_tick,
            idempotency_key: Some(IdempotencyKey::new("original-installed-call", limits).unwrap()),
            request: HostRequest::Program(ProgramRequest::Call {
                program: mainframe_env_host_api::ProgramName::new("MQFLOW", 128).unwrap(),
                payload,
                service: None,
            }),
        };
        let mut caller = OriginalCaller {
            original: Some(original.clone()),
            reply: None,
        };
        let outcome = ExecutionCoordinator::durable(host, store.clone(), Default::default())
            .execute_with_control(&mut caller, &parent, || {
                Ok(ExecutionControl {
                    now_tick: 1,
                    cancellation_requested: false,
                })
            });
        assert!(
            matches!(outcome, ExecutionOutcome::Completed(_)),
            "{outcome:?}"
        );
        let Ok(HostResult::Program(payload)) = caller.reply.unwrap().outcome else {
            panic!("installed reply")
        };
        let output: ProgramOutput = serde_json::from_slice(payload.bytes()).unwrap();
        assert_eq!(output.records, vec![b"DONE".to_vec()]);
        let keys = provider.keys.lock().unwrap().clone();
        assert_eq!(keys.len(), 2);
        for key in &keys {
            assert_eq!(
                store.effect(key).unwrap().unwrap().state,
                EffectState::Completed
            );
        }
        assert_eq!(provider.registry.lock().unwrap().active_handles(), 0);
        let observed = factory.observed.lock().unwrap();
        assert_eq!(observed.len(), 1);
        assert_eq!(
            observed[0].parent_execution_id.as_ref(),
            Some(&parent.execution_id)
        );
        assert_eq!(observed[0].principal, parent.principal);
        assert_eq!(observed[0].cancellation_probe, parent.cancellation_probe);
        assert_eq!(observed[0].artifact, id);
        assert_eq!(factory.events.lock().unwrap().len(), 1);
        assert!(matches!(
            &factory.events.lock().unwrap()[0],
            ExecutionOutcome::Completed(_)
        ));
        assert!(factory.aborts.lock().unwrap().is_empty());
        assert_eq!(
            store
                .effect(original.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .request_digest,
            canonical_request_digest(&original.request).unwrap()
        );
        if sqlite {
            drop(observed);
            drop(router);
            drop(provider);
            drop(store);
            let reopened = SqliteStateStore::open(&url, 8 * 1024 * 1024, 65536).unwrap();
            for key in &keys {
                assert_eq!(
                    reopened.effect(key).unwrap().unwrap().state,
                    EffectState::Completed
                );
            }
        }
    }
}
