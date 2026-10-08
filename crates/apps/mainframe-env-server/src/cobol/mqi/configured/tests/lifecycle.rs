use super::setup::*;
use super::*;
use mainframe_env_execution_api::*;
use std::sync::atomic::Ordering;

#[test]
fn lifecycle_revokes_before_waiting_for_inflight_observation_without_drop_cleanup() {
    for returning in [false, true] {
        let f = Fixture::new(false);
        let root = f.mq.runtime.admit_root(f.parent.clone()).unwrap();
        let parent = root.frame();
        let mut child = f.parent.clone();
        child.execution_id = ExecutionId::new("race-child", Default::default()).unwrap();
        child.parent_execution_id = Some(f.parent.execution_id.clone());
        child.idempotency_key = IdempotencyKey::new("race-child-key", Default::default()).unwrap();
        let facet =
            f.mq.runtime
                .prepare_same_task_child(
                    &parent,
                    child.clone(),
                    MqTrustedBatchRelationship::SameTaskCall,
                )
                .unwrap();
        let closed = Arc::new(frame::ClosedFrame::new(facet, f.mq.control.clone(), 20));
        let mut session =
            frame::Session::new(closed.clone(), f.store.clone(), f.mq.control.clone());
        let observation = session.program_frame().unwrap();
        let entered = Arc::new(std::sync::Barrier::new(2));
        let released = Arc::new(std::sync::Barrier::new(2));
        let a = entered.clone();
        let b = released.clone();
        let mut visited = false;
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            if !visited {
                visited = true;
                a.wait();
                b.wait();
            }
        }));
        let reader = std::thread::spawn(move || observation.profile(&child));
        entered.wait();
        let before = f.rows();
        let lifecycle = std::thread::spawn(move || {
            if returning {
                let outcome = ExecutionOutcome::Completed(Completion {
                    return_code: 0,
                    output: BoundedPayload::new("test@1", vec![], Default::default()).unwrap(),
                });
                session.finish(&outcome).unwrap();
                assert!(session.finish(&outcome).is_err());
            } else {
                drop(session);
            }
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while closed.test_active() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let revoked = !closed.test_active();
        released.wait();
        assert!(
            revoked,
            "lifecycle must revoke before taking the frame lock"
        );
        assert_eq!(reader.join().unwrap(), Err(HostProblem::Unauthorized));
        lifecycle.join().unwrap();
        assert_eq!(f.rows(), before);
        parent.context().unwrap();
    }
}

#[test]
fn physical_publication_then_late_cancellation_is_unknown_once_without_redispatch() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        let store = f.store.clone();
        let probe = f.parent.cancellation_probe.clone().unwrap();
        *f.clock.1.lock().unwrap() = Some(Box::new(move || {
            // Trigger from physical committed receipt presence, not guessed
            // clock-call counts or a mock claiming publication atomicity.
            if !store
                .list_provider_state("mq-selected-v1-occurrence", 100)
                .unwrap()
                .is_empty()
            {
                probe.request();
            }
        }));
        let original = f.effect(1, "MQFLOW");
        let (outcome, reply) = f.run_raw(original.clone());
        assert!(reply.is_none_or(|r| r.outcome.is_err()), "{outcome:?}");
        let rows = f.rows();
        assert_eq!(
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 100)
                .unwrap()
                .len(),
            1
        );
        assert!(f.router.invoke(&f.parent, original).outcome.is_err());
        assert_eq!(f.rows(), rows);
        assert_eq!(f.factory.children.lock().unwrap().len(), 1);
        let child = f.factory.children.lock().unwrap()[0].clone();
        assert!(
            f.factory.observations.lock().unwrap()[0]
                .profile(&child)
                .is_err()
        );
    }
}

#[test]
fn genuine_admission_drop_and_raw_child_failure_retain_uncertainty_without_retry() {
    for sqlite in [false, true] {
        for raw in [false, true] {
            let f = Fixture::new(sqlite);
            let source = "IDENTIFICATION DIVISION. PROGRAM-ID. MQFLOW. DATA DIVISION. WORKING-STORAGE SECTION. 01 QM PIC X(48) VALUE SPACES. 01 HC PIC S9(9) BINARY. 01 CC PIC S9(9) BINARY. 01 RC PIC S9(9) BINARY. PROCEDURE DIVISION. CALL 'MQCONN' USING QM HC CC RC. CALL 'MISSING'. GOBACK.";
            f.install("MQFLOW", source);
            f.factory.drop_at_admission.store(!raw, Ordering::SeqCst);
            let original = f.effect(1, "MQFLOW");
            let (_, reply) = f.run_raw(original.clone());
            assert!(reply.is_none_or(|r| r.outcome.is_err()));
            let child = f.factory.children.lock().unwrap()[0].clone();
            assert!(
                f.factory.observations.lock().unwrap()[0]
                    .profile(&child)
                    .is_err()
            );
            let rows = f.rows();
            let receipts = f
                .store
                .list_provider_state("mq-selected-v1-occurrence", 100)
                .unwrap();
            assert_eq!(receipts.len(), usize::from(raw));
            assert!(f.router.invoke(&f.parent, original).outcome.is_err());
            assert_eq!(f.rows(), rows);
            assert_eq!(f.factory.children.lock().unwrap().len(), 1);
            assert_eq!(f.mq.topology.lock().unwrap().roots.len(), 1);
        }
    }
}

#[test]
fn actual_session_drop_only_revokes_transport_without_service_or_durable_cleanup() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        let root = f.mq.runtime.admit_root(f.parent.clone()).unwrap();
        let parent = root.frame();
        let mut child = f.parent.clone();
        child.execution_id = ExecutionId::new("drop-child", Default::default()).unwrap();
        child.parent_execution_id = Some(f.parent.execution_id.clone());
        child.idempotency_key = IdempotencyKey::new("drop-child-key", Default::default()).unwrap();
        let facet =
            f.mq.runtime
                .prepare_same_task_child(
                    &parent,
                    child.clone(),
                    MqTrustedBatchRelationship::SameTaskCall,
                )
                .unwrap();
        let closed = Arc::new(frame::ClosedFrame::new(facet, f.mq.control.clone(), 20));
        let session = frame::Session::new(closed.clone(), f.store.clone(), f.mq.control.clone());
        let observation = session.program_frame().unwrap();
        observation.profile(&child).unwrap();
        let rows = f.rows();
        drop(session);
        assert_eq!(observation.profile(&child), Err(HostProblem::Unauthorized));
        assert_eq!(f.rows(), rows);
        // The exact provider frame was NOT retired by transport Drop. Genuine
        // surviving root remains active; no fabricated task-end or UOW decision.
        parent.context().unwrap();
        assert!(f.saf.resources.lock().unwrap().is_empty());
    }
}

#[test]
fn original_binding_parent_topology_and_deadline_conflicts_refuse_before_saf() {
    for sqlite in [false, true] {
        for case in 0..4 {
            let mut f = Fixture::new(sqlite);
            f.install("MQFLOW", SOURCE);
            match case {
                0 => {
                    f.parent.bindings.insert(
                        "mq.host-context".into(),
                        BoundedPayload::new(
                            "mainframe-env.mq.host-context@1",
                            b"client|queue-manager".to_vec(),
                            Default::default(),
                        )
                        .unwrap(),
                    );
                }
                1 => {
                    f.parent.bindings.insert(
                        "mq.host-context".into(),
                        BoundedPayload::new(
                            "wrong@1",
                            b"zos-batch|queue-manager".to_vec(),
                            Default::default(),
                        )
                        .unwrap(),
                    );
                }
                2 => {
                    f.parent.parent_execution_id =
                        Some(ExecutionId::new("unretained-parent", Default::default()).unwrap())
                }
                _ => {
                    f.clock.0.store(1000, Ordering::SeqCst);
                }
            }
            let before = f.rows();
            let (_, reply) = f.run_raw(f.effect(1, "MQFLOW"));
            assert!(reply.is_none_or(|r| r.outcome.is_err()));
            assert_eq!(f.rows(), before);
            assert!(f.saf.resources.lock().unwrap().is_empty());
        }
    }
}

#[test]
fn late_original_call_and_catalog_changes_abort_only_new_volatile_frame() {
    for sqlite in [false, true] {
        for catalog in [false, true] {
            let f = Fixture::new(sqlite);
            f.install("MQFLOW", SOURCE);
            let store = f.store.clone();
            *f.factory.hook.lock().unwrap() = Some(Box::new(move |proof| {
                let mut record = if catalog {
                    proof.catalog_record().unwrap().clone()
                } else {
                    proof.call_reservation().clone()
                };
                let expected = record.version;
                record.version += 1;
                store.put_provider_state(record, Some(expected)).unwrap();
            }));
            let before = f.rows();
            let (_, reply) = f.run_raw(f.effect(1, "MQFLOW"));
            assert!(reply.is_none_or(|r| r.outcome.is_err()));
            assert_eq!(f.rows(), before);
            assert!(f.saf.resources.lock().unwrap().is_empty());
            let child = f.factory.children.lock().unwrap()[0].clone();
            assert!(
                f.factory.observations.lock().unwrap()[0]
                    .profile(&child)
                    .is_err()
            );
            f.mq.parent_frame(&f.parent)
                .unwrap()
                .check_original(&f.parent)
                .unwrap();
        }
    }
}

#[test]
fn late_physical_catalog_cas_rolls_back_provider_batch_and_atomic_audit() {
    for sqlite in [false, true] {
        let f = Fixture::new(sqlite);
        f.install("MQFLOW", SOURCE);
        let store = f.store.clone();
        let raced = Arc::new(Mutex::new(vec![]));
        let capture = raced.clone();
        *f.saf.hook.lock().unwrap() = Some(Box::new(move || {
            let mut row = store
                .get_provider_state("mq-v1-object-catalog", "catalog")
                .unwrap()
                .unwrap();
            let version = row.version;
            row.version += 1;
            store.put_provider_state(row, Some(version)).unwrap();
            *capture.lock().unwrap() = store.list_provider_state_prefix("mq-", 4096).unwrap();
        }));
        let (_, reply) = f.run_raw(f.effect(1, "MQFLOW"));
        assert!(reply.is_none_or(|r| r.outcome.is_err()));
        assert_eq!(f.rows(), *raced.lock().unwrap());
        let child = f.factory.children.lock().unwrap()[0].clone();
        let audit = f.store.audit_records(&child.execution_id, 0, 100).unwrap();
        assert!(audit.iter().all(|a| a.decision != AuditDecision::Success));
        assert!(
            f.store
                .list_provider_state("mq-selected-v1-occurrence", 100)
                .unwrap()
                .is_empty()
        );
    }
}
