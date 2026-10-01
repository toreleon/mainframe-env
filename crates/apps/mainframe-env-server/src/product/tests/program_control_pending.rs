//! Unresolved installed control is not a known LINK condition or a normal return.
#[cfg(test)]
mod tests {
    use super::super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use mainframe_env_execution_api::{MachineDrive, MachineResume, Quantum};
    use mainframe_env_store_api::{CheckpointRecord, ExecutionRecord};

    #[derive(Clone, Copy, Debug)]
    enum PendingControl {
        ProgramExit,
        DirectProgramExit,
        Xctl,
        Suspend,
    }

    #[test]
    fn compiled_non_root_program_exit_remains_unknown_after_sqlite_reopen() {
        pending_control(None, PendingControl::ProgramExit);
    }

    #[test]
    fn compiled_nested_xctl_remains_unknown_after_sqlite_reopen() {
        pending_control(None, PendingControl::Xctl);
    }

    #[test]
    fn compiled_current_level_program_exit_stages_issuer_commarea_after_sqlite_reopen() {
        pending_control(None, PendingControl::DirectProgramExit);
    }

    #[test]
    fn compiled_child_suspend_remains_unknown_after_sqlite_reopen() {
        pending_control(None, PendingControl::Suspend);
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_non_root_program_exit_remains_unknown_after_reopen() {
        pending_control(
            Some(required_postgres_route_url()),
            PendingControl::ProgramExit,
        );
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_nested_xctl_remains_unknown_after_reopen() {
        pending_control(Some(required_postgres_route_url()), PendingControl::Xctl);
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_current_level_program_exit_stages_issuer_commarea_after_reopen() {
        pending_control(
            Some(required_postgres_route_url()),
            PendingControl::DirectProgramExit,
        );
    }

    #[test]
    #[ignore = "requires fresh MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
    fn postgres_child_suspend_remains_unknown_after_reopen() {
        pending_control(Some(required_postgres_route_url()), PendingControl::Suspend);
    }

    fn pending_control(postgres: Option<String>, control: PendingControl) {
        let parent = published_source_fixture(
            "PCROOT",
            "IDENTIFICATION DIVISION. PROGRAM-ID. PCROOT. DATA DIVISION. \
             WORKING-STORAGE SECTION. 01 AREA-X PIC X(4) VALUE 'ROOT'. \
             01 PATH-X PIC X(4) VALUE 'NONE'. 01 RESP-X PIC S9(9) COMP. \
             PROCEDURE DIVISION. EXEC CICS LINK PROGRAM('PCMID') \
             COMMAREA(AREA-X) LENGTH(4) RESP(RESP-X) END-EXEC. \
             MOVE 'WRNG' TO PATH-X. EXEC CICS SUSPEND END-EXEC. GOBACK.",
        );
        let middle_control = match control {
            PendingControl::ProgramExit => {
                "EXEC CICS HANDLE ABEND PROGRAM('PCEXIT') END-EXEC. \
                 EXEC CICS LINK PROGRAM('PCLEAF') COMMAREA(LEAF-AREA) LENGTH(4) END-EXEC."
            }
            PendingControl::Xctl => {
                "EXEC CICS XCTL PROGRAM('PCEXIT') COMMAREA(DFHCOMMAREA) LENGTH(4) END-EXEC."
            }
            PendingControl::DirectProgramExit => {
                "EXEC CICS HANDLE ABEND PROGRAM('PCEXIT') END-EXEC. \
                 EXEC CICS ABEND ABCODE('U789') NODUMP END-EXEC."
            }
            PendingControl::Suspend => "EXEC CICS SUSPEND END-EXEC.",
        };
        let middle = published_source_fixture(
            "PCMID",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. PCMID. DATA DIVISION. \
             WORKING-STORAGE SECTION. 01 LEAF-AREA PIC X(4) VALUE 'LEAF'. \
             LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). \
             PROCEDURE DIVISION USING DFHCOMMAREA. {middle_control} \
             MOVE 'WRNG' TO DFHCOMMAREA. GOBACK."
            ),
        );
        let leaf = published_source_fixture(
            "PCLEAF",
            "IDENTIFICATION DIVISION. PROGRAM-ID. PCLEAF. DATA DIVISION. \
             LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). \
             PROCEDURE DIVISION USING DFHCOMMAREA. \
             EXEC CICS ABEND ABCODE('U789') NODUMP END-EXEC. GOBACK.",
        );
        let exit = published_source_fixture(
            "PCEXIT",
            "IDENTIFICATION DIVISION. PROGRAM-ID. PCEXIT. DATA DIVISION. \
             LINKAGE SECTION. 01 DFHCOMMAREA PIC X(4). \
             PROCEDURE DIVISION USING DFHCOMMAREA. MOVE 'GOOD' TO DFHCOMMAREA. GOBACK.",
        );
        let root = std::env::temp_dir().join(format!(
            "mainframe-pending-control-{}-{control:?}-{}",
            std::process::id(),
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
        let session = SessionId::new("pending-control", 64).unwrap();
        let principal = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let invocation;
        let link;
        let receipts;
        let runs;
        let protocols;
        let instances;
        let cics_replays;
        let suspended;
        let outer;
        {
            let (server, store) = backend.open_server(settings.clone(), secrets.clone());
            server.bootstrap_user("IBMUSER", b"TESTPASS").unwrap();
            for name in ["PCMID", "PCLEAF", "PCEXIT"] {
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
                        OnlineProgramDefinition::current("PCROOT", &parent),
                        OnlineProgramDefinition::current("PCMID", &middle),
                        OnlineProgramDefinition::current("PCLEAF", &leaf),
                        OnlineProgramDefinition::current("PCEXIT", &exit),
                    ],
                    transactions: BTreeMap::from([("PC01".into(), "PCROOT".into())]),
                    maps: vec![BmsMapDefinition {
                        mapset: "PCROOT".into(),
                        map: "PCROOT".into(),
                        line: 1,
                        column: 1,
                        rows: 24,
                        columns: 80,
                        fields: Vec::new(),
                    }],
                })
                .unwrap();
            let definitions = [("PCMID", &middle), ("PCLEAF", &leaf), ("PCEXIT", &exit)].map(
                |(name, artifact)| mainframe_env_cics::CicsProgramDefinition {
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
                },
            );
            server
                .cics
                .register_program_definitions(&definitions)
                .unwrap();
            let mut launch = server
                .cics_invocation(
                    "IBMUSER",
                    "PC01",
                    Some(
                        ArtifactRef::new(
                            parent.content_id().to_reference(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    ),
                )
                .unwrap();
            launch.selector = Selector::new("program:PCROOT", InvocationLimits::default()).unwrap();
            launch.bindings.insert(
                "cics.session".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.session@1",
                    session.as_str().as_bytes().to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            launch.bindings.insert(
                "cics.transaction".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.transaction@1",
                    b"PC01".to_vec(),
                    InvocationLimits::default(),
                )
                .unwrap(),
            );
            bind_compatible_runtime_services(&mut launch).unwrap();
            invocation = launch;
            server
                .cics
                .launch_terminal(
                    invocation.clone(),
                    &session,
                    "PC01",
                    24,
                    80,
                    "pending-control-csrf",
                    1,
                    10_000,
                )
                .unwrap();
            let mut probe = ReferenceMachine::from_binary(
                parent.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .unwrap();
            let MachineDrive::HostCall(effect) = probe.drive(
                MachineResume::Start,
                Quantum::new(10_000, 16 * 1024 * 1024).unwrap(),
            ) else {
                panic!("expected LINK");
            };
            link = effect;
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
                matches!(outcome, ExecutionOutcome::ProviderFailure(ref problem) if problem.has_unknown_outcome()),
                "{control:?}: {outcome:?}"
            );
            assert_eq!(machine.variable("AREA-X").unwrap().bytes(), b"ROOT");
            assert_eq!(machine.variable("PATH-X").unwrap().bytes(), b"NONE");
            outer = store
                .effect(link.idempotency_key.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(outer.state, EffectState::UnknownOutcome);
            receipts = store.list_provider_state("cobol-call-replay@1", 8).unwrap();
            assert_eq!(
                receipts.len(),
                if matches!(control, PendingControl::ProgramExit) {
                    2
                } else {
                    1
                }
            );
            assert!(receipts.iter().all(|row| {
                serde_json::from_slice::<Value>(&row.payload).unwrap()["reply"].is_null()
            }));
            suspended = receipts
                .iter()
                .filter_map(|row| {
                    let receipt: Value = serde_json::from_slice(&row.payload).unwrap();
                    let id = ExecutionId::new(
                        receipt["child_execution"].as_str().unwrap(),
                        InvocationLimits::default(),
                    )
                    .unwrap();
                    let execution = store.get_execution(&id).unwrap().unwrap();
                    (execution.state == ExecutionState::Suspended)
                        .then(|| (execution, store.get_checkpoint(&id).unwrap().unwrap()))
                })
                .collect::<Vec<_>>();
            assert_eq!(suspended.len(), 1);
            assert_eq!(suspended[0].0.selector.as_str(), "program:PCMID");
            assert!(suspended[0].0.terminal_tick.is_none());
            let mid_receipt = receipts
                .iter()
                .find(|row| {
                    serde_json::from_slice::<Value>(&row.payload).unwrap()["child_execution"]
                        == suspended[0].0.execution_id.as_str()
                })
                .unwrap();
            let mid_value: Value = serde_json::from_slice(&mid_receipt.payload).unwrap();
            if matches!(control, PendingControl::Suspend) {
                assert_eq!(mid_receipt.version, 1);
                assert_eq!(mid_value["schema_version"], 2);
                assert!(mid_value.get("transfer").is_none());
            } else {
                assert_eq!(mid_receipt.version, 3);
                assert_eq!(mid_value["schema_version"], 4);
                let target = &mid_value["target"];
                assert_eq!(target["selector"], "program:PCEXIT");
                assert_eq!(target["generation"], 1);
                assert_eq!(target["artifact"], exit.content_id().to_reference());
                assert_eq!(
                    target["checkpoint_schema"],
                    "mainframe-env.reference-machine-checkpoint@12"
                );
                assert!(
                    target["checkpoint"]
                        .as_str()
                        .is_some_and(|bytes| !bytes.is_empty())
                );
                assert!(
                    target["context_digest"]
                        .as_str()
                        .is_some_and(|digest| digest.len() == 64)
                );
                let transfer = &mid_value["transfer"];
                assert_eq!(transfer["selector"], "PCEXIT");
                assert_eq!(transfer["schema"], "mainframe-env.cics.payload@1");
                assert_eq!(transfer["bytes"], serde_json::json!(b"ROOT".to_vec()));
                assert_eq!(transfer["source_version"], suspended[0].0.version);
                assert_eq!(transfer["source_attempt"], suspended[0].0.attempt);
                assert_eq!(
                    transfer["source_artifact"],
                    suspended[0].0.artifact.as_str()
                );
                assert_eq!(transfer["source_selector"], "program:PCMID");
                assert_eq!(
                    transfer["checkpoint_sequence"],
                    suspended[0].1.effect_sequence
                );
                assert_eq!(
                    transfer["checkpoint_digest"],
                    hex_digest(&suspended[0].1.payload_digest)
                );
                assert_staged_target(&store, &invocation, target, &exit);
                assert_transfer_selection(
                    &server,
                    &store,
                    &invocation,
                    mid_receipt,
                    &suspended[0].0,
                    &suspended[0].1,
                    &exit,
                );
                // Publish a different latest generation and redirect the generic catalog.
                // The retained transfer result must still select the original artifact.
                let replacement = published_source_fixture(
                    "PCEXIT",
                    "IDENTIFICATION DIVISION. PROGRAM-ID. PCEXIT. PROCEDURE DIVISION. DISPLAY 'NEW'. GOBACK.",
                );
                server
                    .artifacts
                    .put_artifact(
                        crate::cobol::artifact::published_artifact_record(&replacement).unwrap(),
                    )
                    .unwrap();
                let mut definition = definitions[2].clone();
                definition.generation = 2;
                definition.artifact = ArtifactRef::new(
                    replacement.content_id().to_reference(),
                    InvocationLimits::default(),
                )
                .unwrap();
                definition.semantic_identity = replacement.semantic_id().to_reference();
                server
                    .cics
                    .register_program_definitions(&[definition])
                    .unwrap();
                let catalog = store
                    .get_provider_state("online-program", "PCEXIT")
                    .unwrap()
                    .unwrap();
                store
                    .put_provider_state(
                        ProviderStateRecord {
                            version: catalog.version + 1,
                            payload: replacement.content_id().to_reference().into_bytes(),
                            ..catalog.clone()
                        },
                        Some(catalog.version),
                    )
                    .unwrap();
                assert_transfer_selection(
                    &server,
                    &store,
                    &invocation,
                    mid_receipt,
                    &suspended[0].0,
                    &suspended[0].1,
                    &exit,
                );
            }
            runs = store.list_provider_state("cobol-run-state@1", 8).unwrap();
            assert_eq!(runs.len(), 1);
            let run: Value = serde_json::from_slice(&runs[0].payload).unwrap();
            assert_eq!(run["active"], 1);
            assert_eq!(run["ended"], false);
            instances = store
                .list_provider_state(&format!("cobol-instance@1:{}", runs[0].key), 8)
                .unwrap();
            let mid = instances.iter().find(|row| row.key == "PCMID").unwrap();
            let mid: Value = serde_json::from_slice(&mid.payload).unwrap();
            assert_eq!(mid["schema_version"], 1);
            assert_eq!(mid["busy"], true);
            protocols = store
                .list_provider_state("cobol-call-protocol@2", 8)
                .unwrap();
            cics_replays = store
                .list_provider_state("cics-effect-replay-v1", 32)
                .unwrap();
            assert!(
                !cics_replays
                    .iter()
                    .any(|row| row.key == link.idempotency_key.as_ref().unwrap().as_str())
            );
            assert_task_fenced(&server, &invocation, &session, &principal);
        }
        {
            let (server, store) = backend.open_server(settings, secrets);
            server
                .cics
                .restore_terminal_run(invocation.clone(), &session, "PC01", Vec::new(), 3)
                .unwrap();
            let HostRequest::Cics(request) = &link.request else {
                panic!("expected CICS");
            };
            assert_eq!(
                server.cics.invoke(&link, request.clone()),
                Err(HostProblem::UnknownOutcome)
            );
            assert_task_fenced(&server, &invocation, &session, &principal);
            assert_eq!(
                store
                    .effect(link.idempotency_key.as_ref().unwrap())
                    .unwrap()
                    .unwrap(),
                outer
            );
            assert_eq!(
                store.list_provider_state("cobol-call-replay@1", 8).unwrap(),
                receipts
            );
            if !matches!(control, PendingControl::Suspend) {
                let row = receipts
                    .iter()
                    .find(|row| {
                        serde_json::from_slice::<Value>(&row.payload).unwrap()["target"].is_object()
                    })
                    .unwrap();
                let value: Value = serde_json::from_slice(&row.payload).unwrap();
                assert_staged_target(&store, &invocation, &value["target"], &exit);
                assert_transfer_selection(
                    &server,
                    &store,
                    &invocation,
                    row,
                    &suspended[0].0,
                    &suspended[0].1,
                    &exit,
                );
            }
            assert_eq!(
                store.list_provider_state("cobol-run-state@1", 8).unwrap(),
                runs
            );
            assert_eq!(
                store
                    .list_provider_state("cobol-call-protocol@2", 8)
                    .unwrap(),
                protocols
            );
            assert_eq!(
                store
                    .list_provider_state(&format!("cobol-instance@1:{}", runs[0].key), 8)
                    .unwrap(),
                instances
            );
            assert_eq!(
                store
                    .list_provider_state("cics-effect-replay-v1", 32)
                    .unwrap(),
                cics_replays
            );
            for (execution, checkpoint) in suspended {
                assert_eq!(
                    store.get_execution(&execution.execution_id).unwrap(),
                    Some(execution)
                );
                assert_eq!(
                    store.get_checkpoint(&checkpoint.execution_id).unwrap(),
                    Some(checkpoint)
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    fn assert_staged_target(
        store: &Arc<dyn PlatformStore>,
        caller: &Invocation,
        target: &Value,
        artifact: &PublishedArtifact,
    ) {
        let saved = &target["invocation"];
        let limits = InvocationLimits::default();
        let mut next = caller.clone();
        next.request_id = RequestId::new(saved["request"].as_str().unwrap(), limits).unwrap();
        next.execution_id = ExecutionId::new(saved["execution"].as_str().unwrap(), limits).unwrap();
        next.parent_execution_id =
            Some(ExecutionId::new(saved["parent"].as_str().unwrap(), limits).unwrap());
        next.trace_id = TraceId::new(saved["trace"].as_str().unwrap(), limits).unwrap();
        next.idempotency_key = IdempotencyKey::new(saved["key"].as_str().unwrap(), limits).unwrap();
        next.selector = Selector::new("program:PCEXIT", limits).unwrap();
        next.artifact = ArtifactRef::new(artifact.content_id().to_reference(), limits).unwrap();
        next.audit_correlation = saved["audit"].as_str().unwrap().into();
        next.bindings = saved["bindings"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    BoundedPayload::new(
                        value["schema"].as_str().unwrap(),
                        STANDARD.decode(value["bytes"].as_str().unwrap()).unwrap(),
                        limits,
                    )
                    .unwrap(),
                )
            })
            .collect();
        assert_eq!(saved["run"], caller.run_unit_id.as_str());
        assert_eq!(saved["principal"], caller.principal.id().as_str());
        assert_eq!(saved["deadline"], caller.deadline_tick);
        assert_eq!(saved["priority"], caller.priority);
        assert_eq!(saved["attempt"], caller.attempt);
        assert_eq!(
            saved["grants"],
            serde_json::json!(
                caller
                    .principal
                    .grants()
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>()
            )
        );
        assert_eq!(
            saved["generations"],
            serde_json::json!(
                caller
                    .provider_generations
                    .iter()
                    .map(|(id, generation)| (id.as_str(), generation))
                    .collect::<BTreeMap<_, _>>()
            )
        );
        for key in ["cics.session", "cics.transaction"] {
            assert_eq!(next.bindings[key], caller.bindings[key]);
        }
        assert_eq!(next.bindings["cics.commarea"].bytes(), b"ROOT");
        assert!(
            store.get_execution(&next.execution_id).unwrap().is_none(),
            "staging is not execution admission"
        );
        let mut fresh =
            ReferenceMachine::from_binary(artifact.payload(), next, CodecLimits::default())
                .unwrap();
        let bytes = STANDARD
            .decode(target["checkpoint"].as_str().unwrap())
            .unwrap();
        let checkpoint = BoundedPayload::new(
            target["checkpoint_schema"].as_str().unwrap(),
            bytes,
            InvocationLimits {
                max_payload_bytes: 32 * 1024 * 1024,
                ..limits
            },
        )
        .unwrap();
        assert_eq!(
            fresh.checkpoint().unwrap(),
            checkpoint,
            "independent constructor image"
        );
        fresh.restore_checkpoint(&checkpoint).unwrap();
        // Constructor checkpoints precede COBOL init operations; the retained
        // call binding supplies ROOT when execution is eventually admitted.
        assert_eq!(fresh.variable("DFHCOMMAREA").unwrap().bytes(), &[0; 4]);
        assert_eq!(fresh.effect_sequence(), 0);
    }

    fn assert_transfer_selection(
        server: &ProductServer,
        store: &Arc<dyn PlatformStore>,
        caller: &Invocation,
        row: &ProviderStateRecord,
        execution: &ExecutionRecord,
        checkpoint: &CheckpointRecord,
        artifact: &PublishedArtifact,
    ) {
        let mut source = caller.clone();
        source.execution_id = execution.execution_id.clone();
        source.parent_execution_id = Some(caller.execution_id.clone());
        source.selector = execution.selector.clone();
        source.artifact = execution.artifact.clone();
        source.idempotency_key = IdempotencyKey::new(
            format!("online-call-effect-{}", row.key),
            InvocationLimits::default(),
        )
        .unwrap();
        let key = IdempotencyKey::new(
            format!("{}:{}", source.idempotency_key, checkpoint.effect_sequence),
            InvocationLimits::default(),
        )
        .unwrap();
        let effect = store.effect(&key).unwrap().unwrap();
        let observed = mainframe_env_execution_api::Transfer {
            selector: Selector::new("PCEXIT", InvocationLimits::default()).unwrap(),
            payload: BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                b"ROOT".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
            replace_frame: true,
        };
        let selection = server
            .cics
            .attested_program_transfer_selection(&source, &effect, &observed)
            .unwrap();
        assert_eq!(selection.generation, 1);
        assert_eq!(
            selection.artifact.as_str(),
            artifact.content_id().to_reference()
        );
        let mut wrong = effect.clone();
        wrong.result_digest = Some([0; 32]);
        assert_eq!(
            server
                .cics
                .attested_program_transfer_selection(&source, &wrong, &observed),
            Err(HostProblem::UnknownOutcome)
        );
        wrong = effect.clone();
        wrong.intent.owner = caller.execution_id.clone();
        assert_eq!(
            server
                .cics
                .attested_program_transfer_selection(&source, &wrong, &observed),
            Err(HostProblem::UnknownOutcome)
        );
        let mut wrong_transfer = observed.clone();
        wrong_transfer.payload = BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            b"LEAF".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        assert_eq!(
            server
                .cics
                .attested_program_transfer_selection(&source, &effect, &wrong_transfer),
            Err(HostProblem::UnknownOutcome)
        );
        let descriptor = crate::cobol::retention::describe_cobol_retention_row(row).unwrap();
        for (namespace, expected) in [
            (
                "cics-program-definition-v1",
                "PCEXIT:00000000000000000001".to_string(),
            ),
            ("cics-effect-replay-v1", key.as_str().to_string()),
        ] {
            assert!(descriptor.dependencies.iter().any(|dependency| matches!(dependency,
                crate::cobol::retention::CobolRetentionDependency::ProviderRow { namespace: ns, key } if ns == namespace && key == &expected)));
        }
    }

    fn assert_task_fenced(
        server: &ProductServer,
        invocation: &Invocation,
        session: &SessionId,
        principal: &PrincipalId,
    ) {
        assert_eq!(
            server.program.finish_run_unit(invocation),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server
                .cics
                .restore_terminal_run(invocation.clone(), session, "PC01", Vec::new(), 4),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            server.cics.complete_terminal_run(session, principal, 4),
            Err(HostProblem::UnknownOutcome)
        );
        let request = CicsRequest {
            operation: mainframe_env_host_api::CicsOperation::Suspend,
            arguments: BTreeMap::new(),
            condition_policy: mainframe_env_host_api::CicsConditionPolicy::Default,
            mutation: None,
        };
        assert_eq!(
            server.cics.invoke(
                &EffectRequest {
                    run_unit: invocation.run_unit_id.clone(),
                    sequence: 999,
                    deadline_tick: invocation.deadline_tick,
                    idempotency_key: None,
                    request: HostRequest::Cics(request.clone())
                },
                request
            ),
            Err(HostProblem::UnknownOutcome)
        );
    }
}
