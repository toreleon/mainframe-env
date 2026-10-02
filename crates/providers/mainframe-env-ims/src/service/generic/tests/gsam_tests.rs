use super::*;
use mainframe_env_host_api::{
    HostLimits, ImsGsamAddress, ImsGsamRequest, ImsGsamResult, ImsGsamSearchArgument,
};
mod failure_tests;

fn snapshot(service: &ImsService) -> (Vec<u8>, RowVersions) {
    let durable = service.lock().unwrap();
    (
        serde_json::to_vec(&durable.state).unwrap(),
        durable.versions.clone(),
    )
}
fn gu(run: &str, seq: u64, pcb: u16, address: ImsGsamAddress) -> ImsGsamRequest {
    let mut req = gsam(run, seq, ImsOperation::GetUnique, pcb, b"");
    req.search = Some(ImsGsamSearchArgument::Record(address));
    req
}

fn metadata() -> ImsMetadataCatalog {
    let mut metadata = catalog();
    let db = &mut metadata.databases[0];
    db.organization = ImsDatabaseOrganization::Gsam;
    db.segments.truncate(1);
    db.segments[0].fields.clear();
    let mut other = db.clone();
    other.name = "OTHER".into();
    metadata.databases.push(other);
    let ImsPcbMetadata::Database(pcb) = &mut metadata.psbs[0].pcbs[0] else {
        unreachable!()
    };
    pcb.processing_options = "G".into();
    pcb.sensitive_segments.truncate(1);
    let read = pcb.clone();
    let mut write = read.clone();
    write.name = "WRITER".into();
    write.processing_options = "L".into();
    let mut second = read.clone();
    second.name = "SECOND".into();
    let mut foreign = read.clone();
    foreign.name = "FOREIGN".into();
    foreign.database = "OTHER".into();
    metadata.psbs[0].pcbs = vec![
        ImsPcbMetadata::Database(read),
        ImsPcbMetadata::Database(write),
        ImsPcbMetadata::Database(second),
        ImsPcbMetadata::Database(foreign),
    ];
    metadata
}
fn installed(store: Arc<dyn ProviderStateStore>, run: &str) -> Arc<ImsService> {
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    service.install_metadata(metadata()).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    service
}
fn gsam(run: &str, sequence: u64, op: ImsOperation, pcb: u16, data: &[u8]) -> ImsGsamRequest {
    let mut request = request(run, op, sequence, &[], data);
    request.pcb = pcb;
    ImsGsamRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        search: None,
        save_address: op != ImsOperation::GetUnique,
    }
}
fn public(
    service: Arc<ImsService>,
    run: &str,
    req: ImsGsamRequest,
) -> Result<ImsGsamResult, HostProblem> {
    let req = HostRequest::ImsGsam(req);
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
        HostResult::ImsGsam(result) => Ok(result),
        _ => panic!("wrong result family"),
    }
}
#[test]
fn public_gsam_address_generation_lookup_and_independent_pcbs() {
    let run = "gsam-roundtrip";
    let service = installed(Arc::new(MemoryStore::new(Default::default())), run);
    let inserted = public(
        service.clone(),
        run,
        gsam(run, 2, ImsOperation::Insert, 2, b"A1X"),
    )
    .unwrap();
    assert_eq!(inserted.result.status, "  ");
    assert_eq!(inserted.result.affected_segments, 1);
    let address = inserted.address.unwrap();
    let first = public(
        service.clone(),
        run,
        gsam(run, 3, ImsOperation::GetNext, 1, b""),
    )
    .unwrap();
    assert_eq!(first.result.segments[0].data, b"A1X");
    assert_eq!(first.address, Some(address.clone()));
    let second = public(
        service.clone(),
        run,
        gsam(run, 4, ImsOperation::GetNext, 3, b""),
    )
    .unwrap();
    assert_eq!(second, first);
    let mut unique = gsam(run, 5, ImsOperation::GetUnique, 1, b"");
    unique.search = Some(ImsGsamSearchArgument::Record(address));
    assert_eq!(
        public(service.clone(), run, unique)
            .unwrap()
            .result
            .segments[0]
            .data,
        b"A1X"
    );
}

fn exercise(
    store: Arc<dyn ProviderStateStore>,
    run: &str,
) -> (ImsGsamAddress, ImsGsamRequest, ImsGsamResult) {
    let service = installed(store, run);
    let insert = gsam(run, 2, ImsOperation::Insert, 2, b"A1X");
    let inserted = public(service.clone(), run, insert.clone()).unwrap();
    let first = inserted.address.clone().unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 3, &[], b""),
    );
    let second = public(
        service.clone(),
        run,
        gsam(run, 4, ImsOperation::Insert, 2, b"B2Y"),
    )
    .unwrap()
    .address
    .unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Commit, 5, &[], b""),
    );
    let next = gsam(run, 6, ImsOperation::GetNext, 1, b"");
    let found = public(service.clone(), run, next.clone()).unwrap();
    assert_eq!(found.address, Some(first.clone()));
    let before = snapshot(&service);
    assert_eq!(public(service.clone(), run, next.clone()).unwrap(), found);
    assert_eq!(snapshot(&service), before);
    let mut conflict = next.clone();
    conflict.save_address = false;
    assert_eq!(
        public(service.clone(), run, conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(snapshot(&service), before);
    let next_second = public(
        service.clone(),
        run,
        gsam(run, 7, ImsOperation::GetNext, 1, b""),
    )
    .unwrap();
    assert_eq!(next_second.address, Some(second));
    let position = pcb::position(&service.lock().unwrap().state.sessions[run], 1);
    let missing = public(
        service.clone(),
        run,
        gsam(run, 8, ImsOperation::GetUnique, 1, b""),
    )
    .unwrap();
    assert_eq!(missing.result.status, "AH");
    assert_eq!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 1),
        position
    );
    for (seq, address, pcb_number) in [
        (9, first.clone(), 4),
        (
            10,
            ImsGsamAddress {
                database: first.database.clone(),
                token: [0x77; 32],
            },
            1,
        ),
    ] {
        let invalid = public(service.clone(), run, gu(run, seq, pcb_number, address)).unwrap();
        assert_eq!(invalid.result.status, "AJ");
        assert!(invalid.result.segments.is_empty() && invalid.address.is_none());
        assert_eq!(
            pcb::position(&service.lock().unwrap().state.sessions[run], 1),
            position
        );
    }
    let eof = public(
        service.clone(),
        run,
        gsam(run, 11, ImsOperation::GetNext, 1, b""),
    )
    .unwrap();
    assert_eq!(eof.result.status, "GB");
    assert!(eof.result.segments.is_empty() && eof.address.is_none());
    let pos = pcb::position(&service.lock().unwrap().state.sessions[run], 1);
    assert!(pos.current().is_none() && pos.parentage().is_none() && !pos.is_held());
    assert_eq!(
        serde_json::to_value(&service.lock().unwrap().state.sessions[run].system).unwrap()["statuses"]
            ["1"],
        "GB"
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            gsam(run, 12, ImsOperation::GetNext, 1, b"")
        )
        .unwrap()
        .address,
        Some(first.clone())
    );
    let mut beginning = gsam(run, 13, ImsOperation::GetUnique, 1, b"");
    beginning.search = Some(ImsGsamSearchArgument::Beginning);
    assert_eq!(
        public(service.clone(), run, beginning)
            .unwrap()
            .result
            .status,
        "  "
    );
    assert!(
        pcb::position(&service.lock().unwrap().state.sessions[run], 1)
            .current()
            .is_none()
    );
    let mut without = gsam(run, 14, ImsOperation::GetNext, 1, b"");
    without.save_address = false;
    let found = public(service.clone(), run, without).unwrap();
    assert_eq!(found.result.segments[0].data, b"A1X");
    assert_eq!(found.address, None);
    assert_eq!(public(service, run, insert.clone()).unwrap(), inserted);
    (
        first,
        next,
        ImsGsamResult {
            result: found.result,
            address: Some(inserted.address.unwrap()),
        },
    )
}

#[test]
fn public_gsam_status_position_replay_and_memory_reopen() {
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let run = "gsam-memory";
    let (address, next, expected) = exercise(store.clone(), run);
    let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
    let before = snapshot(&reopened);
    assert_eq!(public(reopened.clone(), run, next).unwrap(), expected);
    assert_eq!(snapshot(&reopened), before);
    assert_eq!(
        public(reopened, run, gu(run, 15, 1, address))
            .unwrap()
            .result
            .segments[0]
            .data,
        b"A1X"
    );
}

#[test]
fn public_gsam_status_position_replay_and_fresh_sqlite_reopen() {
    let file = std::env::temp_dir().join(format!(
        "ims-gsam-{}-{}.sqlite",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let run = "gsam-sqlite";
    let (address, next, expected) = {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        exercise(store, run)
    };
    {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        let before = snapshot(&reopened);
        assert_eq!(public(reopened.clone(), run, next).unwrap(), expected);
        assert_eq!(snapshot(&reopened), before);
        assert_eq!(
            public(reopened, run, gu(run, 15, 1, address))
                .unwrap()
                .result
                .segments[0]
                .data,
            b"A1X"
        );
    }
    std::fs::remove_file(file).unwrap();
}

#[test]
fn public_gsam_rollback_reused_occurrence_does_not_revive_stale_address() {
    for store in failure_tests::backends() {
        let run = "gsam-stale";
        let service = installed(store.clone(), run);
        let old = public(
            service.clone(),
            run,
            gsam(run, 2, ImsOperation::Insert, 2, b"A1X"),
        )
        .unwrap()
        .address
        .unwrap();
        execute(
            &service,
            run,
            &request(run, ImsOperation::Rollback, 3, &[], b""),
        );
        let replacement = public(
            service.clone(),
            run,
            gsam(run, 4, ImsOperation::Insert, 2, b"A1X"),
        )
        .unwrap()
        .address
        .unwrap();
        assert_ne!(old, replacement);
        assert_eq!(
            public(service.clone(), run, gu(run, 5, 1, old))
                .unwrap()
                .result
                .status,
            "AJ"
        );
        assert_eq!(
            public(service, run, gu(run, 6, 1, replacement))
                .unwrap()
                .result
                .segments[0]
                .data,
            b"A1X"
        );
    }
}

#[test]
fn public_gsam_rejects_context_pcb_procopt_call_and_ssa_before_mutation() {
    let run = "gsam-invalid";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = installed(store.clone(), run);
    let before = snapshot(&service);
    for (seq, context) in [
        ImsExecutionContext::DbDc,
        ImsExecutionContext::Dbctl,
        ImsExecutionContext::Dcctl,
        ImsExecutionContext::TmBatch,
    ]
    .into_iter()
    .enumerate()
    {
        let mut req = gsam(run, 2 + seq as u64, ImsOperation::GetNext, 1, b"");
        req.context = context;
        assert_eq!(
            public(service.clone(), run, req),
            Err(HostProblem::Unsupported)
        );
    }
    for req in [
        gsam(run, 10, ImsOperation::Insert, 1, b"A1X"),
        gsam(run, 11, ImsOperation::GetNext, 2, b""),
    ] {
        assert_eq!(
            public(service.clone(), run, req),
            Err(HostProblem::Unsupported)
        );
    }
    assert_eq!(
        public(
            service.clone(),
            run,
            gsam(run, 12, ImsOperation::GetNext, 5, b"")
        ),
        Err(HostProblem::NotFound)
    );
    for operation in [
        ImsOperation::Replace,
        ImsOperation::Delete,
        ImsOperation::GetHoldNext,
        ImsOperation::GetNextParent,
    ] {
        assert_eq!(
            public(service.clone(), run, gsam(run, 13, operation, 1, b"")),
            Err(HostProblem::Malformed)
        );
    }
    let mut ssa = gsam(run, 14, ImsOperation::GetNext, 1, b"");
    ssa.request.segments = vec!["ROOT".into()];
    assert_eq!(
        public(service.clone(), run, ssa),
        Err(HostProblem::Malformed)
    );
    let mut wrong = gsam(run, 15, ImsOperation::GetNext, 1, b"");
    wrong.search = Some(ImsGsamSearchArgument::Beginning);
    assert_eq!(
        public(service.clone(), run, wrong),
        Err(HostProblem::Malformed)
    );
    let mut oversized = gsam(run, 16, ImsOperation::Insert, 2, b"");
    oversized.request.data = vec![0; HostLimits::default().max_record_bytes + 1];
    assert_eq!(
        public(service.clone(), run, oversized),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(snapshot(&service), before);
    // The public SSA route continues to reject GSAM.
    assert_eq!(
        service.execute_navigation(
            &invocation(run),
            &mainframe_env_host_api::ImsNavigationRequest {
                request: request(run, ImsOperation::GetNext, 17, &[], b""),
                context: ImsExecutionContext::DbBatch,
                ssas: vec![]
            }
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&service), before);
    let other = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        ImsLimits::default(),
    )
    .unwrap();
    other.install_metadata(catalog()).unwrap();
    execute(
        &other,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let before = snapshot(&other);
    assert_eq!(
        public(
            other.clone(),
            run,
            gsam(run, 2, ImsOperation::GetNext, 1, b"")
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&other), before);
}

#[test]
fn public_gsam_authorization_precedes_address_lookup_and_replay() {
    let run = "gsam-auth";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let seeded = installed(store.clone(), run);
    let inserted = public(seeded, run, gsam(run, 2, ImsOperation::Insert, 2, b"A1X")).unwrap();
    let policy = Arc::new(Policy::default());
    let service = ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
    let req = gu(run, 3, 1, inserted.address.unwrap());
    *policy.deny_update.lock().unwrap() = true;
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, req.clone()),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(
        public(
            service.clone(),
            run,
            gsam(run, 4, ImsOperation::Insert, 2, b"B2Y")
        ),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(snapshot(&service), before);
    *policy.deny_update.lock().unwrap() = false;
    assert_eq!(
        public(service.clone(), run, req.clone())
            .unwrap()
            .result
            .segments[0]
            .data,
        b"A1X"
    );
    *policy.deny_update.lock().unwrap() = true;
    let before = snapshot(&service);
    assert_eq!(
        public(service.clone(), run, req),
        Err(HostProblem::Unauthorized)
    );
    assert_eq!(snapshot(&service), before);
    assert!(
        policy
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.class == EnterpriseResourceClass::ImsDatabase && r.name.as_str() == "GENDB")
    );
}

#[test]
fn public_gsam_empty_eof_no_save_and_source_procopt_variants() {
    let run = "gsam-options";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(store, ImsLimits::default()).unwrap();
    let mut catalog = metadata();
    for (i, raw) in [(0, "GS"), (1, "LS"), (2, "AP")] {
        let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[i] else {
            unreachable!()
        };
        pcb.processing_options = raw.into();
    }
    service.install_metadata(catalog).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    for seq in [2, 3] {
        let eof = public(
            service.clone(),
            run,
            gsam(run, seq, ImsOperation::GetNext, 1, b""),
        )
        .unwrap();
        assert_eq!(eof.result.status, "GB");
        assert!(eof.result.segments.is_empty() && eof.address.is_none());
    }
    let mut insert = gsam(run, 4, ImsOperation::Insert, 2, b"A1X");
    insert.save_address = false;
    let result = public(service.clone(), run, insert).unwrap();
    assert_eq!(result.result.affected_segments, 1);
    assert!(result.address.is_none());
    let address = public(
        service.clone(),
        run,
        gsam(run, 5, ImsOperation::GetNext, 1, b""),
    )
    .unwrap()
    .address
    .unwrap();
    assert_eq!(
        public(service.clone(), run, gu(run, 6, 1, address))
            .unwrap()
            .result
            .segments[0]
            .data,
        b"A1X"
    );
    let before = snapshot(&service);
    assert_eq!(
        public(
            service.clone(),
            run,
            gsam(run, 7, ImsOperation::GetNext, 3, b"")
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&service), before);
}

#[test]
fn public_gsam_record_limits_format_rejection_and_authorizer_failure_are_atomic() {
    let run = "gsam-limits";
    let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
    let service = ImsService::open(
        store.clone(),
        ImsLimits {
            max_roots: 1,
            ..ImsLimits::default()
        },
    )
    .unwrap();
    service.install_metadata(metadata()).unwrap();
    execute(
        &service,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let before = snapshot(&service);
    assert_eq!(
        public(
            service.clone(),
            run,
            gsam(run, 2, ImsOperation::Insert, 2, b"BADLEN")
        ),
        Err(HostProblem::Malformed)
    );
    assert_eq!(snapshot(&service), before);
    public(
        service.clone(),
        run,
        gsam(run, 3, ImsOperation::Insert, 2, b"A1X"),
    )
    .unwrap();
    let before = snapshot(&service);
    assert_eq!(
        public(
            service.clone(),
            run,
            gsam(run, 4, ImsOperation::Insert, 2, b"B2Y")
        ),
        Err(HostProblem::ResourceExhausted)
    );
    assert_eq!(snapshot(&service), before);
    struct Failed;
    impl EnterpriseAuthorizer for Failed {
        fn authorize(&self, _: &PrincipalId, _: &EnterpriseResource) -> Result<(), HostProblem> {
            Err(HostProblem::InfrastructureFailure)
        }
    }
    let failed = ImsService::open_authorized(
        store,
        ImsLimits {
            max_roots: 1,
            ..ImsLimits::default()
        },
        Arc::new(Failed),
    )
    .unwrap();
    let before = snapshot(&failed);
    assert_eq!(
        public(
            failed.clone(),
            run,
            gsam(run, 5, ImsOperation::GetNext, 1, b"")
        ),
        Err(HostProblem::InfrastructureFailure)
    );
    assert_eq!(snapshot(&failed), before);
    let variable = ImsService::open(
        Arc::new(MemoryStore::new(Default::default())),
        ImsLimits::default(),
    )
    .unwrap();
    let mut metadata = metadata();
    metadata.databases[0].segments[0].max_length = 4;
    variable.install_metadata(metadata).unwrap();
    execute(
        &variable,
        run,
        &request(run, ImsOperation::Schedule, 1, &[], b""),
    );
    let before = snapshot(&variable);
    assert_eq!(
        public(
            variable.clone(),
            run,
            gsam(run, 2, ImsOperation::GetNext, 1, b"")
        ),
        Err(HostProblem::Unsupported)
    );
    assert_eq!(snapshot(&variable), before);
}
