//! Compiled installed-program identity across uncertain mutation and reopen.
#[cfg(test)]
mod tests {
    use super::super::*;
    use mainframe_env_execution_api::{MachineDrive, MachineResume, Quantum};

    #[test]
    fn compiled_program_pending_identity_survives_warm_retry_and_sqlite_reopen() {
        pending_identity(None, false);
    }

    #[test]
    fn compiled_application_pending_identity_survives_warm_retry_and_sqlite_reopen() {
        pending_identity(None, true);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_compiled_program_pending_identity_survives_warm_retry_and_reopen() {
        pending_identity(Some(required_postgres_route_url()), false);
    }

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_compiled_application_pending_identity_survives_warm_retry_and_reopen() {
        pending_identity(Some(required_postgres_route_url()), true);
    }

    fn pending_identity(postgres: Option<String>, application: bool) {
        let command = if application {
            "EXEC CICS INVOKE APPLICATION('IDAPP') OPERATION('RUN') PLATFORM('LOCAL') COMMAREA(AREA-X) LENGTH(4) END-EXEC"
        } else {
            "EXEC CICS LINK PROGRAM('IDCHILD') COMMAREA(AREA-X) LENGTH(4) END-EXEC"
        };
        let parent = published_source_fixture(
            "IDMAIN",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. IDMAIN. DATA DIVISION. WORKING-STORAGE SECTION. 01 AREA-X PIC X(4) VALUE 'ROOT'. PROCEDURE DIVISION. {command}. STOP RUN."
            ),
        );
        let child = published_source_fixture(
            "IDCHILD",
            "IDENTIFICATION DIVISION. PROGRAM-ID. IDCHILD. DATA DIVISION. WORKING-STORAGE SECTION. 01 RECORD-X PIC X(4) VALUE 'ONCE'. 01 KEY-X PIC X(3) VALUE 'ONC'. LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). PROCEDURE DIVISION USING DFHCOMMAREA. EXEC CICS WRITE FILE('IDFILE') FROM(RECORD-X) LENGTH(4) RIDFLD(KEY-X) END-EXEC. GOBACK.",
        );
        let root = std::env::temp_dir().join(format!(
            "mainframe-program-identity-{}-{:?}-{}",
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
        let session = SessionId::new("program-pending-identity", 64).unwrap();
        let invocation;
        let effect;
        let calls;
        let dataset;
        let runs;
        let instances;
        {
            let (server, store) = backend.open_server(settings.clone(), secrets.clone());
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            server
                .handle(
                    Authentication::Basic {
                        user: "IBMUSER".into(),
                        secret: b"TESTPASS".to_vec(),
                    },
                    GatewayRequest::DatasetCreate {
                        dataset: "IBMUSER.IDDATA".into(),
                        attributes: json!({"dsorg":"KSDS", "recfm":"V", "lrecl":80, "key_offset":0, "key_length":3}),
                    },
                )
                .unwrap();
            server
                .cics
                .register_file_aliases(&BTreeMap::from([(
                    "IDFILE".into(),
                    DatasetName::new("IBMUSER.IDDATA", 128).unwrap(),
                )]))
                .unwrap();
            server
                .racf
                .define_profile("FACILITY", "CICS.PROGRAM.IDCHILD", "IBMUSER", None)
                .unwrap();
            server
                .racf
                .permit(
                    "FACILITY",
                    "CICS.PROGRAM.IDCHILD",
                    "IBMUSER",
                    AccessIntent::Execute,
                )
                .unwrap();
            server
                .install_online_application(OnlineApplicationDefinition {
                    programs: vec![
                        OnlineProgramDefinition::current("IDMAIN", &parent),
                        OnlineProgramDefinition::current("IDCHILD", &child),
                    ],
                    transactions: BTreeMap::from([("ID01".into(), "IDMAIN".into())]),
                    maps: vec![BmsMapDefinition {
                        mapset: "IDMAIN".into(),
                        map: "IDMAIN".into(),
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
                    name: "IDCHILD".into(),
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
            if application {
                let child_ref = ArtifactRef::new(
                    child.content_id().to_reference(),
                    InvocationLimits::default(),
                )
                .unwrap();
                server
                    .cics
                    .register_application_entries(&[CicsApplicationEntryDefinition {
                        application: "IDAPP".into(),
                        platform: "LOCAL".into(),
                        major_version: 1,
                        minor_version: 0,
                        micro_version: 0,
                        operation: "RUN".into(),
                        program: "IDCHILD".into(),
                        program_generation: 1,
                        program_artifact: child_ref.clone(),
                        application_identity: child_ref.as_str().into(),
                        available: true,
                    }])
                    .unwrap();
            }
            let reference = ArtifactRef::new(
                parent.content_id().to_reference(),
                InvocationLimits::default(),
            )
            .unwrap();
            let mut launch = server
                .cics_invocation("IBMUSER", "ID01", Some(reference))
                .unwrap();
            launch.selector = Selector::new("program:IDMAIN", InvocationLimits::default()).unwrap();
            launch.bindings.insert(
                "cics.session".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.session@1",
                    session.as_str().as_bytes().to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            bind_compatible_runtime_services(&mut launch).unwrap();
            launch.bindings.insert(
                "cics.transaction".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.transaction@1",
                    b"ID01".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            invocation = launch;
            server
                .cics
                .launch_terminal(
                    invocation.clone(),
                    &session,
                    "ID01",
                    24,
                    80,
                    "identity-csrf",
                    1,
                    10_000,
                )
                .unwrap();
            server
                .cics
                .inject_file_fault_once(
                    CicsOperation::Write,
                    "IDFILE",
                    mainframe_env_cics::CicsFileFaultPoint::AfterMutation,
                )
                .unwrap();
            let mut probe = ReferenceMachine::from_binary(
                parent.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .unwrap();
            let MachineDrive::HostCall(first) = probe.drive(
                MachineResume::Start,
                Quantum::new(10_000, 16 * 1024 * 1024).unwrap(),
            ) else {
                panic!("expected compiled LINK")
            };
            effect = first;
            assert!(
                matches!(&effect.request, HostRequest::Cics(request) if request.operation == if application { CicsOperation::InvokeApplication } else { CicsOperation::Link })
            );
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
            let outcome = coordinator.execute_with_control(&mut machine, &invocation, || {
                server.program.observe_execution_control(&invocation)
            });
            assert!(
                matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome()),
                "{outcome:?}"
            );
            assert_eq!(
                store
                    .effect(effect.idempotency_key.as_ref().unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                EffectState::UnknownOutcome
            );
            calls = store.list_provider_state("cobol-call-replay@1", 8).unwrap();
            assert_eq!(calls.len(), 1);
            assert!(serde_json::from_slice::<Value>(&calls[0].payload).unwrap()["reply"].is_null());
            runs = store.list_provider_state("cobol-run-state@1", 8).unwrap();
            assert_eq!(runs.len(), 1);
            assert_eq!(
                serde_json::from_slice::<Value>(&runs[0].payload).unwrap()["active"],
                1
            );
            instances = store
                .list_provider_state(&format!("cobol-instance@1:{}", runs[0].key), 8)
                .unwrap();
            assert_eq!(instances.len(), 1);
            let instance: Value = serde_json::from_slice(&instances[0].payload).unwrap();
            assert_eq!(instance["busy"], true);
            assert_eq!(instance["schema_version"], 1);
            assert!(instance.get("abend").is_none());
            dataset = read_dataset(&server);
            assert_eq!(dataset, b"ONCE");
            retry(&server, &effect);
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                calls
            );
            assert_eq!(read_dataset(&server), dataset);
            assert_eq!(
                store.list_provider_state("cobol-run-state@1", 8).unwrap(),
                runs
            );
            assert_eq!(
                store
                    .list_provider_state(&format!("cobol-instance@1:{}", runs[0].key), 8)
                    .unwrap(),
                instances
            );
        }
        {
            let (server, store) = backend.open_server(settings, secrets);
            server
                .cics
                .restore_terminal_run(invocation, &session, "ID01", Vec::new(), 3)
                .unwrap();
            retry(&server, &effect);
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                calls
            );
            assert_eq!(read_dataset(&server), dataset);
            assert_eq!(
                store.list_provider_state("cobol-run-state@1", 8).unwrap(),
                runs
            );
            assert_eq!(
                store
                    .list_provider_state(&format!("cobol-instance@1:{}", runs[0].key), 8)
                    .unwrap(),
                instances
            );
            assert_eq!(
                store
                    .effect(effect.idempotency_key.as_ref().unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                EffectState::UnknownOutcome
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    fn retry(server: &ProductServer, effect: &EffectRequest) {
        let HostRequest::Cics(request) = &effect.request else {
            panic!("expected CICS LINK")
        };
        assert_eq!(
            server.cics.invoke(effect, request.clone()),
            Err(HostProblem::UnknownOutcome)
        );
    }

    fn read_dataset(server: &ProductServer) -> Vec<u8> {
        let response = server
            .handle(
                Authentication::Basic {
                    user: "IBMUSER".into(),
                    secret: b"TESTPASS".to_vec(),
                },
                GatewayRequest::DatasetRead {
                    dataset: "IBMUSER.IDDATA".into(),
                    member: None,
                },
            )
            .unwrap()
            .body;
        let mainframe_env_zosmf::GatewayBody::Bytes(bytes) = response else {
            panic!("expected raw dataset bytes")
        };
        bytes
    }
}
