use super::*;
use mainframe_env_ir::{IrLimits, ModuleBuilder};

fn machine(statements: &[(&str, &[&str])]) -> ReferenceMachine {
    let mut builder = ModuleBuilder::new(IrLimits::default());
    let region = builder.add_region().unwrap();
    let block = builder.add_block(region).unwrap();
    for (name, args) in statements {
        let attributes = args
            .iter()
            .enumerate()
            .map(|(index, arg)| (format!("arg_{index:03}"), Attribute::Text((*arg).into())))
            .collect();
        builder
            .add_operation(
                block,
                OperationIdentity::new(NAMESPACE, *name, 1).unwrap(),
                Vec::new(),
                0,
                attributes,
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap();
    }
    let binary =
        mainframe_env_ir::encode_binary(&builder.finish().unwrap(), CodecLimits::default())
            .unwrap();
    ReferenceMachine::from_binary(&binary, tests::invocation(), CodecLimits::default()).unwrap()
}

fn assert_host_failure_position(
    statements: &[(&str, &[&str])],
    issuing_pc: usize,
    continuation_pc: usize,
) {
    let mut machine = machine(statements);
    let quantum = Quantum::new(100, 1024).unwrap();
    let MachineDrive::HostCall(effect) = machine.drive(MachineResume::Start, quantum) else {
        panic!("expected the first statement to issue a host call");
    };
    assert!(matches!(
        effect.request,
        HostRequest::Cics(ref request) if request.operation == CicsOperation::AsktimeEib
    ));
    assert_eq!(machine.snapshot().program_counter, continuation_pc);
    assert!(
        machine.checkpoint().is_none(),
        "pending calls are not durable"
    );
    let snapshot = machine.snapshot();
    let continuation_summary = machine.position_summary();
    let MachineDrive::Failed(problem) = machine.drive(
        MachineResume::HostResult(EffectResult {
            sequence: effect.sequence,
            outcome: Err(HostProblem::ResourceExhausted),
        }),
        quantum,
    ) else {
        panic!("expected an unhandled host call failure");
    };
    // This is the same message join used for ExecutionOutcome::Rejected by the server.
    let message = format!(
        "{} at {}",
        problem.public_message,
        machine.position_summary()
    );
    assert!(
        message.contains("host call failed: ResourceExhausted"),
        "{message}"
    );
    assert!(
        message.contains(&format!(
            "at pc={issuing_pc} exec_cics [\"CICS\", \"ASKTIME\"]"
        )),
        "failure must name the issuing statement: {message}"
    );
    assert_eq!(machine.snapshot(), snapshot);
    let checkpoint = machine.checkpoint().unwrap();
    assert_eq!(
        checkpoint.schema(),
        "mainframe-env.reference-machine-checkpoint@12"
    );
    machine.restore_checkpoint(&checkpoint).unwrap();
    assert_eq!(machine.position_summary(), continuation_summary);
}

#[test]
fn host_call_failure_names_first_of_consecutive_statements() {
    assert_host_failure_position(
        &[
            ("label", &["MAIN"]),
            ("exec_cics", &["CICS", "ASKTIME"]),
            ("exec_cics", &["CICS", "FORMATTIME"]),
            ("halt", &[]),
        ],
        1,
        2,
    );
}

#[test]
fn host_call_failure_names_last_statement_of_paragraph() {
    assert_host_failure_position(
        &[
            ("label", &["MAIN"]),
            ("exec_cics", &["CICS", "ASKTIME"]),
            ("label", &["NEXT"]),
            ("halt", &[]),
        ],
        1,
        2,
    );
}

#[test]
fn host_call_failure_names_performed_statement_after_return() {
    assert_host_failure_position(
        &[
            ("perform", &["WORK"]),
            ("halt", &[]),
            ("label", &["WORK"]),
            ("exec_cics", &["CICS", "ASKTIME"]),
            ("label", &["NEXT"]),
            ("halt", &[]),
        ],
        3,
        1,
    );
}

fn sort_machine() -> ReferenceMachine {
    let mut machine = machine(&[
        (
            "sort",
            &[
                "SORT-X", "USING", "INPUT1", "INPUT2", "GIVING", "OUTPUT1", "OUTPUT2",
            ],
        ),
        ("halt", &[]),
    ]);
    machine.files.insert(
        "SORT-X".into(),
        FileMetadata {
            assignment: "SORT-X".into(),
            record_name: Some("SORT-RECORD".into()),
            record_names: vec!["SORT-RECORD".into()],
            organization: "SEQUENTIAL".into(),
            access_mode: "SEQUENTIAL".into(),
            record_key: None,
            alternate_record_keys: Vec::new(),
            relative_key: None,
            file_status: None,
            sort_merge: true,
            description: String::new(),
            record_min: None,
            record_max: None,
            ccsid: None,
            linage: None,
        },
    );
    machine.layouts.insert(
        "SORT-RECORD".into(),
        LayoutMetadata {
            name: "SORT-RECORD".into(),
            simple_name: "SORT-RECORD".into(),
            category: LayoutCategory::Alphanumeric,
            picture: "X(4)".into(),
            digits: 0,
            scale: 0,
            native_binary: false,
            signed: false,
            sign_leading: false,
            sign_separate: false,
            justified_right: false,
            blank_when_zero: false,
            linkage: false,
            offset: 0,
            length: 4,
            element_length: 4,
            occurs: 1,
            occurs_min: 1,
            unbounded: false,
            depending_on: None,
            indexes: Vec::new(),
            keys: Vec::new(),
            dynamic: false,
            dynamic_limit: 0,
            parent: None,
            alias_of: None,
            occurs_clause: false,
            condition_values: Vec::new(),
            object_class: None,
        },
    );
    machine
}

fn assert_sort_failure_position(successful_effects: usize) {
    use mainframe_env_host_api::DatasetResult;

    let mut machine = sort_machine();
    let quantum = Quantum::new(100, 1024).unwrap();
    let mut drive = machine.drive(MachineResume::Start, quantum);
    for (index, expected_dataset) in ["INPUT1", "INPUT2", "OUTPUT1", "OUTPUT2"]
        .into_iter()
        .enumerate()
        .take(successful_effects + 1)
    {
        let MachineDrive::HostCall(effect) = drive else {
            panic!("expected SORT effect {index}: {drive:?}");
        };
        assert_eq!(effect.sequence, index as u64 + 1);
        let outcome = match &effect.request {
            HostRequest::Dataset(DatasetRequest::Read { dataset, .. }) if index < 2 => {
                assert_eq!(dataset.as_str(), expected_dataset);
                HostResult::Dataset(DatasetResult::Records {
                    records: vec![b"AA01".to_vec()],
                    identities: vec![vec![1]],
                    version: 1,
                    more: false,
                })
            }
            HostRequest::Dataset(DatasetRequest::Write {
                dataset, records, ..
            }) if index >= 2 => {
                assert_eq!(dataset.as_str(), expected_dataset);
                assert_eq!(records, &vec![b"AA01".to_vec(), b"AA01".to_vec()]);
                HostResult::Dataset(DatasetResult::Mutated { version: 2 })
            }
            _ => panic!("unexpected SORT effect {index}: {effect:?}"),
        };
        assert_eq!(machine.snapshot().program_counter, 1);
        if index == successful_effects {
            let snapshot = machine.snapshot();
            let MachineDrive::Failed(problem) = machine.drive(
                MachineResume::HostResult(EffectResult {
                    sequence: effect.sequence,
                    outcome: Err(HostProblem::ResourceExhausted),
                }),
                quantum,
            ) else {
                panic!("expected an unhandled SORT host failure");
            };
            assert_eq!(
                problem.public_message,
                "host provider failed: ResourceExhausted"
            );
            let position = machine.position_summary();
            assert!(
                position.starts_with("pc=0 sort [\"SORT-X\", \"USING\""),
                "{position}"
            );
            assert_eq!(machine.snapshot(), snapshot);
            return;
        }
        drive = machine.drive(
            MachineResume::HostResult(EffectResult {
                sequence: effect.sequence,
                outcome: Ok(outcome),
            }),
            quantum,
        );
    }
    panic!("expected the selected SORT effect to fail");
}

#[test]
fn sort_host_call_failure_names_original_statement_on_second_read() {
    assert_sort_failure_position(1);
}

#[test]
fn sort_host_call_failure_names_original_statement_on_first_write() {
    assert_sort_failure_position(2);
}

#[test]
fn sort_host_call_failure_names_original_statement_on_later_write() {
    assert_sort_failure_position(3);
}
