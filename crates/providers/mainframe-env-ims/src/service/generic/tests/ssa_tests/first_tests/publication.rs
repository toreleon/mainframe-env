use super::super::super::session_cas::SessionCasStore;
use super::*;

#[test]
fn ssa_first_direct_child_old_cursor_and_literal_ordinary_receipt_compatibility() {
    backends("old-compatibility", |store, url| {
        let service = seeded(store.clone());
        nav(
            &service,
            3,
            1,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (ROOTKEY EQA1)"],
        )
        .unwrap();
        let old_shape = serde_json::json!({"current":2,"parentage":2,
            "held":{"id":2,"version":1},"after_end":false});
        let decoded: PcbPosition = serde_json::from_value(old_shape.clone()).unwrap();
        assert_eq!(serde_json::to_value(&decoded).unwrap(), old_shape);
        assert_eq!(position(&service, 1), decoded);
        // An independently authored ordinary pre-F root-parent receipt: C1Q.
        // This retained fixture is not fresh execution evidence. No current
        // handler supplies its expected data or reconstructs replayed position.
        let req = navigation(RUN, 4, ImsOperation::GetNextParent, &[b"CHILD    "]);
        let expected = ImsResult {
            status: "  ".into(),
            segments: vec![mainframe_env_host_api::ImsSegment {
                name: "CHILD".into(),
                parent_key: Some(b"A1".to_vec()),
                data: b"C1Q".to_vec(),
            }],
            checkpoint_id: None,
            affected_segments: 0,
            system: None,
        };
        let mut receipt = RecordedResult::from_result(
            mainframe_env_host_api::canonical_request_digest(&HostRequest::ImsNavigation(
                req.clone(),
            ))
            .unwrap(),
            &expected,
        );
        prepare_ims_replay(
            &mut receipt,
            "ssa-first-run-4",
            &invocation(RUN),
            4,
            Default::default(),
        )
        .unwrap();
        resolve_ims_replay(&mut receipt, "ssa-first-run-4", 100, 100).unwrap();
        let historical = ProviderStateRecord {
            namespace: REPLAY_NAMESPACE.into(),
            key: "ssa-first-run-4".into(),
            version: 1,
            payload: encode_object_row("ssa-first-run-4", &receipt).unwrap(),
        };
        store.put_provider_state(historical.clone(), None).unwrap();
        assert_eq!(public(service.clone(), RUN, req.clone()).unwrap(), expected);
        assert_eq!(position(&service, 1), decoded);
        assert_eq!(
            nav(&service, 5, 1, ImsOperation::GetHoldNextParent, &[FIRST])
                .unwrap()
                .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(
            db(
                service.clone(),
                RUN,
                request(RUN, ImsOperation::Replace, 6, &["CHILD"], b"C1Z")
            )
            .status,
            "  "
        );
        nav(
            &service,
            7,
            1,
            ImsOperation::GetUnique,
            &[b"ROOT    (ROOTKEY EQA1)"],
        )
        .unwrap();
        assert_eq!(
            nav(&service, 8, 1, ImsOperation::GetNextParent, &[FIRST])
                .unwrap()
                .segments[0]
                .data,
            b"C1Z"
        );
        let before = rows(&*store);
        let before_cursor = position(&service, 1);
        assert_eq!(public(service.clone(), RUN, req.clone()).unwrap(), expected);
        assert_eq!(rows(&*store), before);
        drop(service);
        let reopened: Arc<dyn ProviderStateStore> = url.map_or_else(
            || store.clone(),
            |url| Arc::new(SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144).unwrap()),
        );
        let service = ImsService::open(reopened.clone(), Default::default()).unwrap();
        assert_eq!(public(service.clone(), RUN, req).unwrap(), expected);
        assert_eq!(position(&service, 1), before_cursor);
        assert_eq!(rows(&*reopened), before);
        assert_eq!(
            reopened
                .get_provider_state(REPLAY_NAMESPACE, "ssa-first-run-4")
                .unwrap(),
            Some(historical)
        );
    });
}

#[test]
fn ssa_first_direct_child_proposal_capacity_cas_and_lost_ack() {
    backends("publication", |store, _| {
        let service = seeded(store.clone());
        nav(
            &service,
            3,
            1,
            ImsOperation::GetHoldUnique,
            &[b"ROOT    (ROOTKEY EQA1)"],
        )
        .unwrap();
        let before = snapshot(&service);
        let stored = rows(&*store);
        let count = service.lock().unwrap().state.replay.len();
        drop(service);
        let limited = ImsService::open(
            store.clone(),
            ImsLimits {
                max_replays: count,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            nav(&limited, 4, 1, ImsOperation::GetHoldNextParent, &[FIRST]),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(snapshot(&limited), before);
        assert_eq!(rows(&*store), stored);
        drop(limited);
        let raced = SessionCasStore::new(store.clone(), RUN);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.arm();
        assert_eq!(
            nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[FIRST]),
            Err(HostProblem::IdempotencyConflict)
        );
        let after = rows(&*store);
        assert_eq!(after.len(), stored.len());
        for (old, new) in stored.iter().zip(&after) {
            assert_eq!(old.payload, new.payload);
            if old.namespace == SESSION_NAMESPACE && old.key == RUN {
                assert_eq!(new.version, old.version + 1);
            } else {
                assert_eq!(new, old);
            }
        }
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, "ssa-first-run-4")
                .unwrap()
                .is_none()
        );
        drop(service);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":2,"parentage":2,"held":{"id":2,"version":1},"after_end":false})
        );
        raced.lose_ack();
        assert_eq!(
            nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[FIRST]),
            Err(HostProblem::UnknownOutcome)
        );
        drop(service);
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
        );
        let published = rows(&*store);
        assert_eq!(
            nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[FIRST])
                .unwrap()
                .segments[0]
                .data,
            b"C1Q"
        );
        assert_eq!(rows(&*store), published);
        assert_eq!(
            nav(&service, 5, 1, ImsOperation::GetNextParent, &[FIRST])
                .unwrap()
                .segments[0]
                .data,
            b"C1Q"
        );
        assert!(!position(&service, 1).is_held());
    });
}

#[test]
fn ssa_first_direct_child_sqlite_separate_process_retained_hold_and_exact_replay() {
    if let Ok(url) = std::env::var("IMS_FIRST_PROCESS_URL") {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        if std::env::var("IMS_FIRST_PROCESS_PHASE").unwrap() == "write" {
            let service = seeded(store);
            nav(
                &service,
                3,
                1,
                ImsOperation::GetHoldUnique,
                &[b"ROOT    (ROOTKEY EQA1)"],
            )
            .unwrap();
            assert_eq!(
                nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[FIRST])
                    .unwrap()
                    .segments[0]
                    .data,
                b"C1Q"
            );
        } else {
            let service = ImsService::open(store.clone(), Default::default()).unwrap();
            assert_eq!(
                cursor(&service, 1),
                serde_json::json!({"current":3,"parentage":2,"held":{"id":3,"version":1},"after_end":false})
            );
            assert_eq!(
                nav(&service, 5, 1, ImsOperation::GetNext, &[])
                    .unwrap()
                    .segments[0]
                    .data,
                b"C2R"
            );
            let before = rows(&*store);
            assert_eq!(
                nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[FIRST])
                    .unwrap()
                    .segments[0]
                    .data,
                b"C1Q"
            );
            assert_eq!(rows(&*store), before);
            assert_eq!(
                cursor(&service, 1),
                serde_json::json!({"current":4,"parentage":4,"held":null,"after_end":false})
            );
        }
        return;
    }
    let (file, url, store) = sqlite();
    drop(store);
    let name = std::thread::current().name().unwrap().to_owned();
    for phase in ["write", "read"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--nocapture"])
            .env("IMS_FIRST_PROCESS_URL", &url)
            .env("IMS_FIRST_PROCESS_PHASE", phase)
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
            "scenario separate-process/{phase}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    std::fs::remove_file(file).unwrap();
}
