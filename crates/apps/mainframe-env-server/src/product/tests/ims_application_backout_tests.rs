//! Signed selection, real images, canonical coordinator and terminal projection.
use super::*;

fn insert(sequence: u64, key: &str, bytes: &[u8]) -> HostRequest {
    let mut request = recovery_db(ImsOperation::Insert, sequence, key);
    request.segments = vec!["ROOT".into()];
    request.data = bytes.to_vec();
    HostRequest::Ims(request)
}

fn image(server: &ProductServer, invocation: &Invocation) -> Vec<Vec<u8>> {
    let mut request = recovery_db(ImsOperation::Unload, 999, "backout-image");
    request.psb = Some("AUTHDB".into());
    request.mutation = None;
    server
        .ims_execute_selected("SIGNED-IMS-APPLICATION", invocation, &request)
        .unwrap()
        .segments
        .into_iter()
        .map(|s| s.data)
        .collect()
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
    let mut invocation = tm_invocation("signed-backout-run", "signed-backout");
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
            &recovery_db(ImsOperation::Schedule, 100, "signed-backout-schedule"),
        )
        .unwrap();
    let recovery = |seq, call| {
        HostRequest::ImsRecovery(recovery_request(
            &installed.identity,
            seq,
            "signed-backout",
            call,
        ))
    };
    let set = ImsRecoveryCall::Sets {
        token: Some(*b"SAVE"),
        user_data: Some(b"saved".to_vec()),
    };
    let rols = ImsRecoveryCall::Rols {
        token: Some(*b"SAVE"),
        area_length: Some(5),
    };
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &invocation,
        vec![
            insert(1, "signed-backout", b"00000001ROOTDATA"),
            recovery(
                2,
                ImsRecoveryCall::BasicCheckpoint {
                    id: "COMMIT1".into(),
                },
            ),
            recovery(3, set.clone()),
            insert(4, "signed-backout", b"00000002ROOTDATA"),
            recovery(
                5,
                ImsRecoveryCall::Setu {
                    token: Some(*b"NEST"),
                    user_data: Some(vec![]),
                },
            ),
            insert(6, "signed-backout", b"00000003ROOTDATA"),
            recovery(7, rols.clone()),
        ],
    );
    assert_eq!(results.len(), 7);
    assert!(results.iter().all(|r| r.outcome.is_ok()), "{results:?}");
    assert_eq!(
        results[6].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::BackedOut {
            status: "  ".into(),
            user_data: b"saved".to_vec()
        }))
    );
    assert_eq!(
        image(&server, &invocation),
        vec![b"00000001ROOTDATA".to_vec()]
    );
    let mut resumed = invocation.clone();
    resumed.execution_id = ExecutionId::new("signed-backout-resume", ids).unwrap();
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &resumed,
        vec![
            insert(8, "signed-backout-resume", b"00000004ROOTDATA"),
            HostRequest::ImsRecovery(recovery_request(
                &installed.identity,
                9,
                "signed-backout-resume",
                ImsRecoveryCall::Rolb,
            )),
            insert(10, "signed-backout-resume", b"00000005ROOTDATA"),
            HostRequest::ImsRecovery(recovery_request(
                &installed.identity,
                11,
                "signed-backout-resume",
                ImsRecoveryCall::Rols {
                    token: Some(*b"NEST"),
                    area_length: Some(0),
                },
            )),
        ],
    );
    assert!(results.iter().all(|r| r.outcome.is_ok()), "{results:?}");
    assert_eq!(
        results[3].outcome,
        Ok(HostResult::ImsRecovery(ImsRecoveryResult::BackedOut {
            status: "RA".into(),
            user_data: vec![]
        }))
    );
    assert_eq!(
        image(&server, &resumed),
        vec![b"00000001ROOTDATA".to_vec(), b"00000005ROOTDATA".to_vec()]
    );
    // Read-only observation of the old receipt cannot back out this later UOW.
    let before = store
        .list_provider_state("ims-v1-generic-unit-of-work", 64)
        .unwrap();
    assert_eq!(
        server
            .ims
            .observe_application_recovery(
                &invocation,
                &recovery_request(&installed.identity, 7, "signed-backout", rols)
            )
            .unwrap(),
        Some(ImsRecoveryResult::BackedOut {
            status: "  ".into(),
            user_data: b"saved".to_vec()
        })
    );
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap(),
        before
    );
    for (seq, call) in [
        (3, set),
        (
            7,
            ImsRecoveryCall::Rols {
                token: Some(*b"SAVE"),
                area_length: Some(5),
            },
        ),
    ] {
        let request = recovery_request(&installed.identity, seq, "signed-backout", call);
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
        assert!(effect.result_digest.is_some());
    }
    assert_eq!(
        store
            .audit_records(&invocation.execution_id, 0, 64)
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        store
            .audit_records(&resumed.execution_id, 0, 64)
            .unwrap()
            .len(),
        4
    );

    // Malformed operands reach the signed host through the coordinator.
    let mut rejected = resumed.clone();
    rejected.execution_id = ExecutionId::new("signed-backout-negative", ids).unwrap();
    let bad = recovery_request(
        &installed.identity,
        1,
        "signed-backout-negative",
        ImsRecoveryCall::Sets {
            token: Some(*b"BAD!"),
            user_data: None,
        },
    );
    let before = store
        .list_provider_state("ims-v1-generic-database", 64)
        .unwrap();
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &rejected,
        vec![HostRequest::ImsRecovery(bad)],
    );
    assert_eq!(results[0].outcome, Err(HostProblem::Malformed));
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-database", 64)
            .unwrap(),
        before
    );

    let mut wrong_binding = recovery_request(
        &installed.identity,
        2,
        "signed-backout-negative",
        ImsRecoveryCall::Rolb,
    );
    wrong_binding.package_identity = format!("sha256:{}", "b".repeat(64));
    let mut binding_actor = rejected.clone();
    binding_actor.execution_id = ExecutionId::new("signed-backout-binding", ids).unwrap();
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &binding_actor,
        vec![HostRequest::ImsRecovery(wrong_binding)],
    );
    assert_eq!(results[0].outcome, Err(HostProblem::IdempotencyConflict));
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-database", 64)
            .unwrap(),
        before
    );
    let mut denied = rejected.clone();
    denied.execution_id = ExecutionId::new("signed-backout-denied", ids).unwrap();
    denied.principal = Principal::new(
        PrincipalId::new("STRANGER", ids).unwrap(),
        [CapabilityId::new("host.ims.write", ids).unwrap()]
            .into_iter()
            .collect(),
        ids,
    )
    .unwrap();
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &denied,
        vec![HostRequest::ImsRecovery(recovery_request(
            &installed.identity,
            3,
            "signed-backout-negative",
            ImsRecoveryCall::Rolb,
        ))],
    );
    assert_eq!(results[0].outcome, Err(HostProblem::Unauthorized));
    assert_eq!(
        store
            .list_provider_state("ims-v1-generic-database", 64)
            .unwrap(),
        before
    );
    assert!(
        store
            .audit_records(&denied.execution_id, 0, 16)
            .unwrap()
            .iter()
            .any(|a| a.decision == mainframe_env_execution_api::AuditDecision::Deny)
    );

    // The typed terminal response is consumed by the application machine and
    // persisted as an actual coordinator Abend, rather than a normal completion.
    let mut terminal = resumed.clone();
    terminal.execution_id = ExecutionId::new("signed-backout-terminal", ids).unwrap();
    let request = recovery_request(
        &installed.identity,
        1,
        "signed-backout-terminal",
        ImsRecoveryCall::Roll,
    );
    let mut machine = TerminalMachine {
        effect: Some(EffectRequest {
            run_unit: terminal.run_unit_id.clone(),
            sequence: 1,
            idempotency_key: Some(request.mutation.idempotency_key.clone()),
            request: HostRequest::ImsRecovery(request),
            deadline_tick: terminal.deadline_tick,
        }),
    };
    let coordinator = ExecutionCoordinator::durable(
        server.host.clone(),
        store.clone(),
        CoordinatorLimits::default(),
    );
    assert!(
        matches!(coordinator.execute(&mut machine, &terminal, ExecutionControl {
        now_tick: server.jes_clock.now_tick().unwrap(), cancellation_requested: false }),
        ExecutionOutcome::Abend(ref a) if a.code == "U0778" && a.dump == mainframe_env_execution_api::AbendDumpDisposition::Suppressed)
    );
    let mut reader = terminal.clone();
    reader.run_unit_id = RunUnitId::new("signed-backout-image-reader", ids).unwrap();
    assert_eq!(image(&server, &reader), vec![b"00000001ROOTDATA".to_vec()]);
}

struct TerminalMachine {
    effect: Option<EffectRequest>,
}
impl Machine for TerminalMachine {
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
                let Ok(HostResult::ImsRecovery(ImsRecoveryResult::Abended { code })) =
                    result.outcome
                else {
                    panic!("{result:?}")
                };
                MachineDrive::Abend(mainframe_env_execution_api::Abend {
                    code,
                    reason: None,
                    dump: mainframe_env_execution_api::AbendDumpDisposition::Suppressed,
                })
            }
            _ => panic!("unexpected terminal resume"),
        }
    }
}

#[test]
fn signed_memory_application_backout_uses_real_images_and_coordinator() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_application_backout_uses_real_images_and_coordinator() {
    let path =
        std::env::temp_dir().join(format!("ims-signed-backout-{}.sqlite", std::process::id()));
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
