use super::*;
use mainframe_env_execution_api::{ArtifactRef, ExecutionId, Selector};
use mainframe_env_store::MemoryStore;

fn fixture() -> (
    Arc<CicsService>,
    Invocation,
    Invocation,
    ProgramLinkSelection,
) {
    let store = Arc::new(MemoryStore::new(Default::default()));
    let service = crate::service::tests::service(store.clone());
    service.bind_artifact_store(store.clone()).unwrap();
    let root = crate::service::tests::invocation();
    let session = SessionId::new("REPLACE-SESSION", 64).unwrap();
    service.create_session(&session, 24, 80).unwrap();
    service
        .register_run(root.clone(), &session, "MENU", "MEAPPL", "MESYS")
        .unwrap();
    let artifact = crate::service::tests::register_load_program(
        &service,
        store.as_ref(),
        "NEXT",
        1,
        b"retained-test-executable",
        0,
    );
    let row = store
        .get_provider_state("cics-program-definition-v1", "NEXT:00000000000000000001")
        .unwrap()
        .unwrap();
    let selection = ProgramLinkSelection {
        artifact,
        generation: 1,
        content_identity: format!("sha256:{:x}", Sha256::digest(&row.payload)),
    };
    let mut source = root.clone();
    source.execution_id = ExecutionId::new("source", InvocationLimits::default()).unwrap();
    source.parent_execution_id = Some(root.execution_id.clone());
    source.selector = Selector::new("program:SOURCE", InvocationLimits::default()).unwrap();
    source.artifact = ArtifactRef::new("source-artifact", InvocationLimits::default()).unwrap();
    (service, root, source, selection)
}

fn target(source: &Invocation, selection: &ProgramLinkSelection) -> Invocation {
    let mut target = source.clone();
    target.execution_id = ExecutionId::new("replacement", InvocationLimits::default()).unwrap();
    target.parent_execution_id = Some(source.execution_id.clone());
    target.selector = Selector::new("program:NEXT", InvocationLimits::default()).unwrap();
    target.artifact = selection.artifact.clone();
    target
}

#[test]
fn same_level_replacement_preserves_root_and_level_and_rejects_source_reentry() {
    let (service, root, source, selection) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let loan = ProgramLease::acquire(
        &service,
        &mut caller,
        "SOURCE",
        Some(source.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&source).unwrap();
    let next = target(&source, &selection);
    let before = service.lock().unwrap().runs[&root.run_unit_id]
        .current_program
        .clone();
    let result = service
        .with_same_level_program_frame(&source, &next, &selection, || {
            assert_eq!(service.ensure_run(&source), Err(HostProblem::Unauthorized));
            service.ensure_run(&next).unwrap();
            let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
            assert_eq!(command.invocation, root);
            assert_eq!(command.current_program.logical_level, before.logical_level);
            assert_eq!(
                command.current_program.invoking_program,
                before.invoking_program
            );
            assert_eq!(
                command.current_program.return_program,
                before.return_program
            );
            assert_eq!(command.current_program.current.as_deref(), Some("NEXT"));
            command
                .current_records
                .insert("FILE".into(), b"shared-update".to_vec());
            command.finish().unwrap();
            bounded(b"known-return".to_vec())
        })
        .unwrap();
    assert_eq!(result.bytes(), b"known-return");
    assert_eq!(
        service.lock().unwrap().runs[&root.run_unit_id]
            .current_program
            .effect_invocation,
        next
    );
    loan.finish().unwrap();
    assert_eq!(caller.current_program.effect_invocation, root);
    assert_eq!(caller.current_records["FILE"], b"shared-update");
    caller.finish().unwrap();
}

#[test]
fn same_level_replacement_rejects_forged_controls_selection_and_foreign_thread_without_dispatch() {
    let (service, root, source, selection) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let loan = ProgramLease::acquire(
        &service,
        &mut caller,
        "SOURCE",
        Some(source.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&source).unwrap();
    let next = target(&source, &selection);
    for case in 0..13 {
        let mut forged = next.clone();
        let mut selected = selection.clone();
        match case {
            0 => forged.parent_execution_id = None,
            1 => forged.execution_id = source.execution_id.clone(),
            2 => forged.deadline_tick += 1,
            3 => forged.limits.max_frames += 1,
            4 => forged.limits.max_steps += 1,
            5 => forged.limits.max_storage_bytes += 1,
            6 => forged.limits.max_output_bytes += 1,
            7 => forged.limits.max_effects += 1,
            8 => forged.limits.max_events += 1,
            9 => forged.priority += 1,
            10 => {
                forged.bindings.insert(
                    "cics.execution-context".into(),
                    bounded(b"local".to_vec()).unwrap(),
                );
            }
            11 => selected.generation += 1,
            _ => selected.content_identity = format!("sha256:{}", "0".repeat(64)),
        }
        assert!(
            service
                .with_same_level_program_frame(&source, &forged, &selected, || panic!(
                    "rejected {case} dispatched"
                ))
                .is_err()
        );
        assert_eq!(
            service.lock().unwrap().runs[&root.run_unit_id]
                .current_program
                .effect_invocation,
            source
        );
    }
    let other = service.clone();
    let actor = source.clone();
    let replacement = next.clone();
    let frozen = selection.clone();
    assert_eq!(
        std::thread::spawn(move || other.with_same_level_program_frame(
            &actor,
            &replacement,
            &frozen,
            || panic!("foreign thread dispatched")
        ))
        .join()
        .unwrap(),
        Err(HostProblem::Unauthorized)
    );
    loan.finish().unwrap();
    caller.finish().unwrap();
}

#[test]
fn same_level_replacement_unknown_or_panic_keeps_the_existing_session_fence() {
    for case in 0..3 {
        let (service, root, source, selection) = fixture();
        let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let loan = ProgramLease::acquire(
            &service,
            &mut caller,
            "SOURCE",
            Some(source.artifact.clone()),
        )
        .unwrap();
        service.ensure_run(&source).unwrap();
        let next = target(&source, &selection);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            service.with_same_level_program_frame(&source, &next, &selection, || {
                if case == 1 {
                    panic!("replacement unwound before known result");
                }
                if case == 2 {
                    return Err(HostProblem::Condition {
                        name: "INSTALLED-CALL-ABEND".into(),
                        response: -1,
                        response2: 0,
                    });
                }
                Err(HostProblem::ProviderFailure)
            })
        }));
        if case == 1 {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap(), Err(HostProblem::UnknownOutcome));
        }
        loan.finish().unwrap();
        caller.finish().unwrap();
        assert_eq!(service.ensure_run(&root), Err(HostProblem::UnknownOutcome));
        assert_eq!(service.ensure_run(&next), Err(HostProblem::UnknownOutcome));
    }
}

#[test]
fn nested_same_level_replacements_do_not_add_a_logical_level_or_restore_retired_actors() {
    let (service, root, source, selection) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let loan = ProgramLease::acquire(
        &service,
        &mut caller,
        "SOURCE",
        Some(source.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&source).unwrap();
    let next = target(&source, &selection);
    let mut last = next.clone();
    last.execution_id = ExecutionId::new("last", InvocationLimits::default()).unwrap();
    last.parent_execution_id = Some(next.execution_id.clone());
    service
        .with_same_level_program_frame(&source, &next, &selection, || {
            service.with_same_level_program_frame(&next, &last, &selection, || {
                service.ensure_run(&last).unwrap();
                let state = service.lock().unwrap();
                let task = &state.runs[&root.run_unit_id];
                assert_eq!(task.current_program.logical_level, 2);
                assert_eq!(state.task_dispatch.claims[&root.run_unit_id].loans.len(), 1);
                drop(state);
                bounded(b"final".to_vec())
            })
        })
        .unwrap();
    assert_eq!(
        service.lock().unwrap().runs[&root.run_unit_id]
            .current_program
            .effect_invocation,
        last
    );
    assert_eq!(service.ensure_run(&next), Err(HostProblem::Unauthorized));
    loan.finish().unwrap();
    caller.finish().unwrap();
}

#[test]
fn post_dispatch_state_failure_cannot_become_a_definitive_host_error() {
    let (service, root, source, selection) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let loan = ProgramLease::acquire(
        &service,
        &mut caller,
        "SOURCE",
        Some(source.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&source).unwrap();
    let next = target(&source, &selection);
    let result = service.with_same_level_program_frame(&source, &next, &selection, || {
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _state = service.lock().unwrap();
            panic!("state authority unavailable after target dispatch");
        }));
        assert!(poisoned.is_err());
        Err(HostProblem::Condition {
            name: "INSTALLED-CALL-ABEND".into(),
            response: -1,
            response2: 0,
        })
    });
    assert_eq!(result, Err(HostProblem::UnknownOutcome));
    // An unavailable authority rejects further admission and cannot restore a
    // loan as a known return after losing its protected state.
    assert_eq!(
        service.ensure_run(&root),
        Err(HostProblem::InfrastructureFailure)
    );
    drop(loan);
    drop(caller);
}
