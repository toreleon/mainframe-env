use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_value, complete_data_reference, numeric_value::cics_integer_value};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &[
            "TRACENUM",
            "FROM",
            "FROMLENGTH",
            "RESOURCE",
            "RESP",
            "RESP2",
        ],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &["EXCEPTION", "NOHANDLE"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &["TRACENUM"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::EnterTraceNum {
        return Ok(Vec::new());
    }
    let number = cics_integer_value(&clauses["TRACENUM"], semantic)?;
    if let HirCicsValue::Data(reference) = &number {
        require_halfword("TRACENUM", reference)?;
    }
    let mut result = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::TraceNum,
        value: number,
    }];
    if let Some(tokens) = clauses.get("FROM") {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TraceFrom,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    if let Some(tokens) = clauses.get("FROMLENGTH") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_halfword("FROMLENGTH", &reference)?;
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TraceFromLength,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("RESOURCE") {
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(text) => text.len() == 8,
            HirCicsValue::Data(reference) => {
                reference.length == 8
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS ENTER TRACENUM RESOURCE requires eight characters".into(),
            ));
        }
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TraceResource,
            value,
        });
    }
    Ok(result)
}

fn require_halfword(name: &str, reference: &super::super::HirDataReference) -> Resolution<()> {
    if reference.length == 2
        && reference.scale == 0
        && matches!(
            reference.usage,
            CobolUsage::Binary | CobolUsage::NativeBinary
        )
    {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS ENTER TRACENUM {name} requires halfword binary storage"
        )))
    }
}
