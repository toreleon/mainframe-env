use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, Resolution, ResolutionFailure, require_writable,
};
use super::{
    Clauses, cics_integer_value, cics_value, complete_data_reference, shape::CommandShape,
};
use crate::{DataCategory, SemanticModel};

pub(super) fn shape(operation: HirCicsOperation) -> Option<CommandShape> {
    Some(match operation {
        HirCicsOperation::FetchAny => CommandShape {
            clauses: &[
                "ANY",
                "COMPSTATUS",
                "CHANNEL",
                "ABCODE",
                "TIMEOUT",
                "RESP",
                "RESP2",
            ],
            options: &["NOSUSPEND", "NOHANDLE"],
            required: &["ANY", "COMPSTATUS"],
        },
        HirCicsOperation::FetchChild => CommandShape {
            clauses: &[
                "CHILD",
                "COMPSTATUS",
                "CHANNEL",
                "ABCODE",
                "TIMEOUT",
                "RESP",
                "RESP2",
            ],
            options: &["NOSUSPEND", "NOHANDLE"],
            required: &["CHILD", "COMPSTATUS"],
        },
        HirCicsOperation::FreeChild => CommandShape {
            clauses: &["CHILD", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["CHILD"],
        },
        HirCicsOperation::LinkAcqActivity => CommandShape {
            clauses: &["INPUTEVENT", "RESP", "RESP2"],
            options: &["ACQACTIVITY", "NOHANDLE"],
            required: &[],
        },
        HirCicsOperation::LinkAcqProcess => CommandShape {
            clauses: &["INPUTEVENT", "RESP", "RESP2"],
            options: &["ACQPROCESS", "NOHANDLE"],
            required: &[],
        },
        HirCicsOperation::LinkActivity => CommandShape {
            clauses: &["ACTIVITY", "INPUTEVENT", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["ACTIVITY"],
        },
        _ => return None,
    })
}

pub(super) fn option(operation: HirCicsOperation, name: &str) -> Option<HirCicsOption> {
    match (operation, name) {
        (HirCicsOperation::FetchAny | HirCicsOperation::FetchChild, "NOSUSPEND") => {
            Some(HirCicsOption::BtsNoSuspend)
        }
        (HirCicsOperation::LinkAcqActivity, "ACQACTIVITY") => Some(HirCicsOption::BtsAcqActivity),
        (HirCicsOperation::LinkAcqProcess, "ACQPROCESS") => Some(HirCicsOption::BtsAcqProcess),
        _ => None,
    }
}

pub(super) fn validate(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if matches!(
        operation,
        HirCicsOperation::FetchAny | HirCicsOperation::FetchChild
    ) && clauses.contains_key("TIMEOUT")
        && options.iter().any(|option| option == "NOSUSPEND")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS FETCH cannot combine TIMEOUT and NOSUSPEND".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let Some(_) = shape(operation) else {
        return Ok(Vec::new());
    };
    let mut values = Vec::new();
    for (name, identity, numeric) in [
        ("CHILD", HirCicsOperandName::BtsChild, false),
        ("ACTIVITY", HirCicsOperandName::BtsLinkActivity, false),
        ("INPUTEVENT", HirCicsOperandName::BtsLinkInputEvent, false),
        ("TIMEOUT", HirCicsOperandName::BtsTimeout, true),
    ] {
        if let Some(value) = clauses.get(name) {
            values.push(HirCicsNamedOperand {
                name: identity,
                value: if numeric {
                    cics_integer_value(value, semantic)?
                } else {
                    cics_value(value, semantic)?
                },
            });
        }
    }
    Ok(values)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !matches!(
        operation,
        HirCicsOperation::FetchAny | HirCicsOperation::FetchChild
    ) {
        return Ok(Vec::new());
    }
    let mut outputs = Vec::new();
    for (name, identity, length, binary) in [
        ("ANY", HirCicsOutputName::BtsAny, 16, false),
        ("COMPSTATUS", HirCicsOutputName::BtsChildCompStatus, 4, true),
        ("CHANNEL", HirCicsOutputName::BtsChannel, 16, false),
        ("ABCODE", HirCicsOutputName::BtsAbcode, 4, false),
    ] {
        let Some(value) = clauses.get(name) else {
            continue;
        };
        let target = complete_data_reference(value, semantic)?;
        require_writable(&target)?;
        let valid = if binary {
            target.category == DataCategory::Binary && target.length == length && target.scale == 0
        } else {
            matches!(
                target.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric
            ) && target.length == length
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS FETCH {name} requires writable {length}-byte {} storage",
                if binary { "binary" } else { "character" }
            )));
        }
        outputs.push(HirCicsOutputBinding {
            name: identity,
            target,
        });
    }
    Ok(outputs)
}
