use super::*;

fn exercise(store: Arc<dyn ProviderStateStore>) {
    let run = "ssa-maintenance";
    let service = installed(store.clone(), run, false);
    let get = nav(run, 2, ImsOperation::GetHoldUnique, 1, &[b"ROOT     "]);
    assert_eq!(
        public(service.clone(), run, get).unwrap().segments[0].data,
        b"A1ZX"
    );
    let primary = service.lock().unwrap().state.sessions[run].position.clone();
    let indexed = nav(run, 3, ImsOperation::GetHoldNext, 2, &[b"ROOT     "]);
    assert_eq!(
        public(service.clone(), run, indexed.clone())
            .unwrap()
            .segments[0]
            .data,
        b"B2AX"
    );
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        primary
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(
                run,
                4,
                ImsOperation::GetHoldUnique,
                3,
                &[b"ROOT    (BYROOT  EQA)"]
            )
        )
        .unwrap()
        .segments[0]
            .data,
        b"B2AX"
    );
    let other = pcb::position(&service.lock().unwrap().state.sessions[run], 3);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 5, ImsOperation::GetNext, 2, &[b"ROOT     "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 3),
        other
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 6, 3, &[], b"B2AQ")
        )
        .status,
        "  "
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 3).parentage(),
        other.parentage()
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 7, 3, &[], b"B2YQ")
        )
        .status,
        "  "
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 3).parentage(),
        None
    );
    assert_eq!(lookup(&service, b"Y"), vec![b"B2YQ".to_vec()]);
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 8, 1, &[], b"A1WX")
        )
        .status,
        "  "
    );
    assert_eq!(lookup(&service, b"W"), vec![b"A1WX".to_vec()]);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(
                run,
                9,
                ImsOperation::GetHoldUnique,
                1,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1WX"
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Delete, 10, 1, &[], b"")
        )
        .affected_segments,
        2
    );
    assert!(lookup(&service, b"W").is_empty());
    call(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 11, &[], b""),
    );
    assert_eq!(lookup(&service, b"Z"), vec![b"A1ZX".to_vec()]);
    assert_eq!(lookup(&service, b"A"), vec![b"B2AX".to_vec()]);
    let before = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        public(service.clone(), run, indexed.clone())
            .unwrap()
            .segments[0]
            .data,
        b"B2AX"
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        before
    );
    let mut conflict = indexed.clone();
    conflict.ssas = vec![b"ROOT    (KIND    EQZ)".to_vec()];
    assert_eq!(
        public(service.clone(), run, conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    let held = nav(
        run,
        12,
        ImsOperation::GetHoldUnique,
        2,
        &[b"ROOT    (BYCHILD EQA)"],
    );
    let expected = public(service.clone(), run, held.clone()).unwrap();
    let position = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    drop(service);
    let reopened = ImsService::open(store, Default::default()).unwrap();
    assert_eq!(
        pcb::position(&reopened.lock().unwrap().state.sessions[run], 2),
        position
    );
    assert_eq!(public(reopened.clone(), run, held).unwrap(), expected);
    assert_eq!(
        public(
            reopened.clone(),
            run,
            nav(run, 13, ImsOperation::GetHoldNextParent, 2, &[b"CHILD    "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"C2AZ"
    );
    assert_eq!(
        call(
            &reopened,
            run,
            &selected(run, ImsOperation::Replace, 14, 2, &[], b"C2BQ")
        )
        .status,
        "  "
    );
    assert_eq!(lookup(&reopened, b"A"), vec![b"B2AX".to_vec()]); // source is ROOT, not this child
    assert_eq!(
        call(
            &reopened,
            run,
            &selected(run, ImsOperation::Delete, 15, 2, &[], b"")
        )
        .affected_segments,
        1
    );
    call(
        &reopened,
        run,
        &request(run, ImsOperation::Rollback, 16, &[], b""),
    );
    assert_eq!(
        public(
            reopened,
            run,
            nav(
                run,
                17,
                ImsOperation::GetUnique,
                2,
                &[b"ROOT    *D(BYCHILD EQA)", b"CHILD    "]
            )
        )
        .unwrap()
        .segments[1]
            .data,
        b"C2AZ"
    );
}

#[test]
fn secondary_ssa_memory_independent_pcb_hold_parentage_real_updates_and_reopen() {
    exercise(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn secondary_ssa_sqlite_independent_pcb_hold_parentage_real_updates_and_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-rich-index-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    exercise(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn secondary_ssa_source_distinct_from_target_changed_and_unchanged_xdfld() {
    let run = "source-target";
    let service = installed(Arc::new(MemoryStore::new(Default::default())), run, true);
    public(
        service.clone(),
        run,
        nav(
            run,
            2,
            ImsOperation::GetHoldUnique,
            2,
            &[b"ROOT    *P(BYCHILD EQAZ)", b"CHILD    "],
        ),
    )
    .unwrap();
    let before = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 3, 2, &[], b"C1ZA")
        )
        .status,
        "  "
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2).parentage(),
        before.parentage()
    );
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Replace, 4, 2, &[], b"C1BY")
        )
        .status,
        "  "
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2).parentage(),
        None
    );
    assert!(lookup(&service, b"AZ").is_empty());
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(
                run,
                5,
                ImsOperation::GetUnique,
                2,
                &[b"ROOT    (BYCHILD EQYB)"]
            )
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
    public(
        service.clone(),
        run,
        nav(run, 6, ImsOperation::GetHoldNextParent, 2, &[b"CHILD    "]),
    )
    .unwrap();
    assert_eq!(
        call(
            &service,
            run,
            &selected(run, ImsOperation::Delete, 7, 2, &[], b"")
        )
        .affected_segments,
        1
    );
    assert!(lookup(&service, b"YB").is_empty());
    call(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 8, &[], b""),
    );
    assert_eq!(
        public(
            service,
            run,
            nav(
                run,
                9,
                ImsOperation::GetUnique,
                2,
                &[b"ROOT    (BYCHILD EQAZ)"]
            )
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
}

#[test]
fn secondary_ssa_duplicate_sources_retain_distinct_pointer_identity() {
    let run = "duplicate-pointers";
    let service = installed(Arc::new(MemoryStore::new(Default::default())), run, true);
    let mut insert = selected(run, ImsOperation::Insert, 2, 1, &["CHILD"], b"C3ZA");
    insert.qualifiers = vec![qualifier(b"A1")];
    assert_eq!(call(&service, run, &insert).status, "  ");
    call(
        &service,
        run,
        &request(run, ImsOperation::Commit, 3, &[], b""),
    );
    let first = public(
        service.clone(),
        run,
        nav(
            run,
            4,
            ImsOperation::GetUnique,
            2,
            &[b"ROOT    (BYCHILD EQAZ)"],
        ),
    )
    .unwrap();
    let p1 = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    let second = public(
        service.clone(),
        run,
        nav(
            run,
            5,
            ImsOperation::GetNext,
            2,
            &[b"ROOT    (BYCHILD EQAZ)"],
        ),
    )
    .unwrap();
    let p2 = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(first, second);
    assert_eq!(first.segments[0].data, b"A1ZX");
    assert_eq!(p1.current(), p2.current());
    assert_ne!(p1, p2);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 6, ImsOperation::GetNext, 2, &[b"ROOT     "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"B2AX"
    );
    assert_eq!(
        public(service, run, nav(run, 7, ImsOperation::GetNext, 2, &[]))
            .unwrap()
            .segments[0]
            .data,
        b"C2AZ"
    );
}
