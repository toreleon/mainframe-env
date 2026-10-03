use super::*;

mod integration_tests;

fn invoke(
    service: &Arc<ImsService>,
    run: &str,
    req: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let invocation = invocation(run);
    let mutation = req.mutation.as_ref().unwrap();
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: mutation.sequence,
        idempotency_key: Some(mutation.idempotency_key.clone()),
        request: HostRequest::Ims(req.clone()),
        deadline_tick: invocation.deadline_tick,
    };
    let providers = ims_providers(service.clone(), InvocationLimits::default());
    match providers[1].invoke(&invocation, effect).outcome? {
        HostResult::Ims(result) => Ok(result),
        other => panic!("unexpected {other:?}"),
    }
}

fn backends(mut case: impl FnMut(Arc<dyn ProviderStateStore>)) {
    case(Arc::new(MemoryStore::new(Default::default())));
    let file = std::env::temp_dir().join(format!(
        "ims-integrity-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    case(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SESSION_NAMESPACE,
        SYSTEM_NAMESPACE,
        REPLAY_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|namespace| store.list_provider_state(namespace, 4096).unwrap())
    .collect()
}

fn read(run: &str, op: ImsOperation, sequence: u64, pcb: u16, segment: &str) -> ImsRequest {
    let mut req = request(run, op, sequence, &[segment], b"");
    req.pcb = pcb;
    req
}

fn seeded(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    seed(&service, read_catalog());
    service
}

fn read_catalog() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let ImsPcbMetadata::Database(template) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    for (index, options) in ["GP", "GO", "GOP", "GON", "GONP", "GOT", "GOTP"]
        .into_iter()
        .enumerate()
    {
        let mut pcb = template.clone();
        pcb.name = format!("READ{index}");
        pcb.processing_options = options.into();
        metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    }
    metadata
}

fn seed(service: &Arc<ImsService>, metadata: ImsMetadataCatalog) {
    service.install_metadata(metadata).unwrap();
    for run in ["integrity-a", "integrity-b"] {
        invoke(
            service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        )
        .unwrap();
    }
    invoke(
        service,
        "integrity-a",
        &request("integrity-a", ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
    )
    .unwrap();
    invoke(
        service,
        "integrity-a",
        &request("integrity-a", ImsOperation::Insert, 3, &["CHILD"], b"C1Y"),
    )
    .unwrap();
    invoke(
        service,
        "integrity-a",
        &request("integrity-a", ImsOperation::Commit, 4, &[], b""),
    )
    .unwrap();
}

fn pending(service: &Arc<ImsService>) {
    invoke(
        service,
        "integrity-a",
        &read("integrity-a", ImsOperation::GetHoldUnique, 5, 1, "CHILD"),
    )
    .unwrap();
    invoke(
        service,
        "integrity-a",
        &request("integrity-a", ImsOperation::Replace, 6, &[], b"C1Z"),
    )
    .unwrap();
}

fn dirty_read(store: Arc<dyn ProviderStateStore>, two_services: bool) {
    let owner = seeded(store.clone());
    invoke(
        &owner,
        "integrity-b",
        &read("integrity-b", ImsOperation::GetUnique, 2, 2, "ROOT"),
    )
    .unwrap();
    let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    pending(&owner);
    let reader = if two_services { &stale } else { &owner };
    let before = rows(&*store);
    assert_eq!(
        invoke(
            reader,
            "integrity-b",
            &read("integrity-b", ImsOperation::GetUnique, 3, 2, "CHILD")
        ),
        Err(HostProblem::IdempotencyConflict),
        "normal G returned another run's C1Z"
    );
    assert_eq!(rows(&*store), before);
}

#[test]
fn integrity_fail_first_public_memory() {
    dirty_read(Arc::new(MemoryStore::new(Default::default())), false);
}

#[test]
fn integrity_fail_first_public_file_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-integrity-fail-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    dirty_read(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap()),
        true,
    );
    std::fs::remove_file(file).unwrap();
}

#[test]
fn integrity_six_reads_selected_pcbs_owner_and_settlement() {
    for op in [
        ImsOperation::GetUnique,
        ImsOperation::GetNext,
        ImsOperation::GetNextParent,
        ImsOperation::GetHoldUnique,
        ImsOperation::GetHoldNext,
        ImsOperation::GetHoldNextParent,
    ] {
        for settlement in [ImsOperation::Commit, ImsOperation::Rollback] {
            backends(|store| {
                let owner = seeded(store.clone());
                // Hold forms use the update PCB; ordinary calls use the selected G PCB.
                let pcb = if matches!(
                    op,
                    ImsOperation::GetHoldUnique
                        | ImsOperation::GetHoldNext
                        | ImsOperation::GetHoldNextParent
                ) {
                    1
                } else {
                    2
                };
                invoke(
                    &owner,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 2, pcb, "ROOT"),
                )
                .unwrap();
                let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
                pending(&owner);
                let req = read("integrity-b", op, 3, pcb, "CHILD");
                let before = rows(&*store);
                for reader in [&owner, &stale] {
                    assert_eq!(
                        invoke(reader, "integrity-b", &req),
                        Err(HostProblem::IdempotencyConflict),
                        "{op:?}"
                    );
                    assert_eq!(rows(&*store), before);
                }
                invoke(
                    &owner,
                    "integrity-a",
                    &read("integrity-a", ImsOperation::GetUnique, 7, 1, "ROOT"),
                )
                .unwrap();
                assert_eq!(
                    invoke(
                        &owner,
                        "integrity-a",
                        &read("integrity-a", op, 8, 1, "CHILD")
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C1Z"
                );
                invoke(
                    &owner,
                    "integrity-a",
                    &request("integrity-a", settlement, 9, &[], b""),
                )
                .unwrap();
                invoke(
                    &stale,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 4, pcb, "ROOT"),
                )
                .unwrap();
                let expected = if settlement == ImsOperation::Commit {
                    b"C1Z"
                } else {
                    b"C1Y"
                };
                assert_eq!(
                    invoke(&stale, "integrity-b", &req).unwrap().segments[0].data,
                    expected
                );
            });
        }
    }
}

#[test]
fn integrity_trusted_pcb_o_forms_read_pending_but_cannot_update() {
    backends(|store| {
        let service = seeded(store.clone());
        pending(&service);
        for pcb in 3..=8 {
            invoke(
                &service,
                "integrity-b",
                &read(
                    "integrity-b",
                    ImsOperation::GetUnique,
                    10 + u64::from(pcb),
                    pcb,
                    "ROOT",
                ),
            )
            .unwrap();
            let req = read(
                "integrity-b",
                ImsOperation::GetUnique,
                20 + u64::from(pcb),
                pcb,
                "CHILD",
            );
            assert_eq!(
                invoke(&service, "integrity-b", &req).unwrap().segments[0].data,
                b"C1Z"
            );
            let mut replace = request(
                "integrity-b",
                ImsOperation::Replace,
                30 + u64::from(pcb),
                &[],
                b"C1Q",
            );
            replace.pcb = pcb;
            assert_eq!(
                invoke(&service, "integrity-b", &replace).unwrap().status,
                "AM"
            );
        }
        // SENSEG O alone cannot bypass a normal PCB's integrity requirements.
        let mut req = read("integrity-b", ImsOperation::GetUnique, 50, 2, "CHILD");
        req.q_class = ImsQClass::new(b'A');
        let before = rows(&*store);
        assert_eq!(
            invoke(&service, "integrity-b", &req),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn integrity_exact_replay_precedes_later_foreign_uow_and_canonical_conflict() {
    backends(|store| {
        let service = seeded(store.clone());
        let original = read("integrity-b", ImsOperation::GetHoldUnique, 2, 1, "CHILD");
        let result = invoke(&service, "integrity-b", &original).unwrap();
        invoke(
            &service,
            "integrity-b",
            &read("integrity-b", ImsOperation::GetUnique, 3, 1, "ROOT"),
        )
        .unwrap();
        pending(&service);
        let before = rows(&*store);
        assert_eq!(invoke(&service, "integrity-b", &original).unwrap(), result);
        assert_eq!(rows(&*store), before);
        let mut changed = original.clone();
        changed.pcb = 2;
        assert_eq!(
            invoke(&service, "integrity-b", &changed),
            Err(HostProblem::IdempotencyConflict)
        );
        let fresh = read("integrity-b", ImsOperation::GetHoldUnique, 4, 1, "CHILD");
        assert_eq!(
            invoke(&service, "integrity-b", &fresh),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
        // Corrupt a later unrelated witness. Existing replay still returns the
        // original result; fresh navigation must fail closed while decoding it.
        let mut row = store
            .get_provider_state(GENERIC_PENDING_NAMESPACE, "integrity-a")
            .unwrap()
            .unwrap();
        let version = row.version;
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        value["value"]["schema_version"] = serde_json::json!("unknown");
        row.payload = serde_json::to_vec(&value).unwrap();
        row.version += 1;
        store.put_provider_state(row, Some(version)).unwrap();
        let before = rows(&*store);
        assert_eq!(invoke(&service, "integrity-b", &original).unwrap(), result);
        assert_eq!(
            invoke(&service, "integrity-b", &fresh),
            Err(HostProblem::InfrastructureFailure)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn integrity_read_cas_new_writer_and_no_op_ownership_races() {
    use std::sync::atomic::Ordering as AtomicOrdering;
    for same_bytes in [false, true] {
        backends(|inner| {
            let store = isolation_tests::InterceptStore::new(inner);
            let reader = seeded(store.clone());
            let writer = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            invoke(
                &writer,
                "integrity-a",
                &read("integrity-a", ImsOperation::GetHoldUnique, 5, 1, "CHILD"),
            )
            .unwrap();
            store.mode.store(1, AtomicOrdering::SeqCst);
            let thread = std::thread::spawn(move || {
                invoke(
                    &reader,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD"),
                )
            });
            store.entered.wait();
            invoke(
                &writer,
                "integrity-a",
                &request(
                    "integrity-a",
                    ImsOperation::Replace,
                    6,
                    &[],
                    if same_bytes { b"C1Y" } else { b"C1Z" },
                ),
            )
            .unwrap();
            store.release.wait();
            assert_eq!(
                thread.join().unwrap(),
                Err(HostProblem::IdempotencyConflict)
            );
            assert!(
                store
                    .get_provider_state(REPLAY_NAMESPACE, "integrity-b-2")
                    .unwrap()
                    .is_none()
            );
            let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            let before = rows(&*store);
            assert_eq!(
                invoke(
                    &reopened,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD")
                ),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(rows(&*store), before);
        });
    }
}

#[test]
fn integrity_read_atomic_failure_lost_ack_and_replay_after_later_write() {
    use std::sync::atomic::Ordering as AtomicOrdering;
    backends(|inner| {
        let store = isolation_tests::InterceptStore::new(inner);
        let service = seeded(store.clone());
        let req = read("integrity-b", ImsOperation::GetHoldUnique, 2, 1, "CHILD");
        let before = rows(&*store);
        store.mode.store(2, AtomicOrdering::SeqCst);
        assert_eq!(
            invoke(&service, "integrity-b", &req),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(rows(&*store), before);
        store.mode.store(3, AtomicOrdering::SeqCst);
        assert_eq!(
            invoke(&service, "integrity-b", &req),
            Err(HostProblem::UnknownOutcome)
        );
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        pending(&reopened);
        let before = rows(&*store);
        assert_eq!(
            invoke(&reopened, "integrity-b", &req).unwrap().segments[0].data,
            b"C1Y"
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn integrity_o_database_classes_and_forbidden_segment_update_metadata() {
    for organization in [
        ImsDatabaseOrganization::Hidam,
        ImsDatabaseOrganization::Dedb,
        ImsDatabaseOrganization::Msdb,
        ImsDatabaseOrganization::Gsam,
    ] {
        backends(|store| {
            let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            let mut metadata = read_catalog();
            metadata.databases[0].organization = organization;
            if matches!(
                organization,
                ImsDatabaseOrganization::Gsam | ImsDatabaseOrganization::Msdb
            ) {
                metadata.databases[0].segments.truncate(1);
                if organization == ImsDatabaseOrganization::Gsam {
                    metadata.databases[0].segments[0].fields[0].sequence = false;
                }
                for pcb in &mut metadata.psbs[0].pcbs {
                    let ImsPcbMetadata::Database(pcb) = pcb else {
                        unreachable!()
                    };
                    pcb.sensitive_segments.truncate(1);
                }
            }
            service.install_metadata(metadata).unwrap();
            invoke(
                &service,
                "integrity-b",
                &request("integrity-b", ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
            let before = rows(&*store);
            let outcome = invoke(
                &service,
                "integrity-b",
                &read("integrity-b", ImsOperation::GetUnique, 2, 3, "ROOT"),
            );
            if matches!(
                organization,
                ImsDatabaseOrganization::Msdb | ImsDatabaseOrganization::Gsam
            ) {
                assert_eq!(outcome, Err(HostProblem::Unsupported));
                assert_eq!(rows(&*store), before);
            } else {
                assert_eq!(outcome.unwrap().status, "GE");
            }
        });
    }
    for override_option in ["I", "R", "D", "A"] {
        backends(|store| {
            let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            let mut metadata = read_catalog();
            let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[2] else {
                unreachable!()
            };
            pcb.sensitive_segments[1].processing_options = Some(override_option.into());
            seed(&service, metadata);
            let before = rows(&*store);
            assert_eq!(
                invoke(
                    &service,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 2, 3, "CHILD")
                ),
                Err(HostProblem::Unsupported)
            );
            assert_eq!(rows(&*store), before);
            let mut insert = request("integrity-b", ImsOperation::Insert, 3, &["CHILD"], b"C2Q");
            insert.pcb = 3;
            assert_eq!(
                invoke(&service, "integrity-b", &insert).unwrap().status,
                "AM"
            );
            assert!(
                store
                    .get_provider_state(GENERIC_PENDING_NAMESPACE, "integrity-b")
                    .unwrap()
                    .is_none()
            );
        });
    }
}

#[test]
fn integrity_senseg_o_does_not_grant_pcb_exemption() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let mut metadata = read_catalog();
        let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
            unreachable!()
        };
        pcb.sensitive_segments[1].processing_options = Some("GO".into());
        seed(&service, metadata);
        pending(&service);
        let before = rows(&*store);
        assert_eq!(
            invoke(
                &service,
                "integrity-b",
                &read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn integrity_authorization_precedes_visibility_and_replay() {
    backends(|store| {
        let policy = Arc::new(Policy::default());
        let service =
            ImsService::open_authorized(store.clone(), ImsLimits::default(), policy.clone())
                .unwrap();
        seed(&service, read_catalog());
        let original = read("integrity-b", ImsOperation::GetUnique, 2, 3, "CHILD");
        let result = invoke(&service, "integrity-b", &original).unwrap();
        pending(&service);
        *policy.deny_update.lock().unwrap() = true;
        let before = rows(&*store);
        for pcb in [2, 3] {
            assert_eq!(
                invoke(
                    &service,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 3, pcb, "CHILD")
                ),
                Err(HostProblem::Unauthorized)
            );
        }
        assert_eq!(
            invoke(&service, "integrity-b", &original),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(rows(&*store), before);
        *policy.deny_update.lock().unwrap() = false;
        assert_eq!(invoke(&service, "integrity-b", &original).unwrap(), result);
        assert_eq!(rows(&*store), before);
    });
}

#[test]
fn integrity_retained_undo_unknown_witness_and_missing_selected_pcb() {
    backends(|store| {
        let service = seeded(store.clone());
        pending(&service);
        let original = store
            .get_provider_state(GENERIC_PENDING_NAMESPACE, "integrity-a")
            .unwrap()
            .unwrap();
        for legacy in [true, false] {
            let mut row = store
                .get_provider_state(GENERIC_PENDING_NAMESPACE, "integrity-a")
                .unwrap()
                .unwrap();
            let version = row.version;
            let mut json: serde_json::Value = serde_json::from_slice(&original.payload).unwrap();
            if legacy {
                json["value"] = json["value"]["images"].clone();
            } else {
                json["value"]["post_images"]["GENDB"] = serde_json::to_value([0u8; 32]).unwrap();
                json["value"]["owned_images"]["GENDB"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::to_value([0u8; 32]).unwrap());
            }
            row.payload = serde_json::to_vec(&json).unwrap();
            row.version += 1;
            store.put_provider_state(row, Some(version)).unwrap();
            let before = rows(&*store);
            assert_eq!(
                invoke(
                    &service,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD")
                ),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(
                invoke(
                    &service,
                    "integrity-a",
                    &read("integrity-a", ImsOperation::GetUnique, 7, 1, "CHILD")
                ),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(
                invoke(
                    &service,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 3, 3, "CHILD")
                ),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(
                invoke(
                    &service,
                    "integrity-b",
                    &read("integrity-b", ImsOperation::GetUnique, 4, 99, "CHILD")
                ),
                Err(HostProblem::NotFound)
            );
            assert_eq!(rows(&*store), before);
        }
    });
}

#[test]
fn integrity_logical_dependency_fence_and_unrelated_database_scope() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        closure_tests::setup(&service, true);
        invoke(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Commit, 3, &[], b""),
        )
        .unwrap();
        // The child image has no pending undo; a foreign related parent does.
        closure_tests::hold_parent(&service, 4);
        let mut replace = request("parent-run", ImsOperation::Replace, 5, &[], b"P1Z");
        replace.pcb = 2;
        invoke(&service, "parent-run", &replace).unwrap();
        let before = rows(&*store);
        assert_eq!(
            invoke(
                &service,
                "child-run",
                &read("child-run", ImsOperation::GetUnique, 4, 1, "ROOT")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*store), before);
    });
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        let mut metadata = read_catalog();
        let mut unrelated = metadata.databases[0].clone();
        unrelated.name = "OTHERDB".into();
        metadata.databases.push(unrelated);
        let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[1].clone() else {
            unreachable!()
        };
        pcb.name = "OTHERPCB".into();
        pcb.database = "OTHERDB".into();
        metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
        seed(&service, metadata);
        pending(&service);
        let original = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap();
        assert_eq!(
            invoke(
                &service,
                "integrity-b",
                &read("integrity-b", ImsOperation::GetUnique, 2, 9, "ROOT")
            )
            .unwrap()
            .status,
            "GE"
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap()
                .unwrap(),
            original
        );
    });
}

#[test]
fn integrity_sqlite_process_reopen_visibility_and_original_replay() {
    const PHASE: &str = "MAINFRAME_ENV_INTEGRITY_PROCESS_PHASE";
    const URL: &str = "MAINFRAME_ENV_INTEGRITY_PROCESS_URL";
    if let Ok(phase) = std::env::var(PHASE) {
        let store: Arc<dyn ProviderStateStore> = Arc::new(
            SqliteStateStore::open(&std::env::var(URL).unwrap(), 64 * 1024 * 1024, 262_144)
                .unwrap(),
        );
        if phase == "seed" {
            let service = seeded(store);
            let original = read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD");
            assert_eq!(
                invoke(&service, "integrity-b", &original).unwrap().segments[0].data,
                b"C1Y"
            );
            pending(&service);
        } else {
            let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
            let fresh = read("integrity-b", ImsOperation::GetUnique, 3, 2, "CHILD");
            if phase == "observe" {
                let before = rows(&*store);
                assert_eq!(
                    invoke(&service, "integrity-b", &fresh),
                    Err(HostProblem::IdempotencyConflict)
                );
                assert_eq!(
                    invoke(
                        &service,
                        "integrity-b",
                        &read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD")
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C1Y"
                );
                assert_eq!(rows(&*store), before);
                assert_eq!(
                    invoke(
                        &service,
                        "integrity-b",
                        &read("integrity-b", ImsOperation::GetUnique, 4, 3, "CHILD")
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C1Z"
                );
                invoke(
                    &service,
                    "integrity-a",
                    &request("integrity-a", ImsOperation::Rollback, 7, &[], b""),
                )
                .unwrap();
            } else {
                assert_eq!(
                    invoke(&service, "integrity-b", &fresh).unwrap().segments[0].data,
                    b"C1Y"
                );
                assert!(
                    store
                        .get_provider_state(GENERIC_PENDING_NAMESPACE, "integrity-a")
                        .unwrap()
                        .is_none()
                );
            }
        }
        return;
    }
    let file = std::env::temp_dir().join(format!(
        "ims-integrity-process-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    for phase in ["seed", "observe", "verify"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::generic::tests::integrity_tests::integrity_sqlite_process_reopen_visibility_and_original_replay", "--nocapture"])
            .env(PHASE, phase).env(URL, &url).output().unwrap();
        assert!(
            output.status.success(),
            "{phase}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn integrity_legacy_route_refreshes_and_fences_foreign_pending_image() {
    backends(|store| {
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        service
            .install(ImsApplicationDefinition {
                databases: vec![ImsDatabaseDefinition {
                    name: "LEGDB".into(),
                    access: "HIDAM".into(),
                    secondary_index: None,
                    segments: vec![ImsSegmentDefinition {
                        name: "ROOT".into(),
                        parent: None,
                        length: 3,
                        key_field: "ROOTKEY".into(),
                        key_offset: 0,
                        key_length: 2,
                    }],
                }],
                psbs: vec![ImsPsbDefinition {
                    name: "GENPSB".into(),
                    pcbs: vec![ImsPcbDefinition {
                        name: "LEGPCB".into(),
                        database: "LEGDB".into(),
                        processing_options: "AP".into(),
                        segments: vec!["ROOT".into()],
                    }],
                }],
            })
            .unwrap();
        for run in ["integrity-a", "integrity-b"] {
            invoke(
                &service,
                run,
                &request(run, ImsOperation::Schedule, 1, &[], b""),
            )
            .unwrap();
        }
        let stale = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        invoke(
            &service,
            "integrity-a",
            &request("integrity-a", ImsOperation::Insert, 2, &["ROOT"], b"A1Z"),
        )
        .unwrap();
        let req = read("integrity-b", ImsOperation::GetUnique, 2, 1, "ROOT");
        let before = store.list_provider_state(DATABASE_NAMESPACE, 100).unwrap();
        assert_eq!(
            invoke(&stale, "integrity-b", &req),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            store.list_provider_state(DATABASE_NAMESPACE, 100).unwrap(),
            before
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "integrity-b-2")
                .unwrap()
                .is_none()
        );
        invoke(
            &service,
            "integrity-a",
            &request("integrity-a", ImsOperation::Commit, 3, &[], b""),
        )
        .unwrap();
        assert_eq!(
            invoke(&stale, "integrity-b", &req).unwrap().segments[0].data,
            b"A1Z"
        );
    });
}

#[test]
fn integrity_normal_procopt_classes_and_dedb_o_pending_visibility() {
    for organization in [
        ImsDatabaseOrganization::Hidam,
        ImsDatabaseOrganization::Dedb,
    ] {
        for option in ["G", "R", "D", "A", "GO", "GON", "GOT"] {
            backends(|store| {
                let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
                let mut metadata = read_catalog();
                metadata.databases[0].organization = organization;
                let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
                    unreachable!()
                };
                pcb.processing_options = option.into();
                seed(&service, metadata);
                pending(&service);
                let req = read("integrity-b", ImsOperation::GetUnique, 2, 2, "CHILD");
                if option.contains('O') {
                    assert_eq!(
                        invoke(&service, "integrity-b", &req).unwrap().segments[0].data,
                        b"C1Z",
                        "{organization:?} {option}"
                    );
                } else {
                    let before = rows(&*store);
                    assert_eq!(
                        invoke(&service, "integrity-b", &req),
                        Err(HostProblem::IdempotencyConflict),
                        "{organization:?} {option}"
                    );
                    assert_eq!(rows(&*store), before);
                    assert_eq!(
                        invoke(
                            &service,
                            "integrity-a",
                            &read("integrity-a", ImsOperation::GetUnique, 7, 2, "CHILD")
                        )
                        .unwrap()
                        .segments[0]
                            .data,
                        b"C1Z"
                    );
                }
            });
        }
    }
}
