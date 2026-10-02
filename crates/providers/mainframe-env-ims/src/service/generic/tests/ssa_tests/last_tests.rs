//! Independent literals through the public SSA provider; no selection oracle helper.
use super::*;

mod fences;
mod publication;
mod recovery;

const RUN: &str = "ssa-last-run";
const LAST: &[u8] = b"CHILD   *L ";

fn last_catalog() -> ImsMetadataCatalog {
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
    service.install_metadata(last_catalog()).unwrap();
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
    let selected = nav(&service, 5, 1, ImsOperation::GetNextParent, &[LAST]);
    assert_eq!(
        selected.as_ref().map(|r| r.segments[0].data.as_slice()),
        Ok(b"C3S".as_slice()),
        "last must execute through the public provider"
    );
    let result = selected.unwrap();
    assert_eq!(&result.segments[0].data[..2], b"C3");
    assert_eq!(
        result.segments[0].parent_key.as_deref(),
        Some(b"A1".as_slice())
    );
    let after = position(&service, 1);
    assert_ne!(after.current(), before.current());
    assert_eq!(after.parentage(), root.current());
    assert!(!after.is_held());
    let durable = service.lock().unwrap();
    let engine = restored(&durable.state, "GENDB", ImsLimits::default()).unwrap();
    assert_eq!(
        engine.path_to(after.current().unwrap()).unwrap()[1].data,
        b"C3S"
    );
}

fn sqlite() -> (std::path::PathBuf, String, Arc<dyn ProviderStateStore>) {
    let file = std::env::temp_dir().join(format!(
        "ims-ssa-last-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    (file, url, store)
}

#[test]
fn ssa_last_direct_child_fail_first_memory() {
    fail_first(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn ssa_last_direct_child_fail_first_sqlite() {
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
fn ssa_last_direct_child_forward_starts_holds_ge_followups_and_pcbs() {
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
        for (op_index, operation) in [ImsOperation::GetNextParent, ImsOperation::GetHoldNextParent]
            .into_iter()
            .enumerate()
        {
            for (start_index, (root, advance)) in [
                (b"ROOT    (ROOTKEY EQA1)".as_slice(), 0),
                (b"ROOT    (ROOTKEY EQA1)", 1),
                (b"ROOT    (ROOTKEY EQA1)", 3),
            ]
            .into_iter()
            .enumerate()
            {
                let seq = 10 + (op_index * 100 + start_index * 20) as u64;
                eprintln!("scenario starts/op={operation:?}/start={start_index}");
                assert_eq!(
                    nav(&service, seq, 1, ImsOperation::GetHoldUnique, &[root])
                        .unwrap()
                        .status,
                    "  "
                );
                for i in 0..advance {
                    let r = nav(
                        &service,
                        seq + 1 + i,
                        1,
                        ImsOperation::GetHoldNextParent,
                        &[b"CHILD    "],
                    )
                    .unwrap();
                    assert_eq!(r.segments[0].data, [b"C1Q", b"C2R", b"C3S"][i as usize]);
                }
                let before = position(&service, 1);
                assert!(before.is_held());
                let found = nav(&service, seq + 5, 1, operation, &[LAST]).unwrap();
                if start_index < 2 {
                    assert_eq!(found.status, "  ");
                    assert_eq!(found.segments[0].data, b"C3S");
                    assert_eq!(
                        found.segments[0].parent_key.as_deref(),
                        Some(b"A1".as_slice())
                    );
                    assert_eq!(
                        cursor(&service, 1),
                        serde_json::json!({"current":5,"parentage":2,"held":if operation == ImsOperation::GetHoldNextParent {serde_json::json!({"id":5,"version":1})} else {serde_json::Value::Null},"after_end":false})
                    );
                } else {
                    assert_eq!(found.status, "GE");
                    assert!(found.segments.is_empty());
                    assert_eq!(position(&service, 1).current(), before.current());
                    assert_eq!(position(&service, 1).parentage(), before.parentage());
                    assert!(!position(&service, 1).is_held());
                }
                for offset in [6, 7] {
                    let r = nav(&service, seq + offset, 1, operation, &[LAST]).unwrap();
                    assert_eq!(r.status, "GE");
                    assert!(r.segments.is_empty());
                }
                let exhausted = position(&service, 1);
                assert!(!exhausted.is_held());
                assert_eq!(
                    nav(
                        &service,
                        seq + 8,
                        1,
                        ImsOperation::GetNextParent,
                        &[b"CHILD    "]
                    )
                    .unwrap()
                    .status,
                    "GE"
                );
                assert_eq!(position(&service, 1), exhausted);
                assert_eq!(
                    nav(&service, seq + 9, 1, ImsOperation::GetNext, &[])
                        .unwrap()
                        .segments[0]
                        .data,
                    b"B2Y"
                );
                assert_eq!(position(&service, 2), other);
            }
        }
    });
}

#[test]
fn ssa_last_direct_child_existing_gn_ghn_and_prior_p_parentage() {
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
                    &[LAST]
                )
                .unwrap()
                .segments[0]
                    .data,
                b"C3S"
            );
            assert_eq!(
                cursor(&service, 1),
                serde_json::json!({"current":5,"parentage":2,"held":{"id":5,"version":1},"after_end":false})
            );
            assert_eq!(
                nav(&service, seq + 3, 1, ImsOperation::GetNext, &[])
                    .unwrap()
                    .segments[0]
                    .data,
                b"B2Y"
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
            nav(&service, 161, 1, ImsOperation::GetNextParent, &[LAST])
                .unwrap()
                .segments[0]
                .data,
            b"C3S"
        );
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":5,"parentage":2,"held":null,"after_end":false})
        );
        assert_eq!(
            nav(&service, 162, 1, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"B2Y"
        );
    });
}

#[test]
fn ssa_last_direct_child_empty_root_ge_keeps_anchor_cancels_hold_and_gn_crosses() {
    backends("empty-root", |store, _| {
        let service = ImsService::open(store, Default::default()).unwrap();
        service.install_metadata(last_catalog()).unwrap();
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
                let result = nav(&service, seq + offset, 1, op, &[LAST]).unwrap();
                assert_eq!(result.status, "GE");
                assert!(result.segments.is_empty());
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
fn ssa_last_direct_child_versioned_replace_backout_replay_and_reopen() {
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
        let hold = navigation(RUN, 4, ImsOperation::GetHoldNextParent, &[LAST]);
        let result = public(service.clone(), RUN, hold.clone()).unwrap();
        assert_eq!(result.segments[0].data, b"C3S");
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":5,"parentage":2,"held":{"id":5,"version":1},"after_end":false})
        );
        assert_eq!(
            db(
                service.clone(),
                RUN,
                request(RUN, ImsOperation::Replace, 5, &["CHILD"], b"C3Z")
            )
            .status,
            "  "
        );
        let state = service.lock().unwrap();
        let image = serde_json::to_value(&state.state.generic_databases["GENDB"]).unwrap();
        assert_eq!(image["records"][4]["data"], serde_json::json!([67, 51, 90]));
        assert_eq!(image["records"][4]["version"], 2);
        assert_eq!(image["records"][2]["data"], serde_json::json!([67, 49, 81]));
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
            nav(&service, 8, 1, ImsOperation::GetNextParent, &[LAST])
                .unwrap()
                .segments[0]
                .data,
            b"C3S"
        );
        assert_eq!(
            nav(&service, 9, 1, ImsOperation::GetNext, &[])
                .unwrap()
                .segments[0]
                .data,
            b"B2Y"
        );
        let before = rows(&*store);
        assert_eq!(public(service.clone(), RUN, hold.clone()).unwrap(), result);
        assert_eq!(rows(&*store), before);
        for kind in 0..3 {
            let mut conflict = hold.clone();
            match kind {
                0 => conflict.ssas[0] = b"CHILD   *L  ".to_vec(),
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
            serde_json::json!({"current":6,"parentage":6,"held":null,"after_end":false})
        );
    });
}
