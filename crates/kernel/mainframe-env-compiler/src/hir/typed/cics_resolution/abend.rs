use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsValue, Resolution, ResolutionFailure,
};
use super::{Clauses, cics_value};
use crate::{DataCategory, SemanticModel};

pub(super) fn operand(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Option<HirCicsNamedOperand>> {
    let Some(tokens) = clauses.get("ABCODE") else {
        return Ok(None);
    };
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => matches!(value.len(), 1..=4),
        HirCicsValue::Data(reference) => {
            matches!(reference.length, 1..=4)
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
    };
    if !valid {
        return Err(ResolutionFailure::Invalid(
            "CICS ABEND ABCODE requires a 1-4 character value".into(),
        ));
    }
    Ok(Some(HirCicsNamedOperand {
        name: HirCicsOperandName::Abcode,
        value,
    }))
}
