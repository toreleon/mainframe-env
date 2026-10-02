use super::*;
use mainframe_env_host_api::{ImsOperation, ImsQualifier};

#[path = "backout_fault_tests.rs"]
mod fault_tests;
#[path = "backout_scope_tests.rs"]
mod scope_tests;

fn snapshot(store: &dyn ProviderStateStore) -> Vec<Vec<ProviderStateRecord>> {
    checkpoint_tests::snapshot(store)
}

fn condition(status: &str) -> ImsRecoveryResult {
    ImsRecoveryResult::BackedOut {
        status: status.into(),
        user_data: vec![],
    }
}

fn call(sequence: u64, operation: ImsRecoveryCall) -> ImsRecoveryRequest {
    ImsRecoveryRequest {
        call: operation,
        ..request(sequence)
    }
}

fn invoke(
    service: &Arc<ImsService>,
    store: &Arc<dyn TestStore>,
    invocation: &Invocation,
    sequence: u64,
    operation: ImsRecoveryCall,
) -> ImsRecoveryResult {
    let request = call(sequence, operation);
    intent(&**store, invocation, &request);
    dispatch(service.clone(), store.clone(), invocation, &request).unwrap()
}

fn seed(service: &ImsService, invocation: &Invocation) {
    for (op, seq, bytes) in [
        (ImsOperation::Schedule, 100, &b""[..]),
        (ImsOperation::Insert, 101, &b"01A"[..]),
        (ImsOperation::Commit, 102, &b""[..]),
    ] {
        assert_eq!(
            service
                .execute(invocation, &database_request(op, seq, bytes))
                .unwrap()
                .status,
            "  "
        );
    }
}

fn data(service: &ImsService, invocation: &Invocation) -> Vec<Vec<u8>> {
    let mut request = database_request(ImsOperation::Unload, 999, &[]);
    request.psb = Some("LOGDB".into());
    service
        .execute(invocation, &request)
        .unwrap()
        .segments
        .into_iter()
        .map(|s| s.data)
        .collect()
}

fn insert(service: &ImsService, invocation: &Invocation, seq: u64, bytes: &[u8]) {
    assert_eq!(
        service
            .execute(
                invocation,
                &database_request(ImsOperation::Insert, seq, bytes)
            )
            .unwrap()
            .status,
        "  "
    );
}

fn point(token: [u8; 4], bytes: &[u8]) -> ImsRecoveryCall {
    ImsRecoveryCall::Sets {
        token: Some(token),
        user_data: Some(bytes.to_vec()),
    }
}

fn rols(token: [u8; 4], size: usize) -> ImsRecoveryCall {
    ImsRecoveryCall::Rols {
        token: Some(token),
        area_length: Some(size),
    }
}

#[test]
fn backout_real_post_checkpoint_batch_updates_roll_back_to_current_commit_only() {
    backends("backout-commit", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::BasicCheckpoint { id: "FIRST".into() },
        );
        insert(&service, &invocation, 103, b"02B");
        assert_eq!(
            data(&service, &invocation),
            vec![b"01A".to_vec(), b"02B".to_vec()]
        );
        assert_eq!(
            invoke(&service, &store, &invocation, 2, ImsRecoveryCall::Rolb),
            ImsRecoveryResult::BackedOut {
                status: "  ".into(),
                user_data: vec![]
            }
        );
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        insert(&service, &invocation, 104, b"03C");
        invoke(
            &service,
            &store,
            &invocation,
            3,
            ImsRecoveryCall::BasicCheckpoint {
                id: "SECOND".into(),
            },
        );
        insert(&service, &invocation, 105, b"04D");
        invoke(&service, &store, &invocation, 4, ImsRecoveryCall::Rolb);
        assert_eq!(
            data(&service, &invocation),
            vec![b"01A".to_vec(), b"03C".to_vec()]
        );
    });
}

#[test]
fn backout_savepoints_use_actual_images_and_replay_never_undoes_later_work() {
    backends("backout-point", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        assert_eq!(
            invoke(&service, &store, &invocation, 1, point(*b"ONE1", b"saved")),
            ImsRecoveryResult::Savepoint {
                status: "  ".into()
            }
        );
        insert(&service, &invocation, 103, b"02B");
        invoke(&service, &store, &invocation, 2, point(*b"TWO2", b"inner"));
        insert(&service, &invocation, 104, b"03C");
        assert_eq!(
            invoke(&service, &store, &invocation, 3, rols(*b"ONE1", 5)),
            ImsRecoveryResult::BackedOut {
                status: "  ".into(),
                user_data: b"saved".to_vec()
            }
        );
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        insert(&service, &invocation, 105, b"04D");
        let replay = call(3, rols(*b"ONE1", 5));
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &replay).unwrap(),
            ImsRecoveryResult::BackedOut {
                status: "  ".into(),
                user_data: b"saved".to_vec()
            }
        );
        assert_eq!(
            data(&service, &invocation),
            vec![b"01A".to_vec(), b"04D".to_vec()]
        );
        assert_eq!(
            invoke(&service, &store, &invocation, 4, rols(*b"TWO2", 5)),
            ImsRecoveryResult::BackedOut {
                status: "RA".into(),
                user_data: vec![]
            }
        );
    });
}

#[test]
fn backout_replaced_cancelled_and_nine_bounded_points_have_source_statuses() {
    backends("backout-capacity", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        for i in 0u8..9 {
            assert_eq!(
                invoke(
                    &service,
                    &store,
                    &invocation,
                    u64::from(i) + 1,
                    point([0, i, 0xff, b' '], &[])
                ),
                ImsRecoveryResult::Savepoint {
                    status: "  ".into()
                }
            );
        }
        let images = store
            .list_provider_state("ims-v1-generic-database", 64)
            .unwrap();
        let undo = store
            .list_provider_state("ims-v1-generic-unit-of-work", 64)
            .unwrap();
        assert_eq!(
            invoke(&service, &store, &invocation, 10, point(*b"FULL", &[])),
            ImsRecoveryResult::Savepoint {
                status: "SB".into()
            }
        );
        // Condition receipts may add replay and CAS fences, never change protected payloads.
        assert_eq!(
            store
                .list_provider_state("ims-v1-generic-database", 64)
                .unwrap()
                .iter()
                .map(|r| &r.payload)
                .collect::<Vec<_>>(),
            images.iter().map(|r| &r.payload).collect::<Vec<_>>()
        );
        assert_eq!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap(),
            undo
        );
        insert(&service, &invocation, 103, b"02B");
        invoke(
            &service,
            &store,
            &invocation,
            11,
            point([0, 0, 0xff, b' '], b"replaced"),
        );
        insert(&service, &invocation, 104, b"03C");
        assert_eq!(
            invoke(
                &service,
                &store,
                &invocation,
                12,
                rols([0, 0, 0xff, b' '], 8)
            ),
            ImsRecoveryResult::BackedOut {
                status: "  ".into(),
                user_data: b"replaced".to_vec()
            }
        );
        assert_eq!(
            data(&service, &invocation),
            vec![b"01A".to_vec(), b"02B".to_vec()]
        );
        assert_eq!(
            invoke(
                &service,
                &store,
                &invocation,
                13,
                rols([0, 8, 0xff, b' '], 0)
            ),
            condition("RA")
        );
        invoke(
            &service,
            &store,
            &invocation,
            14,
            ImsRecoveryCall::Setu {
                token: None,
                user_data: None,
            },
        );
        assert_eq!(
            invoke(
                &service,
                &store,
                &invocation,
                15,
                rols([0, 0, 0xff, b' '], 8)
            ),
            condition("RA")
        );
        invoke(&service, &store, &invocation, 16, ImsRecoveryCall::Rolb);
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
    });
}

#[test]
fn backout_commit_epochs_and_foreign_uow_cannot_restore_an_old_point() {
    backends("backout-foreign", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke(&service, &store, &invocation, 1, point(*b"OLD1", &[]));
        let mut foreign = invocation.clone();
        foreign.run_unit_id = RunUnitId::new("foreign-run", InvocationLimits::default()).unwrap();
        service
            .execute(
                &foreign,
                &database_request(ImsOperation::Schedule, 200, &[]),
            )
            .unwrap();
        assert_eq!(
            service.execute(
                &foreign,
                &database_request(ImsOperation::Insert, 201, b"02F")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        insert(&service, &invocation, 103, b"03C");
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Commit, 104, &[]),
            )
            .unwrap();
        service
            .execute(
                &foreign,
                &database_request(ImsOperation::Insert, 202, b"02F"),
            )
            .unwrap();
        service
            .execute(&foreign, &database_request(ImsOperation::Commit, 203, &[]))
            .unwrap();
        let reopened =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                .unwrap();
        assert_eq!(
            invoke(&reopened, &store, &invocation, 2, rols(*b"OLD1", 0)),
            condition("RA")
        );
        invoke(&reopened, &store, &invocation, 3, ImsRecoveryCall::Rolb);
        assert_eq!(
            data(&reopened, &invocation),
            vec![b"01A".to_vec(), b"02F".to_vec(), b"03C".to_vec()]
        );
    });
}

#[test]
fn backout_rescheduled_run_cannot_reuse_prior_session_points() {
    backends("backout-incarnation", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke(&service, &store, &invocation, 1, point(*b"OLD!", &[]));
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Terminate, 103, &[]),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &database_request(ImsOperation::Schedule, 104, &[]),
            )
            .unwrap();
        insert(&service, &invocation, 105, b"02B");
        assert_eq!(
            invoke(&service, &store, &invocation, 2, rols(*b"OLD!", 0)),
            condition("RA")
        );
        assert_eq!(
            data(&service, &invocation),
            vec![b"01A".to_vec(), b"02B".to_vec()]
        );
        invoke(&service, &store, &invocation, 3, ImsRecoveryCall::Rolb);
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
    });
}

#[test]
fn backout_operand_binding_saf_and_replay_rejections_publish_no_rows() {
    backends("backout-negative", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke(&service, &store, &invocation, 1, point(*b"GOOD", b"saved"));
        insert(&service, &invocation, 103, b"02B");
        let before = snapshot(&*store);
        let mut r = call(2, rols(*b"GOOD", 4));
        intent(&*store, &invocation, &r);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&*store), before);
        for (index, operation) in [
            ImsRecoveryCall::Sets {
                token: Some(*b"GOOD"),
                user_data: None,
            },
            ImsRecoveryCall::Setu {
                token: None,
                user_data: Some(vec![]),
            },
            ImsRecoveryCall::Rols {
                token: None,
                area_length: Some(0),
            },
        ]
        .into_iter()
        .enumerate()
        {
            r = call(index as u64 + 3, operation);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(HostProblem::Malformed)
            );
            assert_eq!(snapshot(&*store), before);
        }
        r = call(8, rols(*b"GOOD", 5));
        intent(&*store, &invocation, &r);
        let denied = ImsService::open_authorized(
            store.clone(),
            ImsLimits::default(),
            Arc::new(DenyDatabase),
        )
        .unwrap();
        assert_eq!(
            dispatch(denied, store.clone(), &invocation, &r),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&*store), before);
        let mut changed = r.clone();
        changed.call = rols(*b"EVIL", 5);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &changed),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(snapshot(&*store), before);
        changed = r.clone();
        changed.context = ImsExecutionContext::DbDc;
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &changed),
            Err(HostProblem::Unsupported)
        );
        changed = r.clone();
        changed.syntax = ImsCallSyntax::Command;
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &changed),
            Err(HostProblem::Unsupported)
        );
        changed = r.clone();
        changed.mutation.transaction = Some("OUTER".into());
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &changed),
            Err(HostProblem::Unsupported)
        );
        store
            .claim_stale_intent(&r.mutation.idempotency_key, 8, "backout-worker", 101, 1, 10)
            .unwrap();
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn backout_roll_and_tokenless_rols_restore_then_persist_terminal_disposition() {
    for (name, call, code) in [
        ("roll", ImsRecoveryCall::Roll, "U0778"),
        (
            "tokenless",
            ImsRecoveryCall::Rols {
                token: None,
                area_length: None,
            },
            "U3303",
        ),
    ] {
        backends(name, |store| {
            let service = open(store.clone());
            let invocation = invocation();
            seed(&service, &invocation);
            insert(&service, &invocation, 103, b"02B");
            let expected = ImsRecoveryResult::Abended { code: code.into() };
            assert_eq!(
                invoke(&service, &store, &invocation, 1, call.clone()),
                expected
            );
            let before = snapshot(&*store);
            let reopened =
                ImsService::open_authorized(store.clone(), ImsLimits::default(), Arc::new(Allow))
                    .unwrap();
            assert_eq!(
                dispatch(
                    reopened.clone(),
                    store.clone(),
                    &invocation,
                    &self::call(1, call.clone())
                ),
                Ok(expected)
            );
            assert_eq!(
                reopened.execute(
                    &invocation,
                    &database_request(ImsOperation::Insert, 104, b"03C")
                ),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(snapshot(&*store), before);
            // Unload is an administrative image read; use a distinct run after termination.
            let mut reader = invocation.clone();
            reader.run_unit_id =
                RunUnitId::new("backout-reader", InvocationLimits::default()).unwrap();
            assert_eq!(data(&reopened, &reader), vec![b"01A".to_vec()]);
        });
    }
}

#[test]
fn backout_terminal_keeps_old_log_replay_and_rejects_new_log_without_mutation() {
    backends("backout-log-terminal", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        let old = invoke(
            &service,
            &store,
            &invocation,
            1,
            ImsRecoveryCall::Log {
                code: 0xa0,
                data: b"retained".to_vec(),
            },
        );
        invoke(&service, &store, &invocation, 2, ImsRecoveryCall::Roll);
        let before = snapshot(&*store);
        assert_eq!(
            dispatch(
                service.clone(),
                store.clone(),
                &invocation,
                &call(
                    1,
                    ImsRecoveryCall::Log {
                        code: 0xa0,
                        data: b"retained".to_vec()
                    }
                )
            ),
            Ok(old)
        );
        assert_eq!(snapshot(&*store), before);
        let new = call(
            3,
            ImsRecoveryCall::Log {
                code: 0xa0,
                data: b"later".to_vec(),
            },
        );
        intent(&*store, &invocation, &new);
        assert_eq!(
            dispatch(service, store.clone(), &invocation, &new),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn backout_intermediate_loses_pcb_hold_but_keeps_shared_q_until_commit_boundary() {
    backends("backout-q", |store| {
        let service = open(store.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        let mut hold = database_request(ImsOperation::GetHoldUnique, 103, &[]);
        hold.segments = vec!["ROOT".into()];
        hold.qualifiers = vec![ImsQualifier {
            segment: "ROOT".into(),
            field: "KEY".into(),
            value: b"01".to_vec(),
        }];
        hold.q_class = mainframe_env_host_api::ImsQClass::new(b'A');
        assert_eq!(service.execute(&invocation, &hold).unwrap().status, "  ");
        invoke(&service, &store, &invocation, 1, point(*b"QPOS", &[]));
        let mut replace = database_request(ImsOperation::Replace, 104, b"01Z");
        replace.segments = vec!["ROOT".into()];
        assert_eq!(service.execute(&invocation, &replace).unwrap().status, "  ");
        invoke(&service, &store, &invocation, 2, rols(*b"QPOS", 0));
        replace.mutation = database_request(ImsOperation::Replace, 105, &[]).mutation;
        assert_eq!(service.execute(&invocation, &replace).unwrap().status, "DJ");
        let q = store.list_provider_state("ims-v1-system", 64).unwrap();
        assert!(!q.is_empty());
        let body: serde_json::Value = serde_json::from_slice(&q[0].payload).unwrap();
        assert_eq!(body["value"]["reservations"].as_object().unwrap().len(), 1);
        assert!(
            body["value"]["reservations"]
                .as_object()
                .unwrap()
                .values()
                .all(|r| r["current"] == false)
        );
        invoke(&service, &store, &invocation, 3, ImsRecoveryCall::Rolb);
        let q = store.list_provider_state("ims-v1-system", 64).unwrap();
        let body: serde_json::Value = serde_json::from_slice(&q[0].payload).unwrap();
        assert!(
            body["value"]["reservations"]
                .as_object()
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn backout_atomic_failure_and_lost_ack_have_actual_image_and_observation_evidence() {
    backends("backout-fault", |store| {
        let faults = Arc::new(FaultRows {
            inner: store.clone(),
            mode: AtomicU8::new(0),
        });
        let service = open(faults.clone());
        let invocation = invocation();
        seed(&service, &invocation);
        invoke(&service, &store, &invocation, 1, point(*b"SAVE", b"x"));
        insert(&service, &invocation, 103, b"02B");
        for (mode, sequence, problem) in [
            (1, 2, HostProblem::ResourceExhausted),
            (3, 3, HostProblem::IdempotencyConflict),
        ] {
            let r = call(sequence, rols(*b"SAVE", 1));
            intent(&*store, &invocation, &r);
            let before = snapshot(&*store);
            faults.mode.store(mode, Ordering::SeqCst);
            assert_eq!(
                dispatch(service.clone(), store.clone(), &invocation, &r),
                Err(problem)
            );
            assert_eq!(snapshot(&*store), before);
        }
        let r = call(4, rols(*b"SAVE", 1));
        intent(&*store, &invocation, &r);
        faults.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            dispatch(service.clone(), store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
        let expected = ImsRecoveryResult::BackedOut {
            status: "  ".into(),
            user_data: b"x".to_vec(),
        };
        assert_eq!(
            service
                .observe_application_recovery(&invocation, &r)
                .unwrap(),
            Some(expected)
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
            dispatch(service, store.clone(), &invocation, &r),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(snapshot(&*store), before);
    });
}

#[test]
fn sqlite_backout_child_process_recovery_restores_real_images_and_retains_replay() {
    let path =
        std::env::temp_dir().join(format!("ims-backout-child-{}.sqlite", std::process::id()));
    for phase in ["seed", "backout", "replay", "commit", "lostack", "unknown"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "backout_tests::sqlite_backout_process_worker",
                "--nocapture",
            ])
            .env("IMS_BACKOUT_TEST_PATH", &path)
            .env("IMS_BACKOUT_TEST_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn sqlite_backout_process_worker() {
    let Ok(path) = std::env::var("IMS_BACKOUT_TEST_PATH") else {
        return;
    };
    let store: Arc<dyn TestStore> = Arc::new(
        SqliteStateStore::open(&format!("sqlite:{path}?mode=rwc"), 64 * 1024 * 1024, 4096).unwrap(),
    );
    let service = open(store.clone());
    let invocation = invocation();
    match std::env::var("IMS_BACKOUT_TEST_PHASE").unwrap().as_str() {
        "seed" => {
            seed(&service, &invocation);
            invoke(&service, &store, &invocation, 1, point(*b"PROC", b"child"));
            insert(&service, &invocation, 103, b"02B");
        }
        "backout" => {
            assert_eq!(data(&service, &invocation).len(), 2);
            assert_eq!(
                invoke(&service, &store, &invocation, 2, rols(*b"PROC", 5)),
                ImsRecoveryResult::BackedOut {
                    status: "  ".into(),
                    user_data: b"child".to_vec()
                }
            );
            assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
            insert(&service, &invocation, 104, b"03C");
        }
        "replay" => {
            let before = snapshot(&*store);
            assert_eq!(
                dispatch(
                    service.clone(),
                    store.clone(),
                    &invocation,
                    &call(2, rols(*b"PROC", 5))
                )
                .unwrap(),
                ImsRecoveryResult::BackedOut {
                    status: "  ".into(),
                    user_data: b"child".to_vec()
                }
            );
            assert_eq!(snapshot(&*store), before);
            assert_eq!(data(&service, &invocation).len(), 2);
            invoke(&service, &store, &invocation, 3, ImsRecoveryCall::Rolb);
        }
        "commit" => {
            assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
            assert_eq!(
                invoke(&service, &store, &invocation, 4, rols(*b"PROC", 5)),
                condition("RA")
            );
        }
        "lostack" => {
            invoke(&service, &store, &invocation, 5, point(*b"ACK!", b"ack"));
            insert(&service, &invocation, 105, b"04D");
            let faults = Arc::new(FaultRows {
                inner: store.clone(),
                mode: AtomicU8::new(0),
            });
            let uncertain =
                ImsService::open_authorized(faults.clone(), ImsLimits::default(), Arc::new(Allow))
                    .unwrap();
            let request = call(6, rols(*b"ACK!", 3));
            intent(&*store, &invocation, &request);
            faults.mode.store(2, Ordering::SeqCst);
            assert_eq!(
                dispatch(uncertain, store.clone(), &invocation, &request),
                Err(HostProblem::UnknownOutcome)
            );
            let mut effect = store
                .effect(&request.mutation.idempotency_key)
                .unwrap()
                .unwrap();
            effect.state = EffectState::UnknownOutcome;
            effect.result_digest = Some(
                mainframe_env_host_api::canonical_result_digest(&Err(HostProblem::UnknownOutcome))
                    .unwrap(),
            );
            store
                .record_result(&request.mutation.idempotency_key, effect)
                .unwrap();
        }
        "unknown" => {
            let request = call(6, rols(*b"ACK!", 3));
            assert_eq!(data(&service, &invocation), vec![b"01A".to_vec()]);
            let before = snapshot(&*store);
            assert_eq!(
                service
                    .observe_application_recovery(&invocation, &request)
                    .unwrap(),
                Some(ImsRecoveryResult::BackedOut {
                    status: "  ".into(),
                    user_data: b"ack".to_vec()
                })
            );
            assert_eq!(
                dispatch(service, store.clone(), &invocation, &request),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(snapshot(&*store), before);
        }
        _ => panic!("unknown child phase"),
    }
}
