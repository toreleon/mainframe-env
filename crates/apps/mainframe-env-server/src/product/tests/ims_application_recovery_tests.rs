//! The real signed-package composition and canonical execution coordinator.
use super::*;
use mainframe_env_execution_api::{Completion, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{
    ImsCallSyntax, ImsExecutionContext, ImsRecoveryCall, ImsRecoveryRequest, ImsRecoveryResult,
};

#[path = "ims_application_backout_tests.rs"]
mod application_backout;

struct LogMachine {
    effect: Option<EffectRequest>,
    result: Option<EffectResult>,
}
impl Machine for LogMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        match resume {
            MachineResume::Start => MachineDrive::HostCall(self.effect.take().unwrap()),
            MachineResume::HostResult(result) => {
                self.result = Some(result);
                MachineDrive::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new("test@1", vec![], InvocationLimits::default())
                        .unwrap(),
                })
            }
            _ => panic!("unexpected LOG machine resume"),
        }
    }
}

fn exercise(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let installed = server
        .install_application_package_v2(&signed_ims_package(&trust, 1, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let ids = InvocationLimits::default();
    let mut invocation = tm_invocation("coordinator-log-run", "coordinator-log");
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = 100;
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", ids).unwrap(),
        [CapabilityId::new("host.ims.write", ids).unwrap()]
            .into_iter()
            .collect(),
        ids,
    )
    .unwrap();
    // The ordinary selected API installs the signed DBD/PSB before dispatch.
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &invocation,
            &ImsRequest {
                operation: ImsOperation::Schedule,
                psb: Some("AUTHPSB".into()),
                pcb: 1,
                segments: vec![],
                data: vec![],
                qualifiers: vec![],
                checkpoint_id: None,
                max_segments: 16,
                system: None,
                q_class: None,
                mutation: Some(Mutation {
                    sequence: 10,
                    idempotency_key: IdempotencyKey::new("log-schedule", ids).unwrap(),
                    transaction: None,
                }),
            },
        )
        .unwrap();
    let key = IdempotencyKey::new("coordinator-log-effect", ids).unwrap();
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 1,
        idempotency_key: Some(key.clone()),
        deadline_tick: invocation.deadline_tick,
        request: HostRequest::ImsRecovery(ImsRecoveryRequest {
            application: "SIGNED-IMS-APPLICATION".into(),
            package_identity: installed.identity,
            psb: "AUTHPSB".into(),
            database: "AUTHDB".into(),
            context: ImsExecutionContext::DbBatch,
            syntax: ImsCallSyntax::Call,
            call: ImsRecoveryCall::Log {
                code: 0xff,
                data: vec![],
            },
            mutation: Mutation {
                sequence: 1,
                idempotency_key: key.clone(),
                transaction: None,
            },
        }),
    };
    let request_digest = mainframe_env_host_api::canonical_request_digest(&effect.request).unwrap();
    let mut machine = LogMachine {
        effect: Some(effect),
        result: None,
    };
    let coordinator = ExecutionCoordinator::durable(
        server.host.clone(),
        store.clone(),
        CoordinatorLimits::default(),
    );
    let outcome = coordinator.execute(
        &mut machine,
        &invocation,
        ExecutionControl {
            now_tick: 1,
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(_)),
        "{outcome:?}"
    );
    let expected = Ok(HostResult::ImsRecovery(ImsRecoveryResult::Logged {
        status: "  ".into(),
        sequence: 1,
    }));
    assert_eq!(machine.result.unwrap().outcome, expected);
    let effect = store.effect(&key).unwrap().unwrap();
    assert_eq!(effect.state, EffectState::Completed);
    assert_eq!(effect.digest_format, EffectDigestFormat::CanonicalHostV1);
    assert_eq!(effect.request_digest, request_digest);
    assert_eq!(
        effect.result_digest,
        Some(mainframe_env_host_api::canonical_result_digest(&expected).unwrap())
    );
    let audits = store
        .audit_records(&invocation.execution_id, 0, 16)
        .unwrap();
    assert_eq!(audits.len(), 1);
    assert_eq!(
        audits[0].decision,
        mainframe_env_execution_api::AuditDecision::Success
    );
    assert_eq!(audits[0].principal, *invocation.principal.id());
    assert_eq!(
        store
            .list_provider_state("ims-recovery-v1-session", 16)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn signed_memory_log_uses_canonical_coordinator_and_durable_audit() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_log_uses_canonical_coordinator_and_durable_audit() {
    let path = std::env::temp_dir().join(format!("ims-server-log-{}.sqlite", std::process::id()));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        config,
    );
    std::fs::remove_file(path).unwrap();
}

struct RecoveryMachine {
    effects: std::collections::VecDeque<EffectRequest>,
    results: Vec<EffectResult>,
}

impl Machine for RecoveryMachine {
    type Effect = EffectRequest;
    type EffectResult = EffectResult;
    fn drive(
        &mut self,
        resume: MachineResume<EffectResult>,
        _: Quantum,
    ) -> MachineDrive<EffectRequest> {
        if let MachineResume::HostResult(result) = resume {
            self.results.push(result);
        }
        if let Some(effect) = self.effects.pop_front() {
            MachineDrive::HostCall(effect)
        } else {
            MachineDrive::Completed(Completion {
                return_code: 0,
                output: BoundedPayload::new("test@1", vec![], InvocationLimits::default()).unwrap(),
            })
        }
    }
}

pub(super) fn recovery_db(operation: ImsOperation, sequence: u64, key_prefix: &str) -> ImsRequest {
    ImsRequest {
        operation,
        psb: (operation == ImsOperation::Schedule).then(|| "AUTHPSB".into()),
        pcb: 1,
        segments: vec![],
        data: vec![],
        qualifiers: vec![],
        checkpoint_id: None,
        max_segments: 16,
        system: None,
        q_class: None,
        mutation: Some(Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("{key_prefix}-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        }),
    }
}

pub(super) fn recovery_request(
    identity: &str,
    sequence: u64,
    key_prefix: &str,
    call: ImsRecoveryCall,
) -> ImsRecoveryRequest {
    ImsRecoveryRequest {
        application: "SIGNED-IMS-APPLICATION".into(),
        package_identity: identity.into(),
        psb: "AUTHPSB".into(),
        database: "AUTHDB".into(),
        context: ImsExecutionContext::DbBatch,
        syntax: ImsCallSyntax::Call,
        call,
        mutation: Mutation {
            sequence,
            idempotency_key: IdempotencyKey::new(
                format!("{key_prefix}-{sequence}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: None,
        },
    }
}

pub(super) fn run_recovery_machine(
    server: &ProductServer,
    store: Arc<dyn PlatformStore>,
    invocation: &Invocation,
    requests: Vec<HostRequest>,
) -> Vec<EffectResult> {
    let effects = requests
        .into_iter()
        .map(|request| {
            let mutation = request.mutation().unwrap();
            EffectRequest {
                run_unit: invocation.run_unit_id.clone(),
                sequence: mutation.sequence,
                idempotency_key: Some(mutation.idempotency_key.clone()),
                deadline_tick: invocation.deadline_tick,
                request,
            }
        })
        .collect();
    let mut machine = RecoveryMachine {
        effects,
        results: vec![],
    };
    let now_tick = server.jes_clock.now_tick().unwrap();
    let coordinator =
        ExecutionCoordinator::durable(server.host.clone(), store, CoordinatorLimits::default());
    let outcome = coordinator.execute(
        &mut machine,
        invocation,
        ExecutionControl {
            now_tick,
            cancellation_requested: false,
        },
    );
    assert!(
        matches!(outcome, ExecutionOutcome::Completed(_)),
        "{outcome:?}"
    );
    machine.results
}

fn exercise_selected_checkpoint_restart(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    use mainframe_env_host_api::{ImsQualifier, ImsRestartSelection};
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let installed = server
        .install_application_package_v2(&signed_ims_package(&trust, 1, 1))
        .unwrap();
    server.publish_application_generation(&installed).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let ids = InvocationLimits::default();
    let mut invocation = tm_invocation("selected-checkpoint-run", "selected-checkpoint");
    invocation.service_class = ServiceClass::Batch;
    invocation.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    invocation.principal = Principal::new(
        PrincipalId::new("IBMUSER", ids).unwrap(),
        [CapabilityId::new("host.ims.write", ids).unwrap()]
            .into_iter()
            .collect(),
        ids,
    )
    .unwrap();
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &invocation,
            &recovery_db(ImsOperation::Schedule, 10, "selected-schedule"),
        )
        .unwrap();
    let image = mainframe_env_ims::ImsGenericLoadImage {
        database: "AUTHDB".into(),
        records: [
            ("ROOT", None, &b"00000001ROOTDATA"[..]),
            ("CHILD", Some(0), &b"CHILD001CHILDONE"[..]),
            ("CHILD", Some(0), &b"CHILD002CHILDTWO"[..]),
            ("ROOT", None, &b"00000002ROOTDATA"[..]),
        ]
        .into_iter()
        .map(
            |(segment, parent, data)| mainframe_env_ims::ImsGenericLoadRecord {
                segment: segment.into(),
                parent,
                data: data.to_vec(),
            },
        )
        .collect(),
    };
    let mut load = recovery_db(ImsOperation::Load, 2, "selected-first");
    load.data = serde_json::to_vec(&image).unwrap();
    let mut gu = recovery_db(ImsOperation::GetHoldUnique, 3, "selected-first");
    gu.segments = vec!["ROOT".into(), "CHILD".into()];
    gu.qualifiers = vec![
        ImsQualifier {
            segment: "ROOT".into(),
            field: "ROOTKEY".into(),
            value: b"00000001".to_vec(),
        },
        ImsQualifier {
            segment: "CHILD".into(),
            field: "CHILDKEY".into(),
            value: b"CHILD001".to_vec(),
        },
    ];
    let checkpoint = recovery_request(
        &installed.identity,
        4,
        "selected-first",
        ImsRecoveryCall::SymbolicCheckpoint {
            id: "CHILD01".into(),
            user_areas: vec![b"saved".to_vec()],
        },
    );
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &invocation,
        vec![
            HostRequest::ImsRecovery(recovery_request(
                &installed.identity,
                1,
                "selected-first",
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Normal,
                    area_lengths: vec![5],
                },
            )),
            HostRequest::Ims(load),
            HostRequest::Ims(gu.clone()),
            HostRequest::ImsRecovery(checkpoint.clone()),
            HostRequest::Ims(recovery_db(ImsOperation::GetNext, 5, "selected-first")),
        ],
    );
    assert_eq!(results.len(), 5);
    assert_eq!(
        results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: None,
            user_areas: vec![],
            pcb_statuses: vec![]
        }))
    );
    assert_eq!(
        results[3].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "CHILD01".into(),
            sequence: 2
        }))
    );
    let Ok(HostResult::Ims(ref reset)) = results[4].outcome else {
        panic!("{:?}", results[4]);
    };
    assert_eq!(reset.segments[0].data, b"00000001ROOTDATA");
    let mut restarted = invocation.clone();
    restarted.execution_id = ExecutionId::new("selected-restart-execution", ids).unwrap();
    let restart = recovery_request(
        &installed.identity,
        1,
        "selected-restart",
        ImsRecoveryCall::Restart {
            selection: ImsRestartSelection::Checkpoint("CHILD01".into()),
            area_lengths: vec![5],
        },
    );
    gu.mutation = recovery_db(ImsOperation::GetUnique, 3, "selected-restart").mutation;
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &restarted,
        vec![
            HostRequest::ImsRecovery(restart.clone()),
            HostRequest::Ims(recovery_db(ImsOperation::GetNext, 2, "selected-restart")),
            HostRequest::Ims(gu),
        ],
    );
    assert_eq!(
        results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("CHILD01".into()),
            user_areas: vec![b"saved".to_vec()],
            pcb_statuses: vec![(1, "  ".into())]
        }))
    );
    let Ok(HostResult::Ims(ref next)) = results[1].outcome else {
        panic!()
    };
    assert_eq!(next.segments[0].data, b"CHILD002CHILDTWO");
    let Ok(HostResult::Ims(ref unique)) = results[2].outcome else {
        panic!()
    };
    assert_eq!(unique.segments[0].data, b"CHILD001CHILDONE");
    let before = store
        .list_provider_state("ims-v1-session-index", 64)
        .unwrap();
    assert_eq!(
        server
            .ims
            .observe_application_recovery(&restarted, &restart)
            .unwrap(),
        Some(ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("CHILD01".into()),
            user_areas: vec![b"saved".to_vec()],
            pcb_statuses: vec![(1, "  ".into())]
        })
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-session-index", 64)
            .unwrap(),
        before
    );
    for request in [checkpoint, restart] {
        let effect = store
            .effect(&request.mutation.idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(effect.state, EffectState::Completed);
        assert_eq!(
            effect.request_digest,
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsRecovery(request))
                .unwrap()
        );
    }
    assert_eq!(
        store
            .audit_records(&restarted.execution_id, 0, 16)
            .unwrap()
            .len(),
        3
    );

    // Exercise basic CHKP through the same signed, canonical public route.
    // Interactive database work retains real undo until the batch CHKP commits it.
    let mut basic = invocation.clone();
    basic.execution_id = ExecutionId::new("selected-basic-execution", ids).unwrap();
    let mut writer = basic.clone();
    writer.service_class = ServiceClass::Interactive;
    let mut insert = recovery_db(ImsOperation::Insert, 10, "selected-basic-write");
    insert.segments = vec!["ROOT".into()];
    insert.data = b"00000003ROOTDATA".to_vec();
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &writer, &insert)
            .unwrap()
            .status,
        "  "
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap()
            .len(),
        1
    );
    let basic_request = recovery_request(
        &installed.identity,
        20,
        "selected-basic",
        ImsRecoveryCall::BasicCheckpoint {
            id: "BASIC01".into(),
        },
    );
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &basic,
        vec![
            HostRequest::ImsRecovery(basic_request.clone()),
            HostRequest::Ims(recovery_db(ImsOperation::GetNext, 21, "selected-basic")),
        ],
    );
    assert_eq!(
        results[0].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "BASIC01".into(),
            sequence: 4,
        }))
    );
    assert!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap()
            .is_empty()
    );
    assert!(
        matches!(results[1].outcome, Ok(HostResult::Ims(ref r)) if r.segments[0].data == b"00000001ROOTDATA")
    );
    let mut later = recovery_db(ImsOperation::Insert, 30, "selected-basic-write");
    later.segments = vec!["ROOT".into()];
    later.data = b"00000004ROOTDATA".to_vec();
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &writer, &later)
            .unwrap()
            .status,
        "  "
    );
    let pending = store
        .list_provider_state("ims-v1-generic-unit-of-work", 64)
        .unwrap();
    assert_eq!(
        server
            .ims
            .observe_application_recovery(&basic, &basic_request)
            .unwrap(),
        Some(ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "BASIC01".into(),
            sequence: 4,
        })
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap(),
        pending
    );
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &writer,
            &recovery_db(ImsOperation::Rollback, 31, "selected-basic-write"),
        )
        .unwrap();
    let mut gu = recovery_db(ImsOperation::GetUnique, 32, "selected-basic");
    gu.segments = vec!["ROOT".into()];
    gu.qualifiers = vec![ImsQualifier {
        segment: "ROOT".into(),
        field: "ROOTKEY".into(),
        value: b"00000003".to_vec(),
    }];
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &basic, &gu)
            .unwrap()
            .segments[0]
            .data,
        b"00000003ROOTDATA"
    );
}

#[test]
fn signed_memory_checkpoint_restart_runs_real_hierarchical_gu_gn_through_coordinator() {
    exercise_selected_checkpoint_restart(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_checkpoint_restart_runs_real_hierarchical_gu_gn_through_coordinator() {
    let path = std::env::temp_dir().join(format!(
        "ims-selected-checkpoint-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise_selected_checkpoint_restart(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        config,
    );
    std::fs::remove_file(path).unwrap();
}
