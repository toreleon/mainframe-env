#[test]
fn compiled_online_source_resolved_bts_browse_recovers_after_sqlite_restart() {
    compiled_bts_browse_recovers_after_restart(None, None);
}

#[test]
fn compiled_getnext_eventtype_writes_cvda_and_advances_once_after_sqlite_restart() {
    compiled_bts_browse_recovers_after_restart(None, Some(("EVENTTYPE", 1004)));
}

#[test]
fn compiled_getnext_firestatus_writes_cvda_and_advances_once_after_sqlite_restart() {
    compiled_bts_browse_recovers_after_restart(None, Some(("FIRESTATUS", 1000)));
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_compiled_bts_browse_cursor_recovers_after_restart() {
    compiled_bts_browse_recovers_after_restart(Some(required_postgres_route_url()), None);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_compiled_getnext_eventtype_writes_cvda_and_advances_once_after_restart() {
    compiled_bts_browse_recovers_after_restart(
        Some(required_postgres_route_url()),
        Some(("EVENTTYPE", 1004)),
    );
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_compiled_getnext_firestatus_writes_cvda_and_advances_once_after_restart() {
    compiled_bts_browse_recovers_after_restart(
        Some(required_postgres_route_url()),
        Some(("FIRESTATUS", 1000)),
    );
}

fn compiled_bts_browse_recovers_after_restart(
    postgres_url: Option<String>,
    getnext_metadata: Option<(&str, i32)>,
) {
    use mainframe_env_cics::bts_lifecycle::{BtsProcessTypeDefinition, BtsTransactionDefinition};
    use mainframe_env_cics::bts_browse::{BrowseEffect, BrowseEventMetadata, BrowseItem, BrowseKind, BrowseOutcome, BrowseOwner, BrowseScope, BtsBrowseStore};

    let root = std::env::temp_dir().join(format!(
        "mainframe-env-bts-browse-restart-{}-{:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        session_tick().unwrap()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let backend = match postgres_url {
        Some(url) => RouteRestartBackend::Postgres(url),
        None => RouteRestartBackend::Sqlite(format!(
            "sqlite://{}?mode=rwc",
            root.join("state.db").display()
        )),
    };
    let settings = backend.settings(root.join("artifacts"));
    let secrets = Arc::new(MemorySecretResolver::default());
    let (first, first_store) = backend.open_server(settings.clone(), secrets.clone());
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
    let replay_owner = BrowseOwner::new("METADATA-RUN", "METADATA-EXEC", "IBMUSER").unwrap();
    let replay_item = BrowseItem::new("BELL", None, 0)
        .unwrap()
        .with_epoch(1)
        .unwrap()
        .with_event_metadata(BrowseEventMetadata {
            event_type: 1004,
            fire_status: 1000,
            composite: None,
            predicate: None,
            timer: Some("WAKE".into()),
        });
    let next_item = BrowseItem::new("CHIME", None, 0)
        .unwrap()
        .with_epoch(1)
        .unwrap()
        .with_event_metadata(BrowseEventMetadata {
            event_type: 1004,
            fire_status: 1000,
            composite: None,
            predicate: None,
            timer: Some("LATER".into()),
        });
    let replay_store = BtsBrowseStore::new(first_store.as_ref());
    let BrowseOutcome::Token(replay_token) = replay_store.apply(&replay_owner, "START", [1; 32], &BrowseEffect::Start {
        scope: BrowseScope::new(BrowseKind::Event, "BTSEVENT", "CICS.BTS.BROWSE", 1).unwrap(),
        items: vec![replay_item.clone(), next_item.clone()],
    }).unwrap() else { panic!("expected browse token") };
    let first_metadata = replay_store.apply(&replay_owner, "NEXT", [2; 32], &BrowseEffect::Next {
        token: replay_token,
        kind: BrowseKind::Event,
        live_epoch: 1,
        expected: replay_item,
    }).unwrap();
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
        "01 CVDA-X PIC S9(9) COMP. 01 ABS-X PIC S9(15) COMP-3. ",
        "01 GET-CVDA PIC S9(9) COMP. 01 GET-RESP PIC S9(9) COMP. ",
        "01 NEXT-EVENT PIC X(16). 01 NEXT-RESP PIC S9(9) COMP. ",
        "PROCEDURE DIVISION. ",
        "EXEC CICS STARTBROWSE PROCESS PROCESSTYPE('TYPE') BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS GETNEXT PROCESS(PROCESS-X) BROWSETOKEN(TOKEN-X) ACTIVITYID(ROOT-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE PROCESS BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS INQUIRE PROCESS('ORDER') PROCESSTYPE('TYPE') ACTIVITYID(ROOT-X) END-EXEC. ",
        "EXEC CICS STARTBROWSE ACTIVITY PROCESS('ORDER') PROCESSTYPE('TYPE') BROWSETOKEN(ACT-TOKEN) END-EXEC. ",
        "EXEC CICS GETNEXT ACTIVITY(ACT-NAME) BROWSETOKEN(ACT-TOKEN) ACTIVITYID(ACT-ID) END-EXEC. ",
        "EXEC CICS ENDBROWSE ACTIVITY BROWSETOKEN(ACT-TOKEN) END-EXEC. ",
        "EXEC CICS INQUIRE ACTIVITYID(ROOT-X) ACTIVITY(ACT-NAME) END-EXEC. ",
        "EXEC CICS INQUIRE ACTIVITYID(ROOT-X) COMPSTATUS(CVDA-X) END-EXEC. ",
        "EXEC CICS INQUIRE ACTIVITYID(ROOT-X) MODE(CVDA-X) END-EXEC. ",
        "EXEC CICS INQUIRE ACTIVITYID(ROOT-X) SUSPSTATUS(CVDA-X) END-EXEC. ",
        "EXEC CICS DEFINE INPUT EVENT('READY') END-EXEC. ",
        "EXEC CICS DEFINE TIMER('WAKE') EVENT('BELL') AFTER SECONDS(5) END-EXEC. ",
        "EXEC CICS STARTBROWSE EVENT ACTIVITYID(ROOT-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS GETNEXT EVENT(EVENT-X) BROWSETOKEN(TOKEN-X) TIMER(ITEM-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE EVENT BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS INQUIRE EVENT('READY') ACTIVITYID(ROOT-X) EVENTTYPE(CVDA-X) FIRESTATUS(CVDA-X) COMPOSITE(EVENT-X) TIMER(ITEM-X) END-EXEC. ",
        "EXEC CICS STARTBROWSE TIMER('WAKE') ACTIVITYID(ROOT-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE TIMER BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS INQUIRE TIMER('WAKE') ACTIVITYID(ROOT-X) EVENT(EVENT-X) STATUS(CVDA-X) ABSTIME(ABS-X) END-EXEC. ",
        "EXEC CICS PUT CONTAINER('ITEM') CHANNEL('WORK') FROM(DATA-X) FLENGTH(4) END-EXEC. ",
        "EXEC CICS STARTBROWSE CONTAINER CHANNEL('WORK') BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS GETNEXT CONTAINER(ITEM-X) BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ENDBROWSE CONTAINER BROWSETOKEN(TOKEN-X) END-EXEC. ",
        "EXEC CICS ACQUIRE PROCESS('ORDER') PROCESSTYPE('TYPE') END-EXEC. ",
        "EXEC CICS INQUIRE CONTAINER('MISSING') PROCESS('ORDER') PROCESSTYPE('TYPE') DATALENGTH(LEN-X) RESP(RESP-X) END-EXEC. ",
        "STOP RUN."
    );
    let source = if let Some((field, _)) = getnext_metadata {
        source
            .replace(
                "EXEC CICS GETNEXT EVENT(EVENT-X) BROWSETOKEN(TOKEN-X) TIMER(ITEM-X) END-EXEC. ",
                &format!(
                    "EXEC CICS GETNEXT EVENT(EVENT-X) BROWSETOKEN(TOKEN-X) {field}(GET-CVDA) RESP(GET-RESP) END-EXEC. \
                     EXEC CICS GETNEXT EVENT(NEXT-EVENT) BROWSETOKEN(TOKEN-X) RESP(NEXT-RESP) END-EXEC. "
                ),
            )
            .replace("STOP RUN.", "EXEC CICS SUSPEND END-EXEC. STOP RUN.")
    } else {
        source.to_owned()
    };
    let artifact = published_source_fixture("BTSBR", &source);
    let artifact_ref = ArtifactRef::new(
        format!("sha256:{:x}", Sha256::digest(artifact.payload())),
        InvocationLimits::default(),
    )
    .unwrap();
    let (second, second_store) = backend.open_server(settings, secrets);
    assert_eq!(
        BtsBrowseStore::new(second_store.as_ref())
            .replay(&replay_owner, "NEXT", [2; 32])
            .unwrap()
            .unwrap()
            .0,
        first_metadata
    );
    let next_metadata = BtsBrowseStore::new(second_store.as_ref())
        .apply(
            &replay_owner,
            "NEXT-AGAIN",
            [3; 32],
            &BrowseEffect::Next {
                token: replay_token,
                kind: BrowseKind::Event,
                live_epoch: 1,
                expected: next_item.clone(),
            },
        )
        .unwrap();
    assert_eq!(next_metadata, BrowseOutcome::Item(next_item));
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
    if let Some((_, expected_cvda)) = getnext_metadata {
        let continuation = second.online_machine_continuation(&session).unwrap().unwrap();
        let mut restored = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        restored.restore_checkpoint(&continuation.checkpoint).unwrap();
        assert_eq!(restored.variable("EVENT-X").unwrap().bytes(), b"BELL            ");
        assert_eq!(restored.variable("GET-CVDA").unwrap().bytes(), &expected_cvda.to_be_bytes());
        assert_eq!(restored.variable("GET-RESP").unwrap().bytes(), &[0; 4]);
        assert_eq!(restored.variable("NEXT-EVENT").unwrap().bytes(), b"READY           ");
        assert_eq!(restored.variable("NEXT-RESP").unwrap().bytes(), &[0; 4]);
        second
            .run_online_exchange(&session, &principal, "BTSBR", tick)
            .unwrap();
    }
    let cursor = second_store
        .get_provider_state("cics-bts-browse-v1", invocation.run_unit_id.as_str())
        .unwrap()
        .expect("selected browse route persisted task-owned cursor state");
    let state: serde_json::Value = serde_json::from_slice(&cursor.payload).unwrap();
    assert_eq!(state["closed"], true);
    assert_eq!(state["book"]["next_token"], 6);
    drop(second);
    drop(second_store);
    let reopened = backend.open_store();
    assert_eq!(
        reopened
            .get_provider_state("cics-bts-browse-v1", invocation.run_unit_id.as_str())
            .unwrap(),
        Some(cursor)
    );
    assert_eq!(
        BtsBrowseStore::new(reopened.as_ref())
            .replay(&replay_owner, "NEXT", [2; 32])
            .unwrap()
            .unwrap()
            .0,
        first_metadata
    );
    assert_eq!(
        BtsBrowseStore::new(reopened.as_ref())
            .replay(&replay_owner, "NEXT-AGAIN", [3; 32])
            .unwrap()
            .unwrap()
            .0,
        next_metadata
    );
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}
