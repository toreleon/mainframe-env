use super::*;

fn metadata() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let mut hidden = metadata.databases[0].segments[1].clone();
    hidden.name = "HIDDEN".into();
    metadata.databases[0].segments.insert(1, hidden);
    let mut other = metadata.databases[0].clone();
    other.name = "OTHERDB".into();
    metadata.databases.push(other);
    let ImsPcbMetadata::Database(mut pcb) = metadata.psbs[0].pcbs[0].clone() else {
        panic!("database fixture");
    };
    pcb.name = "OTHERPCB".into();
    pcb.database = "OTHERDB".into();
    metadata.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(pcb.clone()));
    pcb.name = "ROOTONLY".into();
    pcb.database = "GENDB".into();
    pcb.sensitive_segments.truncate(1);
    metadata.psbs[0]
        .pcbs
        .push(ImsPcbMetadata::Database(pcb.clone()));
    pcb.name = "KEYONLY".into();
    pcb.sensitive_segments[0].processing_options = Some("K".into());
    metadata.psbs[0].pcbs.push(ImsPcbMetadata::Database(pcb));
    metadata
}

fn install(service: &ImsService, run: &str) {
    service.install_metadata(metadata()).unwrap();
    for (sequence, database, prefix) in [(1, "GENDB", b'A'), (2, "OTHERDB", b'B')] {
        let image = ImsGenericLoadImage {
            database: database.into(),
            records: vec![
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: vec![prefix, b'1', b'X'],
                },
                ImsGenericLoadRecord {
                    segment: "HIDDEN".into(),
                    parent: Some(0),
                    data: b"H1S".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "CHILD".into(),
                    parent: Some(0),
                    data: b"C1Q".to_vec(),
                },
                ImsGenericLoadRecord {
                    segment: "ROOT".into(),
                    parent: None,
                    data: vec![prefix, b'2', b'Y'],
                },
            ],
        };
        assert_eq!(
            execute(
                service,
                "ssa-loader",
                &request(
                    "ssa-loader",
                    ImsOperation::Load,
                    sequence,
                    &[],
                    &serde_json::to_vec(&image).unwrap()
                )
            )
            .status,
            "  "
        );
    }
    execute(
        service,
        "ssa-loader",
        &request("ssa-loader", ImsOperation::Commit, 3, &[], b""),
    );
    execute(
        service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
}

fn selected(
    run: &str,
    seq: u64,
    op: ImsOperation,
    pcb: u16,
    ssas: &[&[u8]],
) -> ImsNavigationRequest {
    let mut req = navigation(run, seq, op, ssas);
    req.request.pcb = pcb;
    req
}

fn positions(service: &ImsService, run: &str) -> Vec<PcbPosition> {
    let durable = service.lock().unwrap();
    (1..=4)
        .map(|number| pcb::position(&durable.state.sessions[run], number))
        .collect()
}

fn exercise_selected_positions(service: Arc<ImsService>, run: &str) -> ImsNavigationRequest {
    let first = selected(
        run,
        10,
        ImsOperation::GetHoldUnique,
        1,
        &[b"ROOT    (ROOTKEY EQA1)"],
    );
    let second = selected(
        run,
        11,
        ImsOperation::GetHoldUnique,
        2,
        &[b"ROOT    *DP ", b"CHILD   *C(B1C1)"],
    );
    assert_eq!(
        public(service.clone(), run, first).unwrap().segments[0].data,
        b"A1X"
    );
    let path = public(service.clone(), run, second.clone()).unwrap();
    assert_eq!(path.status, "  ");
    assert_eq!(
        path.segments
            .iter()
            .map(|s| s.data.clone())
            .collect::<Vec<_>>(),
        [b"B1X".to_vec(), b"C1Q".to_vec()]
    );
    assert_eq!(
        path.segments[1].parent_key.as_deref(),
        Some(b"B1".as_slice())
    );
    let held = positions(&service, run);
    assert!(held[0].is_held());
    assert!(held[1].is_held());
    assert_ne!(held[1].current(), held[1].parentage());
    // A different PCB's Get cancels only that PCB's hold.
    assert_eq!(
        public(
            service.clone(),
            run,
            selected(run, 12, ImsOperation::GetNext, 1, &[b"ROOT     "])
        )
        .unwrap()
        .segments[0]
            .data,
        b"A2Y"
    );
    assert_eq!(positions(&service, run)[1], held[1]);
    let after = snapshot(&service);
    assert_eq!(public(service.clone(), run, second.clone()).unwrap(), path);
    assert_eq!(snapshot(&service), after);
    let mut conflicting = second.clone();
    conflicting.request.pcb = 1;
    assert_eq!(
        public(service.clone(), run, conflicting),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(snapshot(&service), after);
    let mut replace = request(run, ImsOperation::Replace, 13, &["CHILD"], b"C1R");
    replace.pcb = 2;
    assert_eq!(execute(&service, run, &replace).status, "  ");
    assert_eq!(
        public(
            service.clone(),
            run,
            selected(run, 14, ImsOperation::GetUnique, 2, &[b"CHILD   *C(B1C1)"])
        )
        .unwrap()
        .segments[0]
            .data,
        b"C1R"
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            selected(run, 15, ImsOperation::GetNext, 1, &[b"ROOT     "])
        )
        .unwrap()
        .status,
        "GE"
    );
    execute(
        &service,
        run,
        &request(run, ImsOperation::Rollback, 16, &[], b""),
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            selected(run, 17, ImsOperation::GetUnique, 2, &[b"CHILD   *C(B1C1)"])
        )
        .unwrap()
        .segments[0]
            .data,
        b"C1Q"
    );
    second
}

#[test]
fn public_ssa_independent_selected_pcb_positions_holds_replay_and_reopen() {
    let run = "ssa-multi-pcb";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    install(&service, run);
    let second = exercise_selected_positions(service.clone(), run);
    let before = snapshot(&service);
    drop(service);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    assert_eq!(snapshot(&reopened), before);
    assert_eq!(
        public(reopened.clone(), run, second).unwrap().segments[1].data,
        b"C1Q"
    );
    assert_eq!(snapshot(&reopened), before);
}

#[test]
fn public_ssa_independent_selected_pcb_positions_survive_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-ssa-pcbs-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let run = "ssa-multi-sqlite";
    let (second, before) = {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        install(&service, run);
        let second = exercise_selected_positions(service.clone(), run);
        (second, snapshot(&service))
    };
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(snapshot(&reopened), before);
        assert_eq!(
            public(reopened.clone(), run, second).unwrap().segments[0].data,
            b"B1X"
        );
        assert_eq!(snapshot(&reopened), before);
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn public_ssa_visibility_key_only_and_sensitivity_statuses_use_selected_pcb() {
    let run = "ssa-visibility";
    let service = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        ImsLimits::default(),
    )
    .unwrap();
    install(&service, run);
    for (seq, expected) in [(10, b"A1X"), (11, b"A2Y")] {
        let result = public(
            service.clone(),
            run,
            selected(run, seq, ImsOperation::GetNext, 3, &[]),
        )
        .unwrap();
        assert_eq!(
            (result.status.as_str(), result.segments[0].data.as_slice()),
            ("  ", expected.as_slice())
        );
    }
    assert_eq!(
        public(
            service.clone(),
            run,
            selected(run, 12, ImsOperation::GetNext, 3, &[])
        )
        .unwrap()
        .status,
        "GB"
    );
    public(
        service.clone(),
        run,
        selected(
            run,
            13,
            ImsOperation::GetHoldUnique,
            3,
            &[b"ROOT    (ROOTKEY EQA1)"],
        ),
    )
    .unwrap();
    let held = positions(&service, run)[2].clone();
    let excluded = public(
        service.clone(),
        run,
        selected(run, 14, ImsOperation::GetNext, 3, &[b"CHILD    "]),
    )
    .unwrap();
    assert_eq!(excluded.status, "AC");
    assert!(excluded.segments.is_empty());
    assert_eq!(positions(&service, run)[2], held);
    let no_child = public(
        service.clone(),
        run,
        selected(run, 15, ImsOperation::GetNextParent, 3, &[]),
    )
    .unwrap();
    assert_eq!(no_child.status, "GE");
    assert!(no_child.segments.is_empty());
    let key_only = public(
        service.clone(),
        run,
        selected(
            run,
            16,
            ImsOperation::GetUnique,
            4,
            &[b"ROOT    (ROOTKEY EQA1)"],
        ),
    )
    .unwrap();
    assert_eq!(key_only.status, "  ");
    assert!(key_only.segments.is_empty());
    assert!(positions(&service, run)[3].current().is_some());
    let missing = public(
        service,
        run,
        selected(run, 17, ImsOperation::GetNext, 4, &[]),
    )
    .unwrap();
    assert_eq!(missing.status, "GB");
    assert!(missing.segments.is_empty());
}

#[derive(Default)]
struct SelectedPolicy {
    deny_other: Mutex<bool>,
    seen: Mutex<Vec<EnterpriseResource>>,
}

impl EnterpriseAuthorizer for SelectedPolicy {
    fn authorize(&self, _: &PrincipalId, resource: &EnterpriseResource) -> Result<(), HostProblem> {
        self.seen.lock().unwrap().push(resource.clone());
        if resource.class == EnterpriseResourceClass::ImsDatabase
            && resource.name.as_str() == "OTHERDB"
            && *self.deny_other.lock().unwrap()
        {
            Err(HostProblem::Unauthorized)
        } else {
            Ok(())
        }
    }
}

#[test]
fn public_ssa_saf_and_replay_bind_the_requested_database_pcb() {
    let run = "ssa-selected-saf";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let plain = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
    install(&plain, run);
    drop(plain);
    let policy = Arc::new(SelectedPolicy::default());
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    let req = selected(
        run,
        10,
        ImsOperation::GetUnique,
        2,
        &[b"ROOT    (ROOTKEY EQB1)"],
    );
    *policy.deny_other.lock().unwrap() = true;
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, req.clone()),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(snapshot(&service), before);
    assert!(
        policy.seen.lock().unwrap().iter().any(
            |r| r.class == EnterpriseResourceClass::ImsDatabase && r.name.as_str() == "OTHERDB"
        )
    );
    assert!(
        !policy
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.class == EnterpriseResourceClass::ImsDatabase && r.name.as_str() == "GENDB")
    );
    *policy.deny_other.lock().unwrap() = false;
    assert_eq!(
        public(service.clone(), run, req.clone()).unwrap().segments[0].data,
        b"B1X"
    );
    *policy.deny_other.lock().unwrap() = true;
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, req),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(snapshot(&service), before);
    assert_eq!(
        public(
            service,
            run,
            selected(
                run,
                11,
                ImsOperation::GetUnique,
                1,
                &[b"ROOT    (ROOTKEY EQA1)"]
            )
        )
        .unwrap()
        .segments[0]
            .data,
        b"A1X"
    );
}
