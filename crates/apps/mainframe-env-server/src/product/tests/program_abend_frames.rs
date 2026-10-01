//! Selected compiled explicit-ABEND propagation through logical program frames.
#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn compiled_child_abend_reaches_caller_label_after_sqlite_restart() {
        ancestor_label(None, false, false);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_compiled_child_abend_reaches_caller_label_after_restart() {
        ancestor_label(Some(required_postgres_route_url()), false, false);
    }

    #[test]
    fn compiled_child_abend_cancel_bypasses_caller_after_sqlite_restart() {
        ancestor_label(None, true, false);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_compiled_child_abend_cancel_bypasses_caller_after_restart() {
        ancestor_label(Some(required_postgres_route_url()), true, false);
    }

    #[test]
    fn compiled_child_local_abend_exit_preserves_task_metadata_after_sqlite_restart() {
        ancestor_label(None, false, true);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_compiled_child_local_abend_exit_preserves_task_metadata_after_restart() {
        ancestor_label(Some(required_postgres_route_url()), false, true);
    }

    fn ancestor_label(postgres_url: Option<String>, cancel: bool, local_exit: bool) {
        use mainframe_env_execution_api::{MachineDrive, MachineResume, Quantum};
        let parent = published_source_fixture(
            "ABMAIN",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. ABMAIN. DATA DIVISION. \
         WORKING-STORAGE SECTION. 01 AREA-X PIC X(8) VALUE 'ROOTDATA'. \
         01 PATH-X PIC X(4) VALUE 'NONE'. 01 CODE-X PIC X(4). 01 ORIG-X PIC X(4). \
         01 RESP-X PIC S9(9) COMP. 01 LEVEL-X PIC S9(4) COMP. \
         PROCEDURE DIVISION. EXEC CICS HANDLE ABEND LABEL(ROOT-EXIT) END-EXEC. \
         EXEC CICS LINK PROGRAM('ABCHILD') COMMAREA(AREA-X) LENGTH(4) \
         RESP(RESP-X) END-EXEC. {} \
         ROOT-EXIT. MOVE 'GOOD' TO PATH-X. \
         CHECK-EXIT. \
         EXEC CICS ASSIGN ABCODE(CODE-X) ORGABCODE(ORIG-X) LINKLEVEL(LEVEL-X) \
         RESP(RESP-X) END-EXEC. EXEC CICS SUSPEND END-EXEC. STOP RUN.",
                if local_exit {
                    "MOVE 'CHLD' TO PATH-X. GO TO CHECK-EXIT."
                } else {
                    "MOVE 'BAD!' TO PATH-X. STOP RUN."
                }
            ),
        );
        let child = published_source_fixture(
            "ABCHILD",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. ABCHILD. DATA DIVISION. \
         LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). \
         PROCEDURE DIVISION USING DFHCOMMAREA. {} \
         EXEC CICS ABEND ABCODE('U777') NODUMP {} END-EXEC. GOBACK. CHILD-EXIT. GOBACK.",
                if local_exit {
                    "EXEC CICS HANDLE ABEND LABEL(CHILD-EXIT) END-EXEC."
                } else {
                    ""
                },
                if cancel { "CANCEL" } else { "" }
            ),
        );
        let root = std::env::temp_dir().join(format!(
            "mainframe-ancestor-abend-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
        let session = SessionId::new("ancestor-abend-selected", 64).unwrap();
        let reference = ArtifactRef::new(
            parent.content_id().to_reference(),
            InvocationLimits::default(),
        )
        .unwrap();
        let invocation;
        let link_effect;
        let retained;
        {
            let (server, store) = backend.open_server(settings.clone(), secrets.clone());
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            server
                .racf
                .define_profile("FACILITY", "CICS.PROGRAM.ABCHILD", "IBMUSER", None)
                .unwrap();
            server
                .racf
                .permit(
                    "FACILITY",
                    "CICS.PROGRAM.ABCHILD",
                    "IBMUSER",
                    AccessIntent::Execute,
                )
                .unwrap();
            server
                .install_online_application(OnlineApplicationDefinition {
                    programs: vec![
                        OnlineProgramDefinition::current("ABMAIN", &parent),
                        OnlineProgramDefinition::current("ABCHILD", &child),
                    ],
                    transactions: BTreeMap::from([("AB01".into(), "ABMAIN".into())]),
                    maps: vec![BmsMapDefinition {
                        mapset: "ABMAIN".into(),
                        map: "ABMAIN".into(),
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
                    name: "ABCHILD".into(),
                    generation: 1,
                    artifact: ArtifactRef::new(
                        child.content_id().to_reference(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    semantic_identity: child.semantic_id().to_reference(),
                    entry_offset: 0,
                    enabled: true,
                    remote: false,
                    reload: false,
                    java_status: mainframe_env_cics::CicsJavaStatus::NotJava,
                }])
                .unwrap();
            let mut launch = server
                .cics_invocation("IBMUSER", "AB01", Some(reference))
                .unwrap();
            launch.selector = Selector::new("program:ABMAIN", InvocationLimits::default()).unwrap();
            for (name, schema, bytes) in [
                (
                    "cics.session",
                    "mainframe-env.cics.session@1",
                    session.as_str().as_bytes(),
                ),
                (
                    "cics.transaction",
                    "mainframe-env.cics.transaction@1",
                    b"AB01".as_slice(),
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
                    "AB01",
                    24,
                    80,
                    "abend-csrf",
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
            if cancel {
                let ExecutionOutcome::Abend(abend) = outcome else {
                    panic!("{outcome:?}")
                };
                assert_eq!(abend.code, "U777");
                assert_eq!(
                    abend.dump,
                    mainframe_env_execution_api::AbendDumpDisposition::Suppressed
                );
                assert_eq!(machine.variable("PATH-X").unwrap().bytes(), b"NONE");
            } else {
                assert!(
                    matches!(outcome, ExecutionOutcome::Suspended(_)),
                    "{outcome:?}"
                );
                assert_eq!(
                    machine.variable("PATH-X").unwrap().bytes(),
                    if local_exit { b"CHLD" } else { b"GOOD" }
                );
            }
            assert_eq!(machine.variable("AREA-X").unwrap().bytes(), b"ROOTDATA");
            if !cancel {
                for name in ["CODE-X", "ORIG-X"] {
                    assert_eq!(machine.variable(name).unwrap().bytes(), b"U777", "{name}");
                }
                assert_eq!(
                    machine.variable("LEVEL-X").unwrap().bytes(),
                    &1_i16.to_be_bytes()
                );
            }
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
            let key = link_effect.idempotency_key.as_ref().unwrap();
            assert_eq!(
                store.effect(key).unwrap().unwrap().state,
                EffectState::Completed
            );
            retained = (
                store.effect(key).unwrap(),
                store
                    .list_provider_state("cics-effect-replay-v1", 32)
                    .unwrap(),
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
            );
            assert_eq!(retained.2.len(), 1);
        }
        {
            let (server, store) = backend.open_server(settings, secrets);
            server
                .cics
                .restore_terminal_run(invocation.clone(), &session, "AB01", Vec::new(), 3)
                .unwrap();
            let HostRequest::Cics(request) = &link_effect.request else {
                panic!("expected CICS")
            };
            let response = server.cics.invoke(&link_effect, request.clone()).unwrap();
            assert_eq!(
                response.disposition,
                if cancel {
                    mainframe_env_host_api::CicsDisposition::Abended
                } else if local_exit {
                    mainframe_env_host_api::CicsDisposition::Complete
                } else {
                    mainframe_env_host_api::CicsDisposition::Handler
                }
            );
            assert_eq!(
                response.target.as_deref(),
                if cancel {
                    None
                } else if local_exit {
                    Some("ABCHILD")
                } else {
                    Some("ROOT-EXIT")
                }
            );
            if local_exit {
                assert_eq!(response.payload.bytes(), b"ROOT");
                assert!(!response.outputs.contains_key("ABEND.CODE"));
            } else {
                assert!(response.payload.bytes().is_empty());
                assert_eq!(response.outputs["ABEND.CODE"].bytes(), b"U777");
            }
            if !cancel {
                let assign = CicsRequest {
                    operation: CicsOperation::Assign,
                    arguments: BTreeMap::from([(
                        "ABCODE".into(),
                        BoundedPayload::new(
                            "mainframe-env.cics.argument@1",
                            b"OUT".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    )]),
                    condition_policy: CicsConditionPolicy::Default,
                    mutation: None,
                };
                let assigned = server
                    .cics
                    .invoke(
                        &EffectRequest {
                            run_unit: invocation.run_unit_id.clone(),
                            sequence: 100,
                            deadline_tick: invocation.deadline_tick,
                            idempotency_key: None,
                            request: HostRequest::Cics(assign.clone()),
                        },
                        assign,
                    )
                    .unwrap();
                assert_eq!(assigned.outputs["ABCODE"].bytes(), b"U777");
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
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
