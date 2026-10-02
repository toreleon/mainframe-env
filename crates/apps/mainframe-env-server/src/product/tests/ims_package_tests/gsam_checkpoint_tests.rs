//! Signed selected package, canonical coordinator, public GSAM route and reopen.
use super::application_recovery::{recovery_db, recovery_request, run_recovery_machine};
use super::*;
use mainframe_env_host_api::{
    ImsExecutionContext, ImsGsamAddress, ImsGsamRequest, ImsGsamSearchArgument, ImsRecoveryCall,
    ImsRecoveryResult, ImsRestartSelection,
};

fn db(sequence: u64, operation: ImsOperation, pcb: u16, data: &[u8]) -> ImsGsamRequest {
    let mut request = recovery_db(operation, sequence, "signed-gsam");
    request.pcb = pcb;
    request.data = data.to_vec();
    ImsGsamRequest {
        undefined_length: None,
        request,
        context: ImsExecutionContext::DbBatch,
        save_address: operation != ImsOperation::GetUnique,
        search: None,
    }
}

fn address(result: &EffectResult) -> ImsGsamAddress {
    let Ok(HostResult::ImsGsam(result)) = &result.outcome else {
        panic!("GSAM output: {result:?}")
    };
    result.address.clone().unwrap()
}

fn exercise(store: Arc<dyn PlatformStore>, config: ServerConfig) {
    exercise_format(store, config, None);
}

fn exercise_format(
    store: Arc<dyn PlatformStore>,
    config: ServerConfig,
    kind: Option<mainframe_env_host_api::ImsGsamRecordFormat>,
) {
    use mainframe_env_host_api::{
        ImsGsamAccessMethod, ImsGsamControl, ImsGsamFormat, ImsGsamRecordFormat,
    };
    let first_area: &[u8] = match kind {
        Some(ImsGsamRecordFormat::V) => &[0, 4, 0, 0xff],
        Some(ImsGsamRecordFormat::U) => b"0123456789AB",
        _ => b"00000001ROOTDATA",
    };
    let second_area: &[u8] = match kind {
        Some(ImsGsamRecordFormat::V) => &[0, 6, 0x41, 0, 0xff, 0x42],
        Some(ImsGsamRecordFormat::U) => b"0123456789ABCDEF",
        _ => b"00000002ROOTDATA",
    };
    let later_area: &[u8] = match kind {
        Some(ImsGsamRecordFormat::V) => &[0, 3, 0x43],
        Some(ImsGsamRecordFormat::U) => b"0123456789ABC",
        _ => b"00000003ROOTDATA",
    };
    let db = |seq, op, pcb, data: &[u8]| {
        let mut req = db(seq, op, pcb, data);
        if op == ImsOperation::Insert && kind == Some(ImsGsamRecordFormat::U) {
            req.undefined_length = Some(data.len() as u32);
        }
        req
    };
    let trust = Arc::new(test_package_trust());
    let server = ProductServer::open_with_package_trust(
        config.clone(),
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust.clone(),
    )
    .unwrap();
    let mut package = signed_ims_package(&trust, 1, 1);
    let catalog = package.sections.ims_metadata.as_mut().unwrap();
    catalog.databases[0].organization = ImsDatabaseOrganization::Gsam;
    catalog.databases[0].segments.truncate(1);
    catalog.databases[0].segments[0].fields.clear();
    if let Some(kind) = kind {
        let record = &mut catalog.databases[0].segments[0];
        record.min_length = if kind == ImsGsamRecordFormat::U {
            12
        } else {
            2
        };
        record.max_length = 16;
        catalog.databases[0].gsam_format = Some(ImsGsamFormat {
            version: 1,
            record_format: kind,
            access_method: ImsGsamAccessMethod::Bsam,
            block_size: 32,
            control: ImsGsamControl::None,
        });
    }
    let ImsPcbMetadata::Database(pcb) = &mut catalog.psbs[0].pcbs[0] else {
        unreachable!()
    };
    pcb.processing_options = "G".into();
    pcb.sensitive_segments.truncate(1);
    let mut output = pcb.clone();
    output.name = "OUTPUT".into();
    output.processing_options = "L".into();
    catalog.psbs[0].pcbs.push(ImsPcbMetadata::Database(output));
    resign_package(&mut package, &trust);
    let staged = server.install_application_package_v2(&package).unwrap();
    server.publish_application_generation(&staged).unwrap();
    server
        .bootstrap_administrator("IBMUSER", b"TESTPASS")
        .unwrap();
    for (class, name) in [("IMSPSB", "AUTHPSB"), ("IMSDB", "AUTHDB")] {
        server
            .racf
            .define_profile(class, name, "IBMUSER", Some(AccessIntent::Control))
            .unwrap();
    }
    let mut first = tm_invocation("signed-gsam-checkpoint-run", "signed-gsam-first");
    first.service_class = ServiceClass::Batch;
    first.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    first.principal = Principal::new(
        PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
        [CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]
            .into_iter()
            .collect(),
        InvocationLimits::default(),
    )
    .unwrap();
    server
        .ims_execute_selected(
            "SIGNED-IMS-APPLICATION",
            &first,
            &recovery_db(ImsOperation::Schedule, 900, "signed-gsam-schedule"),
        )
        .unwrap();
    let recovery = |seq, call| {
        HostRequest::ImsRecovery(recovery_request(
            &staged.identity,
            seq,
            "signed-gsam-recovery",
            call,
        ))
    };
    let requests = vec![
        recovery(
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        ),
        HostRequest::ImsGsam(db(2, ImsOperation::Insert, 2, first_area)),
        HostRequest::ImsGsam(db(3, ImsOperation::Insert, 2, second_area)),
        HostRequest::ImsGsam(db(4, ImsOperation::GetNext, 1, b"")),
        recovery(
            5,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "SIGNED".into(),
                user_areas: vec![b"SAVE".to_vec()],
            },
        ),
        HostRequest::ImsGsam(db(6, ImsOperation::Insert, 2, later_area)),
    ];
    let results = run_recovery_machine(&server, store.clone(), &first, requests.clone());
    let a = address(&results[1]);
    let b = address(&results[2]);
    let later = address(&results[5]);
    assert_eq!(address(&results[3]), a);
    assert!(
        matches!(&results[4].outcome, Ok(HostResult::ImsRecovery(ImsRecoveryResult::Checkpointed { id, .. })) if id == "SIGNED")
    );
    for (request, result) in requests.iter().zip(&results) {
        let journal = store
            .effect(&request.mutation().unwrap().idempotency_key)
            .unwrap()
            .unwrap();
        assert_eq!(journal.state, EffectState::Completed);
        assert_eq!(
            journal.request_digest,
            mainframe_env_host_api::canonical_request_digest(request).unwrap()
        );
        assert_eq!(
            journal.result_digest,
            Some(mainframe_env_host_api::canonical_result_digest(&result.outcome).unwrap())
        );
    }
    drop(server);
    let server = ProductServer::open_with_package_trust(
        config,
        store.clone(),
        Arc::new(MemorySecretResolver::default()),
        default_program_router(),
        trust,
    )
    .unwrap();
    let mut next = first.clone();
    next.execution_id =
        ExecutionId::new("signed-gsam-restarted", InvocationLimits::default()).unwrap();
    next.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &next,
        vec![
            recovery(
                7,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("SIGNED".into()),
                    area_lengths: vec![4],
                },
            ),
            HostRequest::ImsGsam(db(8, ImsOperation::GetNext, 1, b"")),
        ],
    );
    assert!(
        matches!(&results[0].outcome, Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted { user_areas, pcb_statuses, .. })) if user_areas == &vec![b"SAVE".to_vec()] && pcb_statuses == &vec![(1, "  ".into()), (2, "  ".into())])
    );
    assert_eq!(address(&results[1]), b);
    assert!(
        matches!(&results[1].outcome,Ok(HostResult::ImsGsam(r)) if r.result.segments[0].data == second_area && r.undefined_length == (kind==Some(ImsGsamRecordFormat::U)).then_some(16))
    );
    let mut gu = db(9, ImsOperation::GetUnique, 1, b"");
    gu.search = Some(ImsGsamSearchArgument::Record(a));
    assert_eq!(
        server
            .ims_gsam_selected("SIGNED-IMS-APPLICATION", &next, &gu)
            .unwrap()
            .result
            .segments[0]
            .data,
        first_area
    );
    gu.request.mutation = db(10, ImsOperation::GetUnique, 1, b"").request.mutation;
    gu.search = Some(ImsGsamSearchArgument::Record(later));
    assert_eq!(
        server
            .ims_gsam_selected("SIGNED-IMS-APPLICATION", &next, &gu)
            .unwrap()
            .result
            .status,
        "AJ"
    );
    assert_eq!(
        store
            .audit_records(&first.execution_id, 0, 64)
            .unwrap()
            .len(),
        6
    );
    if kind.is_some() {
        let before = store
            .get_provider_state("ims-v1-generic-database", "AUTHDB")
            .unwrap();
        let malformed = db(20, ImsOperation::Insert, 2, &[0, 0]);
        assert_eq!(
            server.ims_gsam_selected("SIGNED-IMS-APPLICATION", &next, &malformed),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            store
                .get_provider_state("ims-v1-generic-database", "AUTHDB")
                .unwrap(),
            before
        );
        let mut denied = next.clone();
        denied.principal = Principal::new(
            PrincipalId::new("DENIED", InvocationLimits::default()).unwrap(),
            [CapabilityId::new("host.ims.write", InvocationLimits::default()).unwrap()]
                .into_iter()
                .collect(),
            InvocationLimits::default(),
        )
        .unwrap();
        for req in [
            db(21, ImsOperation::GetNext, 1, b""),
            db(22, ImsOperation::Insert, 2, first_area),
        ] {
            assert_eq!(
                server.ims_gsam_selected("SIGNED-IMS-APPLICATION", &denied, &req),
                Err(HostProblem::Unauthorized)
            );
            assert_eq!(
                store
                    .get_provider_state("ims-v1-generic-database", "AUTHDB")
                    .unwrap(),
                before
            );
        }
    }
}

#[test]
fn signed_memory_gsam_checkpoint_restart_roundtrip() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_gsam_checkpoint_restart_roundtrip() {
    let path = std::env::temp_dir().join(format!(
        "ims-signed-gsam-checkpoint-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut config = config();
    config.store_profile = crate::StoreProfile::Sqlite;
    config.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        config,
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn signed_gsam_formats_memory_selected_package_checkpoint_restart() {
    for kind in [
        mainframe_env_host_api::ImsGsamRecordFormat::V,
        mainframe_env_host_api::ImsGsamRecordFormat::U,
    ] {
        exercise_format(
            Arc::new(MemoryStore::new(Default::default())),
            config(),
            Some(kind),
        );
    }
}

#[test]
fn signed_gsam_formats_sqlite_selected_package_checkpoint_restart() {
    for kind in [
        mainframe_env_host_api::ImsGsamRecordFormat::V,
        mainframe_env_host_api::ImsGsamRecordFormat::U,
    ] {
        let path = std::env::temp_dir().join(format!(
            "signed-gsam-formats-{kind:?}-{}.sqlite",
            std::process::id()
        ));
        let url = format!("sqlite:{}?mode=rwc", path.display());
        let mut config = config();
        config.store_profile = crate::StoreProfile::Sqlite;
        config.sqlite_url = url.clone();
        exercise_format(
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
            config,
            Some(kind),
        );
        std::fs::remove_file(path).unwrap();
    }
}
