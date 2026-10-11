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
