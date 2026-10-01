//! Actual online PROGRAM exit and the still-unimplemented task-completion boundary.
#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn compiled_program_exit_cold_sqlite_preserves_owner_area_and_completion_fence() {
        program_exit(None);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_compiled_program_exit_cold_preserves_owner_area_and_completion_fence() {
        program_exit(Some(required_postgres_route_url()));
    }

    fn program_exit(postgres: Option<String>) {
        let caller = published_source_fixture(
            "PERoot",
            "IDENTIFICATION DIVISION. PROGRAM-ID. PERoot. DATA DIVISION. WORKING-STORAGE SECTION. 01 CHILD-AREA PIC X(4) VALUE 'LEAF'. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(8). PROCEDURE DIVISION USING DFHCOMMAREA. EXEC CICS HANDLE ABEND PROGRAM('PEEXIT') END-EXEC. EXEC CICS LINK PROGRAM('PELEAF') COMMAREA(CHILD-AREA) LENGTH(4) END-EXEC. STOP RUN.",
        );
        let leaf = published_source_fixture(
            "PELEAF",
            "IDENTIFICATION DIVISION. PROGRAM-ID. PELEAF. DATA DIVISION. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). PROCEDURE DIVISION USING DFHCOMMAREA. EXEC CICS ABEND ABCODE('U789') NODUMP END-EXEC. GOBACK.",
        );
        let exit = published_source_fixture(
            "PEEXIT",
            "IDENTIFICATION DIVISION. PROGRAM-ID. PEEXIT. DATA DIVISION. WORKING-STORAGE SECTION. 01 SEEN-AREA PIC X(8). 01 CODE-X PIC X(4). 01 ORIG-X PIC X(4). 01 FROM-X PIC X(8). 01 HERE-X PIC X(8). 01 LEVEL-X PIC S9(4) COMP. 01 RESP-X PIC S9(9) COMP. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(8). PROCEDURE DIVISION USING DFHCOMMAREA. MOVE DFHCOMMAREA TO SEEN-AREA. EXEC CICS ASSIGN ABCODE(CODE-X) ORGABCODE(ORIG-X) ABPROGRAM(FROM-X) PROGRAM(HERE-X) LINKLEVEL(LEVEL-X) RESP(RESP-X) END-EXEC. EXEC CICS SUSPEND END-EXEC. GOBACK.",
        );
        let root = std::env::temp_dir().join(format!(
            "mainframe-program-abend-exit-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            session_tick().unwrap()
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
        let session = SessionId::new("program-abend-exit", 64).unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let retained;
        let run_rows;
        let instance_rows;
        let protocol_rows;
        {
            let (server, store) = backend.open_server(settings.clone(), secrets.clone());
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            for name in ["PELEAF", "PEEXIT"] {
                let resource = format!("CICS.PROGRAM.{name}");
                server
                    .racf
                    .define_profile("FACILITY", &resource, "IBMUSER", None)
                    .unwrap();
                server
                    .racf
                    .permit("FACILITY", &resource, "IBMUSER", AccessIntent::Execute)
                    .unwrap();
            }
            server
                .install_online_application(OnlineApplicationDefinition {
                    programs: vec![
                        OnlineProgramDefinition::current("PEROOT", &caller),
                        OnlineProgramDefinition::current("PELEAF", &leaf),
                        OnlineProgramDefinition::current("PEEXIT", &exit),
                    ],
                    transactions: BTreeMap::from([("PE01".into(), "PEROOT".into())]),
                    maps: vec![BmsMapDefinition {
                        mapset: "PEROOT".into(),
                        map: "PEROOT".into(),
                        line: 1,
                        column: 1,
                        rows: 24,
                        columns: 80,
                        fields: Vec::new(),
                    }],
                })
                .unwrap();
            server
                .cics
                .register_program_definitions(&[mainframe_env_cics::CicsProgramDefinition {
                    name: "PELEAF".into(),
                    generation: 1,
                    artifact: ArtifactRef::new(
                        leaf.content_id().to_reference(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    semantic_identity: leaf.semantic_id().to_reference(),
                    entry_offset: 0,
                    enabled: true,
                    remote: false,
                    reload: false,
                    java_status: mainframe_env_cics::CicsJavaStatus::NotJava,
                }])
                .unwrap();
            let invocation = server
                .cics_invocation(
                    "IBMUSER",
                    "PE01",
                    Some(
                        ArtifactRef::new(
                            caller.content_id().to_reference(),
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
                    "PE01",
                    24,
                    80,
                    "program-exit-csrf",
                    1,
                    10_000,
                )
                .unwrap();
            server
                .cics
                .restore_terminal_run(invocation, &session, "PE01", b"OWNER123".to_vec(), 2)
                .unwrap();
            server
                .run_online_exchange(&session, &principal, "PEROOT", 2)
                .unwrap();
            inspect_exit(&server, &session, &exit);
            retained = store.list_provider_state("cobol-call-replay@1", 8).unwrap();
            assert_eq!(retained.len(), 1);
            assert!(
                serde_json::from_slice::<Value>(&retained[0].payload).unwrap()["reply"].is_null()
            );
            let receipt: Value = serde_json::from_slice(&retained[0].payload).unwrap();
            let child_execution = mainframe_env_execution_api::ExecutionId::new(
                receipt["child_execution"].as_str().unwrap(),
                InvocationLimits::default(),
            )
            .unwrap();
            let child = store.get_execution(&child_execution).unwrap().unwrap();
            assert_eq!(child.state, mainframe_env_store_api::ExecutionState::Failed);
            run_rows = store.list_provider_state("cobol-run-state@1", 8).unwrap();
            assert_eq!(run_rows.len(), 1);
            let run: Value = serde_json::from_slice(&run_rows[0].payload).unwrap();
            assert_eq!(run["active"], 1);
            assert_eq!(run["ended"], false);
            assert_eq!(run["programs"], serde_json::json!(["PELEAF"]));
            instance_rows = store
                .list_provider_state(&format!("cobol-instance@1:{}", run_rows[0].key), 8)
                .unwrap();
            assert_eq!(instance_rows.len(), 1);
            let instance: Value = serde_json::from_slice(&instance_rows[0].payload).unwrap();
            assert_eq!(instance["busy"], true);
            assert_eq!(instance["open_files"], false);
            protocol_rows = store
                .list_provider_state("cobol-call-protocol@2", 8)
                .unwrap();
            assert_eq!(protocol_rows.len(), 1);
            let protocol: Value = serde_json::from_slice(&protocol_rows[0].payload).unwrap();
            assert_eq!(protocol["owner_execution"], run["owner_execution"]);
            let exchange = server.online_exchange(&session).unwrap().unwrap();
            assert_ne!(
                Some(exchange.execution_id.as_str()),
                run["owner_execution"].as_str()
            );
        }
        {
            let (server, store) = backend.open_server(settings, secrets);
            inspect_exit(&server, &session, &exit);
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                retained
            );
            let exchange = store
                .get_provider_state(ONLINE_EXCHANGE_NAMESPACE, session.as_str())
                .unwrap();
            let continuation = store
                .get_provider_state("online-machine-continuation", session.as_str())
                .unwrap();
            // The selected exit is executable after reopen, but normal completion is
            // not implemented: the abandoned child still owns a busy instance and
            // the replacement execution does not carry the original run owner.
            // This is a diagnostic fence regression, NOT normal-completion credit.
            assert_eq!(
                server.run_online_exchange(&session, &principal, "PEROOT", 3),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(
                store
                    .get_provider_state(ONLINE_EXCHANGE_NAMESPACE, session.as_str())
                    .unwrap(),
                exchange
            );
            assert_eq!(
                store
                    .get_provider_state("online-machine-continuation", session.as_str())
                    .unwrap(),
                continuation
            );
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                retained
            );
            assert_eq!(
                store.list_provider_state("cobol-run-state@1", 8).unwrap(),
                run_rows
            );
            assert_eq!(
                store
                    .list_provider_state(&format!("cobol-instance@1:{}", run_rows[0].key), 8)
                    .unwrap(),
                instance_rows
            );
            assert_eq!(
                store
                    .list_provider_state("cobol-call-protocol@2", 8)
                    .unwrap(),
                protocol_rows
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    fn inspect_exit(
        server: &ProductServer,
        session: &SessionId,
        exit: &mainframe_env_compiler_api::PublishedArtifact,
    ) {
        let saved = server
            .online_machine_continuation(session)
            .unwrap()
            .unwrap();
        assert_eq!(saved.program, "PEEXIT");
        assert_eq!(saved.artifact.as_str(), exit.content_id().to_reference());
        let exchange = server.online_exchange(session).unwrap().unwrap();
        let mut machine = ReferenceMachine::from_binary(
            exit.payload(),
            server.online_exchange_invocation(&exchange).unwrap(),
            CodecLimits::default(),
        )
        .unwrap();
        machine.restore_checkpoint(&saved.checkpoint).unwrap();
        for (name, expected) in [
            ("SEEN-AREA", b"OWNER123".as_slice()),
            ("CODE-X", b"U789"),
            ("ORIG-X", b"U789"),
            ("FROM-X", b"PELEAF  "),
            ("HERE-X", b"PEEXIT  "),
        ] {
            assert_eq!(machine.variable(name).unwrap().bytes(), expected, "{name}");
        }
        assert_eq!(
            machine.variable("LEVEL-X").unwrap().bytes(),
            1_i16.to_be_bytes()
        );
        assert_eq!(machine.variable("RESP-X").unwrap().bytes(), [0; 4]);
    }
}
