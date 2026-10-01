use super::super::handle_state::{AbendExit, HandleFrame};
use super::*;
use mainframe_env_execution_api::{ArtifactRef, Principal, Selector};
use mainframe_env_store::MemoryStore;

#[test]
fn logical_frame_program_identity_binds_outer_actor_and_occurrence_not_task_counter() {
    let (service, root) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    assert_eq!(
        program_effect_key(&caller, 1),
        Err(HostProblem::MissingIdempotency)
    );
    caller.outer_effect_key = Some("durable-outer".into());
    let first = program_effect_key(&caller, 1).unwrap();
    assert_eq!(
        first.as_str(),
        "cics-program-v2:90b1363efbb546b2b15a2fe9538d28dcc0d49957c1dbb05eeca80d033ad2ca5c"
    );
    caller.host_sequence = 98765;
    assert_eq!(program_effect_key(&caller, 1).unwrap(), first);
    assert_ne!(program_effect_key(&caller, 2).unwrap(), first);
    assert_eq!(program_effect_key(&caller, 0), Err(HostProblem::Malformed));
    caller.outer_effect_key = Some("second-outer".into());
    assert_ne!(program_effect_key(&caller, 1).unwrap(), first);
    caller.outer_effect_key = Some("durable-outer".into());
    caller.current_program.program_occurrence = 1;
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    command.current_program.program_occurrence = 999;
    assert_ne!(program_effect_key(&command, 1).unwrap(), first);
    command.finish().unwrap();
    loan.finish().unwrap();
    assert_eq!(caller.current_program.program_occurrence, 1);
    assert_eq!(program_effect_key(&caller, 1).unwrap(), first);
    caller.finish().unwrap();
}

fn explicit_abend(service: &CicsService, run: &mut Run, cancel: bool) -> CicsResponse {
    let mut arguments = BTreeMap::from([(
        "ABCODE".into(),
        BoundedPayload::new(
            "mainframe-env.cics.literal@1",
            b"U777".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap(),
    )]);
    if cancel {
        arguments.insert("OPTION.CANCEL".into(), bounded(Vec::new()).unwrap());
    }
    let request = CicsRequest {
        operation: CicsOperation::Abend,
        arguments,
        condition_policy: CicsConditionPolicy::Default,
        mutation: None,
    };
    super::super::task_control::invoke(service, run, &request, 100).unwrap()
}

fn known_child_abend() -> Result<HostResult, HostProblem> {
    Err(HostProblem::Condition {
        name: "INSTALLED-CALL-ABEND".into(),
        response: -1,
        response2: 0,
    })
}

fn unmatched_pop(
    service: &CicsService,
    run: &mut Run,
    policy: CicsConditionPolicy,
) -> CicsResponse {
    super::super::task_control::invoke(
        service,
        run,
        &CicsRequest {
            operation: CicsOperation::PopHandle,
            arguments: BTreeMap::new(),
            condition_policy: policy,
            mutation: None,
        },
        100,
    )
    .unwrap()
}

#[test]
fn logical_frame_default_pop_preserves_current_level_condition_precedence() {
    for case in 0..7 {
        let (service, root) = fixture();
        let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
        let actor = child(&root, 1);
        let loan =
            ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
                .unwrap();
        service.ensure_run(&actor).unwrap();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        let policy = match case {
            0 => CicsConditionPolicy::NoHandle,
            1 => CicsConditionPolicy::Respond {
                response_field: "RESP-X".into(),
                response2_field: None,
            },
            2 => {
                command.ignored_conditions.insert("INVREQ".into());
                CicsConditionPolicy::Default
            }
            3 => {
                command.handlers.insert("INVREQ".into(), "COND-EXIT".into());
                CicsConditionPolicy::Default
            }
            4 => {
                command.handlers.insert("ERROR".into(), "COND-EXIT".into());
                CicsConditionPolicy::Default
            }
            5 => {
                command.abend_handler = Some(AbendExit::Label("LOCAL-EXIT".into()));
                CicsConditionPolicy::Default
            }
            _ => {
                command.ignored_conditions.insert("ERROR".into());
                CicsConditionPolicy::Default
            }
        };
        let response = unmatched_pop(&service, &mut command, policy);
        assert_eq!(response.condition, "INVREQ");
        assert_eq!(response.response, 16);
        assert!(command.program_abend.is_none());
        assert!(!response.outputs.contains_key("ABEND.DEFAULT"));
        assert_eq!(
            response.disposition,
            match case {
                0 | 1 => CicsDisposition::Complete,
                2 | 6 => CicsDisposition::Ignored,
                _ => CicsDisposition::Handler,
            }
        );
        if case == 5 {
            assert_eq!(response.target.as_deref(), Some("LOCAL-EXIT"));
            assert!(command.abend_handler.is_none());
            assert_eq!(
                command.cancelled_abend_handler,
                Some(AbendExit::Label("LOCAL-EXIT".into()))
            );
        }
        command.finish().unwrap();
        loan.finish().unwrap();
        assert_eq!(
            caller.abend_handler,
            Some(AbendExit::Label("ROOT-EXIT".into()))
        );
        caller.finish().unwrap();
    }
}

#[test]
fn logical_frame_default_pop_requires_known_result_and_valid_replay_origin() {
    let (service, root) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let default = unmatched_pop(&service, &mut command, CicsConditionPolicy::Default);
    assert_eq!(default.disposition, CicsDisposition::Abended);
    assert_eq!(default.outputs["ABEND.DEFAULT"].bytes(), b"POP-HANDLE");
    assert!(command.latest_abend.is_none());
    assert!(command.program_abend.is_some());
    command.finish().unwrap();
    loan.finish().unwrap();
    let before = HandleState::from_run(&caller);
    for problem in [
        HostProblem::UnknownOutcome,
        HostProblem::Cancelled,
        HostProblem::TimedOut,
        HostProblem::Condition {
            name: "OTHER-FAILURE".into(),
            response: -1,
            response2: 0,
        },
    ] {
        assert!(matches!(
            super::super::program_abend::unwind(&service, &mut caller, &Err(problem)),
            Err(HostProblem::UnknownOutcome)
        ));
        assert!(caller.program_abend.is_some());
        assert_eq!(HandleState::from_run(&caller), before);
    }
    let original = caller.program_abend.clone().unwrap();
    for case in 0..6 {
        let mut invalid = original.clone();
        match case {
            0 => {
                invalid.response.outputs.remove("ABEND.DEFAULT");
            }
            1 => invalid.cancel_exits = true,
            2 => invalid.response.target = Some("FORGED".into()),
            3 => invalid.response.disposition = CicsDisposition::Handler,
            4 => invalid.response.response2 = 1,
            _ => invalid.response.payload = bounded(b"U777".to_vec()).unwrap(),
        }
        caller.program_abend = Some(invalid);
        assert!(matches!(
            super::super::program_abend::unwind(&service, &mut caller, &known_child_abend()),
            Err(HostProblem::UnknownOutcome)
        ));
        assert_eq!(HandleState::from_run(&caller), before);
    }
    caller.program_abend = Some(original);
    let response = super::super::program_abend::unwind(&service, &mut caller, &known_child_abend())
        .unwrap()
        .unwrap();
    assert_eq!(response.target.as_deref(), Some("ROOT-EXIT"));
    assert!(response.outputs["ABEND.CODE"].bytes().is_empty());
    assert!(!response.outputs.contains_key("ABEND.DUMP"));
    assert_eq!(
        super::super::program_abend::validate_replay(&response),
        Ok(true)
    );
    for case in 0..12 {
        let mut invalid = response.clone();
        match case {
            0 => invalid.condition = "ERROR".into(),
            1 => invalid.response = 27,
            2 => invalid.response2 = 1,
            3 => invalid.disposition = CicsDisposition::Complete,
            4 => invalid.target = None,
            5 => invalid.target = Some("X".repeat(129)),
            6 => {
                invalid.outputs.remove("ABEND.CODE");
            }
            7 => {
                invalid.outputs.insert(
                    "ABEND.CODE".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.abend-code@1",
                        b"U777".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            8 => {
                invalid.outputs.insert(
                    "ABEND.DEFAULT".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.default-abend@1",
                        b"OTHER".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            9 => {
                invalid.outputs.insert(
                    "ABEND.DEFAULT".into(),
                    bounded(b"POP-HANDLE".to_vec()).unwrap(),
                );
            }
            10 => {
                invalid.outputs.insert(
                    "ABEND.DUMP".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.abend-dump@1",
                        b"suppressed".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            _ => invalid.payload = bounded(b"unexpected".to_vec()).unwrap(),
        }
        assert_eq!(
            super::super::program_abend::validate_replay(&invalid),
            Err(HostProblem::ProviderFailure),
            "case {case}"
        );
    }
    caller.finish().unwrap();
}

#[test]
fn logical_frame_ancestor_abend_retains_task_enqueue_but_cancel_releases_it() {
    for cancel in [false, true] {
        let (service, root) = fixture();
        let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
        let request = CicsRequest {
            operation: CicsOperation::Enq,
            arguments: BTreeMap::from([
                (
                    "RESOURCE".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.storage-identity@1",
                        b"FRAMELOCK".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
                ("OPTION.TASK".into(), bounded(Vec::new()).unwrap()),
            ]),
            condition_policy: CicsConditionPolicy::Default,
            mutation: Some(mainframe_env_host_api::Mutation {
                sequence: 5,
                idempotency_key: IdempotencyKey::new("frame-enq", InvocationLimits::default())
                    .unwrap(),
                transaction: Some("MENU".into()),
            }),
        };
        assert_eq!(
            super::super::task_control::invoke(&service, &mut caller, &request, 100)
                .unwrap()
                .response,
            0
        );
        let before = service
            .store
            .list_provider_state("cics-enqueue-v1", 8)
            .unwrap();
        assert_eq!(before.len(), 1);
        let actor = child(&root, 1);
        let loan =
            ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
                .unwrap();
        service.ensure_run(&actor).unwrap();
        let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        explicit_abend(&service, &mut command, cancel);
        command.finish().unwrap();
        loan.finish().unwrap();
        let response =
            super::super::program_abend::unwind(&service, &mut caller, &known_child_abend())
                .unwrap()
                .unwrap();
        let after = service
            .store
            .list_provider_state("cics-enqueue-v1", 8)
            .unwrap();
        if cancel {
            assert_eq!(response.disposition, CicsDisposition::Abended);
            assert!(after.is_empty());
        } else {
            assert_eq!(response.disposition, CicsDisposition::Handler);
            assert_eq!(after, before);
        }
        caller.finish().unwrap();
    }
}

#[test]
fn logical_frame_ancestor_abend_selects_nearest_active_level_not_suspended_stack() {
    for active_middle in [false, true] {
        let (service, root) = fixture();
        let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
        let actor = child(&root, 1);
        let outer =
            ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
                .unwrap();
        service.ensure_run(&actor).unwrap();
        let mut middle = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        middle.handle_stack.push(HandleFrame {
            handlers: BTreeMap::new(),
            aid_handlers: BTreeMap::new(),
            ignored_conditions: BTreeSet::new(),
            abend_handler: Some(AbendExit::Label("SUSPENDED-EXIT".into())),
            cancelled_abend_handler: None,
        });
        if active_middle {
            middle.abend_handler = Some(AbendExit::Label("MIDDLE-EXIT".into()));
        }
        let grand = child(&actor, 2);
        let inner =
            ProgramLease::acquire(&service, &mut middle, "CHILD", Some(grand.artifact.clone()))
                .unwrap();
        service.ensure_run(&grand).unwrap();
        let mut grand_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        assert_eq!(
            explicit_abend(&service, &mut grand_command, false).disposition,
            CicsDisposition::Abended
        );
        grand_command.finish().unwrap();
        inner.finish().unwrap();
        let result =
            super::super::program_abend::unwind(&service, &mut middle, &known_child_abend())
                .unwrap()
                .unwrap();
        if active_middle {
            assert_eq!(result.target.as_deref(), Some("MIDDLE-EXIT"));
            assert_eq!(result.disposition, CicsDisposition::Handler);
            assert!(middle.abend_handler.is_none());
            assert_eq!(
                middle.cancelled_abend_handler,
                Some(AbendExit::Label("MIDDLE-EXIT".into()))
            );
            assert!(middle.program_abend.is_none());
        } else {
            assert_eq!(result.disposition, CicsDisposition::Abended);
            assert!(middle.program_abend.is_some());
        }
        middle.finish().unwrap();
        outer.finish().unwrap();
        if active_middle {
            assert_eq!(
                caller.abend_handler,
                Some(AbendExit::Label("ROOT-EXIT".into()))
            );
            assert!(caller.program_abend.is_none());
        } else {
            let result =
                super::super::program_abend::unwind(&service, &mut caller, &known_child_abend())
                    .unwrap()
                    .unwrap();
            assert_eq!(result.target.as_deref(), Some("ROOT-EXIT"));
            assert_eq!(result.outputs["ABEND.CODE"].bytes(), b"U777");
            assert_eq!(caller.latest_abend.as_ref().unwrap().original_code, b"U777");
            assert!(caller.abend_handler.is_none());
        }
        caller.finish().unwrap();
    }
}

#[test]
fn logical_frame_abend_cancel_bypasses_and_clears_ancestor_exits() {
    let (service, root) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
    caller.handle_stack.push(HandleFrame {
        handlers: BTreeMap::new(),
        aid_handlers: BTreeMap::new(),
        ignored_conditions: BTreeSet::new(),
        abend_handler: Some(AbendExit::Label("SUSPENDED-EXIT".into())),
        cancelled_abend_handler: None,
    });
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    command.abend_handler = Some(AbendExit::Label("CHILD-EXIT".into()));
    assert_eq!(
        explicit_abend(&service, &mut command, true).disposition,
        CicsDisposition::Abended
    );
    command.finish().unwrap();
    loan.finish().unwrap();
    let result = super::super::program_abend::unwind(&service, &mut caller, &known_child_abend())
        .unwrap()
        .unwrap();
    assert_eq!(result.disposition, CicsDisposition::Abended);
    assert_eq!(result.target, None);
    assert!(caller.abend_handler.is_none());
    assert!(caller.cancelled_abend_handler.is_none());
    assert!(
        caller
            .handle_stack
            .iter()
            .all(|frame| frame.abend_handler.is_none() && frame.cancelled_abend_handler.is_none())
    );
    assert!(caller.program_abend.is_none());
    caller.finish().unwrap();
}

#[test]
fn logical_frame_local_abend_exit_precedes_ancestor_and_does_not_leak() {
    let (service, root) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    command.abend_handler = Some(AbendExit::Label("CHILD-EXIT".into()));
    let result = explicit_abend(&service, &mut command, false);
    assert_eq!(result.disposition, CicsDisposition::Handler);
    assert_eq!(result.target.as_deref(), Some("CHILD-EXIT"));
    assert!(command.abend_handler.is_none());
    assert!(command.program_abend.is_none());
    command.finish().unwrap();
    loan.finish().unwrap();
    assert_eq!(
        caller.abend_handler,
        Some(AbendExit::Label("ROOT-EXIT".into()))
    );
    assert!(caller.program_abend.is_none());
    assert_eq!(caller.latest_abend.as_ref().unwrap().code, b"U777");
    caller.finish().unwrap();
}

#[test]
fn logical_frame_abend_replay_metadata_rejects_partial_forged_and_unbounded_control() {
    let (service, root) = fixture();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    explicit_abend(&service, &mut command, false);
    command.finish().unwrap();
    loan.finish().unwrap();
    let response = super::super::program_abend::unwind(&service, &mut caller, &known_child_abend())
        .unwrap()
        .unwrap();
    assert_eq!(
        super::super::program_abend::validate_replay(&response),
        Ok(true)
    );
    for case in 0..8 {
        let mut forged = response.clone();
        match case {
            0 => {
                forged.outputs.remove("ABEND.DUMP");
            }
            1 => {
                forged
                    .outputs
                    .insert("ABEND.CODE".into(), bounded(b"U777".to_vec()).unwrap());
            }
            2 => {
                forged.outputs.insert(
                    "ABEND.CODE".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.abend-code@1",
                        b"U7777".to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            3 => {
                forged.target = None;
            }
            4 => {
                forged.disposition = CicsDisposition::Complete;
            }
            5 => {
                forged
                    .outputs
                    .insert("COMMAREA".into(), bounded(b"fake".to_vec()).unwrap());
            }
            6 => {
                forged.payload = bounded(b"fake".to_vec()).unwrap();
            }
            7 => {
                forged.condition = "NORMAL".into();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            super::super::program_abend::validate_replay(&forged),
            Err(HostProblem::ProviderFailure),
            "case {case}"
        );
    }
    caller.finish().unwrap();
}

#[test]
fn logical_frame_abend_never_converts_unknown_executor_result_to_handler_success() {
    let (service, root) = fixture();
    let before = service
        .store
        .get_provider_state("cics-session", "FRAME-SESSION")
        .unwrap();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    caller.abend_handler = Some(AbendExit::Label("ROOT-EXIT".into()));
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    explicit_abend(&service, &mut command, false);
    command.finish().unwrap();
    loan.finish().unwrap();
    assert_eq!(
        super::super::program_abend::unwind(
            &service,
            &mut caller,
            &Err(HostProblem::UnknownOutcome)
        ),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        caller.abend_handler,
        Some(AbendExit::Label("ROOT-EXIT".into()))
    );
    assert_eq!(
        service
            .store
            .get_provider_state("cics-session", "FRAME-SESSION")
            .unwrap(),
        before
    );
    caller.finish().unwrap();
    assert!(matches!(
        CommandLease::acquire(&service, &root.run_unit_id),
        Err(HostProblem::UnknownOutcome)
    ));
}

fn fixture() -> (Arc<CicsService>, Invocation) {
    let service = crate::service::tests::service(Arc::new(MemoryStore::new(
        mainframe_env_store::StoreLimits::default(),
    )));
    let root = crate::service::tests::invocation();
    let session = SessionId::new("FRAME-SESSION", 64).unwrap();
    service
        .launch_terminal(
            root.clone(),
            &session,
            "MENU",
            24,
            80,
            "frame-csrf",
            1,
            10_000,
        )
        .unwrap();
    (service, root)
}

#[test]
fn logical_frame_terminal_restore_cannot_replace_a_live_program_loan() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(
        &service,
        &mut command,
        "CHILD",
        Some(actor.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&actor).unwrap();
    let session = SessionId::new("FRAME-SESSION", 64).unwrap();
    assert_eq!(
        service.restore_terminal_program_run(
            root.clone(),
            actor.clone(),
            &session,
            "MENU",
            Vec::new(),
            2
        ),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        service.lock().unwrap().runs[&root.run_unit_id]
            .current_program
            .effect_invocation,
        actor
    );
    loan.finish().unwrap();
    command.finish().unwrap();
}

fn child(parent: &Invocation, ordinal: usize) -> Invocation {
    let mut child = parent.clone();
    child.parent_execution_id = Some(parent.execution_id.clone());
    child.execution_id =
        ExecutionId::new(format!("child-{ordinal}"), InvocationLimits::default()).unwrap();
    child.selector = Selector::new("program:CHILD", InvocationLimits::default()).unwrap();
    child.artifact = ArtifactRef::new("child-artifact", InvocationLimits::default()).unwrap();
    child
}

#[test]
fn logical_frame_restores_caller_but_keeps_shared_task_updates() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    command.handlers.insert("ERROR".into(), "CALLER".into());
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(
        &service,
        &mut command,
        "CHILD",
        Some(actor.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&actor).unwrap();
    {
        let mut child_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
        assert!(child_command.handlers.is_empty());
        assert!(child_command.handle_stack.is_empty());
        assert_eq!(child_command.invocation.execution_id, root.execution_id);
        assert_eq!(
            child_command.current_program.effect_invocation.execution_id,
            actor.execution_id
        );
        let previous = HandleState::from_run(&child_command);
        child_command
            .handlers
            .insert("ERROR".into(), "CHILD".into());
        child_command
            .current_records
            .insert("DATA".into(), b"child-update".to_vec());
        child_command.undo.push(DatasetUndo::Delete {
            dataset: DatasetName::new("USER.DATA", 44).unwrap(),
            key: b"KEY".to_vec(),
        });
        let before = service
            .store
            .get_provider_state("cics-session", "FRAME-SESSION")
            .unwrap();
        super::super::handle_state::persist_handle_state(&service, &mut child_command, previous)
            .unwrap();
        assert_eq!(
            service
                .store
                .get_provider_state("cics-session", "FRAME-SESSION")
                .unwrap(),
            before
        );
        // Drop restores the task even on an early command error path.
    }
    loan.finish().unwrap();
    assert_eq!(command.handlers["ERROR"], "CALLER");
    assert_eq!(command.current_records["DATA"], b"child-update");
    assert_eq!(command.undo.len(), 1);
    assert_eq!(
        command.current_program.effect_invocation.execution_id,
        root.execution_id
    );
    assert_eq!(command.current_program.logical_level, 1);
    command.finish().unwrap();
    let state = service.lock().unwrap();
    assert_eq!(state.runs.len(), 1);
    assert!(state.task_dispatch.claims.is_empty());
}

#[test]
fn logical_frame_admission_rejects_foreign_and_widened_children() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let valid = child(&root, 1);
    let loan = ProgramLease::acquire(
        &service,
        &mut command,
        "CHILD",
        Some(valid.artifact.clone()),
    )
    .unwrap();
    for case in 0..10 {
        let mut invalid = valid.clone();
        let limits = InvocationLimits::default();
        match case {
            0 => invalid.parent_execution_id = None,
            1 => {
                invalid.principal = Principal::new(
                    PrincipalId::new("OTHER", limits).unwrap(),
                    root.principal.grants().clone(),
                    limits,
                )
                .unwrap()
            }
            2 => invalid.deadline_tick += 1,
            3 => invalid.limits.max_frames += 1,
            4 => invalid.selector = Selector::new("program:OTHER", limits).unwrap(),
            5 => invalid.artifact = ArtifactRef::new("foreign-artifact", limits).unwrap(),
            6 => {
                invalid.provider_generations.insert(
                    CapabilityId::new("host.cics.execute", limits).unwrap(),
                    "foreign-generation".into(),
                );
            }
            7 => {
                invalid.bindings.insert(
                    "cics.session".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.session@1",
                        b"other-session".to_vec(),
                        limits,
                    )
                    .unwrap(),
                );
            }
            8 => {
                invalid.cancellation_probe =
                    Some(mainframe_env_execution_api::CancellationProbe::new());
            }
            9 => invalid.limits.max_effects += 1,
            _ => unreachable!(),
        }
        assert_eq!(
            service.ensure_run(&invalid),
            Err(HostProblem::Unauthorized),
            "case {case}"
        );
    }
    service.ensure_run(&valid).unwrap();
    let mut conflict = valid.clone();
    conflict.execution_id =
        ExecutionId::new("different-child", InvocationLimits::default()).unwrap();
    assert_eq!(
        service.ensure_run(&conflict),
        Err(HostProblem::IdempotencyConflict)
    );
    loan.finish().unwrap();
    command.finish().unwrap();
    assert_eq!(service.lock().unwrap().sessions.len(), 1);
}

#[test]
fn logical_frame_fences_other_threads_and_duplicate_registration() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let valid = child(&root, 1);
    let loan = ProgramLease::acquire(
        &service,
        &mut command,
        "CHILD",
        Some(valid.artifact.clone()),
    )
    .unwrap();
    let foreign_service = service.clone();
    let foreign_actor = valid.clone();
    assert_eq!(
        std::thread::spawn(move || foreign_service.ensure_run(&foreign_actor))
            .join()
            .unwrap(),
        Err(HostProblem::Unauthorized)
    );
    let session = SessionId::new("FRAME-SESSION", 64).unwrap();
    let before = service
        .store
        .get_provider_state("cics-session", session.as_str())
        .unwrap();
    assert_eq!(
        service.suspend_terminal_run(&session, root.principal.id(), 2),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        service.discard_terminal_run_if_present(&session, root.principal.id(), 2),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        service.complete_terminal_run(&session, root.principal.id(), 2),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        service.restore_terminal_run(root.clone(), &session, "MENU", Vec::new(), 2),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        service.disconnect_terminal(&session, root.principal.id(), "frame-csrf", 2),
        Err(HostProblem::IdempotencyConflict)
    );
    assert_eq!(
        service
            .store
            .get_provider_state("cics-session", session.as_str())
            .unwrap(),
        before
    );
    assert_eq!(
        service.register_run(root.clone(), &session, "MENU", "ME01", "S001"),
        Err(HostProblem::IdempotencyConflict)
    );
    service.ensure_run(&valid).unwrap();
    drop(loan);
    drop(command);
    let state = service.lock().unwrap();
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.runs.len(), 1);
    assert!(state.task_dispatch.claims.is_empty());
}

fn descend(service: &CicsService, task: &mut Run, level: usize) {
    let actor = child(&task.current_program.effect_invocation, level);
    if level == MAX_PROGRAM_LEVELS {
        assert!(matches!(
            ProgramLease::acquire(service, task, "CHILD", Some(actor.artifact.clone())),
            Err(HostProblem::ResourceExhausted)
        ));
        return;
    }
    let loan = ProgramLease::acquire(service, task, "CHILD", Some(actor.artifact.clone())).unwrap();
    service.ensure_run(&actor).unwrap();
    let mut command = CommandLease::acquire(service, &actor.run_unit_id).unwrap();
    assert_eq!(command.current_program.logical_level, (level + 1) as u32);
    descend(service, &mut command, level + 1);
    command.finish().unwrap();
    loan.finish().unwrap();
}

#[test]
fn logical_frame_held_task_still_counts_against_run_capacity() {
    let (mut service, root) = fixture();
    Arc::get_mut(&mut service).unwrap().limits.max_runs = 1;
    let command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let session = SessionId::new("second-session", 64).unwrap();
    service.create_session(&session, 24, 80).unwrap();
    let mut second = root.clone();
    second.run_unit_id = RunUnitId::new("second-run", InvocationLimits::default()).unwrap();
    assert_eq!(
        service.register_run(second, &session, "MENU", "ME01", "S001"),
        Err(HostProblem::ResourceExhausted)
    );
    command.finish().unwrap();
    assert_eq!(service.lock().unwrap().runs.len(), 1);
}

#[test]
fn logical_frame_uncertain_session_fence_retains_resources_and_is_task_scoped() {
    let (service, root) = fixture();
    {
        let mut state = service.lock().unwrap();
        let run = state.runs.get_mut(&root.run_unit_id).unwrap();
        run.current_records
            .insert("DATA".into(), b"held-record".to_vec());
        run.handlers.insert("ERROR".into(), "SAVED".into());
        state
            .task_dispatch
            .uncertain_sessions
            .insert("FRAME-SESSION".into());
    }
    let session = SessionId::new("FRAME-SESSION", 64).unwrap();
    let saved = service
        .store
        .get_provider_state("cics-session", session.as_str())
        .unwrap();
    assert!(matches!(
        CommandLease::acquire(&service, &root.run_unit_id),
        Err(HostProblem::UnknownOutcome)
    ));
    assert_eq!(
        service.restore_terminal_run(root.clone(), &session, "MENU", Vec::new(), 2),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        service.complete_terminal_run(&session, root.principal.id(), 2),
        Err(HostProblem::UnknownOutcome)
    );
    assert_eq!(
        service
            .store
            .get_provider_state("cics-session", session.as_str())
            .unwrap(),
        saved
    );
    let mut other = root.clone();
    other.run_unit_id = RunUnitId::new("unrelated-run", InvocationLimits::default()).unwrap();
    other.execution_id =
        ExecutionId::new("unrelated-execution", InvocationLimits::default()).unwrap();
    let other_session = SessionId::new("unrelated-session", 64).unwrap();
    service.create_session(&other_session, 24, 80).unwrap();
    service
        .register_run(other.clone(), &other_session, "MENU", "ME01", "S001")
        .unwrap();
    CommandLease::acquire(&service, &other.run_unit_id)
        .unwrap()
        .finish()
        .unwrap();
    let state = service.lock().unwrap();
    assert_eq!(state.runs.len(), 2);
    assert_eq!(
        state.runs[&root.run_unit_id].current_records["DATA"],
        b"held-record"
    );
    assert_eq!(state.runs[&root.run_unit_id].handlers["ERROR"], "SAVED");
    assert_eq!(state.task_dispatch.uncertain_sessions.len(), 1);
    assert!(state.task_dispatch.claims.is_empty());
    assert!(state.task_dispatch.cleaning_sessions.is_empty());
}

#[test]
fn logical_frame_recursion_is_bounded_and_restores_every_lease() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    descend(&service, &mut command, 1);
    command.finish().unwrap();
    let state = service.lock().unwrap();
    assert_eq!(state.runs.len(), 1);
    assert_eq!(
        state.runs[&root.run_unit_id].current_program.logical_level,
        1
    );
    assert!(state.task_dispatch.claims.is_empty());
}

#[test]
fn logical_frame_live_cancellation_restores_caller_without_session_mutation() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let probe = mainframe_env_execution_api::CancellationProbe::new();
    command.current_program.effect_invocation.cancellation_probe = Some(probe.clone());
    command.handlers.insert("ERROR".into(), "CALLER".into());
    let mut actor = child(&command.current_program.effect_invocation, 1);
    actor.deadline_tick -= 1;
    let before = service
        .store
        .get_provider_state("cics-session", "FRAME-SESSION")
        .unwrap();
    {
        let _loan = ProgramLease::acquire(
            &service,
            &mut command,
            "CHILD",
            Some(actor.artifact.clone()),
        )
        .unwrap();
        service.ensure_run(&actor).unwrap();
        probe.request();
        assert_eq!(service.ensure_run(&actor), Err(HostProblem::Cancelled));
    }
    assert_eq!(command.handlers["ERROR"], "CALLER");
    assert_eq!(command.current_program.logical_level, 1);
    assert_eq!(
        service
            .store
            .get_provider_state("cics-session", "FRAME-SESSION")
            .unwrap(),
        before
    );
    command.finish().unwrap();
    let state = service.lock().unwrap();
    assert_eq!(state.runs.len(), 1);
    assert!(state.task_dispatch.claims.is_empty());
}

#[test]
fn logical_frame_narrowed_budget_bounds_further_reentry() {
    let (service, root) = fixture();
    let mut command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let mut actor = child(&root, 1);
    actor.limits.max_frames = 2;
    let loan = ProgramLease::acquire(
        &service,
        &mut command,
        "CHILD",
        Some(actor.artifact.clone()),
    )
    .unwrap();
    service.ensure_run(&actor).unwrap();
    let mut child_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let descendant = child(&actor, 2);
    assert!(matches!(
        ProgramLease::acquire(
            &service,
            &mut child_command,
            "CHILD",
            Some(descendant.artifact)
        ),
        Err(HostProblem::ResourceExhausted)
    ));
    child_command.finish().unwrap();
    loan.finish().unwrap();
    command.finish().unwrap();
    assert!(service.lock().unwrap().task_dispatch.claims.is_empty());
}

#[test]
fn logical_frame_bare_return_preserves_claimed_task_continuation() {
    let (service, root) = fixture();
    let continuation = DurableContinuation {
        transaction: "NEXT".into(),
        commarea: b"caller-state".to_vec(),
        claimed_by: Some(root.run_unit_id.as_str().into()),
        effect_key: "earlier-return".into(),
        version: 1,
    };
    service
        .persist_continuation("FRAME-SESSION", &continuation, None)
        .unwrap();
    service
        .lock()
        .unwrap()
        .continuations
        .insert("FRAME-SESSION".into(), continuation);
    let before = service
        .store
        .get_provider_state("cics-continuation", "FRAME-SESSION")
        .unwrap();
    let mut caller = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    let actor = child(&root, 1);
    let loan = ProgramLease::acquire(&service, &mut caller, "CHILD", Some(actor.artifact.clone()))
        .unwrap();
    service.ensure_run(&actor).unwrap();
    let child_command = CommandLease::acquire(&service, &root.run_unit_id).unwrap();
    assert_eq!(child_command.session, "FRAME-SESSION");
    assert_eq!(
        service.lock().unwrap().continuations["FRAME-SESSION"]
            .claimed_by
            .as_deref(),
        Some(child_command.invocation.run_unit_id.as_str())
    );
    let request = CicsRequest {
        operation: CicsOperation::Return,
        arguments: BTreeMap::new(),
        condition_policy: mainframe_env_host_api::CicsConditionPolicy::Default,
        mutation: Some(Mutation {
            sequence: 1,
            idempotency_key: IdempotencyKey::new("child-return", InvocationLimits::default())
                .unwrap(),
            transaction: Some("MENU".into()),
        }),
    };
    let response = super::super::task_return::invoke(&service, &child_command, &request).unwrap();
    assert_eq!(response.disposition, CicsDisposition::Returned);
    assert_eq!(
        service
            .store
            .get_provider_state("cics-continuation", "FRAME-SESSION")
            .unwrap(),
        before
    );
    assert!(
        service
            .lock()
            .unwrap()
            .continuations
            .contains_key("FRAME-SESSION")
    );
    child_command.finish().unwrap();
    loan.finish().unwrap();
    // Root RETURN, unlike child RETURN, retires the claimed task continuation.
    assert_eq!(caller.session, "FRAME-SESSION");
    super::super::task_return::invoke(&service, &caller, &request).unwrap();
    assert_eq!(
        service
            .store
            .get_provider_state("cics-continuation", "FRAME-SESSION")
            .unwrap(),
        None
    );
    caller.finish().unwrap();
}
