use super::*;

pub(crate) fn execute_legacy(
    machine: &mut ReferenceMachine,
    args: &[String],
) -> Result<Step, MachineProblem> {
    let operation = CicsOperation::from_tokens(args).ok_or(MachineProblem::UnsupportedForm)?;
    let mut arguments = legacy_arguments(args)?;
    let into = legacy_destination(&arguments, "INTO").map(CicsTarget::Legacy);
    let output_names: &[&str] = match operation {
        CicsOperation::Asktime => &["ABSTIME"],
        CicsOperation::AsktimeEib => &[],
        CicsOperation::Assign => CICS_ASSIGN_OUTPUT_NAMES,
        CicsOperation::FormatTime => &[
            "YYYYMMDD",
            "YYMMDD",
            "MMDDYY",
            "MMDDYYYY",
            "YYDDD",
            "TIME",
            "MILLISECONDS",
        ],
        CicsOperation::Link => &["COMMAREA"],
        CicsOperation::ReadNext | CicsOperation::ReadPrev => &["RIDFLD"],
        _ => &[],
    };
    let outputs = output_names
        .iter()
        .filter_map(|name| {
            legacy_destination(&arguments, name)
                .map(|target| ((*name).into(), CicsTarget::Legacy(target)))
        })
        .collect::<BTreeMap<_, _>>();
    if operation == CicsOperation::Assign {
        validate_legacy_assign_outputs(machine, &outputs)?;
    }
    let response_target = legacy_destination(&arguments, "RESP").map(CicsTarget::Legacy);
    let response2_target = legacy_destination(&arguments, "RESP2").map(CicsTarget::Legacy);
    let absolute_time = legacy_destination(&arguments, "ABSTIME");
    for key in [
        "FROM", "COMMAREA", "RIDFLD", "QUEUE", "MAP", "MAPSET", "TRANSID", "PROGRAM", "DATASET",
        "FILE", "MEMBER", "VERSION",
    ] {
        if outputs.contains_key(key) {
            continue;
        }
        let Some(argument) = arguments.get(key) else {
            continue;
        };
        if argument.schema() == "mainframe-env.cics.literal@1" {
            continue;
        }
        let token = String::from_utf8_lossy(argument.bytes()).into_owned();
        let reference_tokens = token
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if let Ok(reference) = machine.reference(&reference_tokens) {
            let value = machine.read_reference(&reference)?;
            arguments.insert(
                key.into(),
                payload("mainframe-env.cics.storage-value@1", value)?,
            );
        }
    }
    if matches!(
        operation,
        CicsOperation::Read
            | CicsOperation::Write
            | CicsOperation::Rewrite
            | CicsOperation::Delete
            | CicsOperation::StartBrowse
            | CicsOperation::ReadNext
            | CicsOperation::ReadPrev
            | CicsOperation::EndBrowse
            | CicsOperation::SendText
    ) {
        for key in ["LENGTH", "KEYLENGTH"] {
            let Some(argument) = arguments.get(key) else {
                continue;
            };
            if argument.schema() == "mainframe-env.cics.literal@1" {
                continue;
            }
            let token = String::from_utf8_lossy(argument.bytes()).into_owned();
            let reference_tokens = token
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if let Ok(decimal) = legacy_numeric_operand(machine, &reference_tokens) {
                arguments.insert(
                    key.into(),
                    payload("mainframe-env.cics.decimal@1", decimal)?,
                );
            }
        }
    }
    if operation == CicsOperation::FormatTime
        && let Some(target) = absolute_time
    {
        let tokens = target
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let reference = machine.reference(&tokens)?;
        let bytes = machine.read_reference(&reference)?;
        if !is_numeric(reference.layout.category) {
            return Err(MachineProblem::DataException);
        }
        let value = decode_decimal(&reference.layout, &bytes)?;
        if value.scale != 0 {
            return Err(MachineProblem::DataException);
        }
        arguments.insert(
            "ABSTIME".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                value.coefficient.to_string().into_bytes(),
            )?,
        );
    }
    let condition_policy = legacy_condition_policy(args, &arguments)?;
    let mut mutation = operation
        .is_mutating()
        .then(|| machine.mutation())
        .transpose()?;
    if let Some(mutation) = &mut mutation {
        mutation.transaction = Some(
            machine
                .invocation
                .bindings
                .get("cics.transaction")
                .map(|payload| String::from_utf8_lossy(payload.bytes()).into_owned())
                .unwrap_or_else(|| "DEFAULT".into()),
        );
    }
    let argument_summary = argument_summary(&arguments);
    machine.effect(
        HostRequest::Cics(CicsRequest {
            operation,
            arguments,
            condition_policy,
            mutation,
        }),
        PendingKind::Cics {
            operation,
            argument_summary,
            into,
            outputs,
            response: response_target,
            response2: response2_target,
            address_set: None,
            no_handle: args.iter().any(|argument| argument == "NOHANDLE"),
        },
    )
}
