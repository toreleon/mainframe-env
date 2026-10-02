use super::*;

fn snapshot(service: &ImsService) -> (Vec<u8>, RowVersions) {
    let durable = service.lock().unwrap();
    (
        serde_json::to_vec(&durable.state).unwrap(),
        durable.versions.clone(),
    )
}

#[test]
fn secondary_ssa_malformed_commands_context_and_saf_preserve_all_rows() {
    let run = "ssa-index-deny";
    let store = Arc::new(MemoryStore::new(Default::default()));
    drop(installed(store.clone(), run, true));
    let policy = Arc::new(crate::service::generic::tests::Policy::default());
    let service = ImsService::open_authorized(store, Default::default(), policy.clone()).unwrap();
    let held = nav(
        run,
        2,
        ImsOperation::GetHoldUnique,
        2,
        &[b"ROOT    (BYCHILD EQZA)"],
    );
    public(service.clone(), run, held.clone()).unwrap();
    let before = snapshot(&service);
    for (raw, error) in [
        (b"ROOT    *Q ".as_slice(), HostProblem::Unsupported),
        (b"ROOT    *L ".as_slice(), HostProblem::Unsupported),
        (
            b"ROOT    (BYCHILD EQZA|BYCHILD EQAZ#BYCHILD EQ??)".as_slice(),
            HostProblem::Unsupported,
        ),
        (b"ROOT    *P".as_slice(), HostProblem::Malformed),
        (b"ROOT    (BYROOT  EQA)".as_slice(), HostProblem::Malformed),
        (b"CHILD   (BYCHILD EQZA)".as_slice(), HostProblem::Malformed),
        (
            b"ROOT    *O(00040002EQXY)".as_slice(),
            HostProblem::Malformed,
        ),
        (b"ROOT    (BYCHILD EQZ)".as_slice(), HostProblem::Malformed),
        (
            b"ROOT    (BYCHILD EQZA)junk".as_slice(),
            HostProblem::Malformed,
        ),
        (
            b"ROOT    *O(00000001EQA)".as_slice(),
            HostProblem::Malformed,
        ),
    ] {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, 3, ImsOperation::GetNext, 2, &[raw])
            ),
            Err(error),
            "{raw:?}"
        );
        assert_eq!(snapshot(&service), before);
    }
    let mut context = nav(run, 4, ImsOperation::GetUnique, 2, &[b"ROOT     "]);
    context.context = ImsExecutionContext::Dcctl;
    assert_eq!(
        public(service.clone(), run, context),
        Err(HostProblem::Unsupported)
    );
    let mut raw = held.clone();
    raw.request.mutation = request(run, ImsOperation::GetUnique, 5, &[], b"").mutation;
    raw.request.qualifiers = vec![qualifier(b"B2")];
    assert_eq!(
        public(service.clone(), run, raw),
        Err(HostProblem::Malformed)
    );
    assert_eq!(snapshot(&service), before);
    *policy.deny_update.lock().unwrap() = true;
    for req in [held, nav(run, 6, ImsOperation::GetNext, 2, &[b"ROOT     "])] {
        assert_eq!(
            public(service.clone(), run, req),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(snapshot(&service), before);
    }
}

#[test]
fn secondary_ssa_exact_ge_gb_gp_and_independent_holds() {
    let run = "ssa-index-status";
    let service = installed(Arc::new(MemoryStore::new(Default::default())), run, false);
    let empty = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 2, ImsOperation::GetHoldNextParent, 2, &[b"CHILD    "])
        )
        .unwrap()
        .status,
        "GP"
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        empty
    );
    let hold = nav(
        run,
        3,
        ImsOperation::GetHoldUnique,
        2,
        &[b"ROOT    (BYCHILD EQA)"],
    );
    public(service.clone(), run, hold).unwrap();
    let prior = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 4, ImsOperation::GetHoldNextParent, 2, &[b"ROOT     "])
        )
        .unwrap()
        .status,
        "GP"
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        prior
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(
                run,
                5,
                ImsOperation::GetHoldUnique,
                2,
                &[b"ROOT    (BYCHILD EQ?)"]
            )
        )
        .unwrap()
        .status,
        "GE"
    );
    let failed = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(failed.current(), prior.current());
    assert_eq!(failed.parentage(), None);
    assert!(!failed.is_held());
    for (seq, expected) in [(6, b"C2AZ"), (7, b"A1ZX"), (8, b"C1ZA")] {
        assert_eq!(
            public(
                service.clone(),
                run,
                nav(run, seq, ImsOperation::GetNext, 2, &[])
            )
            .unwrap()
            .segments[0]
                .data,
            expected
        );
    }
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 9, ImsOperation::GetNext, 2, &[])
        )
        .unwrap()
        .status,
        "GB"
    );
    let end = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        serde_json::to_value(end).unwrap(),
        serde_json::json!({"current":null,"parentage":null,"held":null,"after_end":true})
    );
    assert_eq!(
        public(service, run, nav(run, 10, ImsOperation::GetNext, 2, &[]))
            .unwrap()
            .segments[0]
            .data,
        b"B2AX"
    );
}

#[test]
fn secondary_ssa_selected_sensitivity_key_only_and_targetless_filter() {
    let run = "ssa-index-sensitive";
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store, Default::default()).unwrap();
    let mut metadata = indexed_catalog(false, true);
    let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[1] else {
        unreachable!()
    };
    pcb.sensitive_segments.truncate(1);
    let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[2] else {
        unreachable!()
    };
    pcb.sensitive_segments[0].processing_options = Some("K".into());
    service.install_metadata(metadata).unwrap();
    let image = ImsGenericLoadImage {
        database: "GENDB".into(),
        records: vec![
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"A1ZX".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "CHILD".into(),
                parent: Some(0),
                data: b"C1ZA".to_vec(),
            },
            ImsGenericLoadRecord {
                segment: "ROOT".into(),
                parent: None,
                data: b"B2AX".to_vec(),
            },
        ],
    };
    call(
        &service,
        "sensitive-load",
        &request(
            "sensitive-load",
            ImsOperation::Load,
            1,
            &[],
            &serde_json::to_vec(&image).unwrap(),
        ),
    );
    call(
        &service,
        "sensitive-load",
        &request("sensitive-load", ImsOperation::Commit, 2, &[], b""),
    );
    call(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let first = public(
        service.clone(),
        run,
        nav(run, 2, ImsOperation::GetUnique, 2, &[b"ROOT     "]),
    )
    .unwrap();
    assert_eq!(first.segments[0].data, b"B2AX");
    let before = pcb::position(&service.lock().unwrap().state.sessions[run], 2);
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 3, ImsOperation::GetUnique, 2, &[b"CHILD    "])
        )
        .unwrap()
        .status,
        "AC"
    );
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 2),
        before
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 4, ImsOperation::GetNext, 2, &[])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            nav(run, 5, ImsOperation::GetNext, 2, &[])
        )
        .unwrap()
        .status,
        "GB"
    );
    let keyed = public(
        service.clone(),
        run,
        nav(
            run,
            6,
            ImsOperation::GetUnique,
            3,
            &[b"ROOT    (BYROOT  EQA)"],
        ),
    )
    .unwrap();
    assert_eq!(keyed.status, "  ");
    assert!(keyed.segments.is_empty());
    assert!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 3)
            .current()
            .is_some()
    );
    assert_eq!(
        public(service, run, nav(run, 7, ImsOperation::GetNext, 3, &[]))
            .unwrap()
            .segments[0]
            .data,
        b"C1ZA"
    );
}

fn publication(store: Arc<dyn ProviderStateStore>) {
    let run = "ssa-index-cas";
    let intercepted = super::super::session_cas::SessionCasStore::new(store.clone(), run);
    let service = installed(intercepted.clone(), run, false);
    let get = nav(run, 2, ImsOperation::GetHoldNext, 2, &[b"ROOT     "]);
    let database = store
        .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
        .unwrap();
    intercepted.arm();
    assert_eq!(
        public(service.clone(), run, get.clone()),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        store
            .get_provider_state(GENERIC_DATABASE_NAMESPACE, "GENDB")
            .unwrap(),
        database
    );
    let fresh = ImsService::open(store.clone(), Default::default()).unwrap();
    assert!(
        pcb::position(&fresh.lock().unwrap().state.sessions[run], 2)
            .current()
            .is_none()
    );
    drop(fresh);
    intercepted.lose_ack();
    assert_eq!(
        public(service.clone(), run, get.clone()),
        Err(HostProblem::UnknownOutcome)
    );
    drop(service);
    let fresh = ImsService::open(store, Default::default()).unwrap();
    let published = pcb::position(&fresh.lock().unwrap().state.sessions[run], 2);
    assert!(published.is_held());
    assert_eq!(
        public(fresh.clone(), run, get).unwrap().segments[0].data,
        b"B2AX"
    );
    assert_eq!(
        pcb::position(&fresh.lock().unwrap().state.sessions[run], 2),
        published
    );
    assert_eq!(
        public(
            fresh,
            run,
            nav(run, 3, ImsOperation::GetNext, 2, &[b"ROOT     "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1ZX"
    );
}

#[test]
fn secondary_ssa_real_atomic_cas_and_lost_ack_memory() {
    publication(Arc::new(MemoryStore::new(Default::default())));
}

#[test]
fn secondary_ssa_real_atomic_cas_and_lost_ack_sqlite() {
    let file = std::env::temp_dir().join(format!(
        "ims-rich-cas-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    publication(Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap(),
    ));
    std::fs::remove_file(file).unwrap();
}
