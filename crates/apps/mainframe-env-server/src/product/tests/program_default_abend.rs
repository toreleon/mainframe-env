//! Source-derived POP HANDLE default abend through selected nested programs.
#[cfg(test)]
mod tests {
    use super::super::*;
    use mainframe_env_execution_api::{MachineDrive, MachineResume, Quantum};

    #[derive(Clone, Copy)]
    enum Recovery {
        Middle,
        Root,
        Respond,
    }

    #[test]
    fn compiled_default_pop_abend_selects_nearest_label_after_sqlite_reopen() {
        default_pop(None, Recovery::Middle);
        default_pop(None, Recovery::Root);
    }

    #[test]
    fn compiled_child_pop_resp_prevents_default_abend_after_sqlite_reopen() {
        default_pop(None, Recovery::Respond);
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_default_pop_abend_selects_middle_label_after_reopen() {
        default_pop(Some(required_postgres_route_url()), Recovery::Middle);
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_default_pop_abend_selects_root_label_after_reopen() {
        default_pop(Some(required_postgres_route_url()), Recovery::Root);
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_child_pop_resp_prevents_default_abend_after_reopen() {
        default_pop(Some(required_postgres_route_url()), Recovery::Respond);
    }

    fn default_pop(postgres: Option<String>, recovery: Recovery) {
        let parent = published_source_fixture(
            "DPROOT",
            "IDENTIFICATION DIVISION. PROGRAM-ID. DPROOT. DATA DIVISION. \
             WORKING-STORAGE SECTION. 01 AREA-X PIC X(4) VALUE 'NONE'. \
             01 PATH-X PIC X(6) VALUE 'NONE'. 01 RESP-X PIC S9(9) COMP. \
             PROCEDURE DIVISION. EXEC CICS HANDLE ABEND LABEL(ROOT-EXIT) END-EXEC. \
             EXEC CICS LINK PROGRAM('DPMID') COMMAREA(AREA-X) LENGTH(4) \
             RESP(RESP-X) END-EXEC. MOVE 'RETURN' TO PATH-X. \
             EXEC CICS SUSPEND END-EXEC. GOBACK. \
             ROOT-EXIT. MOVE 'ROOTEX' TO PATH-X. EXEC CICS SUSPEND END-EXEC. GOBACK.",
        );
        let handle = if matches!(recovery, Recovery::Middle) {
            "EXEC CICS HANDLE ABEND LABEL(MIDDLE-EXIT) END-EXEC."
        } else {
            ""
        };
        let middle = published_source_fixture(
            "DPMID",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. DPMID. DATA DIVISION. \
                 WORKING-STORAGE SECTION. 01 LEAF-AREA PIC X(4) VALUE 'LEAF'. \
                 01 RESP-X PIC S9(9) COMP. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). \
                 PROCEDURE DIVISION USING DFHCOMMAREA. {handle} \
                 EXEC CICS HANDLE CONDITION INVREQ(WRONG-EXIT) END-EXEC. \
                 EXEC CICS LINK PROGRAM('DPLEAF') COMMAREA(LEAF-AREA) LENGTH(4) \
                 RESP(RESP-X) END-EXEC. MOVE 'NORM' TO DFHCOMMAREA. GOBACK. \
                 MIDDLE-EXIT. MOVE 'GOOD' TO DFHCOMMAREA. GOBACK. \
                 WRONG-EXIT. MOVE 'BAD!' TO DFHCOMMAREA. GOBACK."
            ),
        );
        let respond = if matches!(recovery, Recovery::Respond) {
            "RESP(RESP-X)"
        } else {
            ""
        };
        let leaf = published_source_fixture(
            "DPLEAF",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. DPLEAF. DATA DIVISION. \
                 WORKING-STORAGE SECTION. 01 RESP-X PIC S9(9) COMP. \
                 LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). \
                 PROCEDURE DIVISION USING DFHCOMMAREA. \
                 EXEC CICS POP HANDLE {respond} END-EXEC. GOBACK."
            ),
        );
        let root = std::env::temp_dir().join(format!(
            "mainframe-default-pop-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let backend = match postgres {
            Some(url) => RouteRestartBackend::Postgres(url),
            None => RouteRestartBackend::Sqlite(format!(
                "sqlite://{}?mode=rwc",
                root.join("state.db").display()
            )),
        };
        let settings = backend.settings(root.join("artifacts"));
        let secrets = Arc::new(MemorySecretResolver::default());
        let session = SessionId::new("default-pop", 64).unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let invocation;
        let retained;
        let checkpoint;
        let checkpoint_payload;
        let link_effect;
        let expected_area = match recovery {
            Recovery::Middle => b"GOOD",
            Recovery::Root => b"NONE",
            Recovery::Respond => b"NORM",
        };
        let expected_path = if matches!(recovery, Recovery::Root) {
            b"ROOTEX"
        } else {
            b"RETURN"
        };
        {
            let (server, store) = backend.open_server(settings.clone(), secrets.clone());
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            for name in ["DPMID", "DPLEAF"] {
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
                        OnlineProgramDefinition::current("DPROOT", &parent),
                        OnlineProgramDefinition::current("DPMID", &middle),
                        OnlineProgramDefinition::current("DPLEAF", &leaf),
                    ],
                    transactions: BTreeMap::from([("DP01".into(), "DPROOT".into())]),
                    maps: vec![BmsMapDefinition {
                        mapset: "DPROOT".into(),
                        map: "DPROOT".into(),
                        line: 1,
                        column: 1,
                        rows: 24,
                        columns: 80,
                        fields: Vec::new(),
                    }],
                })
                .unwrap();
            let definitions = [("DPMID", &middle), ("DPLEAF", &leaf)].map(|(name, artifact)| {
                mainframe_env_cics::CicsProgramDefinition {
                    name: name.into(),
                    generation: 1,
                    artifact: ArtifactRef::new(
                        artifact.content_id().to_reference(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    semantic_identity: artifact.semantic_id().to_reference(),
                    entry_offset: 0,
                    enabled: true,
                    remote: false,
                    reload: false,
                    java_status: mainframe_env_cics::CicsJavaStatus::NotJava,
                }
            });
            server
                .cics
                .register_program_definitions(&definitions)
                .unwrap();
            let mut launch = server
                .cics_invocation(
                    "IBMUSER",
                    "DP01",
                    Some(
                        ArtifactRef::new(
                            parent.content_id().to_reference(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                )
                .unwrap();
            launch.selector = Selector::new("program:DPROOT", InvocationLimits::default()).unwrap();
            for (name, schema, bytes) in [
                (
                    "cics.session",
                    "mainframe-env.cics.session@1",
                    session.as_str().as_bytes(),
                ),
                (
                    "cics.transaction",
                    "mainframe-env.cics.transaction@1",
                    b"DP01".as_slice(),
                ),
            ] {
                launch.bindings.insert(
                    name.into(),
                    BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default())
                        .unwrap(),
                );
            }
            bind_compatible_runtime_services(&mut launch).unwrap();
            invocation = launch;
            server
                .cics
                .launch_terminal(
                    invocation.clone(),
                    &session,
                    "DP01",
                    24,
                    80,
                    "default-pop-csrf",
                    1,
                    10_000,
                )
                .unwrap();
            let mut machine = ReferenceMachine::from_binary(
                parent.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .unwrap();
            let coordinator = ExecutionCoordinator::durable(
                server.host.clone(),
                store.clone(),
                CoordinatorLimits::default(),
            );
            let outcome =
                coordinator.execute_resumable_with_control(&mut machine, &invocation, || {
                    server.program.observe_execution_control(&invocation)
                });
            assert!(
                matches!(outcome, ExecutionOutcome::Suspended(_)),
                "{outcome:?}"
            );
            assert_eq!(machine.variable("AREA-X").unwrap().bytes(), expected_area);
            assert_eq!(machine.variable("PATH-X").unwrap().bytes(), expected_path);
            checkpoint = store
                .get_checkpoint(&invocation.execution_id)
                .unwrap()
                .unwrap();
            checkpoint_payload = machine.checkpoint().unwrap();
            assert_eq!(checkpoint.payload, checkpoint_payload.bytes());
            let mut probe = ReferenceMachine::from_binary(
                parent.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .unwrap();
            let quantum = Quantum::new(10_000, 16 * 1024 * 1024).unwrap();
            let MachineDrive::HostCall(handle) = probe.drive(MachineResume::Start, quantum) else {
                panic!("expected HANDLE ABEND")
            };
            let HostRequest::Cics(request) = &handle.request else {
                panic!("expected CICS")
            };
            let response = server.cics.invoke(&handle, request.clone()).unwrap();
            let MachineDrive::HostCall(link) = probe.drive(
                MachineResume::HostResult(mainframe_env_host_api::EffectResult {
                    sequence: handle.sequence,
                    outcome: Ok(HostResult::Cics(response)),
                }),
                quantum,
            ) else {
                panic!("expected LINK")
            };
            link_effect = link;
            retained = (
                store
                    .effect(link_effect.idempotency_key.as_ref().unwrap())
                    .unwrap(),
                store
                    .list_provider_state("cics-effect-replay-v1", 32)
                    .unwrap(),
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
            );
            assert_eq!(retained.0.as_ref().unwrap().state, EffectState::Completed);
            assert_eq!(retained.2.len(), 2);
            let pending = retained
                .2
                .iter()
                .filter(|row| {
                    serde_json::from_slice::<Value>(&row.payload).unwrap()["reply"].is_null()
                })
                .count();
            assert_eq!(
                pending,
                match recovery {
                    Recovery::Middle => 1,
                    Recovery::Root => 2,
                    Recovery::Respond => 0,
                }
            );
        }
        {
            let (server, store) = backend.open_server(settings, secrets);
            server
                .cics
                .restore_terminal_run(invocation.clone(), &session, "DP01", Vec::new(), 3)
                .unwrap();
            let HostRequest::Cics(request) = &link_effect.request else {
                panic!("expected CICS")
            };
            let response = server.cics.invoke(&link_effect, request.clone()).unwrap();
            if matches!(recovery, Recovery::Root) {
                assert_eq!(
                    response.disposition,
                    mainframe_env_host_api::CicsDisposition::Handler
                );
                assert_eq!(response.target.as_deref(), Some("ROOT-EXIT"));
                assert_eq!(response.condition, "INVREQ");
                assert_eq!(response.response, 16);
                assert_eq!(response.outputs["ABEND.DEFAULT"].bytes(), b"POP-HANDLE");
                assert!(response.outputs["ABEND.CODE"].bytes().is_empty());
                assert!(!response.outputs.contains_key("ABEND.DUMP"));
            } else {
                assert_eq!(
                    response.disposition,
                    mainframe_env_host_api::CicsDisposition::Complete
                );
                assert_eq!(response.outputs["COMMAREA"].bytes(), expected_area);
            }
            assert_eq!(
                store
                    .effect(link_effect.idempotency_key.as_ref().unwrap())
                    .unwrap(),
                retained.0
            );
            assert_eq!(
                store
                    .list_provider_state("cics-effect-replay-v1", 32)
                    .unwrap(),
                retained.1
            );
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                retained.2
            );
            assert_eq!(
                store
                    .get_checkpoint(&invocation.execution_id)
                    .unwrap()
                    .unwrap(),
                checkpoint
            );
            let mut machine = ReferenceMachine::from_binary(
                parent.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .unwrap();
            machine.restore_checkpoint(&checkpoint_payload).unwrap();
            assert_eq!(machine.variable("AREA-X").unwrap().bytes(), expected_area);
            assert_eq!(machine.variable("PATH-X").unwrap().bytes(), expected_path);
            let coordinator = ExecutionCoordinator::durable(
                server.host.clone(),
                store.clone(),
                CoordinatorLimits::default(),
            );
            assert!(matches!(
                coordinator.execute_resumable_with_control(&mut machine, &invocation, || server
                    .program
                    .observe_execution_control(&invocation)),
                ExecutionOutcome::Completed(_)
            ));
            server.program.finish_run_unit(&invocation).unwrap();
            server
                .cics
                .complete_terminal_run(&session, &principal, 4)
                .unwrap();
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                retained.2
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
