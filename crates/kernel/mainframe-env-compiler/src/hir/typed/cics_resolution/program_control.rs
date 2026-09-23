use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{
    Clauses, cics_integer_value, cics_value, complete_data_reference, program_name,
    transaction_name,
};
use crate::{DataCategory, SemanticModel};

pub(super) const INVOKE_CLAUSES: &[&str] = &[
    "APPLICATION",
    "OPERATION",
    "PLATFORM",
    "MAJORVERSION",
    "MINORVERSION",
    "COMMAREA",
    "LENGTH",
    "CHANNEL",
    "RESP",
    "RESP2",
];
pub(super) const LOAD_CLAUSES: &[&str] = &[
    "PROGRAM", "SET", "ENTRY", "LENGTH", "FLENGTH", "RESP", "RESP2",
];
pub(super) const RELEASE_CLAUSES: &[&str] = &["PROGRAM", "RESP", "RESP2"];

pub(super) fn validate(
    operation: HirCicsOperation,
    clauses: &Clauses,
    options: &[String],
) -> Resolution<()> {
    validate_constraints(operation, clauses)?;
    validate_options(operation, clauses, options)
}

fn validate_constraints(operation: HirCicsOperation, clauses: &Clauses) -> Resolution<()> {
    if operation == HirCicsOperation::InvokeApplication {
        validate_invoke_constraints(clauses)
    } else if operation == HirCicsOperation::Load {
        if clauses.contains_key("LENGTH") && clauses.contains_key("FLENGTH") {
            Err(ResolutionFailure::Invalid(
                "CICS LOAD accepts LENGTH or FLENGTH, not both".into(),
            ))
        } else {
            Ok(())
        }
    } else if matches!(
        operation,
        HirCicsOperation::Link | HirCicsOperation::Xctl | HirCicsOperation::Return
    ) && clauses.contains_key("LENGTH")
        && !clauses.contains_key("COMMAREA")
    {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} LENGTH requires COMMAREA"
        )))
    } else if operation == HirCicsOperation::Link
        && clauses.contains_key("DATALENGTH")
        && (!clauses.contains_key("COMMAREA") || !clauses.contains_key("LENGTH"))
    {
        Err(ResolutionFailure::Invalid(
            "CICS LINK DATALENGTH requires COMMAREA and LENGTH".into(),
        ))
    } else if operation == HirCicsOperation::Return
        && clauses.contains_key("COMMAREA")
        && !clauses.contains_key("TRANSID")
    {
        Err(ResolutionFailure::Invalid(
            "CICS RETURN COMMAREA requires TRANSID in the typed local subset".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_options(
    operation: HirCicsOperation,
    clauses: &Clauses,
    options: &[String],
) -> Resolution<()> {
    if operation != HirCicsOperation::InvokeApplication {
        return Ok(());
    }
    let exact = options.iter().any(|option| option == "EXACTMATCH");
    let minimum = options.iter().any(|option| option == "MINIMUM");
    if exact && minimum {
        return Err(ResolutionFailure::Invalid(
            "CICS INVOKE APPLICATION accepts EXACTMATCH or MINIMUM, not both".into(),
        ));
    }
    if (exact || minimum) && !clauses.contains_key("MAJORVERSION") {
        return Err(ResolutionFailure::Invalid(
            "CICS INVOKE APPLICATION version matching requires MAJORVERSION and MINORVERSION"
                .into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    operation: HirCicsOperation,
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    match operation {
        HirCicsOperation::InvokeApplication => invoke_operands(clauses, semantic),
        HirCicsOperation::Load => load_operands(clauses, semantic),
        HirCicsOperation::Release => Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Program,
            value: program_name::value(&clauses["PROGRAM"], semantic, "RELEASE")?,
        }]),
        HirCicsOperation::Link => transfer_operands(clauses, semantic, "LINK"),
        HirCicsOperation::Xctl => transfer_operands(clauses, semantic, "XCTL"),
        HirCicsOperation::Return => return_operands(clauses, semantic),
        _ => Ok(Vec::new()),
    }
}

fn load_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Program,
        value: program_name::value(&clauses["PROGRAM"], semantic, "LOAD")?,
    }];
    for (clause, name, pointer, bytes) in [
        ("SET", HirCicsOperandName::LoadSet, true, 0),
        ("ENTRY", HirCicsOperandName::Entry, true, 0),
        ("LENGTH", HirCicsOperandName::LoadLength, false, 2),
        ("FLENGTH", HirCicsOperandName::LoadFlength, false, 4),
    ] {
        let Some(tokens) = clauses.get(clause) else {
            continue;
        };
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if pointer
            && !matches!(
                target.usage,
                crate::CobolUsage::Pointer | crate::CobolUsage::Pointer32
            )
            || !pointer
                && (target.length != bytes
                    || target.category != DataCategory::Binary
                    || !matches!(target.usage, crate::CobolUsage::Binary))
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS LOAD {clause} requires {}",
                if pointer {
                    "a POINTER or POINTER-32 reference"
                } else if bytes == 2 {
                    "a writable halfword-binary area"
                } else {
                    "a writable fullword-binary area"
                }
            )));
        }
        operands.push(HirCicsNamedOperand {
            name,
            value: HirCicsValue::Data(target),
        });
    }
    Ok(operands)
}

fn validate_invoke_constraints(clauses: &Clauses) -> Resolution<()> {
    for required in ["APPLICATION", "OPERATION"] {
        if !clauses.contains_key(required) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS INVOKE APPLICATION requires {required}"
            )));
        }
    }
    if clauses.contains_key("MAJORVERSION") != clauses.contains_key("MINORVERSION") {
        return Err(ResolutionFailure::Invalid(
            "CICS INVOKE APPLICATION requires MAJORVERSION and MINORVERSION together".into(),
        ));
    }
    if clauses.contains_key("LENGTH") && !clauses.contains_key("COMMAREA") {
        return Err(ResolutionFailure::Invalid(
            "CICS INVOKE APPLICATION LENGTH requires COMMAREA".into(),
        ));
    }
    if clauses.contains_key("COMMAREA") && clauses.contains_key("CHANNEL") {
        return Err(ResolutionFailure::Invalid(
            "CICS INVOKE APPLICATION accepts COMMAREA or CHANNEL, not both".into(),
        ));
    }
    Ok(())
}

fn invoke_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    for (clause, name, maximum) in [
        ("APPLICATION", HirCicsOperandName::Application, 64),
        ("OPERATION", HirCicsOperandName::ApplicationOperation, 64),
        ("PLATFORM", HirCicsOperandName::Platform, 64),
        ("CHANNEL", HirCicsOperandName::Channel, 16),
    ] {
        let Some(tokens) = clauses.get(clause) else {
            continue;
        };
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(value) if clause == "CHANNEL" => {
                matches!(value.len(), 1..=16)
                    && !value.bytes().any(|byte| byte.is_ascii_whitespace())
            }
            HirCicsValue::Literal(value) => {
                (1..=maximum).contains(&value.len())
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'.' | b'_' | b'#' | b'@' | b'-')
                    })
            }
            HirCicsValue::Data(reference) => {
                (1..=maximum).contains(&reference.length)
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS INVOKE APPLICATION {clause} requires a 1-{maximum} character name"
            )));
        }
        operands.push(HirCicsNamedOperand { name, value });
    }
    for (clause, name) in [
        ("MAJORVERSION", HirCicsOperandName::MajorVersion),
        ("MINORVERSION", HirCicsOperandName::MinorVersion),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_integer_value(tokens, semantic)?,
            });
        }
    }
    if let Some(tokens) = clauses.get("COMMAREA") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_writable(&reference)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Commarea,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: commarea_length(tokens, semantic)?,
        });
    }
    Ok(operands)
}

fn transfer_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
    command: &str,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Program,
        value: program_name::value(&clauses["PROGRAM"], semantic, command)?,
    }];
    if let Some(tokens) = clauses.get("COMMAREA") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_writable(&reference)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Commarea,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: commarea_length(tokens, semantic)?,
        });
    }
    if let Some(tokens) = clauses.get("DATALENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::DataLength,
            value: cics_integer_value(tokens, semantic)?,
        });
    }
    Ok(operands)
}

pub(super) fn link_commarea_output(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Option<HirCicsOutputBinding>> {
    let Some(tokens) = clauses.get("COMMAREA") else {
        return Ok(None);
    };
    let target = complete_data_reference(tokens, semantic)?;
    require_writable(&target)?;
    Ok(Some(HirCicsOutputBinding {
        name: HirCicsOutputName::Commarea,
        target,
    }))
}

fn return_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = Vec::new();
    if let Some(tokens) = clauses.get("TRANSID") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TransId,
            value: transaction_name::value(tokens, semantic)?,
        });
    }
    if let Some(tokens) = clauses.get("COMMAREA") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_writable(&reference)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Commarea,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: commarea_length(tokens, semantic)?,
        });
    }
    Ok(operands)
}

fn commarea_length(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
        && tokens
            .get(1)
            .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
    {
        complete_data_reference(&tokens[2..], semantic).map(HirCicsValue::LengthOf)
    } else {
        cics_integer_value(tokens, semantic)
    }
}
