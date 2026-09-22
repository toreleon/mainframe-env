use super::*;

pub(in crate::machine) fn legacy_arguments(
    tokens: &[String],
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let mut arguments = BTreeMap::new();
    let command = tokens
        .iter()
        .position(|token| {
            !matches!(
                token.to_ascii_uppercase().as_str(),
                "EXEC" | "CICS" | "END-EXEC"
            )
        })
        .ok_or(MachineProblem::InvalidOperation)?;
    let first = tokens[command].to_ascii_uppercase();
    let mut index = command + 1;
    if matches!(first.as_str(), "HANDLE" | "RECEIVE" | "SEND" | "WRITEQ")
        && tokens.get(index).is_some_and(|token| {
            matches!(
                token.to_ascii_uppercase().as_str(),
                "ABEND" | "CONDITION" | "MAP" | "TEXT" | "TD"
            )
        })
    {
        let subcommand = tokens[index].to_ascii_uppercase();
        index += 1;
        if tokens.get(index).is_some_and(|token| token == "(") {
            let end = matching_close(tokens, index).ok_or(MachineProblem::InvalidOperation)?;
            let raw = tokens[index + 1..end].join(" ");
            let literal = raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']);
            arguments.insert(
                subcommand,
                payload(
                    if literal {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    },
                    raw.trim_matches(['\'', '"']).as_bytes().to_vec(),
                )?,
            );
            index = end + 1;
        }
    }
    while index < tokens.len() {
        let key = tokens[index].to_ascii_uppercase();
        if key == "END-EXEC" {
            break;
        }
        if tokens.get(index + 1).is_some_and(|token| token == "(") {
            let end = matching_close(tokens, index + 1).ok_or(MachineProblem::InvalidOperation)?;
            let raw = tokens[index + 2..end].join(" ");
            let literal = raw.starts_with(['\'', '"']) && raw.ends_with(['\'', '"']);
            arguments.insert(
                key,
                payload(
                    if literal {
                        "mainframe-env.cics.literal@1"
                    } else {
                        "mainframe-env.cics.argument@1"
                    },
                    raw.trim_matches(['\'', '"']).as_bytes().to_vec(),
                )?,
            );
            index = end + 1;
        } else {
            arguments.insert(
                format!("OPTION.{key}"),
                payload("mainframe-env.cics.option@1", Vec::new())?,
            );
            index += 1;
        }
    }
    Ok(arguments)
}

fn legacy_destination(arguments: &BTreeMap<String, BoundedPayload>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .map(|value| String::from_utf8_lossy(value.bytes()).into_owned())
}

fn legacy_numeric_operand(
    machine: &ReferenceMachine,
    reference_tokens: &[String],
) -> Result<Vec<u8>, MachineProblem> {
    if let [literal] = reference_tokens
        && !literal.is_empty()
        && literal.bytes().all(|byte| byte.is_ascii_digit())
    {
        let value: u32 = literal.parse().map_err(|_| MachineProblem::DataException)?;
        return Ok(value.to_string().into_bytes());
    }
    if let [head, of, rest @ ..] = reference_tokens
        && head.eq_ignore_ascii_case("LENGTH")
        && of.eq_ignore_ascii_case("OF")
    {
        let reference = machine.reference(rest)?;
        return Ok(machine
            .read_reference(&reference)?
            .len()
            .to_string()
            .into_bytes());
    }
    let reference = machine.reference(reference_tokens)?;
    let bytes = machine.read_reference(&reference)?;
    let value = decode_decimal(&reference.layout, &bytes)?;
    if value.scale != 0 {
        return Err(MachineProblem::DataException);
    }
    Ok(value.coefficient.to_string().into_bytes())
}

fn validate_legacy_assign_outputs(
    machine: &ReferenceMachine,
    outputs: &BTreeMap<String, CicsTarget>,
) -> Result<(), MachineProblem> {
    for (name, target) in outputs {
        let CicsTarget::Legacy(target) = target else {
            return Err(MachineProblem::InvalidOperation);
        };
        let tokens = target
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let reference = machine.reference(&tokens)?;
        if name == "TASKPRIORITY"
            && (reference.layout.category != LayoutCategory::Binary
                || reference.length != 2
                || reference.layout.scale != 0)
        {
            return Err(MachineProblem::DataException);
        }
    }
    Ok(())
}

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
