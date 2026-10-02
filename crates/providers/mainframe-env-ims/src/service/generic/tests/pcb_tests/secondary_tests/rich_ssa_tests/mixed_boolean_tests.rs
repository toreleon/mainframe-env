//! Local fail-closed acceptance, not an IBM mixed-expression evaluation oracle.
use super::*;

fn installed_mixed(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    let service = ImsService::open(store, Default::default()).unwrap();
    let mut metadata = indexed_catalog(true, true);
    metadata.databases[0].secondary_indexes[0].source_fields = vec!["KIND".into(), "ZONE".into()];
    service.install_metadata(metadata).unwrap();
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: [
            ("ROOT", None, b"A1ZX"),
            ("CHILD", Some(0), b"C1ZA"),
            ("ROOT", None, b"B2AX"),
            ("CHILD", Some(2), b"C2AZ"),
        ]
        .into_iter()
        .map(|(segment, parent, data)| ImsGenericLoadRecord {
            segment: segment.into(),
            parent,
            data: data.to_vec(),
        })
        .collect(),
    };
    call(
        &service,
        "mixed-seed",
        &request(
            "mixed-seed",
            ImsOperation::Load,
            1,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    call(
        &service,
        "mixed-seed",
        &request("mixed-seed", ImsOperation::Commit, 2, &[], b""),
    );
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    service
}

fn snapshot(service: &ImsService) -> (Vec<u8>, RowVersions) {
    let durable = service.lock().unwrap();
    (
        serde_json::to_vec(&durable.state).unwrap(),
        durable.versions.clone(),
    )
}

fn reject_mixed(service: Arc<ImsService>, run: &str, selected: u16) {
    let hold = nav(
        run,
        2 + u64::from(selected),
        ImsOperation::GetHoldUnique,
        selected,
        &[if selected == 1 {
            b"ROOT    *CP(A1)"
        } else {
            b"ROOT    *P(BYCHILD EQAZ)"
        }],
    );
    let found = public(service.clone(), run, hold.clone()).unwrap();
    assert_eq!(found.status, "  ");
    assert_eq!(
        found.segments[0].data,
        if selected == 1 { b"A1ZX" } else { b"B2AX" }
    );
    let before = snapshot(&service);
    let position = pcb::position(&service.lock().unwrap().state.sessions[run], selected);
    assert!(position.is_held());

    // All three predicates are true. Flattening the two AND identities used to
    // return success and publish a new session/replay row. Neither mixed AND nor
    // OR/AND precedence is established by the registered body pins.
    for (first, second) in [
        ('&', '#'),
        ('#', '*'),
        ('&', '|'),
        ('+', '*'),
        ('#', '|'),
        ('+', '#'),
    ] {
        let raw = format!("ROOT    (ROOTKEY GEA1{first}ROOTKEY LEB2{second}KIND    NE?)");
        for operation in [
            ImsOperation::GetUnique,
            ImsOperation::GetHoldNext,
            ImsOperation::GetNextParent,
        ] {
            assert_eq!(
                public(
                    service.clone(),
                    run,
                    nav(run, 20, operation, selected, &[raw.as_bytes()])
                ),
                Err(HostProblem::Unsupported),
                "{raw} / {operation:?} / PCB {selected}"
            );
            assert_eq!(snapshot(&service), before);
        }
    }
    // Multiple alternating groups and binary values must not be scanned as
    // connectors or coerced into a uniform expression.
    let mut raw = b"ROOT    *O(00030001LT".to_vec();
    raw.extend([255]);
    raw.extend(b"&00040001NE");
    raw.extend([0]);
    raw.extend(b"|00030001EQZ#00040001NEX)");
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 21, ImsOperation::GetUnique, selected, &[&raw])
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&service), before);
    for raw in [
        b"ROOT    ((ROOTKEY EQA1|ROOTKEY EQB2)&KIND    EQZ)".as_slice(),
        b"ROOT    (ROOTKEY EQA1&ROOTKEY EQB2#)".as_slice(),
        b"ROOT    (ROOTKEY EQA1&&KIND    EQZ)".as_slice(),
        b"ROOT    (ROOTKEY EQA1|ROOTKEY EQB2#KIND    EQ)".as_slice(),
    ] {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 22, ImsOperation::GetUnique, selected, &[raw])
            ),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&service), before);
    }
    assert_eq!(public(service.clone(), run, hold.clone()).unwrap(), found);
    assert_eq!(snapshot(&service), before);
    let mut conflict = hold;
    conflict.ssas[0] = if selected == 1 {
        b"ROOT    *CP(B2)".to_vec()
    } else {
        b"ROOT    *P(BYCHILD EQZA)".to_vec()
    };
    assert_eq!(
        public(service.clone(), run, conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(snapshot(&service), before);
}

#[test]
fn mixed_boolean_primary_no_flatten_memory() {
    let run = "mixed-primary";
    let service = installed_mixed(Arc::new(MemoryStore::new(Default::default())), run);
    reject_mixed(service, run, 1);
}

#[test]
fn mixed_boolean_selected_composite_no_flatten_memory() {
    let run = "mixed-index";
    let service = installed_mixed(Arc::new(MemoryStore::new(Default::default())), run);
    reject_mixed(service, run, 2);
}

#[test]
fn mixed_boolean_gap_preserves_uniform_encoding_aliases_and_saf_order() {
    let run = "mixed-saf";
    let store = Arc::new(MemoryStore::new(Default::default()));
    drop(installed_mixed(store.clone(), run));
    let policy = Arc::new(crate::service::generic::tests::Policy::default());
    let service = ImsService::open_authorized(store, Default::default(), policy.clone()).unwrap();
    for selected in [1, 2] {
        for (seq, raw) in [
            (
                10,
                b"ROOT    (ROOTKEY GEA1&ROOTKEY LEB2*KIND    NE?)".as_slice(),
            ),
            (
                11,
                b"ROOT    (ROOTKEY EQ??|ROOTKEY EQA1+ROOTKEY EQB2)".as_slice(),
            ),
            (
                12,
                b"ROOT    (ROOTKEY GEA1#ROOTKEY LEB2#KIND    NE?)".as_slice(),
            ),
        ] {
            let found = public(
                service.clone(),
                run,
                nav(
                    run,
                    seq + 10 * u64::from(selected),
                    ImsOperation::GetHoldUnique,
                    selected,
                    &[raw],
                ),
            )
            .unwrap();
            assert_eq!(found.status, "  ");
            assert_eq!(
                found.segments[0].data,
                if selected == 1 { b"A1ZX" } else { b"B2AX" }
            );
        }
    }
    *policy.deny_update.lock().unwrap() = true;
    let before = snapshot(&service);
    for selected in [1, 2] {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(
                    run,
                    50,
                    ImsOperation::GetUnique,
                    selected,
                    &[b"ROOT    (ROOTKEY GEA1&ROOTKEY LEB2#KIND    NE?)"]
                )
            ),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&service), before);
    }
}

#[test]
fn mixed_boolean_primary_secondary_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-mixed-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let run = "mixed-reopen";
    let before = {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = installed_mixed(store, run);
        reject_mixed(service.clone(), run, 1);
        reject_mixed(service.clone(), run, 2);
        snapshot(&service)
    };
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, Default::default()).unwrap();
        assert_eq!(snapshot(&service), before);
        reject_mixed(service.clone(), run, 1);
        reject_mixed(service.clone(), run, 2);
        assert_eq!(snapshot(&service), before);
    }
    std::fs::remove_file(file).unwrap();
}
