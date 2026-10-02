//! The real signed-package composition and canonical execution coordinator.
use super::*;
use mainframe_env_execution_api::{Completion, MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::{
    ImsCallSyntax, ImsExecutionContext, ImsRecoveryCall, ImsRecoveryRequest, ImsRecoveryResult,
};

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
