use super::*;
use std::sync::atomic::Ordering as AtomicOrdering;

fn invoke(
    service: &Arc<ImsService>,
    run: &str,
    req: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let invocation = invocation(run);
    let providers = ims_providers(service.clone(), InvocationLimits::default());
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: req.mutation.as_ref().unwrap().sequence,
        idempotency_key: req
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: HostRequest::Ims(req.clone()),
        deadline_tick: invocation.deadline_tick,
    };
    match providers[usize::from(req.operation.is_mutating())]
        .invoke(&invocation, effect)
        .outcome?
    {
        HostResult::Ims(result) => Ok(result),
        other => panic!("unexpected {other:?}"),
    }
}

fn backends(mut case: impl FnMut(Arc<dyn ProviderStateStore>)) {
    case(Arc::new(MemoryStore::new(Default::default())));
    let file = std::env::temp_dir().join(format!(
        "ims-q-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    case(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

fn deq(run: &str, sequence: u64, class: u8) -> ImsRequest {
    system_request(
        run,
        sequence,
        0,
        ImsExecutionContext::DbDc,
        ImsCallSyntax::Call,
        ImsSystemCall::Dequeue {
            class: ImsQClass::new(class),
        },
    )
}

fn released(service: &ImsService, run: &str, sequence: u64, class: u8) -> u32 {
    match execute(service, run, &deq(run, sequence, class))
        .system
        .unwrap()
    {
        ImsSystemResult::Dequeued { released } => released,
        other => panic!("unexpected {other:?}"),
    }
}

fn seeded(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    for run in ["reserve-a", "reserve-b"] {
        execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
    }
    execute(
        &service,
        "reserve-a",
        &request("reserve-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
    );
    execute(
        &service,
        "reserve-a",
        &request("reserve-a", ImsOperation::Insert, 3, &["ROOT"], b"B2Y"),
    );
    execute(
        &service,
        "reserve-a",
        &request("reserve-a", ImsOperation::Commit, 4, &[], b""),
    );
    service
}

fn get(run: &str, sequence: u64, key: &[u8], hold: bool, q: bool) -> ImsRequest {
    let mut req = request(
        run,
        if hold {
            ImsOperation::GetHoldUnique
        } else {
            ImsOperation::GetUnique
        },
        sequence,
        &["ROOT"],
        b"",
    );
    req.qualifiers.push(qualifier(key));
    if q {
        req.q_class = ImsQClass::new(b'A');
    }
    req
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SYSTEM_NAMESPACE,
        SESSION_NAMESPACE,
        REPLAY_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
    .collect()
}

fn bypass(store: Arc<dyn ProviderStateStore>, two_services: bool) {
    let owner = seeded(store.clone());
    execute(
        &owner,
        "reserve-a",
        &get("reserve-a", 5, b"A1", false, true),
    );
    let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    let writer = if two_services { &second } else { &owner };
    assert!(
        store
            .list_provider_state(GENERIC_PENDING_NAMESPACE, 64)
            .unwrap()
            .is_empty()
    );
    execute(
        writer,
        "reserve-b",
        &get("reserve-b", 2, b"A1", true, false),
    );
    for (sequence, op, data) in [
        (3, ImsOperation::Replace, b"A1Z".as_slice()),
        (4, ImsOperation::Delete, b"".as_slice()),
    ] {
        let before = rows(&*store);
        assert_eq!(
            invoke(
                writer,
                "reserve-b",
                &request("reserve-b", op, sequence, &[], data)
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
    }
}

#[test]
fn reservation_two_runs_memory() {
    bypass(Arc::new(MemoryStore::new(Default::default())), false);
}
#[test]
fn reservation_two_services_memory() {
    bypass(Arc::new(MemoryStore::new(Default::default())), true);
}

fn sqlite_bypass(two_services: bool) {
    let file = std::env::temp_dir().join(format!(
        "ims-q-fail-first-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    bypass(store, two_services);
    std::fs::remove_file(file).unwrap();
}
#[test]
fn reservation_two_runs_file_sqlite() {
    sqlite_bypass(false);
}
#[test]
fn reservation_two_services_file_sqlite() {
    sqlite_bypass(true);
}

#[test]
fn reservation_stale_refresh_granularity_authorization_and_load() {
    backends(|store| {
        let owner = seeded(store.clone());
        let policy = Arc::new(Policy::default());
        let stale =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), policy.clone())
                .unwrap();
        execute(
            &owner,
            "reserve-a",
            &get("reserve-a", 5, b"A1", false, true),
        );
        execute(
            &stale,
            "reserve-b",
            &get("reserve-b", 2, b"A1", true, false),
        );
        let replace = request("reserve-b", ImsOperation::Replace, 3, &[], b"A1X");
        *policy.deny_update.lock().unwrap() = true;
        let before = rows(&*store);
        assert_eq!(
            stale.execute(&invocation("reserve-b"), &replace),
            Err(HostProblem::Unauthorized)
        );
        *policy.deny_update.lock().unwrap() = false;
        assert_eq!(
            stale.execute(&invocation("reserve-b"), &replace),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        // Root Q protects its record without excluding an independent root.
        execute(
            &stale,
            "reserve-b",
            &get("reserve-b", 4, b"B2", true, false),
        );
        execute(
            &stale,
            "reserve-b",
            &request("reserve-b", ImsOperation::Replace, 5, &[], b"B2Z"),
        );
        execute(
            &stale,
            "reserve-b",
            &request("reserve-b", ImsOperation::Commit, 6, &[], b""),
        );
        let image = ImsGenericLoadImage {
            database: "GENDB".into(),
            records: vec![ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1X".to_vec(),
            }],
        };
        for run in ["reserve-a", "reserve-b"] {
            let before = rows(&*store);
            let load = request(
                run,
                ImsOperation::Load,
                8,
                &[],
                &serde_json::to_vec(&image).unwrap(),
            );
            assert_eq!(
                stale.execute(&invocation(run), &load),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(rows(&*store), before);
        }
        let before = rows(&*store);
        let malformed = request("reserve-b", ImsOperation::Load, 9, &[], b"invalid");
        assert_eq!(
            stale.execute(&invocation("reserve-b"), &malformed),
            Err(HostProblem::Malformed)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &stale,
            "reserve-a",
            &get("reserve-a", 6, b"A1", false, true),
        );
        *policy.deny_update.lock().unwrap() = true;
        let before = rows(&*store);
        for op in [
            ImsOperation::Commit,
            ImsOperation::Rollback,
            ImsOperation::Checkpoint,
            ImsOperation::Terminate,
        ] {
            assert_eq!(
                stale.execute(
                    &invocation("reserve-a"),
                    &request("reserve-a", op, 7, &[], b"")
                ),
                Err(HostProblem::Unauthorized)
            );
            assert_eq!(rows(&*store), before);
        }
        assert_eq!(
            stale.execute(&invocation("reserve-a"), &deq("reserve-a", 8, b'A')),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(rows(&*store), before);
    });
}

fn child_get(run: &str, sequence: u64, key: &[u8], hold: bool, q: bool) -> ImsRequest {
    let mut req = get(run, sequence, b"A1", hold, q);
    req.segments.push("CHILD".into());
    req.qualifiers.push(ImsQualifier {
        segment: "CHILD".into(),
        field: "CHILDKEY".into(),
        value: key.into(),
    });
    req
}

fn add_children(service: &ImsService) {
    for (sequence, data) in [(10, b"C1X"), (11, b"D2Y")] {
        let mut insert = request(
            "reserve-a",
            ImsOperation::Insert,
            sequence,
            &["CHILD"],
            data,
        );
        insert.qualifiers.push(qualifier(b"A1"));
        execute(service, "reserve-a", &insert);
    }
    execute(
        service,
        "reserve-a",
        &request("reserve-a", ImsOperation::Commit, 12, &[], b""),
    );
}

#[test]
fn reservation_root_and_dependent_scope_subtree_delete_and_record_position() {
    backends(|store| {
        let service = seeded(store.clone());
        add_children(&service);
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 13, b"A1", false, true),
        );
        execute(
            &service,
            "reserve-b",
            &child_get("reserve-b", 2, b"C1", true, false),
        );
        let before = rows(&*store);
        assert_eq!(
            service.execute(
                &invocation("reserve-b"),
                &request("reserve-b", ImsOperation::Replace, 3, &[], b"C1Z")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &service,
            "reserve-a",
            &child_get("reserve-a", 14, b"C1", false, false),
        );
        assert_eq!(released(&service, "reserve-a", 15, b'A'), 0);
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 16, b"B2", false, false),
        );
        assert_eq!(released(&service, "reserve-a", 17, b'B'), 0);
        assert_eq!(released(&service, "reserve-a", 18, b'A'), 1);
        execute(
            &service,
            "reserve-a",
            &child_get("reserve-a", 19, b"C1", false, true),
        );
        execute(
            &service,
            "reserve-b",
            &child_get("reserve-b", 4, b"D2", true, false),
        );
        execute(
            &service,
            "reserve-b",
            &request("reserve-b", ImsOperation::Replace, 5, &[], b"D2Z"),
        );
        execute(
            &service,
            "reserve-b",
            &request("reserve-b", ImsOperation::Commit, 6, &[], b""),
        );
        execute(
            &service,
            "reserve-b",
            &get("reserve-b", 7, b"A1", true, false),
        );
        let before = rows(&*store);
        assert_eq!(
            service.execute(
                &invocation("reserve-b"),
                &request("reserve-b", ImsOperation::Delete, 8, &[], b"")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn reservation_owner_modified_survives_reacquisition_and_other_pcb() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let mut metadata = catalog();
        let mut second = metadata.psbs[0].pcbs[0].clone();
        let ImsPcbMetadata::Database(pcb) = &mut second else {
            unreachable!()
        };
        pcb.name = "SECOND".into();
        metadata.psbs[0].pcbs.push(second);
        service.install_metadata(metadata).unwrap();
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Schedule, 1, &[], b""),
        );
        for (seq, data) in [(2, b"A1X"), (3, b"B2Y")] {
            execute(
                &service,
                "reserve-a",
                &request("reserve-a", ImsOperation::Insert, seq, &["ROOT"], data),
            );
        }
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Commit, 4, &[], b""),
        );
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 5, b"A1", false, true),
        );
        let mut hold = get("reserve-a", 6, b"A1", true, false);
        hold.pcb = 2;
        execute(&service, "reserve-a", &hold);
        let mut replace = request("reserve-a", ImsOperation::Replace, 7, &[], b"A1Z");
        replace.pcb = 2;
        execute(&service, "reserve-a", &replace);
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 8, b"A1", false, true),
        );
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 9, b"B2", false, false),
        );
        assert_eq!(released(&service, "reserve-a", 10, b'A'), 0);
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Rollback, 11, &[], b""),
        );
        assert_eq!(system::reservation_count(&service.lock().unwrap().state), 0);
        assert_eq!(
            execute(
                &service,
                "reserve-a",
                &get("reserve-a", 12, b"A1", false, false)
            )
            .segments[0]
                .data,
            b"A1X"
        );
    });
}

#[test]
fn reservation_settlement_releases_atomically_and_replay_does_not_release_new_q() {
    for op in [
        ImsOperation::Checkpoint,
        ImsOperation::Commit,
        ImsOperation::Rollback,
        ImsOperation::Terminate,
    ] {
        backends(|store| {
            let service = seeded(store.clone());
            execute(
                &service,
                "reserve-a",
                &get("reserve-a", 20, b"A1", false, true),
            );
            let settle = request("reserve-a", op, 21, &[], b"");
            execute(&service, "reserve-a", &settle);
            assert_eq!(system::reservation_count(&service.lock().unwrap().state), 0);
            if op == ImsOperation::Terminate {
                execute(
                    &service,
                    "reserve-a",
                    &request("reserve-a", ImsOperation::Schedule, 22, &[], b""),
                );
            }
            let mut again = get("reserve-a", 23, b"A1", false, true);
            // Different canonical identity on subsequent loop iterations.
            again.mutation.as_mut().unwrap().idempotency_key =
                IdempotencyKey::new(format!("again-{op:?}"), InvocationLimits::default()).unwrap();
            execute(&service, "reserve-a", &again);
            let before = rows(&*store);
            execute(&service, "reserve-a", &settle);
            assert_eq!(rows(&*store), before);
            execute(
                &service,
                "reserve-a",
                &request("reserve-a", ImsOperation::Commit, 24, &[], b""),
            );
        });
    }
}

#[test]
fn reservation_acquisition_and_write_races_have_one_cas_winner() {
    for reserve_first in [false, true] {
        backends(|inner| {
            let store = isolation_tests::InterceptStore::new(inner);
            let first = seeded(store.clone());
            let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            execute(
                &second,
                "reserve-b",
                &get("reserve-b", 2, b"A1", true, false),
            );
            let q = get("reserve-a", 5, b"A1", false, true);
            let write = request("reserve-b", ImsOperation::Replace, 3, &[], b"A1Z");
            store.mode.store(1, AtomicOrdering::SeqCst);
            let blocked_request = if reserve_first {
                q.clone()
            } else {
                write.clone()
            };
            let blocked_run = if reserve_first {
                "reserve-a"
            } else {
                "reserve-b"
            };
            let thread = std::thread::spawn(move || {
                first.execute(
                    &invocation_class(blocked_run, ServiceClass::Batch),
                    &blocked_request,
                )
            });
            store.entered.wait();
            let winner_run = if reserve_first {
                "reserve-b"
            } else {
                "reserve-a"
            };
            let winner_req = if reserve_first { &write } else { &q };
            assert_eq!(
                second
                    .execute(
                        &invocation_class(winner_run, ServiceClass::Batch),
                        winner_req
                    )
                    .unwrap()
                    .status,
                "  "
            );
            store.release.wait();
            assert_eq!(
                thread.join().unwrap(),
                Err(HostProblem::IdempotencyConflict)
            );
            assert!(
                store
                    .get_provider_state(REPLAY_NAMESPACE, blocked_request_key(reserve_first))
                    .unwrap()
                    .is_none()
            );
        });
    }
}

fn blocked_request_key(reserve_first: bool) -> &'static str {
    if reserve_first {
        "reserve-a-5"
    } else {
        "reserve-b-3"
    }
}

#[test]
fn reservation_failure_unknown_ack_replay_legacy_reader_and_undo_acquisition() {
    backends(|inner| {
        let store = isolation_tests::InterceptStore::new(inner);
        let service = seeded(store.clone());
        let q = get("reserve-a", 5, b"A1", false, true);
        let before = rows(&*store);
        store.mode.store(2, AtomicOrdering::SeqCst);
        assert_eq!(
            service.execute(&invocation("reserve-a"), &q),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        store.mode.store(3, AtomicOrdering::SeqCst);
        assert_eq!(
            service.execute(&invocation("reserve-a"), &q),
            Err(HostProblem::UnknownOutcome)
        );
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        assert_eq!(execute(&reopened, "reserve-a", &q).segments[0].data, b"A1X");
        let stable = rows(&*store);
        execute(&reopened, "reserve-a", &q);
        assert_eq!(rows(&*store), stable);
        let mut row = store
            .get_provider_state(SYSTEM_NAMESPACE, "runtime")
            .unwrap()
            .unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        for reservation in json["value"]["reservations"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            reservation.as_object_mut().unwrap().remove("pcb");
        }
        row.payload = serde_json::to_vec(&json).unwrap();
        let version = row.version;
        row.version += 1;
        store.put_provider_state(row, Some(version)).unwrap();
        let reader = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        execute(
            &reader,
            "reserve-b",
            &get("reserve-b", 2, b"A1", true, false),
        );
        let replace = request("reserve-b", ImsOperation::Replace, 3, &[], b"A1Z");
        let before = rows(&*store);
        assert_eq!(
            reader.execute(&invocation("reserve-b"), &replace),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &reader,
            "reserve-a",
            &get("reserve-a", 6, b"B2", false, false),
        );
        assert_eq!(released(&reader, "reserve-a", 7, b'A'), 1);
        let released_rows = rows(&*store);
        execute(&reader, "reserve-a", &q);
        assert_eq!(rows(&*store), released_rows); // Recorded Get does not reacquire Q.
        execute(&reader, "reserve-b", &replace);
        let before = rows(&*store);
        assert_eq!(
            reader.execute(
                &invocation("reserve-a"),
                &get("reserve-a", 8, b"A1", false, true)
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &reader,
            "reserve-b",
            &request("reserve-b", ImsOperation::Rollback, 4, &[], b""),
        );
    });
}

#[test]
fn reservation_logical_cascade_is_atomic_for_reserved_child_database() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        closure_tests::setup(&service, true);
        closure_tests::insert_child(&service, 3);
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Commit, 4, &[], b""),
        );
        let mut q = request("child-run", ImsOperation::GetUnique, 5, &["CHILD"], b"");
        q.q_class = ImsQClass::new(b'A');
        execute(&service, "child-run", &q);
        let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        closure_tests::hold_parent(&stale, 4);
        let mut delete = request("parent-run", ImsOperation::Delete, 5, &[], b"");
        delete.pcb = 2;
        let before = rows(&*store);
        assert_eq!(
            stale.execute(&invocation("parent-run"), &delete),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Commit, 6, &[], b""),
        );
        assert_eq!(execute(&stale, "parent-run", &delete).affected_segments, 2);
        execute(
            &stale,
            "parent-run",
            &request("parent-run", ImsOperation::Rollback, 6, &[], b""),
        );
    });
}

#[test]
fn reservation_acquisition_authorization_and_malformed_are_no_mutation() {
    backends(|store| {
        seeded(store.clone());
        let policy = Arc::new(Policy::default());
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), policy.clone())
                .unwrap();
        let q = get("reserve-a", 5, b"A1", false, true);
        *policy.deny_update.lock().unwrap() = true;
        let before = rows(&*store);
        assert_eq!(
            service.execute(&invocation("reserve-a"), &q),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(rows(&*store), before);
        *policy.deny_update.lock().unwrap() = false;
        let mut invalid = request("reserve-a", ImsOperation::Insert, 5, &["ROOT"], b"C3Z");
        invalid.q_class = ImsQClass::new(b'A');
        assert_eq!(
            service.execute(&invocation("reserve-a"), &invalid),
            Err(HostProblem::Malformed)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn reservation_sqlite_process_restart() {
    const STAGE: &str = "IMS_Q_RESTART_STAGE";
    const FILE: &str = "IMS_Q_RESTART_FILE";
    if let Ok(stage) = std::env::var(STAGE) {
        let file = std::env::var(FILE).unwrap();
        let store: Arc<dyn ProviderStateStore> = Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{file}?mode=rwc"),
                64 * 1024 * 1024,
                262_144,
            )
            .unwrap(),
        );
        match stage.as_str() {
            "seed" => {
                let service = seeded(store);
                execute(
                    &service,
                    "reserve-a",
                    &get("reserve-a", 5, b"A1", false, true),
                );
            }
            "resume" => {
                let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
                execute(
                    &service,
                    "reserve-b",
                    &get("reserve-b", 2, b"A1", true, false),
                );
                let replace = request("reserve-b", ImsOperation::Replace, 3, &[], b"A1Z");
                let before = rows(&*store);
                assert_eq!(
                    service.execute(&invocation("reserve-b"), &replace),
                    Err(HostProblem::IdempotencyConflict)
                );
                assert_eq!(rows(&*store), before);
                execute(
                    &service,
                    "reserve-a",
                    &request("reserve-a", ImsOperation::Commit, 6, &[], b""),
                );
                execute(&service, "reserve-b", &replace);
                execute(
                    &service,
                    "reserve-b",
                    &request("reserve-b", ImsOperation::Commit, 4, &[], b""),
                );
            }
            "verify" => {
                let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
                let replace = request("reserve-b", ImsOperation::Replace, 3, &[], b"A1Z");
                let before = rows(&*store);
                execute(&service, "reserve-b", &replace);
                assert_eq!(rows(&*store), before);
                assert_eq!(
                    execute(
                        &service,
                        "reserve-b",
                        &get("reserve-b", 7, b"A1", false, false)
                    )
                    .segments[0]
                        .data,
                    b"A1Z"
                );
                assert_eq!(system::reservation_count(&service.lock().unwrap().state), 0);
            }
            other => panic!("unknown stage {other}"),
        }
        return;
    }
    let file = std::env::temp_dir().join(format!(
        "ims-q-restart-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    for stage in ["seed", "resume", "verify"] {
        assert!(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::generic::tests::reservation_tests::reservation_sqlite_process_restart", "--nocapture"])
            .env(STAGE, stage).env(FILE, &file).status().unwrap().success());
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn reservation_legacy_location_reader_fences_ordinary_update_delete_and_load() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        service.install(legacy_catalog()).unwrap();
        for run in ["reserve-a", "reserve-b"] {
            let mut schedule = request(run, ImsOperation::Schedule, 1, &[], b"");
            schedule.psb = Some("OLDPSB".into());
            execute(&service, run, &schedule);
        }
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Commit, 3, &[], b""),
        );
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 4, b"A1", false, true),
        );
        let second = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        execute(
            &second,
            "reserve-b",
            &get("reserve-b", 2, b"A1", false, false),
        );
        let namespaces = [
            DATABASE_NAMESPACE,
            PENDING_NAMESPACE,
            SYSTEM_NAMESPACE,
            SESSION_NAMESPACE,
            REPLAY_NAMESPACE,
        ];
        let before = namespaces
            .into_iter()
            .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
            .collect::<Vec<_>>();
        for (op, data) in [
            (ImsOperation::Replace, b"A1Z".as_slice()),
            (ImsOperation::Replace, b"A1X".as_slice()),
            (ImsOperation::Delete, b"".as_slice()),
        ] {
            assert_eq!(
                invoke(
                    &second,
                    "reserve-b",
                    &request("reserve-b", op, 3, &[], data)
                ),
                Err(HostProblem::IdempotencyConflict)
            );
        }
        let load = ImsLoadImage {
            database: "OLDDB".into(),
            roots: vec![],
        };
        assert_eq!(
            invoke(
                &second,
                "reserve-b",
                &request(
                    "reserve-b",
                    ImsOperation::Load,
                    4,
                    &[],
                    &serde_json::to_vec(&load).unwrap()
                )
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        let after = namespaces
            .into_iter()
            .flat_map(|ns| store.list_provider_state(ns, 4096).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(after, before);
    });
}

#[test]
fn reservation_msdb_q_is_explicitly_unsupported_without_mutation() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let mut metadata = catalog();
        metadata.databases[0].organization = ImsDatabaseOrganization::Msdb;
        metadata.databases[0].segments.truncate(1);
        let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[0] else {
            unreachable!()
        };
        pcb.sensitive_segments.truncate(1);
        service.install_metadata(metadata).unwrap();
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Schedule, 1, &[], b""),
        );
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Commit, 3, &[], b""),
        );
        let before = rows(&*store);
        assert_eq!(
            service.execute(
                &invocation("reserve-a"),
                &get("reserve-a", 4, b"A1", false, true)
            ),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn reservation_legacy_record_scope_and_deq_position() {
    backends(|store| {
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let mut metadata = legacy_catalog();
        metadata.databases[0].segments.push(ImsSegmentDefinition {
            name: "CHILD".into(),
            parent: Some("ROOT".into()),
            length: 3,
            key_field: "CHILDKEY".into(),
            key_offset: 0,
            key_length: 2,
        });
        metadata.psbs[0].pcbs[0].segments.push("CHILD".into());
        service.install(metadata).unwrap();
        for run in ["reserve-a", "reserve-b"] {
            let mut schedule = request(run, ImsOperation::Schedule, 1, &[], b"");
            schedule.psb = Some("OLDPSB".into());
            execute(&service, run, &schedule);
        }
        for (seq, data) in [(2, b"A1X"), (3, b"B2Y")] {
            execute(
                &service,
                "reserve-a",
                &request("reserve-a", ImsOperation::Insert, seq, &["ROOT"], data),
            );
        }
        add_children(&service);
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 20, b"A1", false, true),
        );
        assert_eq!(
            invoke(
                &service,
                "reserve-b",
                &child_get("reserve-b", 2, b"C1", false, true)
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        execute(
            &service,
            "reserve-b",
            &child_get("reserve-b", 3, b"C1", false, false),
        );
        assert_eq!(
            invoke(
                &service,
                "reserve-b",
                &request("reserve-b", ImsOperation::Replace, 4, &[], b"C1X")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        execute(
            &service,
            "reserve-a",
            &child_get("reserve-a", 21, b"C1", false, false),
        );
        assert_eq!(released(&service, "reserve-a", 22, b'A'), 0);
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 23, b"B2", false, false),
        );
        assert_eq!(released(&service, "reserve-a", 24, b'A'), 1);
        execute(
            &service,
            "reserve-b",
            &child_get("reserve-b", 5, b"C1", false, true),
        );
        assert_eq!(
            invoke(
                &service,
                "reserve-a",
                &get("reserve-a", 25, b"A1", false, true)
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 26, b"A1", false, false),
        );
        assert_eq!(
            invoke(
                &service,
                "reserve-a",
                &request("reserve-a", ImsOperation::Delete, 27, &[], b"")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
    });
}

#[test]
fn reservation_dependent_does_not_reserve_its_descendants() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let mut metadata = catalog();
        let mut leaf = metadata.databases[0].segments[1].clone();
        leaf.name = "LEAF".into();
        leaf.parent = Some("CHILD".into());
        metadata.databases[0].segments.push(leaf);
        for pcb in &mut metadata.psbs[0].pcbs {
            if let ImsPcbMetadata::Database(pcb) = pcb {
                pcb.sensitive_segments.push(ImsSensitiveSegmentMetadata {
                    name: "LEAF".into(),
                    parent: Some("CHILD".into()),
                    processing_options: None,
                });
            }
        }
        service.install_metadata(metadata).unwrap();
        for run in ["reserve-a", "reserve-b"] {
            execute(
                &service,
                run,
                &request(run, ImsOperation::Schedule, 1, &[], b""),
            );
        }
        execute(
            &service,
            "reserve-a",
            &request("reserve-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        add_children(&service);
        execute(
            &service,
            "reserve-a",
            &child_get("reserve-a", 20, b"C1", false, true),
        );
        let mut insert = request(
            "reserve-b",
            ImsOperation::Insert,
            2,
            &["ROOT", "CHILD", "LEAF"],
            b"L1X",
        );
        insert.qualifiers = child_get("reserve-b", 99, b"C1", false, false).qualifiers;
        execute(&service, "reserve-b", &insert);
        execute(
            &service,
            "reserve-b",
            &request("reserve-b", ImsOperation::Commit, 3, &[], b""),
        );
        let mut leaf_get = child_get("reserve-b", 4, b"C1", true, false);
        leaf_get.segments.push("LEAF".into());
        execute(&service, "reserve-b", &leaf_get);
        execute(
            &service,
            "reserve-b",
            &request("reserve-b", ImsOperation::Delete, 5, &[], b""),
        );
        execute(
            &service,
            "reserve-b",
            &request("reserve-b", ImsOperation::Commit, 6, &[], b""),
        );
        execute(
            &service,
            "reserve-a",
            &get("reserve-a", 21, b"A1", false, true),
        );
        insert.mutation = request("reserve-b", ImsOperation::Insert, 7, &[], b"").mutation;
        let before = rows(&*store);
        assert_eq!(
            invoke(&service, "reserve-b", &insert),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
    });
}
