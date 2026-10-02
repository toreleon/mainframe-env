//! Expectations from pinned SSA/index rules, exercised through the public provider.
use super::*;
use crate::service::generic::tests::ssa_tests::public;
use mainframe_env_host_api::ImsNavigationRequest;

mod integration_tests;
mod lifecycle_tests;
mod mixed_boolean_tests;
mod process_tests;
mod rejection_tests;

fn nav(run: &str, seq: u64, op: ImsOperation, pcb: u16, ssas: &[&[u8]]) -> ImsNavigationRequest {
    ImsNavigationRequest {
        request: selected(run, op, seq, pcb, &[], b""),
        context: ImsExecutionContext::DbBatch,
        ssas: ssas.iter().map(|s| s.to_vec()).collect(),
    }
}

fn installed(store: Arc<dyn ProviderStateStore>, run: &str, composite: bool) -> Arc<ImsService> {
    let service = ImsService::open(store, Default::default()).unwrap();
    seed_index(&service, composite, true);
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    service
}

fn exercise_composite(store: Arc<dyn ProviderStateStore>) {
    let run = "rich-index";
    let service = installed(store.clone(), run, true);
    for (seq, raw, expected) in [
        (2, b"ROOT    (BYCHILD EQZA)".as_slice(), b"B2AX"),
        (3, b"ROOT    (BYCHILD NEZA)".as_slice(), b"A1ZX"),
    ] {
        let found = public(
            service.clone(),
            run,
            nav(run, seq, ImsOperation::GetUnique, 2, &[raw]),
        )
        .unwrap();
        assert_eq!(
            (found.status.as_str(), found.segments[0].data.as_slice()),
            ("  ", expected.as_slice())
        );
    }
    for (i, (relation, key, expected)) in [
        ("LT", "ZA", b"A1ZX"),
        ("LE", "AZ", b"A1ZX"),
        ("GT", "AZ", b"B2AX"),
        ("GE", "ZA", b"B2AX"),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = format!("ROOT    (BYCHILD {relation}{key})");
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(
                    run,
                    10 + i as u64,
                    ImsOperation::GetUnique,
                    2,
                    &[raw.as_bytes()]
                )
            )
            .unwrap()
            .segments[0]
                .data,
            expected
        );
    }
    for (i, raw) in [
        b"ROOT    (BYCHILD GEAZ&KIND    EQZ)".as_slice(),
        b"ROOT    (BYCHILD GEAZ#KIND    EQZ)".as_slice(),
        b"ROOT    (BYCHILD GEAZ*KIND    EQZ)".as_slice(),
        b"ROOT    (BYCHILD EQ??|KIND    EQZ)".as_slice(),
        b"ROOT    (BYCHILD EQ??+KIND    EQZ)".as_slice(),
        b"ROOT    *O(00040001EQX)".as_slice(),
        b"ROOT    *C(A1)".as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 20 + i as u64, ImsOperation::GetUnique, 2, &[raw])
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1ZX"
        );
    }
    let mut raw = b"ROOT    (BYCHILD LT".to_vec();
    raw.extend([0xff, 0xff, b')']);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 30, ImsOperation::GetUnique, 2, &[&raw])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
    let path = public(
        service.clone(),
        run,
        nav(
            run,
            31,
            ImsOperation::GetHoldUnique,
            2,
            &[b"ROOT    *DP(BYCHILD EQZA)", b"CHILD    "],
        ),
    )
    .unwrap();
    assert_eq!(
        path.segments
            .iter()
            .map(|s| s.data.as_slice())
            .collect::<Vec<_>>(),
        vec![b"B2AX".as_slice(), b"C2AZ".as_slice()]
    );
    let position = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert!(position.is_held());
    assert_ne!(position.current(), position.parentage());
    let failed = public(
        service.clone(),
        run,
        nav(
            run,
            32,
            ImsOperation::GetNextParent,
            2,
            &[b"ROOT    (BYCHILD EQAZ)", b"CHILD    "],
        ),
    )
    .unwrap();
    assert_eq!(failed.status, "GE");
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        position
    );
    public(
        service.clone(),
        run,
        nav(
            run,
            33,
            ImsOperation::GetUnique,
            2,
            &[b"ROOT    (BYCHILD EQAZ)"],
        ),
    )
    .unwrap();
    let root = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    let child = public(
        service.clone(),
        run,
        nav(
            run,
            34,
            ImsOperation::GetNextParent,
            2,
            &[b"ROOT    (BYCHILD EQAZ)", b"CHILD    "],
        ),
    )
    .unwrap();
    assert_eq!(child.segments[0].data, b"C1ZA");
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2).parentage(),
        root.parentage()
    );
    let before = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    drop(service);
    let reopened = ImsService::open(store, Default::default()).unwrap();
    assert_eq!(
        pcb::position(&reopened.lock().unwrap().state.sessions[run], 2),
        before
    );
    assert_eq!(
        public(
            reopened,
            run,
            nav(run, 35, ImsOperation::GetNext, 2, &[b"ROOT     "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"B2AX"
    );
}

#[test]
fn secondary_ssa_composite_pointer_qualification_relations_and_commands() {
    exercise_composite(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn secondary_ssa_composite_pointer_qualification_sqlite_fresh_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-rich-composite-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    exercise_composite(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn secondary_ssa_binary_composite_zero_and_ff_boundaries() {
    let run = "binary-index";
    let service = installed(Arc::new(MemoryStore::new(Default::default())), run, true);
    public(
        service.clone(),
        run,
        nav(
            run,
            2,
            ImsOperation::GetHoldUnique,
            1,
            &[b"ROOT    *P ", b"CHILD    "],
        ),
    )
    .unwrap();
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 3, 1, &[], b"C1\xff\0")
        )
        .status,
        "  "
    );
    let mut raw = b"ROOT    (BYCHILD EQ".to_vec();
    raw.extend([0, 255, b')']);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 4, ImsOperation::GetUnique, 2, &[&raw])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
    let mut raw = b"ROOT    (BYCHILD LT".to_vec();
    raw.extend([0, 255, b')']);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 5, ImsOperation::GetUnique, 2, &[&raw])
        )
        .unwrap()
        .status,
        "GE"
    );
    let mut raw = b"ROOT    (BYCHILD GE".to_vec();
    raw.extend([0, 255, b')']);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 6, ImsOperation::GetUnique, 2, &[&raw])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
    let mut raw = b"ROOT    *O(00030001LT".to_vec();
    raw.extend([255, b')']);
    assert_eq!(
        public(
            service,
            run,
            nav(run, 7, ImsOperation::GetUnique, 2, &[&raw])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
}
