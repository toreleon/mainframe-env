use super::*;

#[test]
fn literal_null_ssa_slots_retrieve_and_hold_on_public_provider() {
    let run = "null-ssa-slots";
    let service = seed(Arc::new(MemoryStore::new(Default::default())), run);
    exercise(service, run);
}

fn exercise(service: Arc<ImsService>, run: &str) -> (ImsNavigationRequest, ImsResult) {
    let hold = navigation(
        run,
        5,
        ImsOperation::GetHoldUnique,
        &[b"ROOT    *--(ROOTKEY EQA1)"],
    );
    let got = public(service.clone(), run, hold.clone()).unwrap();
    assert_eq!(got.status, "  ");
    assert_eq!(got.segments[0].data, b"A1X");
    let before = snapshot(&service);
    let repeated = navigation(
        run,
        5,
        ImsOperation::GetHoldUnique,
        &[b"ROOT    *-(ROOTKEY EQA1)"],
    );
    assert_eq!(
        public(service.clone(), run, repeated),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(snapshot(&service), before);
    let mut replace = request(run, ImsOperation::Replace, 6, &["ROOT"], b"A1Z");
    replace.pcb = 1;
    assert_eq!(execute(&service, run, &replace).status, "  ");
    assert_eq!(
        public(
            service.clone(),
            run,
            navigation(run, 7, ImsOperation::GetNext, &[b"ROOT    *- "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"B2Y"
    );
    let before = snapshot(&service);
    assert_eq!(public(service.clone(), run, hold.clone()).unwrap(), got);
    assert_eq!(snapshot(&service), before);
    for (seq, raw) in [
        (8, b"ROOT    *-1 ".as_slice()),
        (9, b"ROOT    *-B "),
        (10, b"ROOT    *----------------- "),
    ] {
        assert!(
            public(
                service.clone(),
                run,
                navigation(run, seq, ImsOperation::GetUnique, &[raw])
            )
            .is_err()
        );
        assert_eq!(snapshot(&service), before);
    }
    (hold, got)
}

#[test]
fn null_ssa_slots_preserve_exact_replay_and_position_on_file_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-null-ssa-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let run = "null-ssa-sqlite";
    let (hold, got, before) = {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = seed(store, run);
        let (hold, got) = exercise(service.clone(), run);
        (hold, got, snapshot(&service))
    };
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(snapshot(&service), before);
        assert_eq!(public(service.clone(), run, hold).unwrap(), got);
        assert_eq!(snapshot(&service), before);
        assert_eq!(
            public(
                service,
                run,
                navigation(run, 11, ImsOperation::GetNext, &[b"ROOT    *- "])
            )
            .unwrap()
            .status,
            "GE"
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn null_ssa_slots_compose_with_path_parent_and_offset_commands() {
    let run = "null-ssa-path";
    let service = seed(Arc::new(MemoryStore::new(Default::default())), run);
    public(
        service.clone(),
        run,
        navigation(
            run,
            5,
            ImsOperation::GetUnique,
            &[b"ROOT    (ROOTKEY EQA1)"],
        ),
    )
    .unwrap();
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Insert, 6, &["CHILD"], b"C1-")
        )
        .status,
        "  "
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 7, &[], b""),
    );
    let path = public(
        service.clone(),
        run,
        navigation(
            run,
            8,
            ImsOperation::GetUnique,
            &[
                b"ROOT    *-D-P-(ROOTKEY EQA1)",
                b"CHILD   *-O-(00030001EQ-)",
            ],
        ),
    )
    .unwrap();
    assert_eq!(path.status, "  ");
    assert_eq!(
        path.segments
            .iter()
            .map(|s| s.data.as_slice())
            .collect::<Vec<_>>(),
        [b"A1X".as_slice(), b"C1-"]
    );
    let position = service.lock().unwrap().state.sessions[run].position.clone();
    assert!(position.current().is_some());
    assert!(position.parentage().is_some());
    assert_ne!(position.current(), position.parentage());
    let held = public(
        service.clone(),
        run,
        navigation(
            run,
            9,
            ImsOperation::GetHoldUnique,
            &[b"CHILD   *-C-(A1C1)"],
        ),
    )
    .unwrap();
    assert_eq!(held.segments[0].data, b"C1-");
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Replace, 10, &["CHILD"], b"C1+")
        )
        .status,
        "  "
    );
}
