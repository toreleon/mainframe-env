use super::super::super::session_cas::SessionCasStore;
use super::*;

#[test]
fn ssa_last_direct_child_proposal_capacity_cas_and_lost_ack() {
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
            nav(&limited, 4, 1, ImsOperation::GetHoldNextParent, &[LAST]),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(snapshot(&limited), before);
        assert_eq!(rows(&*store), stored);
        drop(limited);
        let raced = SessionCasStore::new(store.clone(), RUN);
        let service = ImsService::open(raced.clone(), Default::default()).unwrap();
        raced.arm();
        assert_eq!(
            nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST]),
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
                .get_provider_state(REPLAY_NAMESPACE, "ssa-last-run-4")
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
            nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST]),
            Err(HostProblem::UnknownOutcome)
        );
        drop(service);
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(
            cursor(&service, 1),
            serde_json::json!({"current":5,"parentage":2,"held":{"id":5,"version":1},"after_end":false})
        );
        let published = rows(&*store);
        assert_eq!(
            nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST])
                .unwrap()
                .segments[0]
                .data,
            b"C3S"
        );
        assert_eq!(rows(&*store), published);
        assert_eq!(
            nav(&service, 5, 1, ImsOperation::GetNextParent, &[LAST])
                .unwrap()
                .status,
            "GE"
        );
        assert!(!position(&service, 1).is_held());
    });
}

#[test]
fn ssa_last_direct_child_sqlite_separate_process_retained_hold_and_exact_replay() {
    if let Ok(url) = std::env::var("IMS_LAST_PROCESS_URL") {
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        if std::env::var("IMS_LAST_PROCESS_PHASE").unwrap() == "write" {
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
                nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST])
                    .unwrap()
                    .segments[0]
                    .data,
                b"C3S"
            );
        } else {
            let service = ImsService::open(store.clone(), Default::default()).unwrap();
            assert_eq!(
                cursor(&service, 1),
                serde_json::json!({"current":5,"parentage":2,"held":{"id":5,"version":1},"after_end":false})
            );
            assert_eq!(
                nav(&service, 5, 1, ImsOperation::GetNext, &[])
                    .unwrap()
                    .segments[0]
                    .data,
                b"B2Y"
            );
            let before = rows(&*store);
            assert_eq!(
                nav(&service, 4, 1, ImsOperation::GetHoldNextParent, &[LAST])
                    .unwrap()
                    .segments[0]
                    .data,
                b"C3S"
            );
            assert_eq!(rows(&*store), before);
            assert_eq!(
                cursor(&service, 1),
                serde_json::json!({"current":6,"parentage":6,"held":null,"after_end":false})
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
            .env("IMS_LAST_PROCESS_URL", &url)
            .env("IMS_LAST_PROCESS_PHASE", phase)
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
