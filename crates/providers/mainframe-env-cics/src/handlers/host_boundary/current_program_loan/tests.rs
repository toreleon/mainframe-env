use super::*;
use mainframe_env_execution_api::{ArtifactRef, Selector};
use mainframe_env_store::MemoryStore;
use std::sync::atomic::{AtomicUsize, Ordering};

fn current() -> (Arc<CicsService>, Invocation, SessionId) {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = crate::service::tests::service(store);
    let mut actor = crate::service::tests::invocation();
    actor.selector = Selector::new("program:CODATE01", InvocationLimits::default()).unwrap();
    let session = SessionId::new("loan-session", 64).unwrap();
    for (name, schema, bytes) in [
        (
            "cics.execution-context",
            "mainframe-env.cics.execution-context@1",
            b"local".as_slice(),
        ),
        (
            "cics.session",
            "mainframe-env.cics.session@1",
            session.as_str().as_bytes(),
        ),
    ] {
        actor.bindings.insert(
            name.into(),
            BoundedPayload::new(schema, bytes.to_vec(), InvocationLimits::default()).unwrap(),
        );
    }
    service.create_session(&session, 24, 80).unwrap();
    service
        .register_run(actor.clone(), &session, "CDRD", "ME01", "S001")
        .unwrap();
    (service, actor, session)
}

#[test]
fn current_program_loan_holds_actual_run_blocks_other_entry_and_restores_exact_actor() {
    let (service, actor, session) = current();
    let calls = AtomicUsize::new(0);
    service
        .with_current_program_loan(&actor, 1, |loan| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(loan.invocation(), &actor);
            assert_eq!(loan.session(), session.as_str());
            loan.require_current(&actor, 1).unwrap();
            assert!(
                !service
                    .lock()
                    .unwrap()
                    .runs
                    .contains_key(&actor.run_unit_id)
            );
            assert!(matches!(
                service.with_current_program_loan(&actor, 1, |_| Ok(())),
                Err(HostProblem::Unauthorized)
            ));
            assert!(matches!(
                SessionCleanupLease::acquire(&service, session.as_str()),
                Err(HostProblem::IdempotencyConflict)
            ));
            let other = service.clone();
            let other_actor = actor.clone();
            assert_eq!(
                std::thread::spawn(move || {
                    other.with_current_program_loan(&other_actor, 1, |_| Ok(()))
                })
                .join()
                .unwrap(),
                Err(HostProblem::Unauthorized)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = service.lock().unwrap();
    assert_eq!(
        state.runs[&actor.run_unit_id]
            .current_program
            .effect_invocation,
        actor
    );
    assert!(state.task_dispatch.claims.is_empty());
    assert!(state.task_dispatch.uncertain_sessions.is_empty());
    drop(state);
    // A later loan is freshly borrowed from the restored actual Run.
    service
        .with_current_program_loan(&actor, 2, |loan| loan.require_current(&actor, 2))
        .unwrap();
}

#[test]
fn current_program_loan_refuses_changed_actor_scope_deadline_and_context_without_callback() {
    let (service, actor, session) = current();
    let mut changed = Vec::new();
    let mut candidate = actor.clone();
    candidate.artifact = ArtifactRef::new("foreign-artifact", InvocationLimits::default()).unwrap();
    changed.push(candidate);
    let mut candidate = actor.clone();
    candidate.attempt += 1;
    changed.push(candidate);
    let mut candidate = actor.clone();
    candidate.deadline_tick += 1;
    changed.push(candidate);
    let mut candidate = actor.clone();
    candidate.provider_generations.insert(
        CapabilityId::new("host.cics.command", InvocationLimits::default()).unwrap(),
        "foreign-cics@99".into(),
    );
    changed.push(candidate);
    let mut candidate = actor.clone();
    candidate.bindings.remove("cics.execution-context");
    changed.push(candidate);
    let mut candidate = actor.clone();
    candidate.bindings.insert(
        "cics.session".into(),
        BoundedPayload::new(
            "mainframe-env.cics.session@1",
            b"foreign-session".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    );
    changed.push(candidate);
    let calls = AtomicUsize::new(0);
    for candidate in changed {
        assert_eq!(
            service.with_current_program_loan(&candidate, 1, |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            Err(HostProblem::Unauthorized)
        );
    }
    assert_eq!(
        service.with_current_program_loan(&actor, actor.deadline_tick, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Err(HostProblem::TimedOut)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let state = service.lock().unwrap();
    assert_eq!(
        state.runs[&actor.run_unit_id]
            .current_program
            .effect_invocation,
        actor
    );
    assert!(state.task_dispatch.claims.is_empty());
    assert_eq!(state.runs[&actor.run_unit_id].session, session.as_str());
}

#[test]
fn current_program_loan_unknown_and_panic_fence_session_without_native_settlement() {
    for panic in [false, true] {
        let (service, actor, session) = current();
        if panic {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                service.with_current_program_loan::<(), _>(&actor, 1, |_| {
                    panic!("actual callback failure");
                })
            }));
            assert!(result.is_err());
        } else {
            let other = service.clone();
            let observed_session = session.clone();
            let boundary_seen = Arc::new(AtomicUsize::new(0));
            let seen = boundary_seen.clone();
            BEFORE_RESTORE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |service, original| {
                    let state = service.lock().unwrap();
                    assert!(!state.runs.contains_key(&original.run_unit_id));
                    assert_eq!(
                        state.task_dispatch.claims[&original.run_unit_id].commands,
                        1
                    );
                    assert!(
                        state
                            .task_dispatch
                            .uncertain_sessions
                            .contains(observed_session.as_str(),)
                    );
                    drop(state);
                    let changed_thread = other.clone();
                    let same_actor = original.clone();
                    assert_eq!(
                        std::thread::spawn(move || {
                            changed_thread.with_current_program_loan(&same_actor, 1, |_| Ok(()))
                        })
                        .join()
                        .unwrap(),
                        Err(HostProblem::Unauthorized)
                    );
                    seen.fetch_add(1, Ordering::SeqCst);
                }));
            });
            assert_eq!(
                service.with_current_program_loan::<(), _>(&actor, 1, |_| {
                    Err(HostProblem::UnknownOutcome)
                }),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(boundary_seen.load(Ordering::SeqCst), 1);
        }
        let state = service.lock().unwrap();
        assert_eq!(
            state.runs[&actor.run_unit_id]
                .current_program
                .effect_invocation,
            actor
        );
        assert!(state.task_dispatch.claims.is_empty());
        assert!(
            state
                .task_dispatch
                .uncertain_sessions
                .contains(session.as_str())
        );
        drop(state);
        assert_eq!(
            service.with_current_program_loan(&actor, 2, |_| Ok(())),
            Err(HostProblem::UnknownOutcome)
        );
    }
}
