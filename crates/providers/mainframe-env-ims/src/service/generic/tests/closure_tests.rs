use super::*;
use mainframe_env_store::StoreLimits;

fn hisam_catalog() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    metadata.databases[0].organization = ImsDatabaseOrganization::Hisam;
    metadata.databases[0].segments[1].fields[0].unique = false;
    let ImsPcbMetadata::Database(mut other) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    other.name = "OTHER".into();
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(other));
    metadata
}

fn hisam_public(
    service: Arc<ImsService>,
    run: &str,
    req: ImsRequest,
) -> Result<ImsResult, HostProblem> {
    hisam_public_class(service, run, req, ServiceClass::Batch)
}

fn hisam_public_class(
    service: Arc<ImsService>,
    run: &str,
    req: ImsRequest,
    class: ServiceClass,
) -> Result<ImsResult, HostProblem> {
    let inv = invocation_class(run, class);
    let host = HostRequest::Ims(req);
    host.validate(mainframe_env_host_api::HostLimits::default())
        .unwrap();
    let mutation = host.mutation().unwrap();
    let effect = EffectRequest {
        run_unit: inv.run_unit_id.clone(),
        sequence: mutation.sequence,
        idempotency_key: Some(mutation.idempotency_key.clone()),
        deadline_tick: inv.deadline_tick,
        request: host,
    };
    match ims_providers(service, InvocationLimits::default())[1]
        .invoke(&inv, effect)
        .outcome?
    {
        HostResult::Ims(result) => Ok(result),
        other => panic!("IMS result: {other:?}"),
    }
}

fn hisam_seed(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    hisam_seed_metadata(store, run, hisam_catalog())
}

fn hisam_seed_metadata(
    store: Arc<dyn ProviderStateStore>,
    run: &str,
    metadata: ImsMetadataCatalog,
) -> Arc<ImsService> {
    let service = ImsService::open(store, Default::default()).unwrap();
    service.install_metadata(metadata).unwrap();
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: vec![
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1X".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(0),
                data: b"C1A".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(0),
                data: b"C2Z".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A2Y".to_vec(),
            },
        ],
    };
    for req in [
        request(
            run,
            ImsOperation::Load,
            1,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
        request(run, ImsOperation::Schedule, 2, &[], b""),
        request(run, ImsOperation::Commit, 3, &[], b""),
    ] {
        assert_eq!(
            hisam_public(service.clone(), run, req).unwrap().status,
            "  "
        );
    }
    service
}

fn hisam_root(run: &str, sequence: u64, pcb: u16, key: &[u8]) -> ImsRequest {
    let mut req = request(run, ImsOperation::GetUnique, sequence, &["ROOT"], b"");
    req.pcb = pcb;
    req.qualifiers.push(qualifier(key));
    req
}

fn hisam_fail_first(store: Arc<dyn ProviderStateStore>) {
    let run = "hisam-fail-first";
    let service = hisam_seed(store, run);
    assert_eq!(
        hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1"))
            .unwrap()
            .segments[0]
            .data,
        b"A1X"
    );
    let result = hisam_public(
        service.clone(),
        run,
        request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B"),
    )
    .unwrap();
    assert_eq!(
        result.status, "  ",
        "nonunique dependent is a successful ISRT"
    );
    assert_eq!(result.affected_segments, 1);
    let state = service.lock().unwrap();
    let engine = restored(&state.state, "GENDB", service.limits).unwrap();
    let twins = engine
        .export_records()
        .into_iter()
        .filter(|r| r.segment == "CHILD" && r.data.starts_with(b"C1"))
        .collect::<Vec<_>>();
    assert_eq!(twins.len(), 2);
    assert_ne!(twins[0].id, twins[1].id);
}

#[test]
fn hisam_nonunique_fail_first_public_memory() {
    hisam_fail_first(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn hisam_nonunique_fail_first_public_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "hisam-first-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rwc", file.display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    );
    hisam_fail_first(store);
    std::fs::remove_file(file).unwrap();
}

fn hisam_backends(name: &str, case: impl Fn(Arc<dyn ProviderStateStore>)) {
    case(Arc::new(MemoryStore::new(Default::default())));
    let file = std::env::temp_dir().join(format!(
        "hisam-{name}-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    case(Arc::new(
        SqliteStateStore::open(
            &format!("sqlite:{}?mode=rwc", file.display()),
            64 * 1024 * 1024,
            262_144,
        )
        .unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

fn hisam_cursor(service: &ImsService, run: &str, number: u16) -> serde_json::Value {
    serde_json::to_value(pcb::position(
        &service.lock().unwrap().state.sessions[run],
        number,
    ))
    .unwrap()
}

#[test]
fn hisam_nonunique_insert_clears_real_root_hold_and_qualified_control_stays_unique() {
    hisam_backends("insert-held-root", |store| {
        let run = "hisam-held-root";
        let service = hisam_seed(store.clone(), run);
        hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
        let mut hold = hisam_root(run, 5, 1, b"A1");
        hold.operation = ImsOperation::GetHoldUnique;
        assert_eq!(
            hisam_public(service.clone(), run, hold).unwrap().segments[0].data,
            b"A1X"
        );
        assert_eq!(
            hisam_cursor(&service, run, 1)["held"],
            serde_json::json!({"id":1,"version":1})
        );
        assert_eq!(
            hisam_public(
                service.clone(),
                run,
                request(run, ImsOperation::Insert, 6, &["CHILD"], b"C1B")
            )
            .unwrap()
            .affected_segments,
            1
        );
        assert_eq!(
            hisam_cursor(&service, run, 1),
            serde_json::json!({"current":5,"parentage":1,"held":null,"after_end":false})
        );
        hisam_public(service.clone(), run, hisam_root(run, 7, 1, b"A1")).unwrap();
        let mut qualified = request(run, ImsOperation::Insert, 8, &["CHILD"], b"C1D");
        qualified.qualifiers.push(qualifier(b"A1"));
        let image = store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap();
        let cursor = hisam_cursor(&service, run, 1);
        assert_eq!(
            hisam_public(service.clone(), run, qualified)
                .unwrap()
                .status,
            "II"
        );
        assert_eq!(
            store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap(),
            image
        );
        assert_eq!(hisam_cursor(&service, run, 1), cursor);
    });
}

#[test]
fn hisam_nonunique_context_fence_without_authorizer_and_metadata_controls() {
    for class in [
        ServiceClass::Interactive,
        ServiceClass::Compiler,
        ServiceClass::Blocking,
        ServiceClass::System,
    ] {
        hisam_backends(&format!("context-{class:?}"), |store| {
            let run = "hisam-context";
            let service = hisam_seed(store.clone(), run);
            assert!(service.authorizer.is_none());
            hisam_public_class(service.clone(), run, hisam_root(run, 4, 1, b"A1"), class).unwrap();
            let image = store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap();
            let cursor = hisam_cursor(&service, run, 1);
            assert_eq!(
                hisam_public_class(
                    service.clone(),
                    run,
                    request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B"),
                    class
                )
                .unwrap()
                .status,
                "II"
            );
            assert_eq!(
                store
                    .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                    .unwrap(),
                image
            );
            assert_eq!(hisam_cursor(&service, run, 1), cursor);
        });
    }
    for control in ["unique", "hidam", "variable", "foreign-logical-parent"] {
        hisam_backends(control, |store| {
            let run = "hisam-metadata";
            let mut metadata = hisam_catalog();
            match control {
                "unique" => metadata.databases[0].segments[1].fields[0].unique = true,
                "hidam" => metadata.databases[0].organization = ImsDatabaseOrganization::Hidam,
                "variable" => metadata.databases[0].segments[1].max_length = 4,
                "foreign-logical-parent" => {
                    let mut foreign = metadata.databases[0].clone();
                    foreign.name = "FOREIGNDB".into();
                    foreign.organization = ImsDatabaseOrganization::Hidam;
                    foreign
                        .logical_relationships
                        .push(ImsLogicalRelationshipMetadata {
                            parent_database: "GENDB".into(),
                            parent_segment: "ROOT".into(),
                            child_database: "FOREIGNDB".into(),
                            child_segment: "CHILD".into(),
                            paired: false,
                        });
                    metadata.databases.push(foreign);
                }
                _ => unreachable!(),
            }
            if control == "foreign-logical-parent" {
                let service = ImsService::open(store.clone(), Default::default()).unwrap();
                let before = hisam_rows(&*store);
                assert_eq!(
                    service.install_metadata(metadata),
                    Err(HostProblem::Malformed)
                );
                assert_eq!(hisam_rows(&*store), before);
                return;
            }
            let service = hisam_seed_metadata(store.clone(), run, metadata);
            hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
            let image = store
                .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap();
            let cursor = hisam_cursor(&service, run, 1);
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B")
                )
                .unwrap()
                .status,
                "II"
            );
            assert_eq!(
                store
                    .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                    .unwrap(),
                image
            );
            assert_eq!(hisam_cursor(&service, run, 1), cursor);
        });
    }
}

#[test]
fn hisam_nonunique_utility_twins_and_sensitive_procopt_saf_boundaries() {
    struct Deny;
    impl mainframe_env_host_api::EnterpriseAuthorizer for Deny {
        fn authorize(
            &self,
            _: &PrincipalId,
            _: &mainframe_env_host_api::EnterpriseResource,
        ) -> Result<(), HostProblem> {
            Err(HostProblem::Unauthorized)
        }
    }
    hisam_backends("utility", |store| {
        let run = "hisam-utility";
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        service.install_metadata(hisam_catalog()).unwrap();
        let image = ImsGenericLoadImage {
            database: "GENDB".into(),
            records: vec![
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"A1X".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(0),
                    data: b"C1A".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(0),
                    data: b"C1B".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(0),
                    data: b"C2Z".to_vec(),
                },
            ],
        };
        let load = request(
            run,
            ImsOperation::Load,
            1,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        );
        let before = hisam_rows(&*store);
        assert_eq!(
            hisam_public_class(
                service.clone(),
                run,
                load.clone(),
                ServiceClass::Interactive
            ),
            Err(HostProblem::Malformed)
        );
        assert_eq!(hisam_rows(&*store), before);
        assert_eq!(
            hisam_public(service.clone(), run, load)
                .unwrap()
                .affected_segments,
            4
        );
        let records = restored(&service.lock().unwrap().state, "GENDB", service.limits)
            .unwrap()
            .export_records();
        assert_eq!(
            records
                .iter()
                .map(|r| r.data.as_slice())
                .collect::<Vec<_>>(),
            vec![&b"A1X"[..], &b"C1A"[..], &b"C1B"[..], &b"C2Z"[..]]
        );
        assert_ne!(records[1].id, records[2].id);
    });
    for control in ["sensitivity", "procopt", "saf"] {
        hisam_backends(control, |store| {
            let run = "hisam-access";
            let mut metadata = hisam_catalog();
            let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[0] else {
                unreachable!()
            };
            if control == "sensitivity" {
                pcb.sensitive_segments.truncate(1);
            }
            if control == "procopt" {
                pcb.processing_options = "G".into();
            }
            let service = hisam_seed_metadata(store.clone(), run, metadata);
            hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
            let before = hisam_rows(&*store);
            let cursor = hisam_cursor(&service, run, 1);
            let service = if control == "saf" {
                ImsService::open_authorized(store.clone(), Default::default(), Arc::new(Deny))
                    .unwrap()
            } else {
                service
            };
            let result = hisam_public(
                service.clone(),
                run,
                request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B"),
            );
            if control == "saf" {
                assert_eq!(result, Err(HostProblem::Unauthorized));
                assert_eq!(hisam_rows(&*store), before);
            } else {
                assert_eq!(result.unwrap().status, "AM");
                assert_eq!(hisam_rows(&*store)[0], before[0]);
            }
            assert_eq!(hisam_cursor(&service, run, 1), cursor);
        });
    }
}

#[test]
fn hisam_nonunique_host_conditions_preserve_rows() {
    use mainframe_env_host_api::{HostLimits, RegistrySnapshot, ScopedHostService};
    hisam_backends("conditions", |store| {
        let run = "hisam-conditions";
        let service = hisam_seed(store.clone(), run);
        hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
        let registry = RegistrySnapshot::new(
            1,
            ims_providers(service, InvocationLimits::default()),
            InvocationLimits::default(),
        )
        .unwrap();
        let host = ScopedHostService::new(Arc::new(registry), HostLimits::default());
        for (condition, expected) in [
            ("deadline", HostProblem::TimedOut),
            ("cancel", HostProblem::Cancelled),
            ("capability", HostProblem::Unauthorized),
        ] {
            let mut inv = invocation_class(run, ServiceClass::Batch);
            if condition == "capability" {
                inv.principal = Principal::new(
                    inv.principal.id().clone(),
                    Default::default(),
                    InvocationLimits::default(),
                )
                .unwrap();
            }
            let req = request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B");
            let mutation = req.mutation.as_ref().unwrap();
            let before = hisam_rows(&*store);
            let result = host.invoke(
                &inv,
                if condition == "deadline" {
                    inv.deadline_tick
                } else {
                    1
                },
                condition == "cancel",
                EffectRequest {
                    run_unit: inv.run_unit_id.clone(),
                    sequence: mutation.sequence,
                    idempotency_key: Some(mutation.idempotency_key.clone()),
                    deadline_tick: inv.deadline_tick,
                    request: HostRequest::Ims(req),
                },
            );
            assert_eq!(
                result
                    .persist_with(|audit| store
                        .record_audit(audit)
                        .map_err(|_| HostProblem::InfrastructureFailure))
                    .outcome,
                Err(expected)
            );
            assert_eq!(hisam_rows(&*store), before);
        }
    });
}

#[test]
fn hisam_nonunique_real_interleaving_keeps_only_winning_occurrence() {
    hisam_backends("interleaving", |inner| {
        let store = isolation_tests::InterceptStore::new(inner);
        let run = "hisam-interleaving";
        let first = hisam_seed(store.clone(), run);
        hisam_public(first.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
        let second = ImsService::open(store.clone(), Default::default()).unwrap();
        store.mode.store(1, Ordering::SeqCst);
        let thread = std::thread::spawn(move || {
            hisam_public(
                first,
                run,
                request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B"),
            )
        });
        store.entered.wait();
        hisam_public(second.clone(), run, hisam_root(run, 6, 1, b"A1")).unwrap();
        assert_eq!(
            hisam_public(
                second.clone(),
                run,
                request(run, ImsOperation::Insert, 7, &["CHILD"], b"C1C")
            )
            .unwrap()
            .affected_segments,
            1
        );
        hisam_public(second, run, request(run, ImsOperation::Commit, 8, &[], b"")).unwrap();
        store.release.wait();
        assert_eq!(
            thread.join().unwrap(),
            Err(HostProblem::IdempotencyConflict)
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "hisam-interleaving-5")
                .unwrap()
                .is_none()
        );
        let reopened = ImsService::open(store, Default::default()).unwrap();
        hisam_public(reopened.clone(), run, hisam_root(run, 9, 1, b"A1")).unwrap();
        for (seq, expected) in [(10, &b"C1A"[..]), (11, &b"C1C"[..]), (12, &b"C2Z"[..])] {
            assert_eq!(
                hisam_public(
                    reopened.clone(),
                    run,
                    request(run, ImsOperation::GetNextParent, seq, &["CHILD"], b"")
                )
                .unwrap()
                .segments[0]
                    .data,
                expected
            );
        }
    });
}

fn hisam_rows(
    store: &dyn ProviderStateStore,
) -> Vec<Vec<mainframe_env_store_api::ProviderStateRecord>> {
    [
        GENERIC_DATABASE_NAMESPACE,
        SESSION_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        REPLAY_NAMESPACE,
    ]
    .iter()
    .map(|ns| store.list_provider_state(ns, 65_536).unwrap())
    .collect()
}

#[test]
fn hisam_nonunique_order_hold_replay_memory_and_file_sqlite() {
    for next in [ImsOperation::GetNext, ImsOperation::GetNextParent] {
        hisam_backends(&format!("order-{next:?}"), |store| {
            let run = "hisam-order";
            let service = hisam_seed(store.clone(), run);
            hisam_public(service.clone(), run, hisam_root(run, 4, 2, b"A2")).unwrap();
            let other = hisam_cursor(&service, run, 2);
            hisam_public(service.clone(), run, hisam_root(run, 5, 1, b"A1")).unwrap();
            let insert = request(run, ImsOperation::Insert, 6, &["CHILD"], b"C1B");
            let receipt = hisam_public(service.clone(), run, insert.clone()).unwrap();
            assert_eq!(receipt.status, "  ");
            assert_eq!(receipt.affected_segments, 1);
            assert_eq!(
                hisam_cursor(&service, run, 1),
                serde_json::json!({"current":5,"parentage":1,"held":null,"after_end":false})
            );
            assert_eq!(hisam_cursor(&service, run, 2), other);
            assert_eq!(
                hisam_public(service.clone(), run, request(run, next, 7, &["CHILD"], b""))
                    .unwrap()
                    .segments[0]
                    .data,
                b"C2Z"
            );
            hisam_public(service.clone(), run, hisam_root(run, 8, 1, b"A1")).unwrap();
            for (seq, bytes) in [(9, &b"C1A"[..]), (10, &b"C1B"[..]), (11, &b"C2Z"[..])] {
                assert_eq!(
                    hisam_public(
                        service.clone(),
                        run,
                        request(run, ImsOperation::GetNextParent, seq, &["CHILD"], b"")
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    bytes
                );
            }
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::GetNextParent, 12, &["CHILD"], b"")
                )
                .unwrap()
                .status,
                "GE"
            );
            assert_eq!(hisam_cursor(&service, run, 1)["current"], 3);
            assert_eq!(hisam_cursor(&service, run, 1)["parentage"], 1);
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::GetNext, 13, &[], b"")
                )
                .unwrap()
                .segments[0]
                    .data,
                b"A2Y"
            );
            hisam_public(service.clone(), run, hisam_root(run, 14, 1, b"A1")).unwrap();
            hisam_public(
                service.clone(),
                run,
                request(run, ImsOperation::GetHoldNextParent, 15, &["CHILD"], b""),
            )
            .unwrap();
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::GetHoldNextParent, 16, &["CHILD"], b"")
                )
                .unwrap()
                .segments[0]
                    .data,
                b"C1B"
            );
            assert_eq!(
                hisam_cursor(&service, run, 1)["held"],
                serde_json::json!({"id":5,"version":1})
            );
            for seq in [17, 18] {
                assert_eq!(
                    hisam_public(
                        service.clone(),
                        run,
                        request(run, ImsOperation::Replace, seq, &[], b"C1Q")
                    )
                    .unwrap()
                    .status,
                    "  "
                );
            }
            let held = hisam_cursor(&service, run, 1);
            assert_eq!(held["held"], serde_json::json!({"id":5,"version":3}));
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::Replace, 19, &[], b"C9Q")
                )
                .unwrap()
                .status,
                "DA"
            );
            assert_eq!(hisam_cursor(&service, run, 1), held);
            let stable = hisam_rows(&*store);
            assert_eq!(
                hisam_public(service.clone(), run, insert.clone()).unwrap(),
                receipt
            );
            assert_eq!(hisam_rows(&*store), stable);
            assert_eq!(hisam_cursor(&service, run, 1), held);
            let mut conflict = insert;
            conflict.data = b"C1D".to_vec();
            assert_eq!(
                hisam_public(service.clone(), run, conflict),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(hisam_rows(&*store), stable);
            hisam_public(
                service.clone(),
                run,
                request(run, ImsOperation::GetNextParent, 20, &["CHILD"], b""),
            )
            .unwrap();
            let unheld = hisam_cursor(&service, run, 1);
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::Replace, 21, &[], b"C2Q")
                )
                .unwrap()
                .status,
                "DJ"
            );
            assert_eq!(hisam_cursor(&service, run, 1), unheld);
            let state = service.lock().unwrap();
            let engine = restored(&state.state, "GENDB", service.limits).unwrap();
            assert_eq!(
                engine
                    .export_records()
                    .iter()
                    .find(|r| serde_json::to_value(r.id).unwrap() == 2)
                    .unwrap()
                    .data,
                b"C1A"
            );
            assert_eq!(
                engine
                    .export_records()
                    .iter()
                    .find(|r| serde_json::to_value(r.id).unwrap() == 5)
                    .unwrap()
                    .data,
                b"C1Q"
            );
            assert_eq!(hisam_cursor_after_unlock(&state.state, run, 2), other);
        });
    }
}

fn hisam_cursor_after_unlock(state: &State, run: &str, number: u16) -> serde_json::Value {
    serde_json::to_value(pcb::position(&state.sessions[run], number)).unwrap()
}

#[test]
fn hisam_nonunique_failure_cas_capacity_lost_ack_and_backout() {
    hisam_backends("failure", |inner| {
        let store = isolation_tests::InterceptStore::new(inner.clone());
        let run = "hisam-failure";
        let service = hisam_seed(store.clone(), run);
        hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
        let req = request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B");
        let before = hisam_rows(&*store);
        store.mode.store(2, Ordering::SeqCst);
        assert_eq!(
            hisam_public(service.clone(), run, req.clone()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(hisam_rows(&*store), before);
        store.mode.store(3, Ordering::SeqCst);
        assert_eq!(
            hisam_public(service, run, req.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        let reopened = ImsService::open(store.clone(), Default::default()).unwrap();
        let receipt = hisam_public(reopened.clone(), run, req.clone()).unwrap();
        assert_eq!(receipt.affected_segments, 1);
        let stable = hisam_rows(&*store);
        assert_eq!(hisam_public(reopened.clone(), run, req).unwrap(), receipt);
        assert_eq!(hisam_rows(&*store), stable);
        assert_eq!(
            restored(&reopened.lock().unwrap().state, "GENDB", reopened.limits)
                .unwrap()
                .record_count(),
            5
        );
        hisam_public(
            reopened.clone(),
            run,
            request(run, ImsOperation::Rollback, 6, &[], b""),
        )
        .unwrap();
        assert_eq!(
            hisam_cursor(&reopened, run, 1),
            serde_json::to_value(PcbPosition::default()).unwrap()
        );
        let engine = restored(&reopened.lock().unwrap().state, "GENDB", reopened.limits).unwrap();
        assert_eq!(
            engine
                .ordered_records()
                .iter()
                .map(|r| r.data.clone())
                .collect::<Vec<_>>(),
            [b"A1X", b"C1A", b"C2Z", b"A2Y"].map(|r| r.to_vec())
        );
        let limited = ImsService::open(
            inner.clone(),
            ImsLimits {
                max_roots: 4,
                ..Default::default()
            },
        )
        .unwrap();
        hisam_public(limited.clone(), run, hisam_root(run, 7, 1, b"A1")).unwrap();
        assert_eq!(
            hisam_public(
                limited.clone(),
                run,
                request(run, ImsOperation::Insert, 8, &["CHILD"], b"C1B")
            )
            .unwrap()
            .status,
            "FM"
        );
        assert_eq!(
            restored(&limited.lock().unwrap().state, "GENDB", limited.limits)
                .unwrap()
                .record_count(),
            4
        );
        let cas = session_cas::SessionCasStore::new(inner, run);
        let service = ImsService::open(cas.clone(), Default::default()).unwrap();
        let before = cas
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap();
        cas.arm();
        assert_eq!(
            hisam_public(
                service,
                run,
                request(run, ImsOperation::Insert, 9, &["CHILD"], b"C1B")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            cas.get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
                .unwrap(),
            before
        );
        assert!(
            cas.get_provider_state(REPLAY_NAMESPACE, "hisam-failure-9")
                .unwrap()
                .is_none()
        );
    });
}

#[test]
fn hisam_nonunique_sqlite_process_worker() {
    let Ok(url) = std::env::var("IMS_HISAM_PROCESS_URL") else {
        return;
    };
    let phase = std::env::var("IMS_HISAM_PROCESS_PHASE").unwrap();
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let run = "hisam-process";
    let service = if phase == "seed-hold" {
        hisam_seed(store, run)
    } else {
        ImsService::open(store, Default::default()).unwrap()
    };
    let insert = request(run, ImsOperation::Insert, 5, &["CHILD"], b"C1B");
    if phase == "seed-hold" {
        hisam_public(service.clone(), run, hisam_root(run, 4, 1, b"A1")).unwrap();
        assert_eq!(
            hisam_public(service.clone(), run, insert)
                .unwrap()
                .affected_segments,
            1
        );
        hisam_public(service.clone(), run, hisam_root(run, 6, 1, b"A1")).unwrap();
        hisam_public(
            service.clone(),
            run,
            request(run, ImsOperation::GetHoldNextParent, 7, &["CHILD"], b""),
        )
        .unwrap();
        assert_eq!(
            hisam_public(
                service.clone(),
                run,
                request(run, ImsOperation::GetHoldNextParent, 8, &["CHILD"], b"")
            )
            .unwrap()
            .segments[0]
                .data,
            b"C1B"
        );
    } else {
        let before = hisam_cursor(&service, run, 1);
        assert_eq!(before["held"], serde_json::json!({"id":5,"version":1}));
        assert_eq!(
            hisam_public(service.clone(), run, insert)
                .unwrap()
                .affected_segments,
            1
        );
        assert_eq!(hisam_cursor(&service, run, 1), before);
        if phase == "next-replace" {
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::Replace, 9, &[], b"C1Q")
                )
                .unwrap()
                .status,
                "  "
            );
            assert_eq!(
                hisam_cursor(&service, run, 1)["held"],
                serde_json::json!({"id":5,"version":2})
            );
            assert_eq!(
                hisam_public(
                    service.clone(),
                    run,
                    request(run, ImsOperation::GetNextParent, 10, &["CHILD"], b"")
                )
                .unwrap()
                .segments[0]
                    .data,
                b"C2Z"
            );
        } else {
            assert_eq!(phase, "reopen-replay");
        }
    }
    println!("HISAM_PHASE_OK:{phase}");
}

#[test]
fn hisam_nonunique_three_independent_sqlite_processes() {
    let file = std::env::temp_dir().join(format!(
        "hisam-process-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    for phase in ["seed-hold", "reopen-replay", "next-replace"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "service::generic::tests::closure_tests::hisam_nonunique_sqlite_process_worker",
                "--nocapture",
            ])
            .env("IMS_HISAM_PROCESS_URL", &url)
            .env("IMS_HISAM_PROCESS_PHASE", phase)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{phase}: {stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains("1 passed") && stdout.contains(&format!("HISAM_PHASE_OK:{phase}")));
        println!("{stdout}");
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn public_route_selects_every_pinned_organization_from_metadata() {
    use ImsDatabaseOrganization::*;
    for organization in [
        Dedb, Gsam, Hdam, Hidam, Hisam, Hsam, Index, Msdb, Phdam, Phidam, Psindex, Shisam, Shsam,
    ] {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        let mut metadata = catalog();
        let database = &mut metadata.databases[0];
        database.organization = organization;
        database.segments.truncate(1);
        if organization == Gsam {
            database.segments[0].fields[0].sequence = false;
        }
        metadata.psbs[0].pcbs[0] = ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
            name: "GENPCB".into(),
            database: "GENDB".into(),
            database_version: Some(1),
            secondary_index: None,
            processing_options: "AP".into(),
            sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                name: "ROOT".into(),
                parent: None,
                processing_options: None,
            }],
        });
        service.install_metadata(metadata).unwrap();
        let run = "org-run";
        let scheduled = execute(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        assert_eq!(scheduled.status, "  ", "{organization:?}");
        let index_database = matches!(organization, Index | Psindex);
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Insert, 2, &["ROOT"], b"R1A")
            )
            .status,
            if index_database { "AC" } else { "  " },
            "{organization:?}"
        );
        if index_database {
            let image = ImsGenericLoadImage {
                database: "GENDB".into(),
                records: vec![ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"R1A".to_vec(),
                }],
            };
            assert_eq!(
                execute(
                    &service,
                    run,
                    &request(
                        run,
                        ImsOperation::Load,
                        3,
                        &[],
                        &serde_json::to_vec(&image).unwrap()
                    )
                )
                .affected_segments,
                1
            );
        }
        let read_op = if organization == Gsam {
            ImsOperation::GetNext
        } else {
            ImsOperation::GetUnique
        };
        let read_run = if organization == Gsam {
            // A different normal PCB cannot read this writer's pending image.
            execute(
                &service,
                run,
                &request(run, ImsOperation::Commit, 3, &[], b""),
            );
            let fresh = "org-read";
            assert_eq!(
                execute(
                    &service,
                    fresh,
                    &request(fresh, ImsOperation::Schedule, 1, &[], b"")
                )
                .status,
                "  "
            );
            fresh
        } else {
            run
        };
        assert_eq!(
            execute(
                &service,
                read_run,
                &request(read_run, read_op, 4, &["ROOT"], b"")
            )
            .segments[0]
                .data,
            b"R1A",
            "{organization:?}"
        );
        if organization == Gsam {
            assert_eq!(
                execute(
                    &service,
                    run,
                    &request(run, ImsOperation::GetHoldUnique, 5, &["ROOT"], b"")
                )
                .status,
                "AC"
            );
            continue;
        }
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::GetHoldUnique, 5, &["ROOT"], b"")
            )
            .status,
            "  ",
            "{organization:?}"
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Replace, 6, &[], b"R1Z")
            )
            .status,
            "  ",
            "{organization:?}"
        );
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Delete, 7, &[], b"")
            )
            .affected_segments,
            1,
            "{organization:?}"
        );
    }
}

fn relationship_catalog(paired: bool) -> ImsMetadataCatalog {
    let mut metadata = catalog();
    metadata.databases[0]
        .logical_relationships
        .push(ImsLogicalRelationshipMetadata {
            parent_database: "PARENTDB".into(),
            parent_segment: "PROOT".into(),
            child_database: "GENDB".into(),
            child_segment: "CHILD".into(),
            paired,
        });
    metadata.databases.push(ImsDatabaseMetadata {
        gsam_format: None,
        name: "PARENTDB".into(),
        version: 1,
        organization: ImsDatabaseOrganization::Phidam,
        segments: vec![ImsSegmentMetadata {
            name: "PROOT".into(),
            parent: None,
            min_length: 3,
            max_length: 3,
            fields: vec![
                ImsFieldMetadata {
                    name: Some("PKEY".into()),
                    offset: 0,
                    length: 2,
                    sequence: true,
                    unique: true,
                },
                ImsFieldMetadata {
                    name: Some("VALUE".into()),
                    offset: 2,
                    length: 1,
                    sequence: false,
                    unique: false,
                },
            ],
        }],
        secondary_indexes: vec![],
        logical_relationships: vec![],
    });
    metadata.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(ImsDatabasePcbMetadata {
            name: "PPCB".into(),
            database: "PARENTDB".into(),
            database_version: Some(1),
            secondary_index: None,
            processing_options: "AP".into(),
            sensitive_segments: vec![ImsSensitiveSegmentMetadata {
                name: "PROOT".into(),
                parent: None,
                processing_options: None,
            }],
        }));
    metadata
}

pub(super) fn setup(service: &ImsService, paired: bool) {
    service
        .install_metadata(relationship_catalog(paired))
        .unwrap();
    let parent = "parent-run";
    let child = "child-run";
    let mut schedule = request(parent, ImsOperation::Schedule, 1, &[], b"");
    schedule.pcb = 2;
    assert_eq!(execute(service, parent, &schedule).status, "  ");
    assert_eq!(
        execute(
            service,
            child,
            &request(child, ImsOperation::Schedule, 1, &[], b"")
        )
        .status,
        "  "
    );
    assert_eq!(
        execute(
            service,
            parent,
            &parent_request(ImsOperation::Insert, 2, &["PROOT"], b"P1A")
        )
        .status,
        "  "
    );
    execute(
        service,
        parent,
        &request(parent, ImsOperation::Commit, 100, &[], b""),
    );
    assert_eq!(
        execute(
            service,
            child,
            &request(child, ImsOperation::Insert, 2, &["ROOT"], b"R1A")
        )
        .status,
        "  "
    );
}

pub(super) fn insert_child(service: &ImsService, sequence: u64) -> ImsRequest {
    let run = "child-run";
    let mut insert = request(run, ImsOperation::Insert, sequence, &["CHILD"], b"C1B");
    insert.qualifiers.push(ImsQualifier {
        segment: "PROOT".into(),
        field: "PKEY".into(),
        value: b"P1".to_vec(),
    });
    assert_eq!(execute(service, run, &insert).status, "  ");
    insert
}

pub(super) fn hold_parent(service: &ImsService, sequence: u64) {
    let run = "parent-run";
    let mut hold = parent_request(ImsOperation::GetHoldUnique, sequence, &["PROOT"], b"");
    hold.qualifiers.push(ImsQualifier {
        segment: "PROOT".into(),
        field: "PKEY".into(),
        value: b"P1".to_vec(),
    });
    assert_eq!(execute(service, run, &hold).status, "  ");
}

fn parent_request(op: ImsOperation, sequence: u64, segments: &[&str], data: &[u8]) -> ImsRequest {
    let mut request = request("parent-run", op, sequence, segments, data);
    request.pcb = 2;
    request
}

#[test]
fn logical_child_requires_resolved_parent_and_tracks_parent_replace() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    setup(&service, false);
    let before = restored(
        &service.lock().unwrap().state,
        "GENDB",
        ImsLimits::default(),
    )
    .unwrap()
    .state_digest();
    assert_eq!(
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Insert, 3, &["CHILD"], b"C1B")
        )
        .status,
        "GP"
    );
    assert_eq!(
        restored(
            &service.lock().unwrap().state,
            "GENDB",
            ImsLimits::default()
        )
        .unwrap()
        .state_digest(),
        before
    );
    let insert = insert_child(&service, 4);
    assert_eq!(execute(&service, "child-run", &insert).status, "  ");
    assert_eq!(
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::GetUnique, 5, &["CHILD"], b"")
        )
        .segments[0]
            .data,
        b"C1BP1A"
    );
    execute(
        &service,
        "child-run",
        &request("child-run", ImsOperation::Commit, 100, &[], b""),
    );
    hold_parent(&service, 6);
    assert_eq!(
        execute(
            &service,
            "parent-run",
            &parent_request(ImsOperation::Replace, 7, &[], b"P1Z")
        )
        .status,
        "  "
    );
    let read = request("child-run", ImsOperation::GetUnique, 8, &["CHILD"], b"");
    assert_eq!(
        service.execute(&invocation("child-run"), &read),
        Err(HostProblem::IdempotencyConflict)
    );
    execute(
        &service,
        "parent-run",
        &request("parent-run", ImsOperation::Commit, 9, &[], b""),
    );
    assert_eq!(
        execute(&service, "child-run", &read).segments[0].data,
        b"C1BP1Z"
    );
    assert!(ImsService::open(store, ImsLimits::default()).is_ok());
}

#[test]
fn unpaired_parent_delete_fails_without_partial_mutation() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    setup(&service, false);
    insert_child(&service, 3);
    execute(
        &service,
        "child-run",
        &request("child-run", ImsOperation::Commit, 100, &[], b""),
    );
    hold_parent(&service, 4);
    let before = service.lock().unwrap().state.generic_databases.clone();
    assert_eq!(
        execute(
            &service,
            "parent-run",
            &parent_request(ImsOperation::Delete, 5, &[], b"")
        )
        .status,
        "GP"
    );
    assert_eq!(service.lock().unwrap().state.generic_databases, before);
}

#[test]
fn paired_delete_rolls_back_both_databases_and_reopens_from_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-logical-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        setup(&service, true);
        insert_child(&service, 3);
        execute(
            &service,
            "child-run",
            &request("child-run", ImsOperation::Commit, 4, &[], b""),
        );
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Commit, 4, &[], b""),
        );
        hold_parent(&service, 5);
        assert_eq!(
            execute(
                &service,
                "parent-run",
                &parent_request(ImsOperation::Delete, 6, &[], b"")
            )
            .affected_segments,
            2
        );
        assert_eq!(
            service.execute(
                &invocation("child-run"),
                &request("child-run", ImsOperation::GetUnique, 7, &["CHILD"], b"")
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Rollback, 8, &[], b""),
        );
        assert_eq!(
            execute(
                &service,
                "child-run",
                &request("child-run", ImsOperation::GetUnique, 9, &["CHILD"], b"")
            )
            .segments[0]
                .data,
            b"C1BP1A"
        );
        hold_parent(&service, 10);
        execute(
            &service,
            "parent-run",
            &parent_request(ImsOperation::Delete, 11, &[], b""),
        );
        execute(
            &service,
            "parent-run",
            &request("parent-run", ImsOperation::Commit, 12, &[], b""),
        );
    }
    {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(
            execute(
                &service,
                "child-run",
                &request("child-run", ImsOperation::GetUnique, 13, &["CHILD"], b"")
            )
            .status,
            "GE"
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn corrupted_logical_parent_reference_is_rejected_on_reopen() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    setup(&service, true);
    insert_child(&service, 3);
    drop(service);
    let mut row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    let mut payload: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
    payload["value"]["logical_links"][0]["parent"] = serde_json::json!(999);
    row.payload = serde_json::to_vec(&payload).unwrap();
    let version = row.version;
    row.version += 1;
    store.put_provider_state(row, Some(version)).unwrap();
    assert!(matches!(
        ImsService::open(store, ImsLimits::default()),
        Err(HostProblem::InfrastructureFailure)
    ));
}

#[test]
fn paired_multi_database_write_is_atomic_when_store_capacity_is_exhausted() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits {
        max_provider_state: 64,
        ..Default::default()
    }));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    setup(&service, true);
    insert_child(&service, 3);
    execute(
        &service,
        "child-run",
        &request("child-run", ImsOperation::Commit, 4, &[], b""),
    );
    execute(
        &service,
        "parent-run",
        &request("parent-run", ImsOperation::Commit, 4, &[], b""),
    );
    hold_parent(&service, 5);
    let before = service.lock().unwrap().state.generic_databases.clone();
    let parent_row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "PARENTDB")
        .unwrap()
        .unwrap();
    let child_row = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    let count = store.list_provider_state_prefix("ims", 128).unwrap().len();
    for number in count..64 {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "test-fill".into(),
                    key: format!("{number}"),
                    version: 1,
                    payload: vec![1],
                },
                None,
            )
            .unwrap();
    }
    assert!(matches!(
        service.execute(
            &invocation("parent-run"),
            &parent_request(ImsOperation::Delete, 6, &[], b"")
        ),
        Err(HostProblem::ResourceExhausted)
    ));
    assert_eq!(service.lock().unwrap().state.generic_databases, before);
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "PARENTDB")
            .unwrap(),
        Some(parent_row)
    );
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap(),
        Some(child_row)
    );
}
