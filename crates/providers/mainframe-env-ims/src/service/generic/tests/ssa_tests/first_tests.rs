//! Independent literals through the public SSA provider; no selection oracle helper.
use super::*;

mod fences;
mod publication;
mod recovery;

const RUN: &str = "ssa-first-run";
const FIRST: &[u8] = b"CHILD   *F ";

fn first_catalog() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let ImsPcbMetadata::Database(mut second) = metadata.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    second.name = "SECOND".into();
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(second));
    metadata
}

fn load(service: &Arc<ImsService>, run: &str) {
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: vec![
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A0X".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1X".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(1),
                data: b"C1Q".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(1),
                data: b"C2R".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(1),
                data: b"C3S".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2Y".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(5),
                data: b"D1T".to_vec(),
            },
        ],
    };
    assert_eq!(
        db(
            service.clone(),
            run,
            request(
                run,
                ImsOperation::Load,
                1,
                &[],
                &serde_json::to_vec(&image).unwrap()
            )
        )
        .status,
        "  "
    );
    assert_eq!(
        db(
            service.clone(),
            run,
            request(run, ImsOperation::Schedule, 2, &[], b"")
        )
        .status,
        "  "
    );
}

fn db(service: Arc<ImsService>, run: &str, request: ImsRequest) -> ImsResult {
    let host = HostRequest::Ims(request);
    host.validate(HostLimits::default()).unwrap();
    let mutation = host.mutation().unwrap();
    let effect = EffectRequest {
        run_unit: invocation(run).run_unit_id.clone(),
        sequence: mutation.sequence,
        idempotency_key: Some(mutation.idempotency_key.clone()),
        deadline_tick: 100,
        request: host,
    };
    match ims_providers(service, InvocationLimits::default())[1]
        .invoke(&invocation(run), effect)
        .outcome
        .unwrap()
    {
        HostResult::Ims(result) => result,
        other => panic!("expected IMS: {other:?}"),
    }
}

fn seeded(store: Arc<dyn ProviderStateStore>) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    service.install_metadata(first_catalog()).unwrap();
    load(&service, RUN);
    db(
        service.clone(),
        RUN,
        request(RUN, ImsOperation::Commit, 903, &[], b""),
    );
    service
}

fn nav(
    service: &Arc<ImsService>,
    seq: u64,
    pcb: u16,
    op: ImsOperation,
    ssas: &[&[u8]],
) -> Result<ImsResult, HostProblem> {
    let mut request = navigation(RUN, seq, op, ssas);
    request.request.pcb = pcb;
    public(service.clone(), RUN, request)
}

fn position(service: &ImsService, pcb: u16) -> PcbPosition {
    let state = service.lock().unwrap();
    super::super::super::pcb::position(&state.state.sessions[RUN], pcb)
}

fn fail_first(store: Arc<dyn ProviderStateStore>) {
    let service = seeded(store);
    assert_eq!(
        nav(
            &service,
            3,
            1,
            ImsOperation::GetUnique,
            &[b"ROOT    (ROOTKEY EQA1)"]
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1X"
    );
    let root = position(&service, 1);
    assert_eq!(
        nav(&service, 4, 1, ImsOperation::GetNextParent, &[b"CHILD    "])
            .unwrap()
            .segments[0]
            .data,
        b"C1Q"
    );
    let before = position(&service, 1);
    assert_ne!(before.current(), root.current());
    let selected = nav(&service, 5, 1, ImsOperation::GetNextParent, &[FIRST]);
    assert_eq!(
        selected.as_ref().map(|r| r.segments[0].data.as_slice()),
        Ok(b"C1Q".as_slice()),
        "first must execute through the public provider"
    );
    let result = selected.unwrap();
    assert_eq!(&result.segments[0].data[..2], b"C1");
    assert_eq!(
        result.segments[0].parent_key.as_deref(),
        Some(b"A1".as_slice())
    );
    let after = position(&service, 1);
    assert_eq!(after.current(), before.current());
    assert_eq!(after.parentage(), root.current());
    assert!(!after.is_held());
    let durable = service.lock().unwrap();
    let engine = restored(&durable.state, "GENDB", ImsLimits::default()).unwrap();
    assert_eq!(
        engine.path_to(after.current().unwrap()).unwrap()[1].data,
        b"C1Q"
    );
}

fn sqlite() -> (std::path::PathBuf, String, Arc<dyn ProviderStateStore>) {
    let file = std::env::temp_dir().join(format!(
        "ims-ssa-first-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    (file, url, store)
}

#[test]
fn ssa_first_direct_child_fail_first_memory() {
    fail_first(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn ssa_first_direct_child_fail_first_sqlite() {
    let (file, _, store) = sqlite();
    fail_first(store);
    std::fs::remove_file(file).unwrap();
}

fn backends(label: &str, mut case: impl FnMut(Arc<dyn ProviderStateStore>, Option<&str>)) {
    eprintln!("scenario {label}/memory");
    case(Arc::new(MemoryStore::new(Default::default())), None);
    let (file, url, store) = sqlite();
    eprintln!("scenario {label}/sqlite");
    case(store, Some(&url));
    std::fs::remove_file(file).unwrap();
}

fn cursor(service: &ImsService, number: u16) -> serde_json::Value {
    serde_json::to_value(position(service, number)).unwrap()
}

fn rows(store: &dyn ProviderStateStore) -> Vec<ProviderStateRecord> {
    [
        GENERIC_DATABASE_NAMESPACE,
        GENERIC_PENDING_NAMESPACE,
        SESSION_NAMESPACE,
        REPLAY_NAMESPACE,
        SYSTEM_NAMESPACE,
        CHECKPOINT_NAMESPACE,
        STATE_NAMESPACE,
    ]
    .into_iter()
    .flat_map(|namespace| store.list_provider_state(namespace, 4096).unwrap())
    .collect()
}

#[test]
fn ssa_first_direct_child_root_intermediate_last_repeat_holds_continuations_pcbs() {
    backends("starts", |store, _| {
        let service = seeded(store);
        assert_eq!(
            nav(
                &service,
                3,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQB2)"]
            )
            .unwrap()
            .segments[0]
                .data,
            b"B2Y"
        );
        let other = position(&service, 2);
        for (op_index, op) in [ImsOperation::GetNextParent, ImsOperation::GetHoldNextParent]
            .into_iter()
            .enumerate()
        {
            for advance in 0..=3u64 {
                let seq = 10 + op_index as u64 * 100 + advance * 20;
                eprintln!("scenario first/op={op:?}/advance={advance}");
                assert_eq!(
                    nav(
                        &service,
                        seq,
                        1,
                        ImsOperation::GetHoldUnique,
                        &[b"ROOT    (ROOTKEY EQA1)"]
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"A1X"
                );
                for i in 0..advance {
                    assert_eq!(
                        nav(
                            &service,
                            seq + 1 + i,
                            1,
                            ImsOperation::GetHoldNextParent,
                            &[b"CHILD    "]
                        )
                        .unwrap()
                        .segments[0]
                            .data,
                        [b"C1Q", b"C2R", b"C3S"][i as usize]
                    );
                }
                assert!(position(&service, 1).is_held());
                for offset in [5, 6, 7] {
                    let r = nav(&service, seq + offset, 1, op, &[FIRST]).unwrap();
                    assert_eq!(
                        r,
                        ImsResult {
                            status: "  ".into(),
                            segments: vec![mainframe_env_host_api::ImsSegment {
                                name: "CHILD".into(),
                                parent_key: Some(b"A1".to_vec()),
                                data: b"C1Q".to_vec()
                            }],
                            checkpoint_id: None,
                            affected_segments: 0,
                            system: None,
                        }
                    );
                    assert_eq!(r.status, "  ");
                    assert_eq!(r.segments[0].data, b"C1Q");
                    assert_eq!(r.segments[0].parent_key.as_deref(), Some(b"A1".as_slice()));
                    assert_eq!(
                        cursor(&service, 1),
                        serde_json::json!({"current":3,"parentage":2,
                        "held":if op==ImsOperation::GetHoldNextParent {serde_json::json!({"id":3,"version":1})} else {serde_json::Value::Null},"after_end":false})
                    );
                    let state = service.lock().unwrap();
                    let engine = restored(&state.state, "GENDB", Default::default()).unwrap();
                    let path = engine
                        .path_to(state.state.sessions[RUN].position.current().unwrap())
                        .unwrap();
                    assert_eq!(path[0].data, b"A1X");
                    assert_eq!(path[1].data, b"C1Q");
                }
                // Both ordinary forms continue immediately after C1, independently.
                assert_eq!(
                    nav(
                        &service,
                        seq + 8,
                        1,
                        ImsOperation::GetNextParent,
                        &[b"CHILD    "]
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C2R"
                );
                assert!(!position(&service, 1).is_held());
                assert_eq!(
                    nav(
                        &service,
                        seq + 9,
                        1,
                        ImsOperation::GetHoldNextParent,
                        &[FIRST]
                    )
                    .unwrap()
                    .segments[0]
                        .data,
                    b"C1Q"
                );
                assert_eq!(
                    nav(&service, seq + 10, 1, ImsOperation::GetNext, &[])
                        .unwrap()
                        .segments[0]
                        .data,
                    b"C2R"
                );
                assert_eq!(position(&service, 2), other);
            }
        }
    });
}

#[test]
fn ssa_first_direct_child_existing_gn_ghn_and_prior_p_parentage() {
    backends("parentage", |store, _| {
        let service = seeded(store);
        for (i, setup) in [ImsOperation::GetNext, ImsOperation::GetHoldNext]
            .into_iter()
            .enumerate()
        {
            let seq = 100 + i as u64 * 20;
            assert_eq!(
                db(
                    service.clone(),
                    RUN,
                    request(RUN, ImsOperation::Rollback, seq, &[], b"")
                )
                .status,
                "  "
            );
            assert_eq!(
                nav(&service, seq + 1, 1, setup, &[b"ROOT    (ROOTKEY EQA1)"])
                    .unwrap()
                    .segments[0]
                    .data,
                b"A1X"
            );
            assert_eq!(
                nav(
                    &service,
                    seq + 2,
                    1,
                    ImsOperation::GetHoldNextParent,
                    &[FIRST]
                )
                .unwrap()
                .segments[0]
                    .data,
                b"C1Q"
            );
            assert_eq!(
                cursor(&service, 1),
                serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
            );
            assert_eq!(
                nav(&service, seq + 3, 1, ImsOperation::GetNext, &[])
                    .unwrap()
                    .segments[0]
                    .data,
                b"C2R"
            );
        }
        assert_eq!(
            nav(
                &service,
                160,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    *P(ROOTKEY EQA1)", b"CHILD   (CHILDKEYEQC1)"]
            )
            .unwrap()
            .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
        );
        assert_eq!(
            nav(&service, 161, 1, ImsOperation::GetNextParent, &[FIRST])
                .unwrap()
                .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":3,"parentage":2,"held":null,"after_end":false})
        );
        assert_eq!(
            nav(&service, 162, 1, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"C2R"
        );
    });
}

#[test]
fn ssa_first_direct_child_empty_root_ge_keeps_anchor_cancels_hold_and_gn_crosses() {
    backends("empty-root", |store, _| {
        let service = ImsService::open(store, Default::default()).unwrap();
        service.install_metadata(first_catalog()).unwrap();
        let image = ImsGenericLoadImage {
            database: "GENDB".into(),
            records: vec![
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"A0X".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: b"B2Y".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(1),
                    data: b"D1T".to_vec(),
                },
            ],
        };
        db(
            service.clone(),
            RUN,
            request(
                RUN,
                ImsOperation::Load,
                1,
                &[],
                &serde_json::to_vec(&image).unwrap(),
            ),
        );
        db(
            service.clone(),
            RUN,
            request(RUN, ImsOperation::Schedule, 2, &[], b""),
        );
        nav(
            &service,
            3,
            2,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (ROOTKEY EQB2)"],
        )
        .unwrap();
        let other = position(&service, 2);
        for (i, op) in [ImsOperation::GetNextParent, ImsOperation::GetHoldNextParent]
            .into_iter()
            .enumerate()
        {
            let seq = 10 + i as u64 * 20;
            assert_eq!(
                nav(
                    &service,
                    seq,
                    1,
                    ImsOperation::GetHoldUnique,
                    &[b"ROOT    (ROOTKEY EQA0)"]
                )
                .unwrap()
                .segments[0]
                    .data,
                b"A0X"
            );
            for offset in 1..=3 {
                let result = nav(&service, seq + offset, 1, op, &[FIRST]).unwrap();
                assert_eq!(result.status, "GE");
                assert!(result.segments.is_empty());
                assert_eq!(position(&service, 2), other);
                assert_eq!(
                    cursor(&service, 1),
                    serde_json::json!({"current":1,"parentage":1,"held":null,"after_end":false})
                );
            }
            assert_eq!(
                nav(
                    &service,
                    seq + 4,
                    1,
                    ImsOperation::GetNextParent,
                    &[b"CHILD    "]
                )
                .unwrap()
                .status,
                "GE"
            );
            assert_eq!(
                nav(&service, seq + 5, 1, ImsOperation::GetNext, &[])
                    .unwrap()
                    .segments[0]
                    .data,
                b"B2Y"
            );
        }
    });
}

#[test]
fn ssa_first_direct_child_versioned_replace_backout_replay_and_reopen() {
    backends("replace-replay", |store, url| {
        let service = seeded(store.clone());
        assert_eq!(
            nav(
                &service,
                3,
                1,
                ImsOperation::GetUnique,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1X"
        );
        let hold = navigation(RUN, 4, ImsOperation::GetHoldNextParent, &[FIRST]);
        let result = public(service.clone(), RUN, hold.clone()).unwrap();
        assert_eq!(result.segments[0].data, b"C1Q");
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
        );
        assert_eq!(
            db(
                service.clone(),
                RUN,
                request(RUN, ImsOperation::Replace, 5, &["CHILD"], b"C1Z")
            )
            .status,
            "  "
        );
        let state = service.lock().unwrap();
        let image = serde_json::to_value(&state.state.generic_databases["GENDB"]).unwrap();
        assert_eq!(image["records"][2]["data"], serde_json::json!([67, 49, 90]));
        assert_eq!(image["records"][2]["version"], 2);
        assert_eq!(image["records"][4]["data"], serde_json::json!([67, 51, 83]));
        assert_eq!(image["records"][3]["data"], serde_json::json!([67, 50, 82]));
        drop(state);
        assert_eq!(
            db(
                service.clone(),
                RUN,
                request(RUN, ImsOperation::Rollback, 6, &[], b"")
            )
            .status,
            "  "
        );
        assert_eq!(position(&service, 1), PcbPosition::default());
        assert_eq!(public(service.clone(), RUN, hold.clone()).unwrap(), result);
        assert_eq!(position(&service, 1), PcbPosition::default());
        assert_eq!(
            nav(
                &service,
                7,
                1,
                ImsOperation::GetUnique,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1X"
        );
        assert_eq!(
            nav(&service, 8, 1, ImsOperation::GetNextParent, &[FIRST])
                .unwrap()
                .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(
            nav(&service, 9, 1, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"C2R"
        );
        let before = rows(&*store);
        assert_eq!(public(service.clone(), RUN, hold.clone()).unwrap(), result);
        assert_eq!(rows(&*store), before);
        for kind in 0..3 {
            let mut conflict = hold.clone();
            match kind {
                0 => conflict.ssas[0] = b"CHILD   *-F- ".to_vec(),
                1 => conflict.context = ImsExecutionContext::DbDc,
                _ => conflict.request.pcb = 2,
            }
            assert_eq!(
                public(service.clone(), RUN, conflict),
                Err(HostProblem::IdempotencyConflict)
            );
            assert_eq!(rows(&*store), before);
        }
        let expected = snapshot(&service);
        drop(service);
        let reopened_store: Arc<dyn ProviderStateStore> = url.map_or_else(
            || store.clone(),
            |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
        );
        let reopened = ImsService::open(reopened_store.clone(), ImsLimits::default()).unwrap();
        assert_eq!(snapshot(&reopened), expected);
        assert_eq!(public(reopened.clone(), RUN, hold).unwrap(), result);
        assert_eq!(snapshot(&reopened), expected);
        assert_eq!(
            cursor(&reopened, 1),
            serde_json::json!({"current":4,"parentage":4,"held":null,"after_end":false})
        );
    });
}
