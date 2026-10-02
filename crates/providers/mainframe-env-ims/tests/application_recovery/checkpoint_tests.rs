use super::*;
use mainframe_env_host_api::{ImsOperation, ImsRestartSelection};

#[path = "checkpoint_tests/integrity_tests.rs"]
mod integrity_tests;
#[path = "checkpoint_tests/secondary_tests.rs"]
mod secondary_tests;

pub(super) fn call(sequence: u64, call: ImsRecoveryCall) -> ImsRecoveryRequest {
    ImsRecoveryRequest {
        call,
        ..request(sequence)
    }
}

pub(super) fn invoke_call(
    service: &Arc<ImsService>,
    store: &Arc<dyn TestStore>,
    invocation: &Invocation,
    sequence: u64,
    operation: ImsRecoveryCall,
) -> ImsRecoveryResult {
    let r = call(sequence, operation);
    intent(&**store, invocation, &r);
    dispatch(service.clone(), store.clone(), invocation, &r).unwrap()
}

fn seed(service: &ImsService, invocation: &Invocation) {
    for (op, sequence, data) in [
        (ImsOperation::Schedule, 10, &b""[..]),
        (ImsOperation::Insert, 11, &b"01A"[..]),
        (ImsOperation::Insert, 12, &b"02B"[..]),
        (ImsOperation::Insert, 13, &b"03C"[..]),
    ] {
        assert_eq!(
            service
                .execute(invocation, &database_request(op, sequence, data))
                .unwrap()
                .status,
            "  "
        );
    }
}

pub(super) fn snapshot(store: &dyn ProviderStateStore) -> Vec<Vec<ProviderStateRecord>> {
    [
        "ims-v1-generic-database",
        "ims-v1-session-index",
        "ims-v1-generic-unit-of-work",
        "ims-v1-system",
        "ims-recovery-v1-session",
    ]
    .iter()
    .map(|ns| store.list_provider_state(ns, 64).unwrap())
    .collect()
}

pub(super) fn next_execution(invocation: &Invocation, name: &str) -> Invocation {
    let mut next = invocation.clone();
    next.execution_id = ExecutionId::new(name, InvocationLimits::default()).unwrap();
    next
}

#[test]
fn live_clock_deadline_and_cancellation_leave_checkpoint_rows_unpublished() {
    struct Clock(std::sync::atomic::AtomicU64);
    impl ImsReplayClock for Clock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }
    backends("checkpoint-clock", |store| {
        let clock = Arc::new(Clock(std::sync::atomic::AtomicU64::new(2)));
        let service = ImsService::open_authorized_with_replay_clock(
            store.clone(),
            ImsLimits::default(),
            Arc::new(Allow),
            clock.clone(),
        )
        .unwrap();
        service.install_metadata(catalog()).unwrap();
        service
            .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&catalog()))
            .unwrap();
        let mut invocation = invocation();
        seed(&service, &invocation);
        let r = call(
            1,
            ImsRecoveryCall::BasicCheckpoint {
                id: "DEADLINE".into(),
            },
        );
        intent(&*store, &invocation, &r);
        let before = snapshot(&*store);
        clock.0.store(invocation.deadline_tick, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::TimedOut)
        );
        assert_eq!(snapshot(&*store), before);
        assert_eq!(
            service
                .observe_application_recovery(&invocation, &r)
                .unwrap(),
            None
        );
        clock.0.store(2, Ordering::SeqCst);
        invocation.cancellation_probe = Some(mainframe_env_execution_api::CancellationProbe::new());
        invocation.cancellation_probe.as_ref().unwrap().request();
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::Cancelled)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn trusted_attempt_boundary_allows_one_new_xrst_without_reusing_prior_effects() {
    backends("checkpoint-attempt", |store| {
        let service = open(store.clone());
        let mut invocation = invocation();
        seed(&service, &invocation);
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        invoke_call(
            &service,
            &store,
            &invocation,
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "ATTEMPT1".into(),
                user_areas: vec![b"one".to_vec()],
            },
        );
        invocation.attempt = 2;
        assert_eq!(
            invoke_call(
                &service,
                &store,
                &invocation,
                3,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("ATTEMPT1".into()),
                    area_lengths: vec![3],
                }
            ),
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("ATTEMPT1".into()),
                user_areas: vec![b"one".to_vec()],
                pcb_statuses: vec![(1, "  ".into())],
            }
        );
        let duplicate = call(
            4,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        intent(&*store, &invocation, &duplicate);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &duplicate),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

fn get(pcb: u16, sequence: u64, key: &[u8], hold: bool) -> mainframe_env_host_api::ImsRequest {
    let mut r = database_request(
        if hold {
            ImsOperation::GetHoldUnique
        } else {
            ImsOperation::GetUnique
        },
        sequence,
        &[],
    );
    r.pcb = pcb;
    r.segments = vec!["ROOT".into()];
    r.qualifiers = vec![mainframe_env_host_api::ImsQualifier {
        segment: "ROOT".into(),
        field: "KEY".into(),
        value: key.to_vec(),
    }];
    r
}

fn open_catalog(
    store: Arc<dyn ProviderStateStore>,
    catalog: ImsMetadataCatalog,
) -> Arc<ImsService> {
    let service =
        ImsService::open_authorized(store, ImsLimits::default(), Arc::new(Allow)).unwrap();
    service.install_metadata(catalog.clone()).unwrap();
    service
        .publish_metadata_generation("LOGAPP", 1, PACKAGE, Some(&catalog))
        .unwrap();
    service
}

#[test]
fn checkpoint_releases_all_real_pcb_positions_holds_q_and_database_undo() {
    backends("checkpoint-pcbs", |store| {
        let mut metadata = catalog();
        let mut other_db = metadata.databases[0].clone();
        other_db.name = "OTHERDB".into();
        metadata.databases.push(other_db);
        let ImsPcbMetadata::Database(mut other_pcb) = metadata.psbs[0].pcbs[0].clone() else {
            panic!()
        };
        other_pcb.database = "OTHERDB".into();
        other_pcb.name = "DBPCB2".into();
        metadata.psbs[0]
            .pcbs
            .push(ImsPcbMetadata::Database(other_pcb));
        let ImsPcbMetadata::Database(mut untouched) = metadata.psbs[0].pcbs[0].clone() else {
            panic!()
        };
        untouched.name = "DBPCB3".into();
        metadata.psbs[0]
            .pcbs
            .push(ImsPcbMetadata::Database(untouched));
        let service = open_catalog(store.clone(), metadata);
        let invocation = invocation();
        let mut writer = invocation.clone();
        writer.service_class = ServiceClass::Interactive;
        seed(&service, &writer);
        for (sequence, data) in [(30, b"11X"), (31, b"12Y")] {
            let mut insert = database_request(ImsOperation::Insert, sequence, data);
            insert.pcb = 2;
            assert_eq!(service.execute(&writer, &insert).unwrap().status, "  ");
        }
        // Normal XRST permits preceding committed DB calls, without restoring.
        service
            .execute(&writer, &database_request(ImsOperation::Commit, 32, &[]))
            .unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        for (pcb, sequence, key) in [(1, 33, &b"02"[..]), (2, 34, &b"11"[..])] {
            let mut gu = get(pcb, sequence, key, true);
            gu.q_class = Some(mainframe_env_host_api::ImsQClass::new(b'A').unwrap());
            assert_eq!(service.execute(&writer, &gu).unwrap().status, "  ");
        }
        let row = store
            .get_provider_state("ims-v1-system", "runtime")
            .unwrap()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(json["value"]["reservations"].as_object().unwrap().len(), 2);
        let mut insert = database_request(ImsOperation::Insert, 35, b"13Z");
        insert.pcb = 2;
        service.execute(&writer, &insert).unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "ALLPCBS".into(),
                user_areas: vec![],
            },
        );
        let row = store
            .get_provider_state("ims-v1-system", "runtime")
            .unwrap()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert!(
            json["value"]["reservations"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
        for (pcb, sequence, expected) in [
            (1, 36, &b"01A"[..]),
            (2, 37, &b"11X"[..]),
            (3, 38, &b"01A"[..]),
        ] {
            let mut gn = database_request(ImsOperation::GetNext, sequence, &[]);
            gn.pcb = pcb;
            assert_eq!(
                service.execute(&invocation, &gn).unwrap().segments[0].data,
                expected
            );
            let mut repl = database_request(ImsOperation::Replace, sequence + 10, expected);
            repl.pcb = pcb;
            assert_eq!(service.execute(&writer, &repl).unwrap().status, "DJ");
        }
        let restarted = next_execution(&invocation, "multi-restart");
        assert_eq!(
            invoke_call(
                &service,
                &store,
                &restarted,
                3,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("ALLPCBS".into()),
                    area_lengths: vec![]
                }
            ),
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("ALLPCBS".into()),
                user_areas: vec![],
                pcb_statuses: vec![(1, "  ".into()), (2, "  ".into())]
            }
        );
        // INSERT established position 13Z on PCB 2 at checkpoint; GN is at end.
        let mut gn = database_request(ImsOperation::GetNext, 60, &[]);
        gn.pcb = 2;
        assert_eq!(service.execute(&restarted, &gn).unwrap().status, "GB");
        gn.pcb = 1;
        gn.mutation = database_request(ImsOperation::GetNext, 61, &[]).mutation;
        assert_eq!(
            service.execute(&restarted, &gn).unwrap().segments[0].data,
            b"03C"
        );
        gn.pcb = 3;
        gn.mutation = database_request(ImsOperation::GetNext, 62, &[]).mutation;
        assert_eq!(
            service.execute(&restarted, &gn).unwrap().segments[0].data,
            b"01A"
        );
    });
}

#[test]
fn checkpoint_selectors_operand_context_and_order_errors_preserve_real_rows() {
    backends("checkpoint-errors", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        for (sequence, operation, expected) in [
            (
                1,
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "BEFORE".into(),
                    user_areas: vec![b"abc".to_vec()],
                },
                HostProblem::Malformed,
            ),
            (
                2,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("MISSING".into()),
                    area_lengths: vec![],
                },
                HostProblem::NotFound,
            ),
            (
                3,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Last,
                    area_lengths: vec![],
                },
                HostProblem::Unsupported,
            ),
            (
                4,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Timestamp("00012741234560".into()),
                    area_lengths: vec![],
                },
                HostProblem::Unsupported,
            ),
        ] {
            let r = call(sequence, operation);
            intent(&*store, &invocation, &r);
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(expected)
            );
            assert_eq!(snapshot(&*store), before);
        }
        invoke_call(
            &service,
            &store,
            &invocation,
            5,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![3],
            },
        );
        for (sequence, operation) in [
            (
                6,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Normal,
                    area_lengths: vec![],
                },
            ),
            (7, ImsRecoveryCall::BasicCheckpoint { id: "MIXED".into() }),
        ] {
            let r = call(sequence, operation);
            intent(&*store, &invocation, &r);
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(HostProblem::Malformed)
            );
            assert_eq!(snapshot(&*store), before);
        }
        invoke_call(
            &service,
            &store,
            &invocation,
            8,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "AREAS".into(),
                user_areas: vec![b"abc".to_vec(), b"xy".to_vec()],
            },
        );
        for (index, lengths) in [vec![2], vec![3, 2, 1]].into_iter().enumerate() {
            let restarted = next_execution(&invocation, &format!("bad-areas-{index}"));
            let r = call(
                9 + index as u64,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("AREAS".into()),
                    area_lengths: lengths,
                },
            );
            intent(&*store, &restarted, &r);
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &restarted, &r),
                Err(HostProblem::Malformed)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let restarted = next_execution(&invocation, "normal-again");
        let before = store
            .list_provider_state("ims-v1-session-index", 64)
            .unwrap();
        invoke_call(
            &service,
            &store,
            &restarted,
            12,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![3],
            },
        );
        let after = store
            .list_provider_state("ims-v1-session-index", 64)
            .unwrap();
        let mut a: serde_json::Value = serde_json::from_slice(&before[0].payload).unwrap();
        let mut b: serde_json::Value = serde_json::from_slice(&after[0].payload).unwrap();
        a["value"].as_object_mut().unwrap().remove("recovery");
        b["value"].as_object_mut().unwrap().remove("recovery");
        assert_eq!(a["value"], b["value"]);
        // Normal must not select the latest saved position (ROOT 03).
        assert_eq!(
            service
                .execute(
                    &restarted,
                    &database_request(ImsOperation::GetNext, 99, &[])
                )
                .unwrap()
                .segments[0]
                .data,
            b"01A"
        );
        for (index, context) in [
            ImsExecutionContext::DbDc,
            ImsExecutionContext::Dbctl,
            ImsExecutionContext::Dcctl,
            ImsExecutionContext::TmBatch,
        ]
        .into_iter()
        .enumerate()
        {
            let mut r = call(
                20 + index as u64,
                ImsRecoveryCall::BasicCheckpoint {
                    id: "CONTEXT".into(),
                },
            );
            r.context = context;
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let mut r = call(
            25,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        r.syntax = ImsCallSyntax::Command;
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn missing_saved_segment_reports_gu_ge_and_gn_continues_after_deleted_key() {
    backends("checkpoint-deleted", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        service
            .execute(&invocation, &get(1, 20, b"02", true))
            .unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "DELETED".into(),
                user_areas: vec![],
            },
        );
        service
            .execute(&invocation, &get(1, 21, b"02", true))
            .unwrap();
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Delete, 22, &[]),
            )
            .unwrap();
        let restarted = next_execution(&invocation, "deleted-restart");
        assert_eq!(
            invoke_call(
                &service,
                &store,
                &restarted,
                3,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("DELETED".into()),
                    area_lengths: vec![]
                }
            ),
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("DELETED".into()),
                user_areas: vec![],
                pcb_statuses: vec![(1, "GE".into())]
            }
        );
        assert_eq!(
            service
                .execute(
                    &restarted,
                    &database_request(ImsOperation::GetNext, 23, &[])
                )
                .unwrap()
                .segments[0]
                .data,
            b"03C"
        );
        let rows = store
            .list_provider_state("ims-v1-session-index", 64)
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&rows[0].payload).unwrap();
        assert_eq!(json["value"]["system"]["statuses"]["1"], "  ");
    });
}

#[test]
fn checkpoint_faults_and_authoritative_observation_never_redispatch_unknown_mutations() {
    backends("checkpoint-fault", |store| {
        let faults = Arc::new(FaultRows {
            inner: store.clone(),
            mode: AtomicU8::new(0),
        });
        let service = open(faults.clone());
        let invocation = invocation();
        let mut writer = invocation.clone();
        writer.service_class = ServiceClass::Interactive;
        seed(&service, &writer);
        for (mode, sequence, problem) in [
            (1, 1, HostProblem::ResourceExhausted),
            (3, 2, HostProblem::IdempotencyConflict),
        ] {
            let r = call(
                sequence,
                ImsRecoveryCall::BasicCheckpoint { id: "FAULT".into() },
            );
            intent(&*store, &invocation, &r);
            let before = snapshot(&*store);
            faults.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(problem)
            );
            assert_eq!(snapshot(&*store), before);
            assert_eq!(
                service
                    .observe_application_recovery(&invocation, &r)
                    .unwrap(),
                None
            );
        }
        let r = call(3, ImsRecoveryCall::BasicCheckpoint { id: "FAULT".into() });
        intent(&*store, &invocation, &r);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
        let expected = ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "FAULT".into(),
            sequence: 1,
        };
        assert_eq!(
            service
                .observe_application_recovery(&invocation, &r)
                .unwrap(),
            Some(expected.clone())
        );
        let mut effect = store.effect(&r.mutation.idempotency_key).unwrap().unwrap();
        effect.state = EffectState::UnknownOutcome;
        effect.result_digest = Some(
            mainframe_env_host_api::canonical_result_digest(&Err(HostProblem::UnknownOutcome))
                .unwrap(),
        );
        store
            .record_result(&r.mutation.idempotency_key, effect)
            .unwrap();
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            service
                .observe_application_recovery(&invocation, &r)
                .unwrap(),
            Some(expected.clone())
        );
        assert_eq!(snapshot(&*store), before);
        // Explicit unknown-result reconciliation preserves the digest domain.
        // The stale-intent lease path below covers an orphan with no result.
        store
            .reconcile_unknown_versioned(
                &r.mutation.idempotency_key,
                EffectState::Completed,
                EffectDigestFormat::CanonicalHostV1,
                mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::ImsRecovery(
                    expected.clone(),
                )))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(snapshot(&*store), before);
        let ambiguous = call(
            4,
            ImsRecoveryCall::BasicCheckpoint {
                id: "ABSENT".into(),
            },
        );
        intent(&*store, &invocation, &ambiguous);
        assert_eq!(
            service
                .observe_application_recovery(&invocation, &ambiguous)
                .unwrap(),
            None
        );
        let mut changed = r.clone();
        changed.call = ImsRecoveryCall::BasicCheckpoint {
            id: "CHANGED".into(),
        };
        assert_eq!(
            service.observe_application_recovery(&invocation, &changed),
            Err(HostProblem::IdempotencyConflict)
        );
        let orphan = call(
            5,
            ImsRecoveryCall::BasicCheckpoint {
                id: "ORPHAN".into(),
            },
        );
        intent(&*store, &invocation, &orphan);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &orphan),
            Err(HostProblem::UnknownOutcome)
        );
        let observed = service
            .observe_application_recovery(&invocation, &orphan)
            .unwrap()
            .unwrap();
        let before = snapshot(&*store);
        let claimed = store
            .claim_stale_intent(
                &orphan.mutation.idempotency_key,
                5,
                "checkpoint-resolver",
                101,
                1,
                10,
            )
            .unwrap();
        let lease = claimed.intent.recovery_lease.unwrap();
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &orphan),
            Err(HostProblem::UnknownOutcome)
        );
        store
            .reconcile_stale_intent(
                &orphan.mutation.idempotency_key,
                "checkpoint-resolver",
                lease.epoch,
                101,
                EffectState::Completed,
                EffectDigestFormat::CanonicalHostV1,
                mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::ImsRecovery(
                    observed,
                )))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn sqlite_checkpoint_process_exit_retains_areas_positions_and_exact_replay() {
    let path = std::env::temp_dir().join(format!(
        "ims-checkpoint-process-{}.sqlite",
        std::process::id()
    ));
    for phase in ["checkpoint", "restart", "resume"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "checkpoint_tests::sqlite_checkpoint_process_worker",
                "--nocapture",
            ])
            .env("IMS_CHECKPOINT_TEST_PATH", &path)
            .env("IMS_CHECKPOINT_TEST_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sqlite_checkpoint_process_worker() {
    let Ok(path) = std::env::var("IMS_CHECKPOINT_TEST_PATH") else {
        return;
    };
    let store: Arc<dyn TestStore> = Arc::new(
        SqliteStateStore::open(&format!("sqlite:{path}?mode=rwc"), 64 * 1024 * 1024, 4096).unwrap(),
    );
    let service = open(store.clone());
    let invocation = invocation();
    if std::env::var("IMS_CHECKPOINT_TEST_PHASE").unwrap() == "checkpoint" {
        seed(&service, &invocation);
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        service
            .execute(&invocation, &get(1, 20, b"02", true))
            .unwrap();
        invoke_call(
            &service,
            &store,
            &invocation,
            2,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "PROCESS".into(),
                user_areas: vec![b"XYZ".to_vec()],
            },
        );
    } else {
        let restarted = next_execution(&invocation, "process-restart");
        let r = call(
            3,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("PROCESS".into()),
                area_lengths: vec![3],
            },
        );
        if std::env::var("IMS_CHECKPOINT_TEST_PHASE").unwrap() == "restart" {
            intent(&*store, &restarted, &r);
        }
        let expected = ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("PROCESS".into()),
            user_areas: vec![b"XYZ".to_vec()],
            pcb_statuses: vec![(1, "  ".into())],
        };
        assert_eq!(
            dispatch(service.clone(), store.clone(), &restarted, &r),
            Ok(expected)
        );
        if std::env::var("IMS_CHECKPOINT_TEST_PHASE").unwrap() == "resume" {
            let before = snapshot(&*store);
            dispatch(service.clone(), store.clone(), &restarted, &r).unwrap();
            assert_eq!(snapshot(&*store), before);
            assert_eq!(
                service
                    .execute(
                        &restarted,
                        &database_request(ImsOperation::GetNext, 21, &[])
                    )
                    .unwrap()
                    .segments[0]
                    .data,
                b"03C"
            );
        }
    }
}

#[test]
fn seven_bounded_areas_round_trip_and_excess_or_zero_lengths_never_mutate() {
    backends("checkpoint-bounds", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke_call(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        for (sequence, areas) in [
            (2, vec![vec![1]; 8]),
            (3, vec![vec![0; 32 * 1024 + 1]]),
            (4, vec![vec![]]),
        ] {
            let r = call(
                sequence,
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "LIMIT".into(),
                    user_areas: areas,
                },
            );
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(HostProblem::ResourceExhausted)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let areas = (0..7).map(|byte| vec![byte; 32 * 1024]).collect::<Vec<_>>();
        invoke_call(
            &service,
            &store,
            &invocation,
            5,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "SEVEN".into(),
                user_areas: areas.clone(),
            },
        );
        let restarted = next_execution(&invocation, "seven-restart");
        let result = invoke_call(
            &service,
            &store,
            &restarted,
            6,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("SEVEN".into()),
                area_lengths: vec![32 * 1024; 7],
            },
        );
        let ImsRecoveryResult::Restarted {
            user_areas,
            checkpoint_id,
            ..
        } = result
        else {
            panic!()
        };
        assert_eq!(checkpoint_id.as_deref(), Some("SEVEN"));
        assert_eq!(user_areas, areas);
    });
}

#[test]
fn checkpoint_saf_corruption_and_backward_session_readers_fail_closed() {
    backends("checkpoint-readers", |store| {
        let initial = open(store.clone());
        let invocation = invocation();
        seed(&initial, &invocation);
        let r = call(
            1,
            ImsRecoveryCall::BasicCheckpoint {
                id: "DENIED".into(),
            },
        );
        intent(&*store, &invocation, &r);
        for problem in [
            HostProblem::Unauthorized,
            HostProblem::InfrastructureFailure,
        ] {
            let service = ImsService::open_authorized(
                store.clone(),
                ImsLimits::default(),
                Arc::new(Deny(problem.clone())),
            )
            .unwrap();
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(service, store.clone(), &invocation, &r),
                Err(problem)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let original = store
            .list_provider_state("ims-v1-session-index", 64)
            .unwrap()
            .remove(0);
        let mut old: serde_json::Value = serde_json::from_slice(&original.payload).unwrap();
        old["value"].as_object_mut().unwrap().remove("recovery");
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: original.version + 1,
                    payload: serde_json::to_vec(&old).unwrap(),
                    ..original.clone()
                },
                Some(original.version),
            )
            .unwrap();
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Ok(ImsRecoveryResult::Checkpointed {
                status: "  ".into(),
                id: "DENIED".into(),
                sequence: 1
            })
        );
        let restart = call(
            2,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        );
        intent(&*store, &invocation, &restart);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &restart),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&*store), before);
        let mut recovery = recovery_rows(&*store).remove(0);
        let old_version = recovery.version;
        let mut body: serde_json::Value = serde_json::from_slice(&recovery.payload).unwrap();
        body["checkpoints"]["DENIED"]["request"]["id"] = "ALTERED".into();
        recovery.version += 1;
        recovery.payload = serde_json::to_vec(&body).unwrap();
        store
            .put_provider_state(recovery, Some(old_version))
            .unwrap();
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::ProviderFailure)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn basic_checkpoint_commits_real_undo_releases_position_and_replays_without_committing_later_work()
{
    backends("checkpoint-basic", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        let mut writer = invocation.clone();
        writer.service_class = ServiceClass::Interactive;
        seed(&service, &writer);
        assert_eq!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .len(),
            1
        );
        let expected = ImsRecoveryResult::Checkpointed {
            status: "  ".into(),
            id: "BASIC01".into(),
            sequence: 1,
        };
        let r = call(
            1,
            ImsRecoveryCall::BasicCheckpoint {
                id: "BASIC01".into(),
            },
        );
        intent(&*store, &invocation, &r);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Ok(expected.clone())
        );
        assert!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
        let gn = service
            .execute(
                &invocation,
                &database_request(ImsOperation::GetNext, 14, &[]),
            )
            .unwrap();
        assert_eq!(gn.segments[0].data, b"01A");
        service
            .execute(&writer, &database_request(ImsOperation::Insert, 15, b"04D"))
            .unwrap();
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Ok(expected)
        );
        assert_eq!(snapshot(&*store), before);
        service
            .execute(&writer, &database_request(ImsOperation::Rollback, 16, &[]))
            .unwrap();
        let all = service
            .execute(
                &invocation,
                &database_request(ImsOperation::Unload, 17, &[]),
            )
            .unwrap();
        assert_eq!(
            all.segments
                .iter()
                .map(|s| s.data.clone())
                .collect::<Vec<_>>(),
            vec![b"01A".to_vec(), b"02B".to_vec(), b"03C".to_vec()]
        );
    });
}

#[test]
fn symbolic_checkpoint_restores_bounded_areas_and_real_gu_gn_position_after_new_execution() {
    backends("checkpoint-symbolic", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        assert_eq!(
            invoke_call(
                &service,
                &store,
                &invocation,
                1,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Normal,
                    area_lengths: vec![3, 2]
                }
            ),
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: None,
                user_areas: vec![],
                pcb_statuses: vec![]
            }
        );
        let mut gu = database_request(ImsOperation::GetHoldUnique, 20, &[]);
        gu.segments = vec!["ROOT".into()];
        gu.qualifiers = vec![mainframe_env_host_api::ImsQualifier {
            segment: "ROOT".into(),
            field: "KEY".into(),
            value: b"02".to_vec(),
        }];
        assert_eq!(
            service.execute(&invocation, &gu).unwrap().segments[0].data,
            b"02B"
        );
        assert_eq!(
            invoke_call(
                &service,
                &store,
                &invocation,
                2,
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "SAVE02".into(),
                    user_areas: vec![vec![0, 255, 10], b"XY".to_vec()]
                }
            ),
            ImsRecoveryResult::Checkpointed {
                status: "  ".into(),
                id: "SAVE02".into(),
                sequence: 2
            }
        );
        assert_eq!(
            service
                .execute(
                    &invocation,
                    &database_request(ImsOperation::GetNext, 21, &[])
                )
                .unwrap()
                .segments[0]
                .data,
            b"01A"
        );
        let mut restarted = invocation.clone();
        restarted.execution_id =
            ExecutionId::new("restart-execution", InvocationLimits::default()).unwrap();
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        let r = call(
            3,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Checkpoint("SAVE02".into()),
                area_lengths: vec![3],
            },
        );
        intent(&*store, &restarted, &r);
        let expected = ImsRecoveryResult::Restarted {
            status: "  ".into(),
            checkpoint_id: Some("SAVE02".into()),
            user_areas: vec![vec![0, 255, 10]],
            pcb_statuses: vec![(1, "  ".into())],
        };
        assert_eq!(
            dispatch(service.clone(), store.clone(), &restarted, &r),
            Ok(expected.clone())
        );
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &restarted, &r),
            Ok(expected)
        );
        assert_eq!(snapshot(&*store), before);
        assert_eq!(
            service
                .execute(
                    &restarted,
                    &database_request(ImsOperation::GetNext, 22, &[])
                )
                .unwrap()
                .segments[0]
                .data,
            b"03C"
        );
        gu.mutation = database_request(ImsOperation::GetUnique, 23, &[]).mutation;
        assert_eq!(
            service.execute(&restarted, &gu).unwrap().segments[0].data,
            b"02B"
        );
    });
}
