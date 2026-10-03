//! Independent expectations from the four mixed-SSA supplement body pins.
use super::*;

fn snapshot(service: &ImsService) -> (Vec<u8>, RowVersions) {
    let durable = service.lock().unwrap();
    (
        serde_json::to_vec(&durable.state).unwrap(),
        durable.versions.clone(),
    )
}

fn expect(service: &Arc<ImsService>, run: &str, seq: u64, pcb: u16, raw: &[u8], data: &[u8]) {
    let result = public(
        service.clone(),
        run,
        nav(run, seq, ImsOperation::GetUnique, pcb, &[raw]),
    )
    .unwrap();
    assert_eq!(result.status, "  ");
    assert_eq!(result.segments[0].data, data);
}

fn mixed_sets(store: Arc<dyn ProviderStateStore>) {
    let run = "mixed-evaluation";
    let service = installed(store.clone(), run, true);
    // true OR (false AND false): succeeds; a left fold would reject it.
    // false AND false OR true: succeeds; uniform AND would reject it.
    // false OR (true AND false): rejects A1; uniform OR would select A1.
    for selected in [1, 2] {
        for (offset, raw, expected) in [
            (
                0,
                b"ROOT    (ROOTKEY EQA1|KIND    EQ?&ZONE    EQ?)".as_slice(),
                b"A1ZX",
            ),
            (
                1,
                b"ROOT    (ROOTKEY EQ??*KIND    EQ?+ROOTKEY EQB2)".as_slice(),
                b"B2AX",
            ),
            (
                2,
                b"ROOT    (ROOTKEY EQB2+KIND    EQZ*ZONE    EQ?)".as_slice(),
                b"B2AX",
            ),
            (
                3,
                b"ROOT    (ROOTKEY EQA1&KIND    EQZ|ROOTKEY EQB2&KIND    EQA)".as_slice(),
                b"A1ZX",
            ),
        ] {
            expect(
                &service,
                run,
                10 * u64::from(selected) + offset,
                selected,
                raw,
                expected,
            );
        }
    }
    // Pointer AZ selects physical A1, pointer ZA selects physical B2.
    expect(
        &service,
        run,
        30,
        2,
        b"ROOT    (BYCHILD EQAZ|BYCHILD EQZA&KIND    EQ?)",
        b"A1ZX",
    );
    expect(
        &service,
        run,
        31,
        2,
        b"ROOT    (BYCHILD EQ??|BYCHILD EQZA&KIND    EQA)",
        b"B2AX",
    );
    let held = nav(
        run,
        32,
        ImsOperation::GetHoldUnique,
        2,
        &[
            b"ROOT    *DP(BYCHILD EQAZ|BYCHILD EQZA&KIND    EQ?)",
            b"CHILD    ",
        ],
    );
    let found = public(service.clone(), run, held.clone()).unwrap();
    assert_eq!(
        found
            .segments
            .iter()
            .map(|s| s.data.as_slice())
            .collect::<Vec<_>>(),
        [b"A1ZX".as_slice(), b"C1ZA".as_slice()]
    );
    let pos = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert!(pos.is_held());
    assert_ne!(pos.current(), pos.parentage());
    let failure = public(
        service.clone(),
        run,
        nav(
            run,
            33,
            ImsOperation::GetNextParent,
            2,
            &[
                b"ROOT    (BYCHILD EQZA|BYCHILD EQ??&KIND    EQZ)",
                b"CHILD    ",
            ],
        ),
    )
    .unwrap();
    assert_eq!(failure.status, "GE");
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        pos
    );
    public(
        service.clone(),
        run,
        nav(
            run,
            34,
            ImsOperation::GetHoldNext,
            2,
            &[b"ROOT    (BYCHILD EQ??|BYCHILD EQZA&KIND    EQA)"],
        ),
    )
    .unwrap();
    let before = snapshot(&service);
    assert_eq!(public(service.clone(), run, held.clone()).unwrap(), found);
    assert_eq!(snapshot(&service), before);
    let mut conflict = held.clone();
    conflict.ssas[0] = b"ROOT    *DP(BYCHILD EQZA|BYCHILD EQAZ&KIND    EQ?)".to_vec();
    assert_eq!(
        public(service.clone(), run, conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(snapshot(&service), before);
    drop(service);
    let reopened = ImsService::open(store, Default::default()).unwrap();
    assert_eq!(snapshot(&reopened), before);
    assert_eq!(public(reopened.clone(), run, held).unwrap(), found);
    assert_eq!(snapshot(&reopened), before);
}

#[test]
fn mixed_evaluation_or_sets_primary_secondary_memory() {
    mixed_sets(Arc::new(MemoryStore::new(Default::default())));
}

// The manager integration_tests module owns the physical/virtual field collision regression.

fn correlated(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    let service = installed(store, run, true);
    for (seq, data) in [(2, b"C3AZ"), (3, b"C4ZA")] {
        let mut insert = selected(run, ImsOperation::Insert, seq, 1, &["CHILD"], data);
        insert.qualifiers = vec![qualifier(b"A1")];
        assert_eq!(call(&service, run, &insert).status, "  ");
    }
    call(
        &service,
        run,
        &request(run, ImsOperation::Commit, 4, &[], b""),
    );
    service
}

const INDEPENDENT: &[u8] = b"ROOT    (BYCHILD EQZA#BYCHILD EQAZ)";

#[test]
fn mixed_evaluation_independent_target_correlation_group_order_and_duplicates() {
    let run = "correlation";
    let service = correlated(Arc::new(MemoryStore::new(Default::default())), run);
    let dependent = public(
        service.clone(),
        run,
        nav(
            run,
            5,
            ImsOperation::GetUnique,
            2,
            &[b"ROOT    (BYCHILD EQZA&BYCHILD EQAZ)"],
        ),
    )
    .unwrap();
    assert_eq!(dependent.status, "GE");
    let get = nav(run, 6, ImsOperation::GetHoldUnique, 2, &[INDEPENDENT]);
    let found = public(service.clone(), run, get.clone()).unwrap();
    assert_eq!(found.segments[0].data, b"A1ZX"); // B2 has only ZA, so cannot satisfy both.
    let first = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert!(first.is_held());
    assert_eq!(
        serde_json::to_value(&first).unwrap()["secondary"]["source"],
        5
    );
    let next = public(
        service.clone(),
        run,
        nav(run, 7, ImsOperation::GetHoldNext, 2, &[INDEPENDENT]),
    )
    .unwrap();
    assert_eq!(next, found);
    let second = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(first.current(), second.current());
    assert_eq!(
        serde_json::to_value(&second).unwrap()["secondary"]["source"],
        2
    );
    assert_ne!(first, second);
    // C4 duplicates C1's AZ key for A1: once per independent group, not per source.
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 8, ImsOperation::GetNext, 2, &[INDEPENDENT])
        )
        .unwrap()
        .status,
        "GE"
    );
    let before = snapshot(&service);
    assert_eq!(public(service.clone(), run, get).unwrap(), found);
    assert_eq!(snapshot(&service), before);
    expect(
        &service,
        run,
        9,
        2,
        b"ROOT    (BYCHILD EQAZ#BYCHILD EQZA)",
        b"A1ZX",
    );
    assert_eq!(
        serde_json::to_value(pcb::position(
            &service.lock().unwrap().state.sessions[run],
            2
        ))
        .unwrap()["secondary"]["source"],
        2
    );
    let path = public(
        service.clone(),
        run,
        nav(
            run,
            10,
            ImsOperation::GetHoldNextParent,
            2,
            &[INDEPENDENT, b"CHILD   (CHILDKEYEQC3)"],
        ),
    )
    .unwrap();
    assert_eq!(path.segments[0].data, b"C3AZ");
    assert!(pcb::position(&service.lock().unwrap().state.sessions[run], 2).is_held());
    let path = public(
        service.clone(),
        run,
        nav(
            run,
            11,
            ImsOperation::GetNextParent,
            2,
            &[INDEPENDENT, b"CHILD   (CHILDKEYEQC4)"],
        ),
    )
    .unwrap();
    assert_eq!(path.segments[0].data, b"C4ZA");
    assert!(!pcb::position(&service.lock().unwrap().state.sessions[run], 2).is_held());
    // A physical field makes both kinds of AND dependent, including mixed forms.
    expect(
        &service,
        run,
        12,
        2,
        b"ROOT    (BYCHILD EQZA#ROOTKEY EQA1&KIND    EQZ|ROOTKEY EQ??)",
        b"A1ZX",
    );
    // All bytes in the comparative value are data, including connector encodings.
    let mut raw = b"ROOT    *O(00030001LT".to_vec();
    raw.push(0x80);
    raw.extend(b"&00040001GE");
    raw.push(0);
    raw.extend(b"|00030001EQ");
    raw.push(255);
    raw.extend(b"&00040001EQ+)");
    expect(&service, run, 13, 1, &raw, b"A1ZX");
    let mut insert = selected(run, ImsOperation::Insert, 14, 1, &["CHILD"], b"C5BA");
    insert.qualifiers = vec![qualifier(b"A1")];
    assert_eq!(call(&service, run, &insert).status, "  ");
    call(
        &service,
        run,
        &request(run, ImsOperation::Commit, 15, &[], b""),
    );
    // The secondary body permits more than two independent qualifications.
    let three = b"ROOT    (BYCHILD EQZA#BYCHILD EQAB#BYCHILD EQAZ)";
    expect(&service, run, 16, 2, three, b"A1ZX");
    for (seq, source) in [(17, 7), (18, 2)] {
        let result = public(
            service.clone(),
            run,
            nav(run, seq, ImsOperation::GetNext, 2, &[three]),
        )
        .unwrap();
        assert_eq!(result.segments[0].data, b"A1ZX");
        assert_eq!(
            serde_json::to_value(pcb::position(
                &service.lock().unwrap().state.sessions[run],
                2
            ))
            .unwrap()["secondary"]["source"],
            source
        );
    }
    assert_eq!(
        public(
            service,
            run,
            nav(run, 19, ImsOperation::GetNext, 2, &[three])
        )
        .unwrap()
        .status,
        "GE"
    );
}

#[test]
fn mixed_evaluation_unsupported_malformed_context_and_saf_no_publication() {
    let run = "mixed-rejections";
    let store = Arc::new(MemoryStore::new(Default::default()));
    drop(correlated(store.clone(), run));
    let policy = Arc::new(crate::service::generic::tests::Policy::default());
    let service = ImsService::open_authorized(store, Default::default(), policy.clone()).unwrap();
    public(
        service.clone(),
        run,
        nav(run, 5, ImsOperation::GetHoldUnique, 2, &[INDEPENDENT]),
    )
    .unwrap();
    let before = snapshot(&service);
    for (selected, raw, error) in [
        (
            2,
            b"ROOT    (BYCHILD EQAB#BYCHILD EQXY)".as_slice(),
            HostProblem::Unsupported,
        ),
        (
            1,
            b"ROOT    (ROOTKEY EQA1#KIND    EQZ)".as_slice(),
            HostProblem::Unsupported,
        ),
        (
            2,
            b"ROOT    (BYCHILD EQAZ#BYCHILD EQAZ)".as_slice(),
            HostProblem::Unsupported,
        ),
        (
            2,
            b"ROOT    (BYCHILD GEAZ#BYCHILD LEZA)".as_slice(),
            HostProblem::Unsupported,
        ),
        (
            2,
            b"ROOT    (BYCHILD EQAZ#BYCHILD EQZA&BYCHILD NE??)".as_slice(),
            HostProblem::Unsupported,
        ),
        (
            2,
            b"ROOT    (BYCHILD EQAZ#BYCHILD EQZA|BYCHILD EQ??)".as_slice(),
            HostProblem::Unsupported,
        ),
        (
            2,
            b"ROOT    ((BYCHILD EQAZ|BYCHILD EQZA)&KIND    EQZ)".as_slice(),
            HostProblem::Malformed,
        ),
        (
            2,
            b"ROOT    (BYCHILD EQAZ|BYCHILD EQZ)".as_slice(),
            HostProblem::Malformed,
        ),
        (
            2,
            b"ROOT    (BYCHILD EQAZ&&KIND    EQZ)".as_slice(),
            HostProblem::Malformed,
        ),
        (
            2,
            b"ROOT    *A(BYCHILD EQAZ|BYCHILD EQZA&KIND    EQZ)".as_slice(),
            HostProblem::Unsupported,
        ),
    ] {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 6, ImsOperation::GetHoldNext, selected, &[raw])
            ),
            Err(error)
        );
        assert_eq!(snapshot(&service), before);
    }
    let mut denied = nav(run, 7, ImsOperation::GetUnique, 2, &[INDEPENDENT]);
    denied.context = ImsExecutionContext::Dcctl;
    assert_eq!(
        public(service.clone(), run, denied),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&service), before);
    *policy.deny_update.lock().unwrap() = true;
    for raw in [
        INDEPENDENT,
        b"ROOT    (BYCHILD EQAZ|BYCHILD EQZA&KIND    EQ?)".as_slice(),
    ] {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 8, ImsOperation::GetUnique, 2, &[raw])
            ),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&service), before);
    }
}

fn sqlite_file(label: &str) -> (std::path::PathBuf, String) {
    let file = std::env::temp_dir().join(format!(
        "ims-mixed-eval-{label}-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    (file, url)
}

#[test]
fn mixed_evaluation_sqlite_full_mixed_sets_fresh_reopen() {
    let (file, url) = sqlite_file("sets");
    mixed_sets(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

fn publication(store: Arc<dyn ProviderStateStore>) {
    use super::super::session_cas::SessionCasStore;
    let run = "mixed-cas";
    drop(correlated(store.clone(), run));
    let database = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    let intercepted = SessionCasStore::new(store.clone(), run);
    let service = ImsService::open(intercepted.clone(), Default::default()).unwrap();
    let before = snapshot(&service).0;
    let get = nav(run, 5, ImsOperation::GetHoldUnique, 2, &[INDEPENDENT]);
    intercepted.arm();
    assert_eq!(
        public(service.clone(), run, get.clone()),
        Err(HostProblem::IdempotencyConflict)
    );
    let fresh = ImsService::open(store.clone(), Default::default()).unwrap();
    assert_eq!(snapshot(&fresh).0, before);
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap(),
        database
    );
    drop(fresh);
    intercepted.lose_ack();
    assert_eq!(
        public(service.clone(), run, get.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    let published = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap()
        .unwrap();
    // The manager's existing UOW fence advances the CAS envelope on publication,
    // even though navigation leaves the database image bytes unchanged.
    assert_eq!(published.version, database.version + 1);
    assert_eq!(published.payload, database.payload);
    drop(service);
    let fresh = ImsService::open(store.clone(), Default::default()).unwrap();
    let before = snapshot(&fresh);
    assert_eq!(
        public(fresh.clone(), run, get).unwrap().segments[0].data,
        b"A1ZX"
    );
    assert_eq!(snapshot(&fresh), before);
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap()
            .unwrap(),
        published
    );
}

#[test]
fn mixed_evaluation_real_cas_lost_ack_memory() {
    publication(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn mixed_evaluation_real_cas_lost_ack_sqlite() {
    let (file, url) = sqlite_file("cas");
    publication(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}

#[test]
fn mixed_evaluation_hdam_dedb_root_anchors_remain_unsupported() {
    for organization in [ImsDatabaseOrganization::Hdam, ImsDatabaseOrganization::Dedb] {
        let run = "unrepresented-anchor";
        let service = ImsService::open(
            Arc::new(MemoryStore::new(Default::default())),
            Default::default(),
        )
        .unwrap();
        let mut metadata = catalog();
        metadata.databases[0].organization = organization;
        service.install_metadata(metadata).unwrap();
        call(
            &service,
            run,
            &request(run, ImsOperation::Schedule, 1, &[], b""),
        );
        call(
            &service,
            run,
            &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
        );
        let before = snapshot(&service);
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(
                    run,
                    3,
                    ImsOperation::GetUnique,
                    1,
                    &[b"ROOT    (ROOTKEY GEA1|ROOTKEY EQB2&KIND    EQX)"]
                )
            ),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), before);
    }
}

#[test]
fn mixed_evaluation_process_worker() {
    let Ok(url) = std::env::var("IMS_MIXED_EVAL_PROCESS_URL") else {
        return;
    };
    let stage = std::env::var("IMS_MIXED_EVAL_PROCESS_STAGE").unwrap();
    let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let run = "independent-process";
    let service = if stage == "seed" {
        correlated(store, run)
    } else {
        ImsService::open(store, Default::default()).unwrap()
    };
    let get = nav(run, 5, ImsOperation::GetHoldUnique, 2, &[INDEPENDENT]);
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, get).unwrap().segments[0].data,
        b"A1ZX"
    );
    if stage != "seed" {
        assert_eq!(snapshot(&service), before);
    }
    if stage == "reopen" || stage == "verify" {
        let before = snapshot(&service);
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 6, ImsOperation::GetHoldNext, 2, &[INDEPENDENT])
            )
            .unwrap()
            .segments[0]
                .data,
            b"A1ZX"
        );
        if stage == "verify" {
            assert_eq!(snapshot(&service), before);
            assert_eq!(
                public(
                    service,
                    run,
                    nav(run, 7, ImsOperation::GetNext, 2, &[INDEPENDENT])
                )
                .unwrap()
                .status,
                "GE"
            );
        }
    } else {
        assert_eq!(stage, "seed");
    }
}

#[test]
fn mixed_evaluation_three_independent_sqlite_processes() {
    let (file, url) = sqlite_file("process");
    for stage in ["seed", "reopen", "verify"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "service::generic::tests::pcb_tests::secondary_tests::rich_ssa_tests::mixed_evaluation_tests::mixed_evaluation_process_worker", "--nocapture"])
            .env("IMS_MIXED_EVAL_PROCESS_URL", &url).env("IMS_MIXED_EVAL_PROCESS_STAGE", stage).output().unwrap();
        assert!(
            result.status.success(),
            "stage {stage}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed"));
    }
    std::fs::remove_file(file).unwrap();
}
