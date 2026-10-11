use super::*;

const RECOVERY_NAMESPACE: &str = "jes-child-admission-recovery-v1";
const PARENT_JCL: &str = "//PARENT JOB CLASS=A\n//SUBMIT EXEC PGM=IEBGENER\n//SYSUT1 DD DATA,DLM=@@\n//CHILD JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n@@\n//SYSUT2 DD SYSOUT=(A,INTRDR)\n";

fn exhausted_parent() -> (
    Arc<ProductServer>,
    Arc<MemoryStore>,
    Arc<ManualJesClock>,
    String,
) {
    let store = Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
        max_work_items: 1,
        ..Default::default()
    }));
    let clock = Arc::new(ManualJesClock::new(700));
    let server = ProductServer::open_with_clock(config(), store.clone(), clock.clone()).unwrap();
    server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
    let parent = submit_jcl_direct(&server, "ALICE", b"ALICEPASS", PARENT_JCL.into());
    let id = parent["jobid"].as_str().unwrap().to_string();
    for attempt in 1..=2 {
        clock.advance(JES_IDLE_MILLIS + 10);
        server.run_jes_worker_once("parent").unwrap().unwrap();
        let work = store.get_work(&format!("jes:{id}")).unwrap().unwrap();
        assert_eq!((work.state, work.attempt), (WorkState::Queued, attempt));
    }
    (server, store, clock, id)
}

fn deny_child_cancel(server: &ProductServer) {
    server.bootstrap_identity("OTHER", b"OTHERPASS1").unwrap();
    server
        .racf
        .define_profile("JESJOBS", "JOB.CHILD", "OTHER", None)
        .unwrap();
}

fn allow_child_cancel(server: &ProductServer) {
    server
        .racf
        .permit("JESJOBS", "JOB.CHILD", "ALICE", AccessIntent::Alter)
        .unwrap();
}

#[test]
fn exhausted_child_admission_retains_recovery_when_terminalization_fails() {
    let (server, store, clock, parent) = exhausted_parent();
    deny_child_cancel(&server);
    clock.advance(JES_IDLE_MILLIS + 10);
    server.run_jes_worker_once("last-attempt").unwrap();
    let (child, _) = only_internal_reader_child(&server, &parent);
    assert_eq!(child.state, mainframe_env_batch::JobState::Queued);
    assert!(
        store
            .get_work(&format!("jes:{}", child.id))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_work(&format!("jes:{parent}"))
            .unwrap()
            .unwrap()
            .state,
        WorkState::DeadLetter
    );
    assert!(
        store
            .get_provider_state(RECOVERY_NAMESPACE, &parent)
            .unwrap()
            .is_some(),
        "failed child terminalization must retain a durable recovery obligation"
    );
    allow_child_cancel(&server);
    assert!(server.run_jes_worker_once("recovery").unwrap().is_none());
    assert_eq!(
        server.batch.get(&child.id).unwrap().state,
        mainframe_env_batch::JobState::Cancelled
    );
    assert!(
        store
            .get_provider_state(RECOVERY_NAMESPACE, &parent)
            .unwrap()
            .is_none()
    );
}

#[test]
fn exhausted_child_admission_recovers_each_restart_window_once() {
    // Crash before the last process, after failed cancellation, after successful
    // cancellation but before finishing the lease, and after dead-lettering.
    for window in 0..4 {
        let (server, store, clock, parent) = exhausted_parent();
        deny_child_cancel(&server);
        clock.advance(JES_IDLE_MILLIS + 10);
        let last = server.claim_jes_work("crashed").unwrap().unwrap();
        assert_eq!(last.attempt, 3);
        if window > 0 {
            let outcome = server.process_claimed_jes_work(&last);
            assert_eq!(outcome, Err(HostProblem::InfrastructureFailure));
            if window == 2 {
                allow_child_cancel(&server);
                let (child, _) = only_internal_reader_child(&server, &parent);
                assert!(server.cancel_internal_reader_child(&child));
            }
            if window == 3 {
                server.finish_claimed_jes_work(&last, outcome).unwrap();
            }
        }
        let (child, _) = only_internal_reader_child(&server, &parent);
        let settings = server.config.clone();
        drop(server);
        clock.advance(crate::jes_worker::JES_LEASE_TICKS + 1);
        let reopened = ProductServer::open_with_clock(settings, store.clone(), clock).unwrap();
        allow_child_cancel(&reopened);
        assert!(reopened.run_jes_worker_once("restart").unwrap().is_none());
        let terminal = reopened.batch.get(&child.id).unwrap();
        assert_eq!(
            terminal.state,
            mainframe_env_batch::JobState::Cancelled,
            "restart window {window}"
        );
        assert!(reopened.run_jes_worker_once("repeat").unwrap().is_none());
        assert_eq!(
            reopened.batch.get(&child.id).unwrap(),
            terminal,
            "recovery must cancel exactly once"
        );
        assert!(
            store
                .get_provider_state(RECOVERY_NAMESPACE, &parent)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn exhausted_child_admission_replay_does_not_enqueue_a_terminal_child() {
    let (server, store, _) = worker_test_server(700);
    server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
    let parent = submit_jcl_direct(&server, "ALICE", b"ALICEPASS", PARENT_JCL.into());
    let parent = parent["jobid"].as_str().unwrap();
    let work = server.claim_jes_work("parent").unwrap().unwrap();
    let payload = JesWorkPayload::decode(&work.payload).unwrap();
    let invocation = server.jes_work_invocation(&work, &payload, false).unwrap();
    server
        .batch
        .run_claimed(&invocation, parent, "INIT0001", false)
        .unwrap()
        .unwrap();
    let (child, _) = only_internal_reader_child(&server, parent);
    assert!(server.cancel_internal_reader_child(&child));
    let terminal = server.batch.get(&child.id).unwrap();
    for _ in 0..2 {
        assert!(matches!(
            server.admit_internal_reader_children(parent, false),
            Ok(ChildAdmissionResult::Ok)
        ));
        assert_eq!(server.batch.get(&child.id).unwrap(), terminal);
        assert!(
            store
                .get_work(&format!("jes:{}", child.id))
                .unwrap()
                .is_none(),
            "terminal child replay must not create fresh execution work"
        );
    }
}

#[test]
fn exhausted_child_admission_racing_cancel_has_one_terminal_transition() {
    let (server, store, _, parent) = exhausted_parent();
    let (child, _) = only_internal_reader_child(&server, &parent);
    let before = child.version;
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| {
        let cancel_barrier = barrier.clone();
        let server = &server;
        let child = &child;
        scope.spawn(move || {
            cancel_barrier.wait();
            assert!(server.cancel_internal_reader_child(child));
        });
        barrier.wait();
        assert!(matches!(
            server.admit_internal_reader_children(&parent, false),
            Ok(ChildAdmissionResult::Ok)
        ));
    });
    let after = server.batch.get(&child.id).unwrap();
    assert_eq!(after.state, mainframe_env_batch::JobState::Cancelled);
    assert_eq!(after.version, before + 1);
    assert!(
        store
            .get_work(&format!("jes:{}", child.id))
            .unwrap()
            .is_none()
    );
}

#[test]
fn exhausted_child_admission_sqlite_reopen_recovers_each_window_once() {
    for window in 0..4 {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-child-recovery-225-{}-{:?}-{window}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
        let mut settings = config();
        settings.store_profile = crate::StoreProfile::Sqlite;
        settings.sqlite_url = url.clone();
        settings.artifact_root = directory.join("artifacts");
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        let clock = Arc::new(ManualJesClock::new(700));
        let server =
            ProductServer::open_with_clock(settings.clone(), store.clone(), clock.clone()).unwrap();
        server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
        let parent = submit_jcl_direct(&server, "ALICE", b"ALICEPASS", PARENT_JCL.into());
        let parent = parent["jobid"].as_str().unwrap().to_string();
        // Existing fault injection: a conflicting work identity rejects child
        // admission without requiring a live database or a store wrapper.
        let mut blocked = store.get_work(&format!("jes:{parent}")).unwrap().unwrap();
        blocked.work_id = "jes:JOB00002".into();
        blocked.required_generation = "blocked-generation".into();
        blocked.payload = JesWorkPayload::new("JOB00002", "ALICE", ["host.security.authorize"])
            .unwrap()
            .encode()
            .unwrap();
        store.enqueue(blocked).unwrap();
        for _ in 0..2 {
            clock.advance(JES_IDLE_MILLIS + 10);
            server.run_jes_worker_once("parent").unwrap();
        }
        deny_child_cancel(&server);
        clock.advance(JES_IDLE_MILLIS + 10);
        let last = server.claim_jes_work("crashed").unwrap().unwrap();
        assert_eq!(last.attempt, 3);
        if window > 0 {
            let outcome = server.process_claimed_jes_work(&last);
            assert_eq!(outcome, Err(HostProblem::InfrastructureFailure));
            if window == 2 {
                allow_child_cancel(&server);
                let (child, _) = only_internal_reader_child(&server, &parent);
                assert!(server.cancel_internal_reader_child(&child));
            }
            if window == 3 {
                server.finish_claimed_jes_work(&last, outcome).unwrap();
            }
        }
        let (child, _) = only_internal_reader_child(&server, &parent);
        drop((server, store));
        let store = Arc::new(SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144).unwrap());
        clock.advance(crate::jes_worker::JES_LEASE_TICKS + 1);
        let server = ProductServer::open_with_clock(settings, store.clone(), clock).unwrap();
        allow_child_cancel(&server);
        assert!(server.run_jes_worker_once("restart").unwrap().is_none());
        let terminal = server.batch.get(&child.id).unwrap();
        assert_eq!(
            terminal.state,
            mainframe_env_batch::JobState::Cancelled,
            "SQLite restart window {window}"
        );
        assert!(server.run_jes_worker_once("repeat").unwrap().is_none());
        assert_eq!(server.batch.get(&child.id).unwrap(), terminal);
        assert!(
            store
                .get_provider_state(RECOVERY_NAMESPACE, &parent)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .get_work("jes:JOB00002")
                .unwrap()
                .unwrap()
                .required_generation,
            "blocked-generation"
        );
        drop((server, store));
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn exhausted_child_admission_recovery_failure_does_not_block_sibling_work() {
    let (server, store, clock) = worker_test_server(700);
    server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
    let parent = submit_jcl_direct(&server, "ALICE", b"ALICEPASS", PARENT_JCL.into());
    let parent = parent["jobid"].as_str().unwrap().to_string();
    let mut blocked = store.get_work(&format!("jes:{parent}")).unwrap().unwrap();
    blocked.work_id = "jes:JOB00002".into();
    blocked.required_generation = "blocked-generation".into();
    store.enqueue(blocked).unwrap();
    server.run_jes_worker_once("parent").unwrap();
    deny_child_cancel(&server);
    for _ in 0..2 {
        clock.advance(JES_IDLE_MILLIS + 10);
        server.run_jes_worker_once("parent").unwrap();
    }
    let sibling = submit_jcl_direct(
        &server,
        "ALICE",
        b"ALICEPASS",
        "//SIBLING JOB CLASS=A\n//RUN EXEC PGM=IEFBR14\n".into(),
    );
    let sibling = sibling["jobid"].as_str().unwrap();
    assert_eq!(
        server.run_jes_worker_once("sibling").unwrap(),
        Some(format!("jes:{sibling}"))
    );
    assert_eq!(
        server.batch.get(sibling).unwrap().state,
        mainframe_env_batch::JobState::Completed
    );
    assert!(
        store
            .get_provider_state(RECOVERY_NAMESPACE, &parent)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        store
            .get_work(&format!("jes:{parent}"))
            .unwrap()
            .unwrap()
            .attempt,
        3
    );
    allow_child_cancel(&server);
    assert!(server.run_jes_worker_once("recover").unwrap().is_none());
    let (child, _) = only_internal_reader_child(&server, &parent);
    assert_eq!(child.state, mainframe_env_batch::JobState::Cancelled);
}

#[test]
fn exhausted_child_admission_keeps_recovery_before_the_parent_finishes_producing_children() {
    let store = Arc::new(MemoryStore::new(mainframe_env_store::StoreLimits {
        max_work_items: 1,
        ..Default::default()
    }));
    let clock = Arc::new(ManualJesClock::new(700));
    let server = ProductServer::open_with_clock(config(), store.clone(), clock.clone()).unwrap();
    server.bootstrap_user("ALICE", b"ALICEPASS").unwrap();
    let parent = submit_jcl_direct(&server, "ALICE", b"ALICEPASS", PARENT_JCL.into());
    let parent = parent["jobid"].as_str().unwrap();
    let work = store.get_work(&format!("jes:{parent}")).unwrap().unwrap();
    let payload = JesWorkPayload::decode(&work.payload).unwrap();
    let capabilities = payload
        .capabilities
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let invocation = server
        .invocation(
            "ALICE",
            "zosmf:job-submit",
            ServiceClass::Batch,
            &capabilities,
        )
        .unwrap();
    server.batch.hold(&invocation, parent).unwrap();
    for _ in 0..3 {
        clock.advance(JES_IDLE_MILLIS + 10);
        server.run_jes_worker_once("held-parent").unwrap().unwrap();
    }
    assert_eq!(
        store.get_work(&work.work_id).unwrap().unwrap().state,
        WorkState::DeadLetter
    );
    assert!(
        server
            .run_jes_worker_once("early-recovery")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_provider_state(RECOVERY_NAMESPACE, parent)
            .unwrap()
            .is_some(),
        "recovery must not retire while the parent can still produce children"
    );
    // A late execution finishing after its durable work retired must still
    // have an obligation covering the child-creation/admission gap.
    server.batch.release(&invocation, parent).unwrap();
    server
        .batch
        .run_claimed(&invocation, parent, "INIT0001", false)
        .unwrap()
        .unwrap();
    assert!(
        server
            .run_jes_worker_once("late-child-recovery")
            .unwrap()
            .is_none()
    );
    let (child, _) = only_internal_reader_child(&server, parent);
    assert_eq!(child.state, mainframe_env_batch::JobState::Cancelled);
    assert!(
        store
            .get_provider_state(RECOVERY_NAMESPACE, parent)
            .unwrap()
            .is_none()
    );
}
