use super::*;
use mainframe_env_ir::{IrLimits, ModuleBuilder};

fn machine(terminal: &str, args: &[&str]) -> ReferenceMachine {
    machine_with_body(terminal, args, "continue", &[])
}

fn machine_with_body(
    terminal: &str,
    args: &[&str],
    body: &str,
    body_args: &[&str],
) -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    for (name, attributes) in [
        (
            "config",
            BTreeMap::from([
                ("arithmetic_mode".into(), Attribute::Text("extended".into())),
                (
                    "program_lifecycle".into(),
                    Attribute::Text("retained@1".into()),
                ),
                ("entry_formals_v1".into(), Attribute::Text(String::new())),
            ]),
        ),
        (
            body,
            body_args
                .iter()
                .enumerate()
                .map(|(i, arg)| (format!("arg_{i:03}"), Attribute::Text((*arg).into())))
                .collect(),
        ),
        (
            terminal,
            args.iter()
                .enumerate()
                .map(|(i, arg)| (format!("arg_{i:03}"), Attribute::Text((*arg).into())))
                .collect(),
        ),
        ("halt", BTreeMap::new()),
    ] {
        builder
            .add_operation(
                block,
                OperationIdentity::new(NAMESPACE, name, 1).unwrap(),
                Vec::new(),
                0,
                attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
    }
    let bytes = mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
        .unwrap();
    ReferenceMachine::from_binary(
        &bytes,
        super::super::tests::invocation(),
        CodecLimits::default(),
    )
    .unwrap()
}

fn drive(machine: &mut ReferenceMachine) -> MachineDrive<EffectRequest> {
    machine.drive(MachineResume::Start, Quantum::new(20, 4096).unwrap())
}

#[test]
fn executed_goback_attests_exact_live_context_and_readonly_getters() {
    let mut m = machine("go_back", &[]);
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    let before = m.checkpoint().unwrap();
    let state = m.retained_program_state().unwrap();
    let witness = m
        .attest_installed_program_return()
        .expect("executed normal return");
    assert_eq!(witness.kind(), InstalledProgramReturnKind::Goback);
    assert_eq!(witness.program_counter(), 2);
    assert_eq!(witness.executed_steps(), 3);
    assert_eq!(witness.invocation(), &m.invocation);
    assert_eq!(
        m.attest_installed_program_return().unwrap().kind(),
        witness.kind()
    );
    assert_eq!(m.checkpoint().unwrap(), before);
    assert_eq!(m.retained_program_state().unwrap(), state);
}

#[test]
fn executed_exit_program_attests() {
    let mut m = machine("exit", &["PROGRAM"]);
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    let witness = m
        .attest_installed_program_return()
        .expect("executed EXIT PROGRAM");
    assert_eq!(witness.kind(), InstalledProgramReturnKind::ExitProgram);
    assert_eq!(witness.program_counter(), 2);
    assert_eq!(witness.executed_steps(), 3);
}

#[test]
fn constructor_and_continue_at_unexecuted_return_pc_do_not_attest() {
    let mut m = machine("go_back", &[]);
    assert!(m.attest_installed_program_return().is_err());
    assert!(matches!(
        m.drive(MachineResume::Start, Quantum::new(2, 4096).unwrap()),
        MachineDrive::Continue
    ));
    assert_eq!(m.pc, 2);
    assert!(m.attest_installed_program_return().is_err());
}

fn returned() -> ReferenceMachine {
    let mut m = machine("go_back", &[]);
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    assert!(m.attest_installed_program_return().is_ok());
    m
}

#[test]
fn checkpoint_before_and_after_return_never_restore_a_witness() {
    let mut m = machine("go_back", &[]);
    assert!(matches!(
        m.drive(MachineResume::Start, Quantum::new(2, 4096).unwrap()),
        MachineDrive::Continue
    ));
    let before = m.checkpoint().unwrap();
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    let after = m.checkpoint().unwrap();
    for checkpoint in [&before, &after] {
        let mut cold = machine("go_back", &[]);
        cold.restore_checkpoint(checkpoint).unwrap();
        assert!(cold.attest_installed_program_return().is_err());
        let mut warm = returned();
        warm.restore_checkpoint(checkpoint).unwrap();
        assert!(warm.attest_installed_program_return().is_err());
    }
}

#[test]
fn successful_legacy_snapshot_restore_also_clears_witness() {
    for schema in 1..=12 {
        let mut m = returned();
        let mut snapshot = m.snapshot();
        snapshot.schema_version = schema;
        m.restore(snapshot).unwrap();
        assert!(
            m.attest_installed_program_return().is_err(),
            "schema{schema}"
        );
    }
}

#[test]
fn invalid_early_and_late_restore_preserves_machine_and_live_marker() {
    for fault in [
        "version",
        "pc",
        "file-status",
        "dynamic",
        "search",
        "sort-pc",
        "sort-phase",
        "sort-io",
        "freed",
        "linkage",
    ] {
        let mut m = returned();
        let before = m.snapshot();
        let checkpoint = m.checkpoint().unwrap();
        let mut invalid = before.clone();
        // Earlier fields would change if restore validated a later fault only
        // after mutation. The successful-return marker must survive this error.
        invalid.program_counter = 0;
        invalid.output = b"must not be installed".to_vec();
        match fault {
            "version" => invalid.schema_version = 99,
            "pc" => invalid.program_counter = usize::MAX,
            "file-status" => invalid.last_file_status = "bad".into(),
            "dynamic" => {
                invalid.dynamic_lengths.insert("FOREIGN".into(), 1);
            }
            "search" => {
                invalid.search_results.insert(usize::MAX, true);
            }
            "sort-pc" => {
                invalid.active_sort_procedure = Some((usize::MAX, "F".into(), 0, Vec::new()))
            }
            "sort-phase" => invalid.active_sort_procedure = Some((0, "F".into(), 7, Vec::new())),
            "sort-io" => {
                invalid.sort_io = Some((
                    usize::MAX,
                    "F".into(),
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    0,
                    0,
                ))
            }
            "freed" => {
                invalid.freed_allocations.insert(usize::MAX);
            }
            "linkage" => {
                invalid.linkage_addresses.insert("FOREIGN".into(), None);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            m.restore(invalid),
            Err(MachineProblem::IncompatibleSnapshot),
            "{fault}"
        );
        assert_eq!(m.snapshot(), before, "{fault}");
        assert_eq!(m.checkpoint().unwrap(), checkpoint, "{fault}");
        assert_eq!(
            m.attest_installed_program_return()
                .unwrap()
                .program_counter(),
            2,
            "{fault}"
        );
    }
    let mut m = returned();
    let before = m.snapshot();
    let malformed = BoundedPayload::new(
        "mainframe-env.reference-machine-checkpoint@12",
        vec![0xff],
        InvocationLimits::default(),
    )
    .unwrap();
    assert!(m.restore_checkpoint(&malformed).is_err());
    assert_eq!(m.snapshot(), before);
    assert!(m.attest_installed_program_return().is_ok());
}

#[test]
fn subsequent_cancel_timeout_failure_and_continue_clear_stale_witness() {
    for resume in [
        MachineResume::Cancelled,
        MachineResume::TimedOut,
        MachineResume::HostResult(EffectResult {
            sequence: 1,
            outcome: Err(HostProblem::UnknownOutcome),
        }),
    ] {
        let mut m = returned();
        assert!(matches!(
            m.drive(resume, Quantum::new(20, 4096).unwrap()),
            MachineDrive::Failed(_)
        ));
        assert!(m.attest_installed_program_return().is_err());
    }
    let mut m = returned();
    m.invocation.limits.max_steps = m.executed_steps;
    assert!(matches!(drive(&mut m), MachineDrive::Failed(_)));
    assert!(m.attest_installed_program_return().is_err());
    let mut m = returned();
    assert!(matches!(
        m.drive(
            MachineResume::Start,
            Quantum {
                max_steps: 0,
                max_allocated_bytes: 4096
            }
        ),
        MachineDrive::Continue
    ));
    assert!(m.attest_installed_program_return().is_err());
}

#[test]
fn failed_complete_never_attests_and_invalidates_prior_witness() {
    for previous in [false, true] {
        let mut m = if previous {
            returned()
        } else {
            machine("go_back", &[])
        };
        m.implicit.insert(
            "RETURN-CODE".into(),
            CobolValue::Decimal(Decimal {
                coefficient: i128::MAX,
                scale: 0,
            }),
        );
        assert!(matches!(drive(&mut m), MachineDrive::Condition(_)));
        assert!(m.attest_installed_program_return().is_err());
    }
    let mut m = machine("exit", &["PROGRAM"]);
    m.output = vec![b'x'; m.invocation.limits.max_output_bytes as usize + 1];
    assert!(matches!(drive(&mut m), MachineDrive::Failed(_)));
    assert!(m.attest_installed_program_return().is_err());
}

#[test]
fn halt_fallthrough_stop_run_and_other_exit_forms_do_not_attest() {
    for (terminal, args) in [
        ("halt", vec![]),
        ("stop_run", vec![]),
        ("exit", vec!["METHOD"]),
        ("exit", vec!["FUNCTION"]),
        ("exit", vec![]),
        ("exit", vec!["PROGRAM", "METHOD"]),
    ] {
        let mut m = machine(terminal, &args);
        assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
        let before = m.snapshot();
        assert!(m.attest_installed_program_return().is_err());
        assert_eq!(m.snapshot(), before);
    }
    let mut m = machine("go_back", &[]);
    m.pc = m.operations.len();
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    assert!(m.attest_installed_program_return().is_err());
}

fn cics_return_request() -> (ReferenceMachine, u64) {
    let mut m = machine_with_body("go_back", &[], "exec_cics", &["RETURN"]);
    let MachineDrive::HostCall(effect) = drive(&mut m) else {
        panic!("actual CICS RETURN effect")
    };
    assert_eq!(m.pc, 2); // The following GOBACK has not executed.
    (m, effect.sequence)
}

#[test]
fn observed_cics_return_at_following_goback_pc_cannot_fake_native_return() {
    use mainframe_env_host_api::CicsResponse;
    let (mut m, sequence) = cics_return_request();
    assert!(m.attest_installed_program_return().is_err());
    let response = CicsResponse {
        disposition: CicsDisposition::Returned,
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        applid: "MEAPPL".into(),
        sysid: "MESYS".into(),
        transaction: "TEST".into(),
        aid: 0,
        target: None,
        next_transaction: None,
        payload: BoundedPayload::new(
            "mainframe-env.cics.payload@1",
            Vec::new(),
            InvocationLimits::default(),
        )
        .unwrap(),
        outputs: BTreeMap::new(),
        unit_of_work: None,
    };
    assert!(matches!(
        m.drive(
            MachineResume::HostResult(EffectResult {
                sequence,
                outcome: Ok(HostResult::Cics(response))
            }),
            Quantum::new(20, 4096).unwrap()
        ),
        MachineDrive::Completed(_)
    ));
    assert_eq!(m.pc, 2);
    assert_eq!(m.executed_steps, 2);
    let before = m.snapshot();
    assert!(m.attest_installed_program_return().is_err());
    assert_eq!(m.snapshot(), before);
}

#[test]
fn actual_unknown_or_abnormal_host_result_does_not_attest() {
    for problem in [
        HostProblem::UnknownOutcome,
        HostProblem::Cancelled,
        HostProblem::TimedOut,
        HostProblem::Unauthorized,
    ] {
        let (mut m, sequence) = cics_return_request();
        assert!(matches!(
            m.drive(
                MachineResume::HostResult(EffectResult {
                    sequence,
                    outcome: Err(problem)
                }),
                Quantum::new(20, 4096).unwrap()
            ),
            MachineDrive::Failed(_)
        ));
        assert!(m.attest_installed_program_return().is_err());
    }
}

#[test]
fn marker_pc_steps_pending_and_deferred_guards_reject_without_mutation() {
    for guard in ["pc", "steps", "pending", "deferred", "opcode"] {
        let mut m = returned();
        match guard {
            "pc" => m.pc += 1,
            "steps" => m.executed_steps += 1,
            "pending" => {
                m.pending = Some(Pending {
                    sequence: 1,
                    kind: PendingKind::Ignore,
                })
            }
            "deferred" => m.deferred_drive = Some(MachineDrive::Continue),
            "opcode" => {
                m.operations[2].identity = OperationIdentity::new(NAMESPACE, "halt", 1).unwrap()
            }
            _ => unreachable!(),
        }
        let before = m.snapshot();
        assert!(m.attest_installed_program_return().is_err(), "{guard}");
        assert_eq!(m.snapshot(), before, "{guard}");
        assert_eq!(m.pending.is_some(), guard == "pending");
        assert_eq!(m.deferred_drive.is_some(), guard == "deferred");
    }
}

#[test]
fn existing_retained_resource_restrictions_and_open_cursors_reject() {
    for resource in [
        "dynamic",
        "unbounded",
        "pointer",
        "pointer32",
        "procedure-pointer",
        "function-pointer",
        "object-reference",
        "sql",
        "sort",
        "sort-procedure",
        "sort-io",
        "linkage",
        "extra-base",
        "open-cursor",
        "lifecycle",
    ] {
        let mut m = returned();
        match resource {
            "dynamic" | "unbounded" | "pointer" | "pointer32" | "procedure-pointer"
            | "function-pointer" | "object-reference" => {
                let mut layout = super::super::tests::edited_test_layout("9", 1, 0, false);
                match resource {
                    "dynamic" => layout.dynamic = true,
                    "unbounded" => layout.unbounded = true,
                    "pointer" => layout.category = LayoutCategory::Pointer,
                    "pointer32" => layout.category = LayoutCategory::Pointer32,
                    "procedure-pointer" => layout.category = LayoutCategory::ProcedurePointer,
                    "function-pointer" => layout.category = LayoutCategory::FunctionPointer,
                    "object-reference" => layout.category = LayoutCategory::ObjectReference,
                    _ => unreachable!(),
                }
                m.layouts.insert("UNSUPPORTED".into(), layout);
            }
            "sql" => {
                m.sql_cursors.insert("C".into(), Vec::new());
            }
            "sort" => {
                m.sort_workspaces.insert(
                    "S".into(),
                    SortWorkspace {
                        records: Vec::new(),
                        cursor: 0,
                    },
                );
            }
            "sort-procedure" => {
                m.active_sort_procedure = Some(ActiveSortProcedure {
                    sort_pc: 0,
                    sort_file: "S".into(),
                    phase: SortProcedurePhase::Input,
                    arguments: Vec::new(),
                })
            }
            "sort-io" => {
                m.sort_io = Some(SortIoState {
                    sort_pc: 0,
                    sort_file: "S".into(),
                    arguments: Vec::new(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    next_input: 0,
                    next_output: 0,
                })
            }
            "linkage" => {
                m.linkage_addresses.insert("P".into(), None);
            }
            "extra-base" => m.bases.push(Vec::new()),
            "open-cursor" => {
                m.dataset_cursors
                    .insert("DATA.SET".into(), "cursor-1".into());
            }
            "lifecycle" => {
                m.operations[0].attributes.remove("program_lifecycle");
            }
            _ => unreachable!(),
        }
        let before = m.snapshot();
        assert!(m.attest_installed_program_return().is_err(), "{resource}");
        assert_eq!(m.snapshot(), before, "{resource}");
    }
    for name in ["entry", "allocate", "free", "invoke"] {
        let mut m = returned();
        let mut op = m.operations.last().unwrap().clone();
        op.identity = OperationIdentity::new(NAMESPACE, name, 1).unwrap();
        m.operations.insert(3, op);
        assert!(m.attest_installed_program_return().is_err(), "{name}");
    }
}

#[test]
fn retained_api_still_accepts_pre_execution_state_but_witness_does_not() {
    let mut m = machine("go_back", &[]);
    let state = m.retained_program_state().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&state).unwrap()[0],
        1
    );
    assert!(m.attest_installed_program_return().is_err());
    m.install_retained_program_state(&state).unwrap();
    assert_eq!(m.retained_program_state().unwrap(), state);
}

#[test]
fn initial_return_supported_without_silently_tightening_older_api() {
    let mut m = machine("go_back", &[]);
    m.operations[0].attributes.insert(
        "program_lifecycle".into(),
        Attribute::Text("initial@1".into()),
    );
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    assert!(m.attest_installed_program_return().is_ok());
    assert!(m.installed_call_is_initial().unwrap());
    let state = m.retained_program_state().unwrap();
    assert_eq!(
        m.install_retained_program_state(&state),
        Err(MachineProblem::IncompatibleSnapshot)
    );
    // The legacy retained API can record cursor state; the new closed-return
    // observation intentionally refuses it, leaving old callers unchanged.
    let mut ordinary = returned();
    ordinary
        .dataset_cursors
        .insert("DATA.SET".into(), "cursor-1".into());
    assert!(ordinary.retained_program_state().is_ok());
    assert!(ordinary.attest_installed_program_return().is_err());
}

#[test]
fn complete_return_preserves_exact_invocation_controls_and_generations() {
    use mainframe_env_execution_api::{CapabilityId, ExecutionId};
    let mut m = machine("exit", &["PROGRAM"]);
    m.invocation.parent_execution_id =
        Some(ExecutionId::new("actual-caller", InvocationLimits::default()).unwrap());
    m.invocation.attempt = 3;
    m.invocation.priority = 17;
    m.invocation.deadline_tick = 1234;
    m.invocation.provider_generations.insert(
        CapabilityId::new("definition", InvocationLimits::default()).unwrap(),
        "frozen-generation".into(),
    );
    m.invocation.bindings.insert(
        "owned-context".into(),
        BoundedPayload::new("context@1", b"exact".to_vec(), InvocationLimits::default()).unwrap(),
    );
    let expected = m.invocation.clone();
    assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
    assert_eq!(
        m.attest_installed_program_return().unwrap().invocation(),
        &expected
    );
}

#[test]
fn initial_with_file_metadata_is_fenced_but_closed_ordinary_file_shape_is_unchanged() {
    for initial in [false, true] {
        let mut m = machine("go_back", &[]);
        if initial {
            m.operations[0].attributes.insert(
                "program_lifecycle".into(),
                Attribute::Text("initial@1".into()),
            );
        }
        m.files.insert(
            "F".into(),
            FileMetadata {
                assignment: "DATA.SET".into(),
                record_name: None,
                record_names: Vec::new(),
                organization: "SEQUENTIAL".into(),
                access_mode: "SEQUENTIAL".into(),
                record_key: None,
                alternate_record_keys: Vec::new(),
                relative_key: None,
                file_status: None,
                sort_merge: false,
                description: String::new(),
                record_min: None,
                record_max: None,
                ccsid: None,
                linage: None,
            },
        );
        assert!(matches!(drive(&mut m), MachineDrive::Completed(_)));
        let before = m.snapshot();
        assert_eq!(m.attest_installed_program_return().is_ok(), !initial);
        assert_eq!(m.retained_program_state().is_ok(), !initial);
        assert_eq!(m.snapshot(), before);
    }
}
