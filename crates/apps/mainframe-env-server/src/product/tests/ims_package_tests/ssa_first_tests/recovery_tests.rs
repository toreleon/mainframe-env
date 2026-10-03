use super::super::application_recovery::recovery_request;
use super::*;
use mainframe_env_host_api::{ImsRecoveryCall, ImsRecoveryResult, ImsRestartSelection};

fn recovery(
    server: &ProductServer,
    store: Arc<dyn PlatformStore>,
    inv: &Invocation,
    identity: &str,
    seq: u64,
    call: ImsRecoveryCall,
) -> ImsRecoveryResult {
    let effects = run_recovery_machine(
        server,
        store,
        inv,
        vec![HostRequest::ImsRecovery(recovery_request(
            identity,
            seq,
            "signed-first-recovery",
            call,
        ))],
    );
    match &effects[0].outcome {
        Ok(HostResult::ImsRecovery(r)) => r.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ssa_first_direct_child_signed_real_chkp_xrst_replace_backout_and_reopen() {
    backends("checkpoint", |store, cfg, url| {
        let server = open(store.clone(), cfg.clone());
        let identity = install(&server);
        let inv = batch(&server, "signed-first-checkpoint");
        let mut replace = recovery_db(ImsOperation::Replace, 5, "signed-first");
        replace.data = b"C1Z".to_vec();
        replace.segments = vec!["CHILD".into()];
        let mut second_replace = replace.clone();
        second_replace.mutation = recovery_db(ImsOperation::Replace, 9, "signed-first").mutation;
        let requests = vec![
            HostRequest::ImsRecovery(recovery_request(
                &identity,
                100,
                "signed-first-recovery",
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Normal,
                    area_lengths: vec![3],
                },
            )),
            call(
                3,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQA1)"],
            ),
            call(4, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "]),
            HostRequest::Ims(replace),
            HostRequest::Ims(recovery_db(ImsOperation::Rollback, 6, "signed-first")),
            call(7, 1, ImsOperation::GetUnique, &[b"ROOT    (ROOTKEY EQA1)"]),
            call(8, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "]),
            HostRequest::Ims(second_replace),
            call(
                10,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQA1)", b"CHILD   (CHILDKEYEQC1)"],
            ),
            HostRequest::ImsRecovery(recovery_request(
                &identity,
                101,
                "signed-first-recovery",
                ImsRecoveryCall::SymbolicCheckpoint {
                    id: "FIRSTC1".into(),
                    user_areas: vec![b"XYZ".to_vec()],
                },
            )),
        ];
        let effects = run_recovery_machine(&server, store.clone(), &inv, requests);
        assert_eq!(result(&effects[2]).segments[0].data, b"C1Q");
        assert_eq!(result(&effects[3]).status, "  ");
        assert_eq!(
            result(&effects[6]).segments[0].data,
            b"C1Q",
            "real backout restored the selected C1"
        );
        assert_eq!(result(&effects[8]).segments[0].data, b"C1Z");
        assert_eq!(
            effects[9].outcome,
            Ok(HostResult::ImsRecovery(ImsRecoveryResult::Checkpointed {
                status: "  ".into(),
                id: "FIRSTC1".into(),
                sequence: 2
            }))
        );
        assert_eq!(
            cursor(&*store, 1),
            serde_json::json!({"current":null,"parentage":null,"held":null,"after_end":false})
        );
        assert!(
            store
                .list_provider_state("ims-v1-generic-unit-of-work", 64)
                .unwrap()
                .is_empty()
        );
        let before = rows(&*store);
        assert_eq!(
            selected(
                &server,
                &inv,
                call(4, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "])
            )
            .unwrap()
            .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(rows(&*store), before);
        drop(server);
        let reopened: Arc<dyn PlatformStore> = url.map_or_else(
            || store.clone(),
            |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
        );
        let server = open(reopened.clone(), cfg);
        let inv = batch(&server, "signed-first-restarted");
        assert_eq!(
            recovery(
                &server,
                reopened.clone(),
                &inv,
                &identity,
                102,
                ImsRecoveryCall::Restart {
                    selection: ImsRestartSelection::Checkpoint("FIRSTC1".into()),
                    area_lengths: vec![3]
                }
            ),
            ImsRecoveryResult::Restarted {
                status: "  ".into(),
                checkpoint_id: Some("FIRSTC1".into()),
                user_areas: vec![b"XYZ".to_vec()],
                pcb_statuses: vec![(1, "  ".into())]
            }
        );
        assert_eq!(
            cursor(&*reopened, 1),
            serde_json::json!({"current":3,"parentage":3,"held":null,"after_end":false})
        );
        let before = rows(&*reopened);
        assert_eq!(
            selected(
                &server,
                &inv,
                call(11, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "])
            ),
            Err(HostProblem::Unsupported)
        );
        assert_eq!(rows(&*reopened), before);
        // Retained effect identity includes the original execution context.
        assert_eq!(
            selected(
                &server,
                &inv,
                call(4, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "])
            ),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(rows(&*reopened), before);
        let original_inv = batch(&server, "signed-first-checkpoint");
        assert_eq!(
            selected(
                &server,
                &original_inv,
                call(4, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "])
            )
            .unwrap()
            .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(rows(&*reopened), before);
        selected(
            &server,
            &inv,
            call(12, 1, ImsOperation::GetUnique, &[b"ROOT    (ROOTKEY EQA1)"]),
        )
        .unwrap();
        assert_eq!(
            selected(
                &server,
                &inv,
                call(13, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "])
            )
            .unwrap()
            .segments[0]
                .data,
            b"C1Z"
        );
        assert_eq!(
            cursor(&*reopened, 1),
            serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":2},"after_end":false})
        );
        assert_eq!(
            selected(&server, &inv, call(14, 1, ImsOperation::GetNext, &[]))
                .unwrap()
                .segments[0]
                .data,
            b"C2R"
        );
    });
}

#[test]
fn ssa_first_direct_child_signed_sqlite_separate_process_hold_and_replay() {
    if let Ok(url) = std::env::var("IMS_SIGNED_LAST_PROCESS_URL") {
        let mut cfg = config();
        cfg.store_profile = crate::StoreProfile::Sqlite;
        cfg.sqlite_url = url.clone();
        let store: Arc<dyn PlatformStore> =
            Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let server = open(store.clone(), cfg);
        if std::env::var("IMS_SIGNED_LAST_PROCESS_PHASE").unwrap() == "write" {
            install(&server);
            let inv = batch(&server, "signed-first-process");
            let effects = run_recovery_machine(
                &server,
                store.clone(),
                &inv,
                vec![
                    call(
                        3,
                        1,
                        ImsOperation::GetHoldUnique,
                        &[b"ROOT    (ROOTKEY EQA1)"],
                    ),
                    call(4, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "]),
                ],
            );
            assert_eq!(result(&effects[1]).segments[0].data, b"C1Q");
            assert_eq!(
                cursor(&*store, 1),
                serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
            );
        } else {
            let inv = batch(&server, "signed-first-process");
            assert_eq!(
                cursor(&*store, 1),
                serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
            );
            let replay = call(4, 1, ImsOperation::GetHoldNextParent, &[b"CHILD   *F "]);
            let journal = store
                .effect(&replay.mutation().unwrap().idempotency_key)
                .unwrap()
                .unwrap();
            assert_eq!(journal.state, EffectState::Completed);
            assert_eq!(
                journal.request_digest,
                mainframe_env_host_api::canonical_request_digest(&replay).unwrap()
            );
            assert_eq!(
                selected(
                    &server,
                    &batch(&server, "signed-first-process-followup"),
                    call(5, 1, ImsOperation::GetNext, &[])
                )
                .unwrap()
                .segments[0]
                    .data,
                b"C2R"
            );
            let before = rows(&*store);
            assert_eq!(
                selected(&server, &inv, replay.clone()).unwrap().segments[0].data,
                b"C1Q"
            );
            assert_eq!(rows(&*store), before);
            assert_eq!(
                store
                    .effect(&replay.mutation().unwrap().idempotency_key)
                    .unwrap()
                    .unwrap(),
                journal
            );
        }
        return;
    }
    let file = std::env::temp_dir().join(format!(
        "signed-first-process-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite:{}?mode=rwc", file.display());
    let name = std::thread::current().name().unwrap().to_owned();
    for phase in ["write", "read"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env("IMS_SIGNED_LAST_PROCESS_URL", &url)
            .env("IMS_SIGNED_LAST_PROCESS_PHASE", phase)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        eprintln!(
            "scenario signed-separate-process/{phase}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    std::fs::remove_file(file).unwrap();
}
