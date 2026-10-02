use super::*;
use mainframe_env_host_api::{HostLimits, ImsNavigationRequest};

mod first_tests;
mod last_tests;
mod null_slot_tests;
mod pcb_tests;

fn navigation(run: &str, sequence: u64, op: ImsOperation, ssas: &[&[u8]]) -> ImsNavigationRequest {
    ImsNavigationRequest {
        request: request(run, op, sequence, &[], b""),
        context: ImsExecutionContext::DbBatch,
        ssas: ssas.iter().map(|s| s.to_vec()).collect(),
    }
}

pub(super) fn public(
    service: Arc<ImsService>,
    run: &str,
    req: ImsNavigationRequest,
) -> Result<ImsResult, HostProblem> {
    let req = HostRequest::ImsNavigation(req);
    req.validate(HostLimits::default())?;
    let provider = ims_providers(service, InvocationLimits::default()).remove(1);
    match provider
        .invoke(
            &invocation(run),
            EffectRequest {
                run_unit: invocation(run).run_unit_id.clone(),
                sequence: req.mutation().unwrap().sequence,
                deadline_tick: 100,
                idempotency_key: req.mutation().map(|m| m.idempotency_key.clone()),
                request: req,
            },
        )
        .outcome?
    {
        HostResult::Ims(result) => Ok(result),
        _ => panic!("wrong host result"),
    }
}

fn seed(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    service.install_metadata(catalog()).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 2, &["ROOT"], b"A1X"),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Insert, 3, &["ROOT"], b"B2Y"),
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 4, &[], b""),
    );
    service
}

#[test]
fn public_ssa_relations_offsets_and_failed_position() {
    let run = "ssa";
    let service = seed(Arc::new(MemoryStore::new(Default::default())), run);
    let result = public(
        service.clone(),
        run,
        navigation(
            run,
            5,
            ImsOperation::GetUnique,
            &[b"ROOT    (ROOTKEY GT A)"],
        ),
    )
    .unwrap();
    assert_eq!(result.status, "  ");
    assert_eq!(result.segments[0].data, b"A1X");
    let result = public(
        service.clone(),
        run,
        navigation(run, 6, ImsOperation::GetNext, &[b"ROOT    *O(00030001NEY)"]),
    )
    .unwrap();
    assert_eq!(result.status, "GE");
    assert!(result.segments.is_empty());
    assert_eq!(
        serde_json::to_value(&service.lock().unwrap().state.sessions[run].position).unwrap(),
        serde_json::json!({"current": null, "parentage": null, "held": null, "after_end": true})
    );
    let result = public(service, run, navigation(run, 7, ImsOperation::GetNext, &[])).unwrap();
    assert_eq!(result.status, "  ");
    assert_eq!(result.segments[0].data, b"A1X");
}

fn snapshot(service: &ImsService) -> (Vec<u8>, RowVersions) {
    let durable = service.lock().unwrap();
    (
        serde_json::to_vec(&durable.state).unwrap(),
        durable.versions.clone(),
    )
}

#[test]
fn public_ssa_hold_next_and_parent_failure_positions() {
    let run = "ssa-next-holds";
    let service = seed(Arc::new(MemoryStore::new(Default::default())), run);
    execute(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 5, &[], b""),
    );
    let position = || service.lock().unwrap().state.sessions[run].position.clone();
    let original = position();
    let missing = public(
        service.clone(),
        run,
        navigation(run, 6, ImsOperation::GetHoldNextParent, &[b"CHILD    "]),
    )
    .unwrap();
    assert_eq!(missing.status, "GP");
    assert!(missing.segments.is_empty());
    assert_eq!(position(), original);
    assert_eq!(
        public(
            service.clone(),
            run,
            navigation(
                run,
                7,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1X"
    );
    assert!(position().is_held());
    let next = public(
        service.clone(),
        run,
        navigation(run, 8, ImsOperation::GetHoldNext, &[b"ROOT     "]),
    )
    .unwrap();
    assert_eq!(next.status, "  ");
    assert_eq!(next.segments[0].data, b"B2Y");
    let held = position();
    assert!(held.is_held());
    assert_eq!(held.current(), held.parentage());
    let above = public(
        service.clone(),
        run,
        navigation(run, 9, ImsOperation::GetHoldNextParent, &[b"ROOT     "]),
    )
    .unwrap();
    assert_eq!(above.status, "GP");
    assert!(above.segments.is_empty());
    assert_eq!(position(), held);
    let absent = public(
        service.clone(),
        run,
        navigation(run, 10, ImsOperation::GetHoldNextParent, &[b"CHILD    "]),
    )
    .unwrap();
    assert_eq!(absent.status, "GE");
    assert!(absent.segments.is_empty());
    assert_eq!(position().current(), held.current());
    assert_eq!(position().parentage(), held.parentage());
    assert!(!position().is_held());
}

#[test]
fn public_ssa_binary_relations_and_boolean_classes() {
    let run = "ssa-relations";
    let service = seed(Arc::new(MemoryStore::new(Default::default())), run);
    for (i, (operator, key, expected)) in [
        ("EQ", "B2", b"B2Y"),
        ("NE", "A1", b"B2Y"),
        ("LT", "B2", b"A1X"),
        ("LE", "A1", b"A1X"),
        ("GT", "A1", b"B2Y"),
        ("GE", "B2", b"B2Y"),
        (" =", "B2", b"B2Y"),
        ("=>", "B2", b"B2Y"),
    ]
    .into_iter()
    .enumerate()
    {
        let raw = format!("ROOT    (ROOTKEY {operator}{key})").into_bytes();
        let result = public(
            service.clone(),
            run,
            navigation(run, 10 + i as u64, ImsOperation::GetUnique, &[&raw]),
        )
        .unwrap();
        assert_eq!(result.status, "  ");
        assert_eq!(result.segments[0].data, expected);
    }
    for (i, raw) in [
        b"ROOT    (ROOTKEY GEA1&ROOTKEY LEB2)".as_slice(),
        b"ROOT    (ROOTKEY GEA1*ROOTKEY LEB2)".as_slice(),
        b"ROOT    (ROOTKEY EQZ9|KIND    EQX)".as_slice(),
        b"ROOT    (ROOTKEY EQZ9+KIND    EQX)".as_slice(),
        b"ROOT    *(ROOTKEY EQA1)".as_slice(),
    ]
    .into_iter()
    .enumerate()
    {
        let result = public(
            service.clone(),
            run,
            navigation(run, 30 + i as u64, ImsOperation::GetUnique, &[raw]),
        )
        .unwrap();
        assert_eq!(
            (result.status.as_str(), result.segments[0].data.as_slice()),
            ("  ", b"A1X".as_slice())
        );
    }
    let before = snapshot(&service);
    assert_eq!(
        public(
            service.clone(),
            run,
            navigation(
                run,
                39,
                ImsOperation::GetUnique,
                &[b"ROOT    (ROOTKEY GEA1#ROOTKEY LEB2)"]
            )
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&service), before);
    let mut raw = b"ROOT    *O(00030001LT".to_vec();
    raw.extend([0xff, b')']);
    assert_eq!(
        public(
            service,
            run,
            navigation(run, 40, ImsOperation::GetUnique, &[&raw])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1X"
    );
}

#[test]
fn public_ssa_rejections_preserve_all_rows_and_position() {
    let run = "ssa-reject";
    let service = seed(Arc::new(MemoryStore::new(Default::default())), run);
    public(
        service.clone(),
        run,
        navigation(
            run,
            5,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (ROOTKEY EQA1)"],
        ),
    )
    .unwrap();
    let before = snapshot(&service);
    for (i, code) in [
        "A", "F", "G", "L", "N", "Q", "U", "V", "M1", "R1", "S1", "W1", "Z1",
    ]
    .into_iter()
    .enumerate()
    {
        let raw = format!("ROOT    *{code} ").into_bytes();
        assert_eq!(
            public(
                service.clone(),
                run,
                navigation(run, 10 + i as u64, ImsOperation::GetNext, &[&raw])
            ),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(snapshot(&service), before, "command {code}");
    }
    for raw in [
        b"ROOT".as_slice(),
        b"ROOT    (UNKNOWN EQA1)".as_slice(),
        b"ROOT    *O(00030002EQXY)".as_slice(),
        b"ROOT    (ROOTKEY EQA1)X".as_slice(),
        b"UNKNOWN  ".as_slice(),
    ] {
        assert_eq!(
            public(
                service.clone(),
                run,
                navigation(run, 30, ImsOperation::GetUnique, &[raw])
            ),
            Err(HostProblem::Malformed)
        );
        assert_eq!(snapshot(&service), before);
    }
    let mut context = navigation(run, 31, ImsOperation::GetUnique, &[b"ROOT     "]);
    context.context = ImsExecutionContext::Dcctl;
    assert_eq!(
        public(service.clone(), run, context),
        Err(HostProblem::Unsupported)
    );
    let mut pcb = navigation(run, 32, ImsOperation::GetUnique, &[b"ROOT     "]);
    pcb.request.pcb = 2;
    assert_eq!(
        public(service.clone(), run, pcb),
        Err(HostProblem::NotFound)
    );
    let mut large = navigation(run, 33, ImsOperation::GetUnique, &[]);
    large.ssas = vec![b"ROOT     ".to_vec(); 16];
    assert_eq!(
        public(service.clone(), run, large),
        Err(HostProblem::ResourceExhausted)
    );
    let mixed = navigation(
        run,
        34,
        ImsOperation::GetUnique,
        &[b"ROOT    (ROOTKEY EQA1|ROOTKEY EQB2#KIND    EQX)"],
    );
    assert_eq!(
        public(service.clone(), run, mixed),
        Err(HostProblem::Unsupported)
    );
    let wrong_path = navigation(
        run,
        35,
        ImsOperation::GetUnique,
        &[b"CHILD    ", b"ROOT     "],
    );
    assert_eq!(
        public(service.clone(), run, wrong_path),
        Err(HostProblem::Malformed)
    );
    assert_eq!(snapshot(&service), before);
}

fn exercise_path_hold_replay_rollback(service: Arc<ImsService>, run: &str) -> ImsNavigationRequest {
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
    for (seq, bytes) in [(6, b"C1Q"), (7, b"C2Z")] {
        assert_eq!(
            execute(
                &service,
                run,
                &request(run, ImsOperation::Insert, seq, &["CHILD"], bytes)
            )
            .status,
            "  "
        );
    }
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 8, &[], b""),
    );
    let hold = navigation(
        run,
        9,
        ImsOperation::GetHoldUnique,
        &[b"ROOT    *DP ", b"CHILD   *C(A1C1)"],
    );
    let before_limit = snapshot(&service);
    let mut limited = hold.clone();
    limited.request.max_segments = 1;
    assert_eq!(
        public(service.clone(), run, limited),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(snapshot(&service), before_limit);
    let result = public(service.clone(), run, hold.clone()).unwrap();
    assert_eq!(result.status, "  ");
    assert_eq!(
        result
            .segments
            .iter()
            .map(|s| s.data.clone())
            .collect::<Vec<_>>(),
        [b"A1X".to_vec(), b"C1Q".to_vec()]
    );
    assert_eq!(
        result.segments[1].parent_key.as_deref(),
        Some(b"A1".as_slice())
    );
    let position = service.lock().unwrap().state.sessions[run].position.clone();
    assert!(position.is_held());
    assert_ne!(position.current(), position.parentage());
    let before = snapshot(&service);
    assert_eq!(public(service.clone(), run, hold.clone()).unwrap(), result);
    assert_eq!(snapshot(&service), before);
    let mut conflict = hold.clone();
    conflict.ssas[1] = b"CHILD   *C(A1C2)".to_vec();
    assert_eq!(
        public(service.clone(), run, conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        execute(
            &service,
            run,
            &request(run, ImsOperation::Replace, 10, &["CHILD"], b"C1R")
        )
        .status,
        "  "
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 11, &[], b""),
    );
    assert_eq!(public(service.clone(), run, hold.clone()).unwrap(), result);
    assert!(
        !service.lock().unwrap().state.sessions[run]
            .position
            .is_held()
    );
    let reposition = navigation(
        run,
        12,
        ImsOperation::GetUnique,
        &[b"ROOT    *P ", b"CHILD   *C(A1C1)"],
    );
    assert_eq!(
        public(service.clone(), run, reposition).unwrap().segments[0].data,
        b"C1Q"
    );
    let mismatch = navigation(
        run,
        13,
        ImsOperation::GetNextParent,
        &[b"ROOT    (ROOTKEY EQB2)", b"CHILD    "],
    );
    let before_mismatch = service.lock().unwrap().state.sessions[run].position.clone();
    assert_eq!(public(service.clone(), run, mismatch).unwrap().status, "GE");
    assert_eq!(
        service.lock().unwrap().state.sessions[run].position,
        before_mismatch
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            navigation(run, 14, ImsOperation::GetNextParent, &[b"CHILD    "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"C2Z"
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            navigation(run, 15, ImsOperation::GetNextParent, &[b"CHILD    "])
        )
        .unwrap()
        .status,
        "GE"
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            navigation(run, 16, ImsOperation::GetNextParent, &[b"CHILD    "])
        )
        .unwrap()
        .status,
        "GE"
    );
    hold
}

#[test]
fn public_ssa_path_hold_replay_rollback_and_memory_reopen() {
    let run = "ssa-path";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = seed(store.clone(), run);
    let hold = exercise_path_hold_replay_rollback(service.clone(), run);
    let before = snapshot(&service);
    drop(service);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(snapshot(&reopened), before);
    assert_eq!(
        public(reopened.clone(), run, hold).unwrap().segments[1].data,
        b"C1Q"
    );
    assert_eq!(snapshot(&reopened), before);
    assert_eq!(
        public(
            reopened,
            run,
            navigation(run, 17, ImsOperation::GetNext, &[])
        )
        .unwrap()
        .segments[0]
            .data,
        b"B2Y"
    );
}

#[test]
fn public_ssa_path_hold_replay_rollback_and_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-ssa-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let run = "ssa-sqlite";
    let (hold, before) = {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = seed(store, run);
        let hold = exercise_path_hold_replay_rollback(service.clone(), run);
        (hold, snapshot(&service))
    };
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(snapshot(&reopened), before);
        assert_eq!(
            public(reopened.clone(), run, hold).unwrap().segments[1].data,
            b"C1Q"
        );
        assert_eq!(snapshot(&reopened), before);
        assert_eq!(
            public(
                reopened,
                run,
                navigation(run, 17, ImsOperation::GetNext, &[])
            )
            .unwrap()
            .segments[0]
                .data,
            b"B2Y"
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn public_ssa_authorization_precedes_observation_and_replay() {
    let run = "ssa-deny";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    drop(seed(store.clone(), run));
    let policy = Arc::new(Policy::default());
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    let req = navigation(
        run,
        5,
        ImsOperation::GetUnique,
        &[b"ROOT    (ROOTKEY EQA1)"],
    );
    *policy.deny_update.lock().unwrap() = true;
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, req.clone()),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(snapshot(&service), before);
    *policy.deny_update.lock().unwrap() = false;
    assert_eq!(
        public(service.clone(), run, req.clone()).unwrap().segments[0].data,
        b"A1X"
    );
    *policy.deny_update.lock().unwrap() = true;
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, req),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(snapshot(&service), before);
}
