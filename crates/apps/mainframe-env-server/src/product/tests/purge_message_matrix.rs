//! Selected compiled PURGE route over reachable ACCUM control state.
//! Multiple control frames are not multiple pages: overflow construction is unready.
use super::*;
use mainframe_env_execution_api::{MachineDrive, MachineResume, Quantum};
use mainframe_env_host_api::EffectResult;

#[test]
fn compiled_purge_message_empty_and_accumulated_controls_memory() {
    matrix(None, false);
}

#[test]
fn compiled_purge_message_empty_and_accumulated_controls_sqlite_restart() {
    matrix(None, true);
}

#[test]
#[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18.6"]
fn postgres_compiled_purge_message_empty_and_accumulated_controls_restart() {
    matrix(Some(required_postgres_route_url()), true);
}

fn matrix(postgres: Option<String>, restart: bool) {
    // IBM CICS TS 6.x sources B 0148 / C 0188,0190 and dfhp37j.html:
    // PURGE discards the incomplete message; SEND PAGE completes a separate message.
    for frames in [0, 1, 4] {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mainframe-purge-198-{}-{:?}-{frames}-{nonce}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let backend = if let Some(url) = &postgres {
            Some(RouteRestartBackend::Postgres(url.clone()))
        } else if restart {
            Some(RouteRestartBackend::Sqlite(format!(
                "sqlite://{}?mode=rwc",
                root.join("state.db").display()
            )))
        } else {
            None
        };
        let mut settings = backend
            .as_ref()
            .map(|backend| backend.settings(root.join("artifacts")))
            .unwrap_or_else(config);
        settings.artifact_root = root.join("artifacts");
        let secrets = Arc::new(MemorySecretResolver::default());
        let open = || {
            backend
                .as_ref()
                .map(|backend| backend.open_server(settings.clone(), secrets.clone()))
                .unwrap_or_else(|| {
                    let server = ProductServer::memory(settings.clone()).unwrap();
                    (server.clone(), server.store.clone())
                })
        };
        let (mut server, mut store) = open();
        server
            .bootstrap_administrator("IBMUSER", b"TESTPASS")
            .unwrap();
        server
            .racf
            .execute_command(
                &mainframe_env_racf::CommandContext::new(
                    PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
                    "purge-198-class",
                    "purge-198-class",
                    1,
                )
                .unwrap(),
                "SETROPTS CLASSACT(FACILITY)",
            )
            .unwrap();
        for (class, resource, intent) in [
            ("TCICSTRN", "CICS.PG00", AccessIntent::Execute),
            ("FACILITY", "CICS.TERMINAL.CONTROL", AccessIntent::Update),
            ("FACILITY", "CICS.TERMINAL.PAGE", AccessIntent::Update),
        ] {
            let definition = server.racf.define_profile(class, resource, "IBMUSER", None);
            assert!(
                matches!(definition, Ok(()) | Err(HostProblem::IdempotencyConflict)),
                "{definition:?}"
            );
            server
                .racf
                .permit(class, resource, "IBMUSER", intent)
                .unwrap();
        }
        let accum = "EXEC CICS SEND CONTROL ACCUM PAGING ERASE ALARM REQID('AB') END-EXEC. ";
        let artifact = published_source_fixture(
            "PURGEM",
            &format!(
                "IDENTIFICATION DIVISION. PROGRAM-ID. PURGEM. DATA DIVISION. \
                 WORKING-STORAGE SECTION. 01 TEXT-X PIC X(5) VALUE 'FIRST'. \
                 01 RC PIC S9(9) COMP. 01 RC2 PIC S9(9) COMP. \
                 01 PURGE-FN PIC X(2). 01 PAGE-RC PIC S9(9) COMP. \
                 PROCEDURE DIVISION. \
                 EXEC CICS SEND TEXT FROM(TEXT-X) END-EXEC. {} \
                 EXEC CICS PURGE MESSAGE RESP(RC) RESP2(RC2) END-EXEC. \
                 MOVE EIBFN TO PURGE-FN. \
                 EXEC CICS PURGE MESSAGE NOHANDLE RESP(RC) END-EXEC. \
                 EXEC CICS SEND PAGE RESP(PAGE-RC) END-EXEC. \
                 MOVE 'LATER' TO TEXT-X. \
                 EXEC CICS SEND TEXT FROM(TEXT-X) END-EXEC. \
                 EXEC CICS SEND CONTROL ACCUM PAGING ALARM REQID('CD') END-EXEC. \
                 EXEC CICS SEND PAGE NOAUTOPAGE RESP(RC) END-EXEC. \
                 EXEC CICS RETURN END-EXEC.",
                accum.repeat(frames)
            ),
        );
        let artifact_ref = ArtifactRef::new(
            format!("sha256:{:x}", Sha256::digest(artifact.payload())),
            InvocationLimits::default(),
        )
        .unwrap();
        let mut invocation = server
            .cics_invocation("IBMUSER", "PG00", Some(artifact_ref))
            .unwrap();
        bind_compatible_runtime_services(&mut invocation).unwrap();
        invocation.bindings.insert(
            "cics.transaction".into(),
            BoundedPayload::new(
                "mainframe-env.cics.transaction@1",
                b"PG00".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        let session = SessionId::new(format!("purge-198-{frames}-{nonce}"), 64).unwrap();
        server
            .cics
            .launch_terminal(
                invocation.clone(),
                &session,
                "PG00",
                24,
                80,
                "purge-198-csrf",
                1,
                10_000,
            )
            .unwrap();
        let mut machine = ReferenceMachine::from_binary(
            artifact.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .unwrap();
        let checkpoint = machine.checkpoint().unwrap();
        let mut prefix = Vec::new();
        let mut resume = MachineResume::Start;
        let mut original_purge = None;
        let mut purges = 0;
        let mut returned = false;
        for _ in 0..32 {
            match machine.drive(resume, Quantum::new(10_000, 16 * 1024 * 1024).unwrap()) {
                MachineDrive::HostCall(effect) => {
                    let HostRequest::Cics(request) = &effect.request else {
                        panic!("expected typed CICS effect: {effect:?}");
                    };
                    if request.operation == CicsOperation::PurgeMessage {
                        assert!(request.mutation.is_some());
                        let before = server.cics.terminal_control_snapshot(&session).unwrap();
                        assert_eq!(before.pending_logical_message, purges == 0 && frames != 0);
                        assert_eq!(
                            before.alarm_count, 0,
                            "discarded controls must never be sent"
                        );
                    }
                    let response = server.cics.invoke(&effect, request.clone());
                    prefix.push((effect.clone(), response.clone()));
                    if request.operation == CicsOperation::SendPage
                        && original_purge.is_some()
                        && purges == 2
                    {
                        // The first SEND PAGE follows PURGE; no message can be completed.
                        if machine.variable("PAGE-RC").unwrap().bytes() == &[0; 4] {
                            let response = response.as_ref().unwrap();
                            assert_eq!(
                                (
                                    response.condition.as_str(),
                                    response.response,
                                    response.response2
                                ),
                                ("INVREQ", 16, 0)
                            );
                        }
                    }
                    if request.operation == CicsOperation::PurgeMessage {
                        let response = response.as_ref().unwrap();
                        assert_eq!(
                            (
                                response.condition.as_str(),
                                response.response,
                                response.response2
                            ),
                            ("NORMAL", 0, 0)
                        );
                        assert!(
                            !server
                                .cics
                                .terminal_control_snapshot(&session)
                                .unwrap()
                                .pending_logical_message
                        );
                        assert_eq!(
                            server
                                .cics
                                .terminal_snapshot(&session, invocation.principal.id(), 2)
                                .unwrap()
                                .screen,
                            b"FIRST"
                        );
                        purges += 1;
                        if purges == 1 {
                            original_purge = Some((effect.clone(), response.clone()));
                            if restart {
                                drop(server);
                                drop(store);
                                (server, store) = open();
                                server
                                    .cics
                                    .register_run(
                                        invocation.clone(),
                                        &session,
                                        "PG00",
                                        "ME01",
                                        "S001",
                                    )
                                    .unwrap();
                                machine = ReferenceMachine::from_binary(
                                    artifact.payload(),
                                    invocation.clone(),
                                    CodecLimits::default(),
                                )
                                .unwrap();
                                machine.restore_checkpoint(&checkpoint).unwrap();
                                // Checkpoints are taken with no pending host effect.
                                // Restore the entry checkpoint and replay the durable prefix
                                // through the original PURGE before resuming its result.
                                let mut replay_resume = MachineResume::Start;
                                for (expected, outcome) in &prefix {
                                    let MachineDrive::HostCall(replayed) = machine.drive(
                                        replay_resume,
                                        Quantum::new(10_000, 16 * 1024 * 1024).unwrap(),
                                    ) else {
                                        panic!("expected prefix host effect")
                                    };
                                    assert_eq!(replayed, *expected);
                                    let HostRequest::Cics(replayed_request) = &replayed.request
                                    else {
                                        unreachable!()
                                    };
                                    assert_eq!(
                                        server.cics.invoke(&replayed, replayed_request.clone()),
                                        *outcome
                                    );
                                    replay_resume = MachineResume::HostResult(EffectResult {
                                        sequence: replayed.sequence,
                                        outcome: outcome.clone().map(HostResult::Cics),
                                    });
                                }
                            }
                        }
                    }
                    if request.operation == CicsOperation::SendControl && purges == 2 {
                        // Replaying an earlier PURGE must not delete the new message or display.
                        let (purge_effect, purge_response) = original_purge.as_ref().unwrap();
                        let HostRequest::Cics(purge_request) = &purge_effect.request else {
                            unreachable!()
                        };
                        let before = store
                            .get_provider_state("cics-terminal-bms-v1", session.as_str())
                            .unwrap();
                        let receipts = store
                            .list_provider_state("cics-terminal-bms-receipt-v1", 32)
                            .unwrap();
                        assert_eq!(
                            server
                                .cics
                                .invoke(purge_effect, purge_request.clone())
                                .unwrap(),
                            *purge_response
                        );
                        assert_eq!(
                            store
                                .get_provider_state("cics-terminal-bms-v1", session.as_str())
                                .unwrap(),
                            before
                        );
                        assert_eq!(
                            store
                                .list_provider_state("cics-terminal-bms-receipt-v1", 32)
                                .unwrap(),
                            receipts
                        );
                        assert!(
                            server
                                .cics
                                .terminal_control_snapshot(&session)
                                .unwrap()
                                .pending_logical_message
                        );
                        assert_eq!(
                            server
                                .cics
                                .terminal_snapshot(&session, invocation.principal.id(), 2)
                                .unwrap()
                                .screen,
                            b"LATER"
                        );
                    }
                    returned |= request.operation == CicsOperation::Return;
                    resume = MachineResume::HostResult(EffectResult {
                        sequence: effect.sequence,
                        outcome: response.map(HostResult::Cics),
                    });
                }
                MachineDrive::Completed(_) => break,
                other => panic!("unexpected selected-route result: {other:?}"),
            }
        }
        assert!(
            returned,
            "PURGE then new SEND PAGE then RETURN must complete"
        );
        assert_eq!(purges, 2);
        assert_eq!(machine.variable("PURGE-FN").unwrap().bytes(), &[0x18, 0x0a]);
        assert_eq!(
            machine.variable("PAGE-RC").unwrap().bytes(),
            &16_i32.to_be_bytes()
        );
        let snapshot = server.cics.terminal_control_snapshot(&session).unwrap();
        assert_eq!((snapshot.alarm_count, snapshot.queued_pages), (1, 1));
        assert!(!snapshot.pending_logical_message);
        error_matrix(&server, store.as_ref(), &session);
        drop(server);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn error_matrix(server: &ProductServer, store: &dyn PlatformStore, session: &SessionId) {
    let artifact = published_source_fixture(
        "PURGERR",
        "IDENTIFICATION DIVISION. PROGRAM-ID. PURGERR. DATA DIVISION. \
         WORKING-STORAGE SECTION. 01 RC PIC S9(9) COMP. 01 RC2 PIC S9(9) COMP. \
         PROCEDURE DIVISION. EXEC CICS PURGE MESSAGE RESP(RC) RESP2(RC2) END-EXEC. \
         STOP RUN.",
    );
    let artifact_ref = ArtifactRef::new(
        format!("sha256:{:x}", Sha256::digest(artifact.payload())),
        InvocationLimits::default(),
    )
    .unwrap();
    for context in [
        "dpl-synconreturn",
        "dpl-without-synconreturn",
        "dpl-executionset-subset",
        "local",
    ] {
        let mut invocation = server
            .cics_invocation("IBMUSER", "PG00", Some(artifact_ref.clone()))
            .unwrap();
        bind_compatible_runtime_services(&mut invocation).unwrap();
        for (name, schema, bytes) in [
            (
                "cics.transaction",
                "mainframe-env.cics.transaction@1",
                b"PG00".as_slice(),
            ),
            (
                "cics.execution-context",
                "mainframe-env.cics.execution-context@1",
                context.as_bytes(),
            ),
        ] {
            invocation.bindings.insert(
                name.into(),
                BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap(),
            );
        }
        server
            .cics
            .register_run(invocation.clone(), session, "PG00", "ME01", "S001")
            .unwrap();
        if context == "local" {
            // No deterministic BMS temporary-storage I/O injection exists.
            // Corruption must fail closed rather than manufacture TSIOERR 35.
            let mut row = store
                .get_provider_state("cics-terminal-bms-v1", session.as_str())
                .unwrap()
                .unwrap();
            let version = row.version;
            row.version += 1;
            row.payload = b"invalid-bms-record".to_vec();
            store.put_provider_state(row, Some(version)).unwrap();
        }
        let before = store
            .get_provider_state("cics-terminal-bms-v1", session.as_str())
            .unwrap();
        let receipts = store
            .list_provider_state("cics-terminal-bms-receipt-v1", 32)
            .unwrap();
        let mut machine =
            ReferenceMachine::from_binary(artifact.payload(), invocation, CodecLimits::default())
                .unwrap();
        let MachineDrive::HostCall(effect) = machine.drive(
            MachineResume::Start,
            Quantum::new(10_000, 16 * 1024 * 1024).unwrap(),
        ) else {
            panic!("expected typed PURGE MESSAGE");
        };
        let HostRequest::Cics(request) = &effect.request else {
            unreachable!()
        };
        assert_eq!(request.operation, CicsOperation::PurgeMessage);
        let response = server.cics.invoke(&effect, request.clone());
        if context == "local" {
            assert_eq!(response, Err(HostProblem::InfrastructureFailure));
        } else {
            let response = response.as_ref().unwrap();
            assert_eq!(
                (
                    response.condition.as_str(),
                    response.response,
                    response.response2
                ),
                ("INVREQ", 16, 200)
            );
        }
        assert_eq!(
            store
                .get_provider_state("cics-terminal-bms-v1", session.as_str())
                .unwrap(),
            before
        );
        assert_eq!(
            store
                .list_provider_state("cics-terminal-bms-receipt-v1", 32)
                .unwrap(),
            receipts
        );
        let next = machine.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: response.map(HostResult::Cics),
            }),
            Quantum::new(10_000, 16 * 1024 * 1024).unwrap(),
        );
        if context == "local" {
            assert!(matches!(next, MachineDrive::Failed(_)), "{next:?}");
        } else {
            assert!(matches!(next, MachineDrive::Completed(_)), "{next:?}");
            assert_eq!(
                machine.variable("RC").unwrap().bytes(),
                &16_i32.to_be_bytes()
            );
            assert_eq!(
                machine.variable("RC2").unwrap().bytes(),
                &200_i32.to_be_bytes()
            );
        }
    }
}
