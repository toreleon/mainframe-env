//! Real signed selected package and canonical coordinator, both store profiles.
use super::application_recovery::{recovery_db, recovery_request, run_recovery_machine};
use super::*;
use mainframe_env_host_api::{
    ImsExecutionContext, ImsNavigationRequest, ImsRecoveryCall, ImsRecoveryResult,
    ImsRestartSelection, ImsSecondaryIndexMetadata,
};

fn nav(seq: u64, pcb: u16, operation: ImsOperation, ssas: &[&[u8]]) -> HostRequest {
    let mut request = recovery_db(operation, seq, "signed-secondary");
    request.pcb = pcb;
    HostRequest::ImsNavigation(ImsNavigationRequest {
        request,
        context: ImsExecutionContext::DbBatch,
        ssas: ssas.iter().map(|s| s.to_vec()).collect(),
    })
}

fn bytes(result: &EffectResult) -> &[u8] {
    match &result.outcome {
        Ok(HostResult::Ims(result)) => &result.segments[0].data,
        other => panic!("selected database result: {other:?}"),
    }
}

fn exercise(store: Arc<dyn PlatformStore>, config: ServerConfig) {
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
    let c = package.sections.ims_metadata.as_mut().unwrap();
    c.databases[0].segments[0].fields.push(ImsFieldMetadata {
        name: Some("KIND".into()),
        offset: 8,
        length: 1,
        sequence: false,
        unique: false,
    });
    c.databases[0]
        .secondary_indexes
        .push(ImsSecondaryIndexMetadata {
            name: "BYVALUE".into(),
            source_segment: "ROOT".into(),
            target_segment: "ROOT".into(),
            source_fields: vec!["KIND".into()],
        });
    let ImsPcbMetadata::Database(mut indexed) = c.psbs[0].pcbs[0].clone() else {
        unreachable!()
    };
    indexed.name = "INDEXPCB".into();
    indexed.secondary_index = Some("BYVALUE".into());
    c.psbs[0].pcbs.push(ImsPcbMetadata::Database(indexed));
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
    let mut first = tm_invocation("signed-secondary-run", "signed-secondary-first");
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
            &recovery_db(ImsOperation::Schedule, 900, "signed-secondary-schedule"),
        )
        .unwrap();
    let recovery = |seq, call| {
        HostRequest::ImsRecovery(recovery_request(
            &staged.identity,
            seq,
            "signed-secondary-recovery",
            call,
        ))
    };
    let insert = |seq, data: &[u8]| {
        let mut r = recovery_db(ImsOperation::Insert, seq, "signed-secondary");
        r.segments = vec!["ROOT".into()];
        r.data = data.to_vec();
        HostRequest::Ims(r)
    };
    let requests = vec![
        recovery(
            1,
            ImsRecoveryCall::Restart {
                selection: ImsRestartSelection::Normal,
                area_lengths: vec![],
            },
        ),
        insert(2, b"00000001ZROOTDAT"),
        insert(3, b"00000002AROOTDAT"),
        nav(
            4,
            2,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (BYVALUE EQA)"],
        ),
        recovery(
            5,
            ImsRecoveryCall::SymbolicCheckpoint {
                id: "SECINDEX".into(),
                user_areas: vec![b"SAVE".to_vec()],
            },
        ),
    ];
    let results = run_recovery_machine(&server, store.clone(), &first, requests.clone());
    assert_eq!(bytes(&results[3]), b"00000002AROOTDAT");
    assert!(matches!(
        &results[4].outcome,
        Ok(HostResult::ImsRecovery(
            ImsRecoveryResult::Checkpointed { .. }
        ))
    ));
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
        ExecutionId::new("signed-secondary-restart", InvocationLimits::default()).unwrap();
    next.deadline_tick = server.jes_clock.now_tick().unwrap() + 60_000;
    let results = run_recovery_machine(
        &server,
        store.clone(),
        &next,
        vec![
            recovery(
                6,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("SECINDEX".into()),
                    area_lengths: vec![4],
                },
            ),
            nav(7, 2, ImsOperation::GetNext, &[b"ROOT     "]),
            nav(
                8,
                2,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (BYVALUE EQA)"],
            ),
        ],
    );
    assert!(
        matches!(&results[0].outcome,Ok(HostResult::ImsRecovery(ImsRecoveryResult::Restarted { user_areas,pcb_statuses,.. })) if user_areas==&vec![b"SAVE".to_vec()] && pcb_statuses==&vec![(1,"  ".into()),(2,"  ".into())])
    );
    assert_eq!(bytes(&results[1]), b"00000001ZROOTDAT");
    assert_eq!(bytes(&results[2]), b"00000002AROOTDAT");
    let mut replace = recovery_db(ImsOperation::Replace, 9, "signed-secondary");
    replace.pcb = 2;
    replace.data = b"00000002BROOTDAT".to_vec();
    assert_eq!(
        server
            .ims_execute_selected("SIGNED-IMS-APPLICATION", &next, &replace)
            .unwrap()
            .status,
        "  "
    );
}

#[test]
fn signed_memory_secondary_checkpoint_restart_canonical_coordinator() {
    exercise(Arc::new(MemoryStore::new(Default::default())), config());
}

#[test]
fn signed_sqlite_secondary_checkpoint_restart_canonical_coordinator_reopen() {
    let path = std::env::temp_dir().join(format!(
        "ims-signed-secondary-checkpoint-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let mut c = config();
    c.store_profile = crate::StoreProfile::Sqlite;
    c.sqlite_url = url.clone();
    exercise(
        Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262144).unwrap()),
        c,
    );
    std::fs::remove_file(path).unwrap();
}
