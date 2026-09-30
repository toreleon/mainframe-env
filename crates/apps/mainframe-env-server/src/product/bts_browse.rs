#[test]
fn compiled_online_source_resolved_bts_browse_recovers_after_sqlite_restart() {
    use mainframe_env_cics::bts_lifecycle::{BtsProcessTypeDefinition, BtsTransactionDefinition};

    let root = std::env::temp_dir().join(format!(
        "mainframe-env-bts-browse-restart-{}-{:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        session_tick().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
    let mut settings = config();
    settings.store_profile = crate::StoreProfile::Sqlite;
    settings.sqlite_url = url.clone();
    settings.artifact_root = root.join("artifacts");
    let secrets = Arc::new(MemorySecretResolver::default());
    let first_store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let first_platform: Arc<dyn PlatformStore> = first_store.clone();
    let first = ProductServer::open(
        settings.clone(),
        first_platform,
        secrets.clone(),
        default_program_router(),
    )
    .unwrap();
    first.bootstrap_administrator("IBMUSER", b"TESTPASS").unwrap();
    first
        .cics
        .register_bts_process_type(
            BtsProcessTypeDefinition::new("TYPE", "BTS.REPO", true).unwrap(),
        )
        .unwrap();
    first
        .cics
        .register_bts_transaction(
            BtsTransactionDefinition::new("BT01", "BTSBR", true, false).unwrap(),
        )
        .unwrap();
    first
        .racf
        .define_profile("BTSREPO", "BTS.REPO", "IBMUSER", None)
        .unwrap();
    first
        .racf
        .permit("BTSREPO", "BTS.REPO", "IBMUSER", AccessIntent::Read)
        .unwrap();
    first
        .racf
        .permit("BTSREPO", "BTS.REPO", "IBMUSER", AccessIntent::Update)
        .unwrap();
    let resource = BtsLifecycleStore::saf_resource("TYPE", "ORDER").unwrap();
    first
        .racf
        .define_profile("BTSLIFE", &resource, "IBMUSER", None)
        .unwrap();
    first
        .racf
        .permit("BTSLIFE", &resource, "IBMUSER", AccessIntent::Read)
        .unwrap();
    first
        .racf
        .permit("BTSLIFE", &resource, "IBMUSER", AccessIntent::Update)
        .unwrap();
    let authority = BtsLifecycleStore::new(first_store.as_ref());
    let root_id = BtsLifecycleStore::root_id("TYPE", "ORDER", "SEED-UOW").unwrap();
    authority
        .define_process(
            BtsProcess::new("TYPE", "ORDER", &root_id, "BTSBR", "BT01", "IBMUSER", "SEED-UOW")
                .unwrap(),
            "SEED-UOW",
            "SEED-EXEC",
            "IBMUSER",
        )
        .unwrap();
    authority
        .finish_uow("SEED-UOW", "SEED-EXEC", "IBMUSER", true)
        .unwrap();
    drop(first);
    drop(first_store);

    let source = concat!(
        "IDENTIFICATION DIVISION. PROGRAM-ID. BTSBR. ",
        "DATA DIVISION. WORKING-STORAGE SECTION. ",
        "01 TOKEN-X PIC S9(9) COMP. 01 PROCESS-X PIC X(36). ",
        "01 ROOT-X PIC X(52). 01 ACT-TOKEN PIC S9(9) COMP. ",
        "01 ACT-NAME PIC X(16). 01 ACT-ID PIC X(52). 01 EVENT-X PIC X(16). ",
        "01 ITEM-X PIC X(16). 01 DATA-X PIC X(4) VALUE 'DATA'. ",
        "01 LEN-X PIC S9(9) COMP. 01 RESP-X PIC S9(9) COMP. ",
        "PROCEDURE DIVISION. ",
        "EXEC CICS STARTBROWSE PROCESS PROCESSTYPE('TYPE') BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS GETNEXT PROCESS(PROCESS-X) BROWSETOKEN(TOKEN-X) ACTIVITYID(ROOT-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE PROCESS BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS INQUIRE PROCESS('ORDER') PROCESSTYPE('TYPE') ACTIVITYID(ROOT-X) END-EXEC. ",
        "EXEC CICS STARTBROWSE ACTIVITY PROCESS('ORDER') PROCESSTYPE('TYPE') BROWSETOKEN(ACT-TOKEN) END-EXEC. ",
        "EXEC CICS GETNEXT ACTIVITY(ACT-NAME) BROWSETOKEN(ACT-TOKEN) ACTIVITYID(ACT-ID) END-EXEC. ",
        "EXEC CICS ENDBROWSE ACTIVITY BROWSETOKEN(ACT-TOKEN) END-EXEC. ",
        "EXEC CICS INQUIRE ACTIVITYID(ROOT-X) ACTIVITY(ACT-NAME) END-EXEC. ",
        "EXEC CICS DEFINE INPUT EVENT('READY') END-EXEC. ",
        "EXEC CICS DEFINE TIMER('WAKE') EVENT('BELL') AFTER SECONDS(5) END-EXEC. ",
        "EXEC CICS STARTBROWSE EVENT ACTIVITYID(ROOT-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS GETNEXT EVENT(EVENT-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE EVENT BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS INQUIRE EVENT('READY') ACTIVITYID(ROOT-X) END-EXEC. ",
        "EXEC CICS STARTBROWSE TIMER('WAKE') ACTIVITYID(ROOT-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE TIMER BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS INQUIRE TIMER('WAKE') ACTIVITYID(ROOT-X) END-EXEC. ",
        "EXEC CICS PUT CONTAINER('ITEM') CHANNEL('WORK') FROM(DATA-X) FLENGTH(4) END-EXEC. ",
        "EXEC CICS STARTBROWSE CONTAINER CHANNEL('WORK') BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS GETNEXT CONTAINER(ITEM-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE CONTAINER BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ACQUIRE PROCESS('ORDER') PROCESSTYPE('TYPE') END-EXEC. ",
        "EXEC CICS INQUIRE CONTAINER('MISSING') PROCESS('ORDER') PROCESSTYPE('TYPE') DATALENGTH(LEN-X) RESP(RESP-X) END-EXEC. ",
        "STOP RUN."
    );
    let artifact = published_source_fixture("BTSBR", source);
    let artifact_ref = ArtifactRef::new(
        format!("sha256:{:x}", Sha256::digest(artifact.payload())),
        InvocationLimits::default(),
    )
    .unwrap();
    let second_store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
    let second_platform: Arc<dyn PlatformStore> = second_store.clone();
    let second = ProductServer::open(settings, second_platform, secrets, default_program_router())
        .unwrap();
    second
        .install_online_application(OnlineApplicationDefinition {
            programs: vec![OnlineProgramDefinition {
                name: "BTSBR".into(),
                artifact: artifact_ref.clone(),
                payload: artifact.payload().to_vec(),
                manifest: VersionedArtifactManifest::V3(artifact.manifest().clone()),
                semantic_identity: artifact.semantic_id().to_reference(),
            }],
            transactions: BTreeMap::from([("BT01".into(), "BTSBR".into())]),
            maps: vec![BmsMapDefinition {
                mapset: "BTSBR".into(),
                map: "BTSBR".into(),
                line: 1,
                column: 1,
                rows: 24,
                columns: 80,
                fields: Vec::new(),
            }],
        })
        .unwrap();
    let invocation = second
        .cics_invocation("IBMUSER", "BT01", Some(artifact_ref))
        .unwrap();
    let session = SessionId::new("bts-browse-selected", 64).unwrap();
    second
        .cics
        .launch_background_task(invocation.clone(), &session, "BT01")
        .unwrap();
    second.cics.bind_event_activity(&invocation.run_unit_id, &root_id, None, None).unwrap();
    let browse_resource = format!("CICS.BTS.{root_id}.BROWSE");
    second.racf.define_profile("BTSEVENT", &browse_resource, "IBMUSER", None).unwrap();
    second.racf.permit("BTSEVENT", &browse_resource, "IBMUSER", AccessIntent::Read).unwrap();
    second.racf.define_profile("BTSTIMER", &browse_resource, "IBMUSER", None).unwrap();
    second.racf.permit("BTSTIMER", &browse_resource, "IBMUSER", AccessIntent::Read).unwrap();
    let event_resource = format!("CICS.BTS.{root_id}.READY");
    second.racf.define_profile("BTSEVENT", &event_resource, "IBMUSER", None).unwrap();
    second.racf.permit("BTSEVENT", &event_resource, "IBMUSER", AccessIntent::Update).unwrap();
    let timer_resource = format!("CICS.BTS.{root_id}.WAKE");
    second.racf.define_profile("BTSTIMER", &timer_resource, "IBMUSER", None).unwrap();
    second.racf.permit("BTSTIMER", &timer_resource, "IBMUSER", AccessIntent::Update).unwrap();
    second.racf.define_profile("CICSCHAN", "CICS.CHANNEL.WORK", "IBMUSER", None).unwrap();
    second.racf.permit("CICSCHAN", "CICS.CHANNEL.WORK", "IBMUSER", AccessIntent::Read).unwrap();
    second.racf.permit("CICSCHAN", "CICS.CHANNEL.WORK", "IBMUSER", AccessIntent::Update).unwrap();
    let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
    let tick = session_tick().unwrap();
    let context = second
        .cics
        .terminal_execution(&session, &principal, tick)
        .unwrap();
    second
        .begin_online_exchange(&session, "BTSBR", &context)
        .unwrap();
    second
        .run_online_exchange(&session, &principal, "BTSBR", tick)
        .unwrap();
    let cursor = second_store
        .get_provider_state("cics-bts-browse-v1", invocation.run_unit_id.as_str())
        .unwrap()
        .expect("selected browse route persisted task-owned cursor state");
    let state: serde_json::Value = serde_json::from_slice(&cursor.payload).unwrap();
    assert_eq!(state["closed"], true);
    assert_eq!(state["book"]["next_token"], 6);
    drop(second);
    drop(second_store);
    std::fs::remove_dir_all(root).unwrap();
}
