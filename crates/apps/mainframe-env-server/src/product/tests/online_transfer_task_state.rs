use super::*;
use mainframe_env_dataset::{
    DATASET_REPLAY_NAMESPACE, DatasetReplayResultState, describe_dataset_replay_row,
};

#[derive(Clone, Copy, Debug)]
enum Scenario {
    UpdateHold,
    Browse,
    MutationKeys,
}

#[test]
fn online_xctl_keeps_read_update_hold_for_target_rewrite() {
    transfer(Scenario::UpdateHold, None);
}

#[test]
fn online_xctl_keeps_started_browse_for_target_readnext() {
    transfer(Scenario::Browse, None);
}

#[test]
fn online_xctl_keeps_nested_mutation_keys_unique() {
    transfer(Scenario::MutationKeys, None);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_online_xctl_keeps_read_update_hold_for_target_rewrite() {
    transfer(Scenario::UpdateHold, Some(required_postgres_route_url()));
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_online_xctl_keeps_started_browse_for_target_readnext() {
    transfer(Scenario::Browse, Some(required_postgres_route_url()));
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_online_xctl_keeps_nested_mutation_keys_unique() {
    transfer(Scenario::MutationKeys, Some(required_postgres_route_url()));
}

fn transfer(scenario: Scenario, postgres: Option<String>) {
    let (before, after) = match scenario {
        Scenario::UpdateHold => (
            "EXEC CICS READ FILE('XCFILE') INTO(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH-X) UPDATE END-EXEC.",
            "EXEC CICS REWRITE FILE('XCFILE') FROM(RECORD-X) LENGTH(7) RESP(RESP-X) END-EXEC.",
        ),
        Scenario::Browse => (
            "EXEC CICS STARTBR FILE('XCFILE') RIDFLD(KEY-X) END-EXEC.",
            "EXEC CICS READNEXT FILE('XCFILE') INTO(RECORD-X) RIDFLD(KEY-X) LENGTH(LENGTH-X) RESP(RESP-X) END-EXEC.",
        ),
        Scenario::MutationKeys => (
            "EXEC CICS WRITE FILE('XCFILE') FROM(RECORD-X) LENGTH(7) RIDFLD(KEY-X) END-EXEC.",
            "EXEC CICS WRITE FILE('XCFILE') FROM(RECORD-X) LENGTH(7) RIDFLD(KEY-X) RESP(RESP-X) END-EXEC.",
        ),
    };
    let main = published_source_fixture(
        "XCMAIN",
        &format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. XCMAIN. DATA DIVISION. \
             WORKING-STORAGE SECTION. 01 RECORD-X PIC X(7) VALUE '003DATA'. \
             01 KEY-X PIC X(3) VALUE '003'. 01 LENGTH-X PIC S9(4) COMP VALUE 7. PROCEDURE DIVISION. {before} \
             EXEC CICS XCTL PROGRAM('XCNEXT') END-EXEC. STOP RUN."
        ),
    );
    let target_record = if matches!(scenario, Scenario::MutationKeys) {
        "004EDIT"
    } else {
        "003EDIT"
    };
    let target_key = &target_record[..3];
    let next = published_source_fixture(
        "XCNEXT",
        &format!(
            "IDENTIFICATION DIVISION. PROGRAM-ID. XCNEXT. DATA DIVISION. \
             WORKING-STORAGE SECTION. 01 RECORD-X PIC X(7) VALUE '{target_record}'. \
             01 KEY-X PIC X(3) VALUE '{target_key}'. 01 RESP-X PIC S9(9) COMP. 01 LENGTH-X PIC S9(4) COMP VALUE 7. \
             PROCEDURE DIVISION. {after} EXEC CICS SUSPEND END-EXEC. STOP RUN."
        ),
    );
    let root = std::env::temp_dir().join(format!(
        "mainframe-xctl-task-state-{}-{:?}-{scenario:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let backend = postgres
        .map(RouteRestartBackend::Postgres)
        .unwrap_or_else(|| {
            RouteRestartBackend::Sqlite(format!(
                "sqlite://{}?mode=rwc",
                root.join("state.db").display()
            ))
        });
    let settings = backend.settings(root.join("artifacts"));
    let secrets = Arc::new(MemorySecretResolver::default());
    let (server, store) = backend.open_server(settings.clone(), secrets.clone());
    server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
    let authentication = || Authentication::Basic {
        user: "IBMUSER".into(),
        secret: b"TESTPASS".to_vec(),
    };
    server.handle(authentication(), GatewayRequest::DatasetCreate {
        dataset: "IBMUSER.XCDATA".into(),
        attributes: json!({"dsorg":"KSDS", "recfm":"V", "lrecl":16, "key_offset":0, "key_length":3}),
    }).unwrap();
    if !matches!(scenario, Scenario::MutationKeys) {
        server
            .handle(
                authentication(),
                GatewayRequest::DatasetWrite {
                    dataset: "IBMUSER.XCDATA".into(),
                    member: None,
                    bytes: b"003DATA".to_vec(),
                },
            )
            .unwrap();
    }
    server
        .cics
        .register_file_aliases(&BTreeMap::from([(
            "XCFILE".into(),
            DatasetName::new("IBMUSER.XCDATA", 128).unwrap(),
        )]))
        .unwrap();
    server
        .racf
        .define_profile("FACILITY", "CICS.PROGRAM.XCNEXT", "IBMUSER", None)
        .unwrap();
    server
        .racf
        .permit(
            "FACILITY",
            "CICS.PROGRAM.XCNEXT",
            "IBMUSER",
            AccessIntent::Execute,
        )
        .unwrap();
    server
        .install_online_application(OnlineApplicationDefinition {
            programs: vec![
                OnlineProgramDefinition::current("XCMAIN", &main),
                OnlineProgramDefinition::current("XCNEXT", &next),
            ],
            transactions: BTreeMap::from([("XC01".into(), "XCMAIN".into())]),
            maps: vec![BmsMapDefinition {
                mapset: "XCMAIN".into(),
                map: "XCMAIN".into(),
                line: 1,
                column: 1,
                rows: 24,
                columns: 80,
                fields: Vec::new(),
            }],
        })
        .unwrap();
    let session = SessionId::new("xctl-task-state", 64).unwrap();
    let invocation = server
        .cics_invocation(
            "IBMUSER",
            "XC01",
            Some(
                ArtifactRef::new(
                    main.content_id().to_reference(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    server
        .cics
        .launch_terminal(
            invocation.clone(),
            &session,
            "XC01",
            24,
            80,
            "xctl-csrf",
            1,
            10_000,
        )
        .unwrap();
    let principal = invocation.principal.id();
    let context = server
        .cics
        .terminal_execution(&session, principal, 2)
        .unwrap();
    server
        .begin_online_exchange(&session, "XCMAIN", &context)
        .unwrap();
    server
        .run_online_exchange(&session, principal, "XCMAIN", 2)
        .expect("same-task XCTL target must execute without nested key conflict");
    let saved = server
        .online_machine_continuation(&session)
        .unwrap()
        .unwrap();
    assert_eq!(saved.program, "XCNEXT");
    let mut machine =
        ReferenceMachine::from_binary(next.payload(), invocation.clone(), CodecLimits::default())
            .unwrap();
    machine.restore_checkpoint(&saved.checkpoint).unwrap();
    assert_eq!(
        machine.variable("RESP-X").unwrap().bytes(),
        0_i32.to_be_bytes(),
        "{scenario:?} must survive same-task XCTL"
    );
    if matches!(scenario, Scenario::Browse) {
        assert_eq!(machine.variable("RECORD-X").unwrap().bytes(), b"003DATA");
    }
    let nested_prefix = format!("cics:{}:", invocation.run_unit_id);
    let nested = store
        .list_provider_state(DATASET_REPLAY_NAMESPACE, 256)
        .unwrap()
        .into_iter()
        .filter(|row| row.key.starts_with(&nested_prefix))
        .collect::<Vec<_>>();
    if matches!(scenario, Scenario::MutationKeys) {
        assert_eq!(
            nested.len(),
            2,
            "both programs must own distinct nested mutations"
        );
        assert_ne!(nested[0].key, nested[1].key);
        let descriptors = nested
            .iter()
            .map(|row| describe_dataset_replay_row(row).unwrap())
            .collect::<Vec<_>>();
        assert_ne!(descriptors[0].sequence, descriptors[1].sequence);
        assert_ne!(
            descriptors[0].owner_execution,
            descriptors[1].owner_execution
        );
        for descriptor in descriptors {
            assert_eq!(descriptor.result_state, DatasetReplayResultState::Resolved);
            assert_eq!(
                descriptor.owner_run_unit.as_deref(),
                Some(invocation.run_unit_id.as_str())
            );
        }
    }
    drop(server);
    drop(store);
    // The completed transfer's checkpoint and dataset effects remain readable after reopen.
    let (server, store) = backend.open_server(settings, secrets);
    for row in nested {
        assert_eq!(
            store
                .get_provider_state(DATASET_REPLAY_NAMESPACE, &row.key)
                .unwrap(),
            Some(row)
        );
    }
    assert_eq!(
        server
            .online_machine_continuation(&session)
            .unwrap()
            .unwrap()
            .checkpoint,
        saved.checkpoint
    );
    let DatasetResult::Records { records, .. } = server
        .dataset
        .invoke(DatasetRequest::Read {
            dataset: DatasetName::new("IBMUSER.XCDATA", 128).unwrap(),
            member: None,
            key: Some(target_key.as_bytes().to_vec()),
            max_records: 1,
            control: Default::default(),
        })
        .unwrap()
    else {
        panic!("expected target record");
    };
    assert_eq!(
        records,
        [if matches!(scenario, Scenario::Browse) {
            b"003DATA".to_vec()
        } else {
            target_record.as_bytes().to_vec()
        }]
    );
    drop(server);
    std::fs::remove_dir_all(root).unwrap();
}
