use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, complete_data_reference, numeric_value::cics_integer_value};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if operation != HirCicsOperation::Getmain {
        return Ok(());
    }
    for required in ["SET", "FLENGTH"] {
        if !clauses.contains_key(required) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS GETMAIN requires {required}"
            )));
        }
    }
    let length = cics_integer_value(&clauses["FLENGTH"], semantic)?;
    match length {
        HirCicsValue::Data(reference)
            if reference.usage != CobolUsage::Binary
                || reference.length != 4
                || reference.scale != 0 =>
        {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN FLENGTH requires fullword binary storage".into(),
            ));
        }
        HirCicsValue::Integer(value) if i32::try_from(value).is_err() => {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN FLENGTH literal must fit a signed fullword".into(),
            ));
        }
        _ => {}
    }
    if let Some(tokens) = clauses.get("INITIMG") {
        let image = complete_data_reference(tokens, semantic)?;
        if image.length != 1
            || !matches!(
                image.category,
                DataCategory::Alphabetic | DataCategory::Alphanumeric
            )
        {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN INITIMG requires a one-byte character data area".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::Getmain {
        return Ok(Vec::new());
    }
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Flength,
        value: cics_integer_value(&clauses["FLENGTH"], semantic)?,
    }];
    if let Some(tokens) = clauses.get("INITIMG") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::InitImage,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    Ok(operands)
}
