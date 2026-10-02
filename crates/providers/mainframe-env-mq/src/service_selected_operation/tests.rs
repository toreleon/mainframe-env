use super::*;
use mainframe_env_execution_api::*;
use mainframe_env_host_api::mq_object_route::*;
use mainframe_env_host_api::*;
use mainframe_env_store::{MemoryStore, SqliteStateStore};
use mainframe_env_store_api::*;
#[path = "tests/batch_child.rs"]
mod batch_child;
#[path = "tests/bounds.rs"]
mod bounds;
#[path = "tests/connection_warning.rs"]
mod connection_warning;
#[path = "tests/failures.rs"]
mod failures;
#[path = "tests/full_get.rs"]
mod full_get;
#[path = "tests/historical.rs"]
mod historical;
#[path = "tests/property.rs"]
mod property;
#[path = "tests/restart.rs"]
mod restart;
#[path = "tests/retention_dependencies.rs"]
mod retention_dependencies;

struct Clock(std::sync::atomic::AtomicU64);
impl MqReplayClock for Clock {
    fn now_tick(&self) -> Result<u64, HostProblem> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
#[derive(Default)]
struct Saf {
    deny: AtomicBool,
    calls: std::sync::atomic::AtomicU64,
    hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl EnterpriseAuthorizer for Saf {
    fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self.hook.lock().unwrap().take() {
            hook();
        }
        if self.deny.load(Ordering::SeqCst) {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}
struct Fixture {
    store: Arc<dyn PlatformStore>,
    service: Arc<MqService>,
    inv: Invocation,
    provider: CapabilityDescriptor,
    frame: FrameLease,
    owner: MqHandleOwner,
    clock: Arc<Clock>,
    saf: Arc<Saf>,
    connection: Mutex<Option<MqHconn>>,
}
impl Fixture {
    fn new(sqlite: bool) -> Self {
        let store: Arc<dyn PlatformStore> = if sqlite {
            Arc::new(SqliteStateStore::open("sqlite::memory:", 64 << 20, 256).unwrap())
        } else {
            Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
                max_audits: 256,
                ..Default::default()
            }))
        };
        Self::from_store(store)
    }
    fn from_store(store: Arc<dyn PlatformStore>) -> Self {
        Self::with_context(store, false)
    }
    fn with_context(store: Arc<dyn PlatformStore>, explicit: bool) -> Self {
        let legacy = MqService::open(store.clone(), MqLimits::default()).unwrap();
        legacy
            .install(vec![MqQueueDefinition {
                name: "Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let plan = legacy
            .plan_legacy_delivery_import(3, 5, Default::default())
            .unwrap();
        store
            .mutate_provider_states_atomic(plan.into_parts().0)
            .unwrap();
        drop(legacy);
        let l = InvocationLimits::default();
        let inv = Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("execution", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("test", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(
                PrincipalId::new("TEST", l).unwrap(),
                if explicit {
                    BTreeSet::from([
                        CapabilityId::new("host.mq.write", l).unwrap(),
                        CapabilityId::new("host.program.invoke", l).unwrap(),
                    ])
                } else {
                    BTreeSet::from([CapabilityId::new("host.mq.write", l).unwrap()])
                },
                l,
            )
            .unwrap(),
            ServiceClass::System,
            0,
            1000,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("invocation", l).unwrap(),
            1,
            ResourceLimits::default(),
            if explicit {
                BTreeMap::new()
            } else {
                BTreeMap::from([(
                    "mq.host-context".into(),
                    BoundedPayload::new(
                        "mainframe-env.mq.host-context@1",
                        b"zos-batch|queue-manager".to_vec(),
                        l,
                    )
                    .unwrap(),
                )])
            },
            l,
        )
        .unwrap()
        .with_cancellation_probe(CancellationProbe::new());
        let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(20)));
        let saf = Arc::new(Saf::default());
        let service = MqService::open_selected_mqi(
            store.clone(),
            MqLimits::default(),
            3,
            5,
            saf.clone(),
            clock.clone(),
        )
        .unwrap();
        let (frame, owner) = if explicit {
            let process = service
                .mint_selected_process_explicit(
                    &inv,
                    crate::host_context::AttestedHostContext {
                        environment: MqHostEnvironment::ZosBatch,
                        owner: MqSyncpointOwner::QueueManager,
                    },
                )
                .unwrap();
            service.bind_selected_root_explicit(process, &inv).unwrap()
        } else {
            let process = service.mint_selected_process(&inv).unwrap();
            service.bind_selected_root(process, &inv).unwrap()
        };
        let provider = CapabilityDescriptor {
            capability: CapabilityId::new("host.mq.write", l).unwrap(),
            provider_id: "mainframe-env-mq".into(),
            generation: "selected-test".into(),
            request_schema: "mainframe-env.mq-request@1".into(),
            result_schema: "mainframe-env.mq-result@1".into(),
            max_request_bytes: 4 << 20,
            max_result_bytes: 4 << 20,
            ready: true,
        };
        store
            .create_execution(ExecutionRecord {
                execution_id: inv.execution_id.clone(),
                run_unit_id: inv.run_unit_id.clone(),
                selector: inv.selector.clone(),
                artifact: inv.artifact.clone(),
                principal: inv.principal.id().clone(),
                state: ExecutionState::Admitted,
                attempt: 1,
                version: 1,
                owner_lease: None,
                lease_expiry_tick: None,
                terminal_tick: None,
            })
            .unwrap();
        store
            .transition_execution(&inv.execution_id, 1, ExecutionState::Queued, 6)
            .unwrap();
        store
            .transition_execution(&inv.execution_id, 2, ExecutionState::Running, 7)
            .unwrap();
        Self {
            store,
            service,
            inv,
            provider,
            frame,
            owner,
            clock,
            saf,
            connection: Mutex::new(None),
        }
    }
    fn effect(&self, sequence: u64, request: MqMqiRequest) -> EffectRequest {
        let key =
            IdempotencyKey::new(format!("effect-{sequence}"), InvocationLimits::default()).unwrap();
        EffectRequest {
            run_unit: self.inv.run_unit_id.clone(),
            sequence,
            deadline_tick: 900,
            idempotency_key: Some(key.clone()),
            request: HostRequest::MqMqi(MqMqiHostRequest {
                mutation: Mutation {
                    sequence,
                    idempotency_key: key,
                    transaction: None,
                },
                envelope: MqMqiRequestEnvelope {
                    context: MqMqiContext {
                        owner: self.owner,
                        syncpoint_owner: MqSyncpointOwner::QueueManager,
                    },
                    limits: Default::default(),
                    request,
                },
            }),
        }
    }
    fn seed(&self, effect: &EffectRequest) {
        self.store
            .record_intent(EffectRecord {
                execution_id: self.inv.execution_id.clone(),
                run_unit_id: self.inv.run_unit_id.clone(),
                sequence: effect.sequence,
                key: effect.idempotency_key.clone().unwrap(),
                digest_format: EffectDigestFormat::CanonicalHostV1,
                request_digest: canonical_request_digest(&effect.request).unwrap(),
                state: EffectState::Intent,
                result_digest: None,
                resolved_tick: None,
                intent: EffectIntentMetadata {
                    owner: self.inv.execution_id.clone(),
                    attempt: 1,
                    capability: Some(self.provider.capability.clone()),
                    audit_resource: Some(canonical_audit_resource_digest(&effect.request)),
                    audit_invocation_key: Some(self.inv.idempotency_key.clone()),
                    created_tick: 8,
                    recovery_after_tick: 900,
                    epoch: 8,
                    recovery_lease: None,
                },
            })
            .unwrap();
    }
    fn execute(&self, effect: &EffectRequest) -> Result<EffectResult, HostProblem> {
        self.service.execute_selected_mqi(
            self.frame,
            &self.inv,
            effect
                .mq_mqi_occurrence(HostLimits::default())?
                .ok_or(HostProblem::Malformed)?,
            &self.provider,
            HostLimits::default(),
        )
    }
    fn rows(&self) -> Vec<ProviderStateRecord> {
        self.store.list_provider_state_prefix("mq-", 4096).unwrap()
    }
    fn connect(&self) -> MqHconn {
        let e = self.effect(
            1,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        self.seed(&e);
        let reply = self.execute(&e).unwrap();
        let before = self.rows();
        assert_eq!(self.execute(&e).unwrap(), reply);
        assert_eq!(self.rows(), before);
        assert_eq!(
            self.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .state,
            EffectState::Intent
        );
        match reply.outcome.unwrap() {
            HostResult::MqMqi(MqMqiHostResult {
                result:
                    MqMqiResult {
                        outcome:
                            MqMqiOutcome::Completed {
                                output: MqMqiOutput::Connected(c),
                                ..
                            },
                        ..
                    },
                ..
            }) => {
                *self.connection.lock().unwrap() = Some(c);
                c
            }
            _ => panic!("connection result"),
        }
    }
    fn call(&self, sequence: u64, request: MqMqiRequest) -> EffectResult {
        let e = self.effect(sequence, request);
        self.seed(&e);
        let result = self.execute(&e).unwrap();
        let rows = self.rows();
        let audits = self
            .store
            .audit_records(&self.inv.execution_id, 0, 128)
            .unwrap();
        assert_eq!(self.execute(&e).unwrap(), result);
        assert_eq!(self.rows(), rows);
        assert_eq!(
            self.store
                .audit_records(&self.inv.execution_id, 0, 128)
                .unwrap(),
            audits
        );
        assert_eq!(
            self.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .state,
            EffectState::Intent
        );
        result
    }
    fn open(&self, c: MqHconn) -> MqHobj {
        let reply = self.call(
            2,
            MqMqiRequest::Open(
                MqObjectOpenRequest::new(
                    c,
                    lookup(),
                    &[
                        MqRouteOpenAccess::InputShared,
                        MqRouteOpenAccess::Output,
                        MqRouteOpenAccess::Browse,
                    ],
                    Default::default(),
                )
                .unwrap(),
            ),
        );
        match output(reply) {
            MqMqiOutput::Opened {
                object,
                dynamic: None,
            } => object,
            _ => panic!("object output"),
        }
    }
    fn unit(&self) -> u64 {
        let connection = self.connection.lock().unwrap().unwrap();
        let MqMqiUnitOfWork::Local { unit } = self
            .service
            .selected_local_unit(self.frame, &self.inv, connection)
            .unwrap()
        else {
            panic!("local owner")
        };
        unit
    }
    fn depth(&self) -> usize {
        let state = self.service.lock_selected().unwrap();
        let rich_state::StoredAuthority::Rich(s) = &*state else {
            panic!("rich state")
        };
        s.delivery
            .depth(&crate::MqObjectName::new("Q").unwrap())
            .unwrap()
    }
}

fn lookup() -> MqRouteLookup {
    MqRouteLookup::Queue {
        name: MqRouteName::new("Q").unwrap(),
        manager: None,
        dynamic_pattern: None,
    }
}
fn output(reply: EffectResult) -> MqMqiOutput {
    match reply.outcome.unwrap() {
        HostResult::MqMqi(MqMqiHostResult {
            result:
                MqMqiResult {
                    outcome:
                        MqMqiOutcome::Completed { output, .. }
                        | MqMqiOutcome::ReviewedOutput { output, .. }
                        | MqMqiOutcome::StatusPending { output },
                    ..
                },
            ..
        }) => output,
        _ => panic!("typed payload output"),
    }
}
fn message() -> MqMessage {
    MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: MqMessageIdentifiers {
                message_id: Some(vec![7; 24]),
                ..Default::default()
            },
            format: Some("BYTES".into()),
            expiry: MqExpiry::Unlimited,
            persistence: MqPersistence::Persistent,
            priority: MqPriority::QueueDefault,
            ordering: Default::default(),
        },
        body: vec![0, 255, 41],
        properties: Vec::new(),
    }
}
fn put(unit: MqMqiUnitOfWork) -> MqMqiPut {
    MqMqiPut {
        message: message(),
        message_handle: None,
        context: MqMqiMessageContext::Default,
        options: MqMqiOptions::ContractDefault,
        unit,
    }
}
fn get(
    c: MqHconn,
    o: MqHobj,
    unit: MqMqiUnitOfWork,
    capacity: usize,
    mode: MqGetMode,
) -> MqMqiRequest {
    MqMqiRequest::Get(MqMqiGet {
        connection: c,
        object: o,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit,
        get: MqGetContract {
            selection: Default::default(),
            mode,
            wait: MqWait::NoWait,
            truncation: MqTruncation::Reject,
            buffer_capacity: capacity,
        },
    })
}

#[test]
fn real_memory_sqlite_connect_receipt_replays_without_new_rows_or_core_completion() {
    for sqlite in [false, true] {
        Fixture::new(sqlite).connect();
    }
}

#[test]
fn forged_owner_and_absent_intent_fail_before_saf_and_rows() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let mut e = f.effect(
            1,
            MqMqiRequest::Connect(MqMqiConnect {
                manager: None,
                sharing: MqHandleSharing::NonShared,
                options: MqMqiOptions::ContractDefault,
            }),
        );
        let rows = f.rows();
        assert!(f.execute(&e).is_err());
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
        if let HostRequest::MqMqi(r) = &mut e.request {
            r.envelope.context.owner.process_id += 1;
        }
        f.seed(&e);
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.saf.calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn memory_sqlite_actual_no_syncpoint_flow_put1_get_close_disc_and_exact_replay() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        assert!(matches!(
            output(f.call(
                3,
                MqMqiRequest::PutOne {
                    connection: c,
                    lookup: lookup(),
                    alternate_user: None,
                    put: put(MqMqiUnitOfWork::NoSyncpoint)
                }
            )),
            MqMqiOutput::Put {
                outcome: MqDeliveryOutcome::Accepted,
                ..
            }
        ));
        assert_eq!(f.depth(), 1);
        let e = f.effect(
            4,
            get(c, o, MqMqiUnitOfWork::NoSyncpoint, 1024, MqGetMode::Remove),
        );
        f.seed(&e);
        let reply = f.execute(&e).unwrap();
        match output(reply.clone()) {
            MqMqiOutput::Got {
                message: Some(m), ..
            } => assert_eq!(m, message()),
            _ => panic!("message"),
        }
        assert_eq!(f.depth(), 0);
        let rows = f.rows();
        assert_eq!(f.execute(&e).unwrap(), reply);
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        f.call(
            5,
            MqMqiRequest::Close(
                MqObjectCloseRequest::new(
                    c,
                    MqRouteCloseTarget::Object {
                        handle: o,
                        lifecycle: MqRouteCloseLifecycle::Predefined,
                    },
                    MqRouteCloseMode::None,
                )
                .unwrap(),
            ),
        );
        f.call(6, MqMqiRequest::Disconnect { connection: c });
    }
}

#[test]
fn memory_sqlite_local_uow_pending_commit_and_backout_use_retained_provenance() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let first = f.unit();
        f.call(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::Local { unit: first }),
            },
        );
        assert_eq!(f.depth(), 0);
        f.call(
            4,
            MqMqiRequest::Commit {
                connection: c,
                unit: first,
            },
        );
        assert_eq!(f.depth(), 1);
        let second = f.unit();
        assert_ne!(first, second);
        f.call(
            5,
            get(
                c,
                o,
                MqMqiUnitOfWork::Local { unit: second },
                1024,
                MqGetMode::Remove,
            ),
        );
        assert_eq!(f.depth(), 0);
        f.call(
            6,
            MqMqiRequest::Back {
                connection: c,
                unit: second,
            },
        );
        assert_eq!(f.depth(), 1);
        let third = f.unit();
        assert_ne!(second, third);
        let e = f.effect(
            7,
            MqMqiRequest::Commit {
                connection: c,
                unit: first,
            },
        );
        f.seed(&e);
        let rows = f.rows();
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
    }
}

#[test]
fn memory_sqlite_truncated_get_keeps_copied_output_required_length_and_queue() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        f.call(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        let r = f.call(
            4,
            get(c, o, MqMqiUnitOfWork::NoSyncpoint, 1, MqGetMode::Remove),
        );
        let HostResult::MqMqi(r) = r.outcome.unwrap() else {
            panic!("typed output")
        };
        match r.result.outcome {
            MqMqiOutcome::ReviewedOutput {
                status,
                output:
                    MqMqiOutput::Got {
                        disposition:
                            MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                                required: 3,
                                copied: 1,
                            }),
                        message: Some(m),
                        ..
                    },
            } => {
                assert_eq!(m.body, vec![0]);
                assert_eq!(status.completion().symbol(), "MQCC_WARNING");
                assert_eq!(status.reason_symbol(), "MQRC_TRUNCATED_MSG_FAILED");
            }
            _ => panic!("lossless truncation"),
        }
        assert_eq!(f.depth(), 1);
    }
}

#[test]
fn memory_sqlite_saf_denial_has_only_denied_audit_no_receipt_or_state_change() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let e = f.effect(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        f.seed(&e);
        let rows = f.rows();
        f.saf.deny.store(true, Ordering::SeqCst);
        assert_eq!(f.execute(&e), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 0);
        let audits = f.store.audit_records(&f.inv.execution_id, 0, 128).unwrap();
        assert_eq!(audits.last().unwrap().decision, AuditDecision::Deny);
        assert_eq!(
            f.store
                .effect(e.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .state,
            EffectState::Intent
        );
    }
}

#[test]
fn memory_sqlite_changed_receipt_payload_and_expired_controls_cannot_publish_again() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let c = f.connect();
        let o = f.open(c);
        let mut e = f.effect(
            3,
            MqMqiRequest::Put {
                connection: c,
                object: o,
                put: put(MqMqiUnitOfWork::NoSyncpoint),
            },
        );
        f.seed(&e);
        f.execute(&e).unwrap();
        let rows = f.rows();
        if let HostRequest::MqMqi(r) = &mut e.request {
            if let MqMqiRequest::Put { put, .. } = &mut r.envelope.request {
                put.message.body.push(7);
            }
        }
        assert_eq!(f.execute(&e), Err(HostProblem::IdempotencyConflict));
        assert_eq!(f.rows(), rows);
        assert_eq!(f.depth(), 1);
        f.clock.0.store(901, Ordering::SeqCst);
        assert!(f.execute(&e).is_err());
        assert_eq!(f.rows(), rows);
    }
}
