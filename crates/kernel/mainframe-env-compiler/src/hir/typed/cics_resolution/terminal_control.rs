use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    let required: &[&str] = match operation {
        HirCicsOperation::ReceiveMap | HirCicsOperation::SendMap => &["MAP"],
        HirCicsOperation::SendText => &["FROM"],
        _ => return Ok(()),
    };
    for name in required {
        if !clauses.contains_key(*name) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} requires {name}"
            )));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::ReceiveMap | HirCicsOperation::SendMap | HirCicsOperation::SendText
    ) {
        return Ok(Vec::new());
    }
    let mut operands = Vec::new();
    for (name, identity) in [
        ("MAP", HirCicsOperandName::Map),
        ("MAPSET", HirCicsOperandName::Mapset),
    ] {
        let Some(tokens) = clauses.get(name) else {
            continue;
        };
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(value) => {
                matches!(value.len(), 1..=7)
                    && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            HirCicsValue::Data(reference) => {
                matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                ) && (matches!(reference.length, 1..=7)
                    || name == "MAPSET" && reference.length == 8)
            }
            HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} {name} requires a 1-7 character name"
            )));
        }
        operands.push(HirCicsNamedOperand {
            name: identity,
            value,
        });
    }
    if let Some(tokens) = clauses.get("FROM") {
        let HirCicsValue::Data(reference) = cics_value(tokens, semantic)? else {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} FROM requires a data area"
            )));
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::From,
            value: HirCicsValue::Data(reference),
        });
    }
    if operation == HirCicsOperation::SendText
        && let Some(tokens) = clauses.get("LENGTH")
    {
        if !tokens
            .first()
            .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
            || !tokens
                .get(1)
                .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
        {
            return Err(ResolutionFailure::Invalid(
                "CICS SEND TEXT LENGTH requires LENGTH OF a data area".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: HirCicsValue::LengthOf(complete_data_reference(&tokens[2..], semantic)?),
        });
    }
    Ok(operands)
}
