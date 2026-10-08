//! Independent actual JES/controller interleavings; no production scheduling hooks.
use super::*;
use std::collections::HashMap;
use std::sync::{Condvar, mpsc};
use std::time::Duration;

struct Pause(Arc<(Mutex<bool>, Condvar)>);
impl Pause {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }
    fn release(&self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    fn wait(pair: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, cv) = &**pair;
        let (released, timeout) = cv
            .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(30), |value| {
                !*value
            })
            .unwrap();
        assert!(
            *released && !timeout.timed_out(),
            "test dispatch pause was not released"
        );
    }
}
impl Drop for Pause {
    fn drop(&mut self) {
        self.release();
    }
}

async fn entered(rx: mpsc::Receiver<()>) -> mpsc::Receiver<()> {
    tokio::task::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_secs(30))
            .expect("actual dispatch hook must fire");
        rx
    })
    .await
    .unwrap()
}

fn pause_dispatch(h: &Harness, program: &'static str) -> (Pause, mpsc::Receiver<()>) {
    let pause = Pause::new();
    let pair = pause.0.clone();
    let counts = Mutex::new(HashMap::<std::thread::ThreadId, usize>::new());
    let (tx, rx) = mpsc::channel();
    h.store.on_read(move |namespace, key| {
        if namespace != "batch-program" || key != program {
            return;
        }
        let hit = {
            let mut counts = counts.lock().unwrap();
            let count = counts.entry(std::thread::current().id()).or_default();
            *count += 1;
            *count == 2
        };
        if hit {
            tx.send(()).unwrap();
            Pause::wait(&pair);
        }
    });
    (pause, rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publication_interleaving_inflight_install_and_rollback_refuse_then_retry() {
    for rollback in [false, true] {
        let h = Harness::first();
        let second = h.stage_second();
        if rollback {
            h.server.publish_application_generation(&second).unwrap();
        }
        let program = if rollback { TWO } else { ONE };
        let marker = if rollback { MARKER_TWO } else { MARKER_ONE };
        let target = if rollback {
            h.first.clone()
        } else {
            second.clone()
        };
        let before = publication_rows(h.store.as_ref());
        let (pause, rx) = pause_dispatch(&h, program);
        let server = h.server.clone();
        let job = tokio::spawn(async move { route(&server, program).await });
        let _rx = entered(rx).await;
        let server = h.server.clone();
        let attempted = tokio::task::spawn_blocking(move || {
            if rollback {
                server.rollback_application_generation(&target)
            } else {
                server.publish_application_generation(&target)
            }
        })
        .await
        .unwrap();
        assert_eq!(attempted, Err(HostProblem::IdempotencyConflict));
        assert_eq!(publication_rows(h.store.as_ref()), before);
        pause.release();
        let (completed, outputs) = job.await.unwrap();
        assert_eq!(completed.return_code, Some(0));
        assert!(outputs.iter().any(|output| output == marker));
        if rollback {
            h.server.rollback_application_generation(&h.first).unwrap();
        } else {
            h.server.publish_application_generation(&second).unwrap();
        }
        runs(
            &h.server,
            if rollback { ONE } else { TWO },
            if rollback { MARKER_ONE } else { MARKER_TWO },
        )
        .await;
        assert!(h.server.graceful_shutdown().await);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publication_interleaving_two_actual_readers_coexist() {
    let h = Harness::first();
    let second = h.stage_second();
    let before = publication_rows(h.store.as_ref());
    let (pause, rx) = pause_dispatch(&h, ONE);
    let first_server = h.server.clone();
    let first = tokio::spawn(async move { route(&first_server, ONE).await });
    let rx = entered(rx).await;
    let second_server = h.server.clone();
    let another = tokio::spawn(async move { route(&second_server, ONE).await });
    let _rx = entered(rx).await;
    assert_eq!(
        h.server.publish_application_generation(&second),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(publication_rows(h.store.as_ref()), before);
    pause.release();
    let one = first.await.unwrap();
    let two = another.await.unwrap();
    assert_ne!(one.0.id, two.0.id);
    for (job, outputs) in [one, two] {
        assert_eq!(job.return_code, Some(0));
        assert!(outputs.iter().any(|output| output == MARKER_ONE));
    }
    h.server.publish_application_generation(&second).unwrap();
    runs(&h.server, TWO, MARKER_TWO).await;
    assert!(h.server.graceful_shutdown().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publication_interleaving_recursive_writer_refuses_in_actual_dispatch() {
    let h = Harness::first();
    let second = h.stage_second();
    let before = publication_rows(h.store.as_ref());
    let server = Arc::downgrade(&h.server);
    let count = std::sync::atomic::AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    h.store.on_read(move |namespace, key| {
        if namespace == "batch-program" && key == ONE && count.fetch_add(1, Ordering::SeqCst) == 1 {
            tx.send(
                server
                    .upgrade()
                    .unwrap()
                    .publish_application_generation(&second),
            )
            .unwrap();
        }
    });
    runs(&h.server, ONE, MARKER_ONE).await;
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(publication_rows(h.store.as_ref()), before);
    assert!(h.server.graceful_shutdown().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn publication_interleaving_writer_held_blocks_old_and_target_jobs() {
    for rollback in [false, true] {
        let h = Harness::first();
        let second = h.stage_second();
        if rollback {
            h.server.publish_application_generation(&second).unwrap();
        }
        let pause = Pause::new();
        let pair = pause.0.clone();
        let once = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        h.store.on_write(move |records| {
            if records
                .iter()
                .any(|row| row.namespace == "application-publication-v2")
                && !once.swap(true, Ordering::SeqCst)
            {
                tx.send(()).unwrap();
                Pause::wait(&pair);
            }
        });
        let server = h.server.clone();
        let target = if rollback {
            h.first.clone()
        } else {
            second.clone()
        };
        let writer = tokio::task::spawn_blocking(move || {
            if rollback {
                server.rollback_application_generation(&target)
            } else {
                server.publish_application_generation(&target)
            }
        });
        let _rx = entered(rx).await;
        refuses(&h.server, ONE).await;
        refuses(&h.server, TWO).await;
        pause.release();
        writer.await.unwrap().unwrap();
        runs(
            &h.server,
            if rollback { ONE } else { TWO },
            if rollback { MARKER_ONE } else { MARKER_TWO },
        )
        .await;
        assert!(h.server.graceful_shutdown().await);
    }
}

#[tokio::test]
async fn publication_registered_program_call_refuses_before_prepared() {
    let h = Harness::first();
    let (mut package, _) = tiny_package(&h.trust, 2);
    let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. IEFBR14.\nPROCEDURE DIVISION.\nDISPLAY 'SIGNED REGISTERED PROGRAM'.\nSTOP RUN.\n";
    let published = published_source_fixture("IEFBR14", source);
    let program = BatchProgramDefinition::current("IEFBR14", &published);
    replace_blob(&mut package, "program/FENCETWO", program.payload.clone());
    package
        .base
        .manifest
        .entries
        .iter_mut()
        .find(|entry| entry.kind == EntryKind::Program)
        .unwrap()
        .path = "program/IEFBR14".into();
    package.sections.batch_controllers[0].program = "program/IEFBR14".into();
    package.sections.batch_controllers[0]
        .properties
        .insert("selector-program".into(), "IEFBR14".into());
    resign_package(&mut package, &h.trust);
    h.server.install_batch_programs(vec![program]).unwrap();
    let staged = h.server.install_application_package_v2(&package).unwrap();
    let before = publication_rows(h.store.as_ref());
    let published_result = h.server.publish_application_generation(&staged);
    let after = publication_rows(h.store.as_ref());
    let (job, outputs) = route(&h.server, "IEFBR14").await;
    assert_eq!(published_result, Err(HostProblem::IdempotencyConflict));
    assert_eq!(after, before);
    assert_refused(&job, &outputs);
    assert!(outputs.iter().all(|output| {
        !output
            .windows(b"SIGNED REGISTERED PROGRAM".len())
            .any(|bytes| bytes == b"SIGNED REGISTERED PROGRAM")
    }));
    runs(&h.server, ONE, MARKER_ONE).await;
    assert!(h.server.graceful_shutdown().await);
}

#[tokio::test]
async fn publication_exact_tuple_and_terminal_states_fence_actual_jes() {
    for case in 0..12 {
        let h = Harness::first();
        let mut row = h
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, APP)
            .unwrap()
            .unwrap();
        let previous = row.version;
        let original = row.payload.clone();
        let mut state: Value = serde_json::from_slice(&original).unwrap();
        match case {
            0 => {
                h.store
                    .delete_provider_state(APPLICATION_PUBLICATION_NAMESPACE, APP, previous)
                    .unwrap();
            }
            1 => row.payload = b"{".to_vec(),
            2 => row.payload = vec![b' '; 64 * 1024 + 1],
            3 => state["generation"] = Value::from(2),
            4 => state["identity"] = Value::String(format!("sha256:{:064x}", 999)),
            5 => state["package"] = Value::String("OTHER".into()),
            6 => {
                state["schema_version"] =
                    Value::String("mainframe-env.application-publication@999".into())
            }
            7 => state["controllers"] = Value::String("pending".into()),
            8 => state["db2"] = Value::String("applying".into()),
            9 => state["ims"] = Value::String("failed".into()),
            10 => state["complete"] = Value::Bool(false),
            _ => state["controllers"] = Value::String("not-applicable".into()),
        }
        if case != 0 {
            if case >= 3 {
                row.payload = serde_json::to_vec(&state).unwrap();
            }
            row.version += 1;
            h.store
                .put_provider_state(row.clone(), Some(previous))
                .unwrap();
        }
        let before = publication_rows(h.store.as_ref());
        refuses(&h.server, ONE).await;
        assert_eq!(publication_rows(h.store.as_ref()), before);
        let current = h
            .store
            .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, APP)
            .unwrap();
        row.payload = original;
        row.version = current.as_ref().map_or(1, |current| current.version + 1);
        h.store
            .put_provider_state(row, current.as_ref().map(|current| current.version))
            .unwrap();
        runs(&h.server, ONE, MARKER_ONE).await;
        assert!(h.server.graceful_shutdown().await);
    }
}

#[tokio::test]
async fn publication_empty_seed_key_refuses_before_controller_or_prepared_mutation() {
    let h = Harness::first();
    let (mut package, program) = tiny_package(&h.trust, 2);
    package
        .sections
        .sql_rows
        .push(mainframe_env_application::SqlSeedRow {
            table: "FENCE.NEXT".into(),
            values: BTreeMap::from([("ID".into(), String::new())]),
        });
    resign_package(&mut package, &h.trust);
    h.server.install_batch_programs(vec![program]).unwrap();
    let staged = h.server.install_application_package_v2(&package).unwrap();
    let before = publication_rows(h.store.as_ref());
    assert_eq!(
        h.server.publish_application_generation(&staged),
        Err(HostProblem::Malformed)
    );
    assert_eq!(publication_rows(h.store.as_ref()), before);
    runs(&h.server, ONE, MARKER_ONE).await;
    refuses(&h.server, TWO).await;
    assert!(h.server.graceful_shutdown().await);
}

#[tokio::test]
async fn publication_retained_ready_retry_preserves_explicit_rollback_selection() {
    let h = Harness::first();
    let second = h.stage_second();
    h.server.publish_application_generation(&second).unwrap();
    h.server.rollback_application_generation(&h.first).unwrap();
    let before = publication_rows(h.store.as_ref());
    assert_eq!(
        h.server.publish_application_generation(&second),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(publication_rows(h.store.as_ref()), before);
    runs(&h.server, ONE, MARKER_ONE).await;
    // Explicit retained selection may restore g2; retrying its Ready commit cannot.
    h.server.rollback_application_generation(&second).unwrap();
    let h = h.reopen().await;
    runs(&h.server, TWO, MARKER_TWO).await;
    assert!(h.server.graceful_shutdown().await);
}

#[test]
fn publication_prevalidation_install_and_retained_rollback_are_read_only() {
    let h = Harness::first();
    let second = h.stage_second();
    let selected = h.server.application_generation_v2(&second).unwrap();
    let before = publication_rows(h.store.as_ref());
    for _ in 0..2 {
        let plan = h
            .server
            .prevalidate_application_publication(&selected, PublicationAction::Install, false)
            .unwrap();
        h.server
            .db2
            .validate_catalog_install(plan.db2.as_ref().unwrap())
            .unwrap();
    }
    assert_eq!(publication_rows(h.store.as_ref()), before);
    h.server.publish_application_generation(&second).unwrap();
    let retained = h.server.application_generation_v2(&h.first).unwrap();
    let before = publication_rows(h.store.as_ref());
    for _ in 0..2 {
        h.server
            .prevalidate_application_publication(&retained, PublicationAction::Rollback, false)
            .unwrap();
        h.server
            .db2
            .validate_catalog_rollback(APP, 1, &h.first.identity)
            .unwrap();
    }
    assert_eq!(
        h.server.db2.validate_catalog_rollback(APP, 1, "wrong"),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(publication_rows(h.store.as_ref()), before);
}

#[tokio::test]
async fn publication_fencing_ordinary_tso_utility_has_no_package_record_requirement() {
    let h = Harness::first();
    let mut row = h
        .store
        .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, APP)
        .unwrap()
        .unwrap();
    let expected = row.version;
    let mut state: Value = serde_json::from_slice(&row.payload).unwrap();
    state["complete"] = false.into();
    row.payload = serde_json::to_vec(&state).unwrap();
    row.version += 1;
    h.store.put_provider_state(row, Some(expected)).unwrap();
    let response = call(&h.server.router(), Method::PUT, "/zosmf/restjobs/jobs", "//UTILJOB JOB CLASS=A\n//STEP1 EXEC PGM=IKJEFT01\n//SYSTSIN DD *\nRUN PROGRAM(DSNTIAD)\n/*\n//SYSIN DD *\nSELECT ID FROM FENCE.T;\n/*\n//SYSPRINT DD SYSOUT=*\n").await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let submitted: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let job = wait_for_terminal_job(&h.server, submitted["jobid"].as_str().unwrap()).await;
    assert_eq!(job.return_code, Some(0), "ordinary utility: {job:?}");
    assert!(h.server.graceful_shutdown().await);
}
