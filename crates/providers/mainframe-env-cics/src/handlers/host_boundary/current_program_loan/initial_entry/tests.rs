use super::*;
use mainframe_env_execution_api::{ArtifactRef, CapabilityId, ExecutionId, Principal, PrincipalId};
use mainframe_env_store::MemoryStore;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    service: Arc<CicsService>,
    store: Arc<MemoryStore>,
    original: Invocation,
    actor: Invocation,
    session: SessionId,
}

fn fixture() -> Fixture {
    fixture_with_installation(0)
}

fn fixture_with_installation(mode: u8) -> Fixture {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = crate::service::tests::service(store.clone());
    service.bind_artifact_store(store.clone()).unwrap();
    let artifact = crate::service::tests::register_load_program(
        &service,
        store.as_ref(),
        "CODATE01",
        7,
        b"actual immutable infrastructure fixture",
        0,
    );
    let program = if mode == 5 { "NAMEONLY" } else { "CODATE01" };
    if (1..=4).contains(&mode) {
        let mut definition = service.lock().unwrap().program_definitions["CODATE01"][&7].clone();
        definition.generation = 8;
        definition.enabled = mode != 1;
        definition.remote = mode == 2;
        definition.java_status = if mode == 3 {
            program_control::CicsJavaStatus::Available
        } else {
            program_control::CicsJavaStatus::NotJava
        };
        definition.entry_offset = u32::from(mode == 4);
        service.register_program_definitions(&[definition]).unwrap();
    }
    if mode == 5 {
        service
            .register_programs(&[program.into()].into_iter().collect())
            .unwrap();
    }
    let installed = if mode == 6 {
        crate::service::tests::register_load_program(
            &service,
            store.as_ref(),
            "FOREIGN",
            1,
            b"other actual artifact",
            0,
        )
    } else {
        artifact.clone()
    };
    for (namespace, key, payload) in [
        ("online-transaction", "CDRD", program.as_bytes().to_vec()),
        (
            "online-program",
            program,
            installed.as_str().as_bytes().to_vec(),
        ),
    ] {
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: namespace.into(),
                    key: key.into(),
                    version: 1,
                    payload,
                },
                None,
            )
            .unwrap();
    }
    let mut original = crate::service::tests::invocation();
    original.selector = Selector::new("cics:CDRD", InvocationLimits::default()).unwrap();
    let session = SessionId::new("initial-mq-task", 64).unwrap();
    for (name, schema, bytes) in [
        (
            "cics.execution-context",
            "mainframe-env.cics.execution-context@1",
            b"local".to_vec(),
        ),
        (
            "cics.session",
            "mainframe-env.cics.session@1",
            session.as_str().as_bytes().to_vec(),
        ),
    ] {
        original.bindings.insert(
            name.into(),
            BoundedPayload::new(schema, bytes, InvocationLimits::default()).unwrap(),
        );
    }
    service
        .launch_terminal(
            original.clone(),
            &session,
            "CDRD",
            24,
            80,
            "initial-csrf",
            1,
            100,
        )
        .unwrap();
    // Independent copy of the existing initial server transformation, not the
    // product helper under test. There is no CICS provider command before entry.
    let mut actor = original.clone();
    actor.selector =
        Selector::new(format!("program:{program}"), InvocationLimits::default()).unwrap();
    actor.artifact = artifact;
    actor.idempotency_key = IdempotencyKey::new(
        format!("{}:{program}", original.idempotency_key.as_str()),
        InvocationLimits::default(),
    )
    .unwrap();
    for (name, schema, bytes) in [
        (
            "cics.program-entry",
            "mainframe-env.cics.program-entry@1",
            b"initial".to_vec(),
        ),
        ("cics.commarea", "mainframe-env.cics.commarea@1", Vec::new()),
        ("cics.aid", "mainframe-env.cics.aid@1", vec![0]),
        (
            "cics.transaction",
            "mainframe-env.cics.transaction@1",
            b"CDRD".to_vec(),
        ),
        (
            "cobol.runtime-service.CEEDAYS",
            "mainframe-env.runtime-service-selector@1",
            b"le:CEEDAYS:1".to_vec(),
        ),
        (
            "cobol.runtime-service.COBDATFT",
            "mainframe-env.runtime-service-selector@1",
            b"le:COBDATFT:1".to_vec(),
        ),
        (
            "cobol.runtime-service.MVSWAIT",
            "mainframe-env.runtime-service-selector@1",
            b"le:MVSWAIT:1".to_vec(),
        ),
        (
            "cobol.runtime-service.CEE3ABD",
            "mainframe-env.runtime-service-selector@1",
            b"le:CEE3ABD:1".to_vec(),
        ),
    ] {
        actor.bindings.insert(
            name.into(),
            BoundedPayload::new(schema, bytes, InvocationLimits::default()).unwrap(),
        );
    }
    Fixture {
        service,
        store,
        original,
        actor,
        session,
    }
}

fn rows(store: &MemoryStore) -> Vec<Vec<ProviderStateRecord>> {
    [
        "online-transaction",
        "online-program",
        "cics-program-definition-v1",
        "cics-session",
        "cics-effect-replay-v1",
    ]
    .into_iter()
    .map(|namespace| store.list_provider_state(namespace, 128).unwrap())
    .collect()
}

#[test]
fn initial_terminal_loan_binds_actual_installed_frame_before_first_external_effect() {
    let f = fixture();
    let before = rows(&f.store);
    let calls = AtomicUsize::new(0);
    f.service
        .with_initial_terminal_program_loan(&f.original, &f.actor, 2, |loan, selection| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(loan.invocation(), &f.actor);
            assert_eq!(selection.artifact, f.actor.artifact);
            assert_eq!(selection.generation, 7);
            assert!(selection.content_identity.starts_with("sha256:"));
            loan.require_current(&f.actor, 2).unwrap();
            assert!(
                !f.service
                    .lock()
                    .unwrap()
                    .runs
                    .contains_key(&f.original.run_unit_id)
            );
            assert_eq!(
                SessionCleanupLease::acquire(&f.service, f.session.as_str()).err(),
                Some(HostProblem::IdempotencyConflict)
            );
            let service = f.service.clone();
            let original = f.original.clone();
            let actor = f.actor.clone();
            assert_eq!(
                std::thread::spawn(move || service.with_initial_terminal_program_loan(
                    &original,
                    &actor,
                    2,
                    |_, _| Ok(()),
                ))
                .join()
                .unwrap(),
                Err(HostProblem::Unauthorized)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(rows(&f.store), before);
    let state = f.service.lock().unwrap();
    let run = &state.runs[&f.original.run_unit_id];
    assert_eq!(run.invocation, f.original);
    assert_eq!(run.current_program.effect_invocation, f.actor);
    assert_eq!(run.current_program.current.as_deref(), Some("CODATE01"));
    assert!(run.current_program.initial_entry);
    assert_eq!(run.current_program.logical_level, 1);
    assert_eq!(run.current_program.program_occurrence, 0);
    assert!(state.task_dispatch.claims.is_empty());
    drop(state);
    f.service
        .with_current_program_loan(&f.actor, 3, |loan| loan.require_current(&f.actor, 3))
        .unwrap();
    assert_eq!(
        f.service
            .with_initial_terminal_program_loan(&f.original, &f.actor, 3, |_, _| Ok(())),
        Err(HostProblem::Unauthorized)
    );
}

#[test]
fn initial_terminal_loan_refuses_changed_actor_installation_and_control_without_replacement() {
    let f = fixture();
    let before = rows(&f.store);
    let mut changed = Vec::new();
    let mut actor = f.actor.clone();
    actor.attempt += 1;
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.deadline_tick += 1;
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.execution_id =
        ExecutionId::new("foreign-execution", InvocationLimits::default()).unwrap();
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.artifact = ArtifactRef::new("foreign-artifact", InvocationLimits::default()).unwrap();
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.selector = Selector::new("program:OTHER", InvocationLimits::default()).unwrap();
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.idempotency_key =
        IdempotencyKey::new("foreign-key:CODATE01", InvocationLimits::default()).unwrap();
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.provider_generations.insert(
        CapabilityId::new("host.cics.command", InvocationLimits::default()).unwrap(),
        "foreign@2".into(),
    );
    changed.push(actor);
    let mut actor = f.actor.clone();
    actor.principal = Principal::new(
        PrincipalId::new("OTHER", InvocationLimits::default()).unwrap(),
        f.actor.provider_generations.keys().cloned().collect(),
        InvocationLimits::default(),
    )
    .unwrap();
    changed.push(actor);
    for name in [
        "cics.session",
        "cics.transaction",
        "cics.aid",
        "cics.commarea",
        "cics.program-entry",
        "cobol.runtime-service.CEEDAYS",
    ] {
        let mut actor = f.actor.clone();
        actor.bindings.remove(name);
        changed.push(actor);
        let mut actor = f.actor.clone();
        actor.bindings.insert(
            name.into(),
            BoundedPayload::new(
                "foreign-schema@1",
                b"foreign".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        changed.push(actor);
    }
    let calls = AtomicUsize::new(0);
    for actor in changed {
        assert_eq!(
            f.service
                .with_initial_terminal_program_loan(&f.original, &actor, 2, |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }),
            Err(HostProblem::Unauthorized)
        );
    }
    assert_eq!(
        f.service.with_initial_terminal_program_loan(
            &f.original,
            &f.actor,
            f.original.deadline_tick,
            |_, _| Ok(())
        ),
        Err(HostProblem::TimedOut)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(rows(&f.store), before);
    let state = f.service.lock().unwrap();
    assert_eq!(
        state.runs[&f.original.run_unit_id]
            .current_program
            .effect_invocation,
        f.original
    );
    assert!(
        state.runs[&f.original.run_unit_id]
            .current_program
            .current
            .is_none()
    );
    assert!(state.task_dispatch.claims.is_empty());
    drop(state);
    for mode in 1..=6 {
        let f = fixture_with_installation(mode);
        let before = rows(&f.store);
        let calls = AtomicUsize::new(0);
        assert!(
            f.service
                .with_initial_terminal_program_loan(&f.original, &f.actor, 2, |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(rows(&f.store), before);
        let state = f.service.lock().unwrap();
        assert_eq!(state.runs[&f.original.run_unit_id].invocation, f.original);
        assert_eq!(
            state.runs[&f.original.run_unit_id]
                .current_program
                .effect_invocation,
            f.original
        );
        assert!(
            state.runs[&f.original.run_unit_id]
                .current_program
                .current
                .is_none()
        );
        assert!(state.task_dispatch.claims.is_empty());
        assert!(state.task_dispatch.uncertain_sessions.is_empty());
    }
}

#[test]
fn initial_terminal_loan_unknown_and_panic_fence_before_actual_claim_release() {
    for panic in [false, true] {
        let f = fixture();
        let seen = Arc::new(AtomicUsize::new(0));
        if panic {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    f.service.with_initial_terminal_program_loan::<(), _>(
                        &f.original,
                        &f.actor,
                        2,
                        |_, _| panic!("initial dispatch panic"),
                    )
                }))
                .is_err()
            );
        } else {
            let observed = seen.clone();
            let session = f.session.clone();
            let service = f.service.clone();
            BEFORE_RESTORE.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |actual, actor| {
                    let state = actual.lock().unwrap();
                    assert!(!state.runs.contains_key(&actor.run_unit_id));
                    assert_eq!(state.task_dispatch.claims[&actor.run_unit_id].commands, 1);
                    assert!(
                        state
                            .task_dispatch
                            .uncertain_sessions
                            .contains(session.as_str())
                    );
                    drop(state);
                    let actor = actor.clone();
                    assert_eq!(
                        std::thread::spawn(move || service.with_current_program_loan(
                            &actor,
                            2,
                            |_| Ok(())
                        ))
                        .join()
                        .unwrap(),
                        Err(HostProblem::Unauthorized)
                    );
                    observed.fetch_add(1, Ordering::SeqCst);
                }))
            });
            assert_eq!(
                f.service.with_initial_terminal_program_loan::<(), _>(
                    &f.original,
                    &f.actor,
                    2,
                    |_, _| Err(HostProblem::UnknownOutcome)
                ),
                Err(HostProblem::UnknownOutcome)
            );
            assert_eq!(seen.load(Ordering::SeqCst), 1);
        }
        let state = f.service.lock().unwrap();
        assert_eq!(state.runs[&f.original.run_unit_id].invocation, f.original);
        assert_eq!(
            state.runs[&f.original.run_unit_id]
                .current_program
                .effect_invocation,
            f.actor
        );
        assert!(state.task_dispatch.claims.is_empty());
        assert!(
            state
                .task_dispatch
                .uncertain_sessions
                .contains(f.session.as_str())
        );
        drop(state);
        assert_eq!(
            f.service.with_current_program_loan(&f.actor, 3, |_| Ok(())),
            Err(HostProblem::UnknownOutcome)
        );
    }
}
