use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, HirDataReference,
    Resolution, ResolutionFailure,
};
use super::{Clauses, complete_data_reference, numeric_value::cics_integer_value};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<()> {
    if operation == HirCicsOperation::Freemain {
        let (name, tokens) = match (clauses.get("DATA"), clauses.get("DATAPOINTER")) {
            (Some(tokens), None) => ("DATA", tokens),
            (None, Some(tokens)) => ("DATAPOINTER", tokens),
            _ => {
                return Err(ResolutionFailure::Invalid(
                    "CICS FREEMAIN requires exactly one of DATA or DATAPOINTER".into(),
                ));
            }
        };
        let storage = freemain_reference(name, tokens, semantic)?;
        if name == "DATAPOINTER"
            && !matches!(storage.usage, CobolUsage::Pointer | CobolUsage::Pointer32)
        {
            return Err(ResolutionFailure::Invalid(
                "CICS FREEMAIN DATAPOINTER requires POINTER or POINTER-32 storage".into(),
            ));
        }
        return Ok(());
    } else if operation != HirCicsOperation::Getmain {
        return Ok(());
    }
    if !clauses.contains_key("SET") {
        return Err(ResolutionFailure::Invalid(
            "CICS GETMAIN requires SET".into(),
        ));
    }
    let (name, tokens) = match (clauses.get("FLENGTH"), clauses.get("LENGTH")) {
        (Some(tokens), None) => ("FLENGTH", tokens),
        (None, Some(tokens)) => ("LENGTH", tokens),
        _ => {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN requires exactly one of FLENGTH or LENGTH".into(),
            ));
        }
    };
    let length = cics_integer_value(tokens, semantic)?;
    match (name, length) {
        ("FLENGTH", HirCicsValue::Data(reference))
            if reference.usage != CobolUsage::Binary
                || reference.length != 4
                || reference.scale != 0 =>
        {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN FLENGTH requires fullword binary storage".into(),
            ));
        }
        ("FLENGTH", HirCicsValue::Integer(value)) if i32::try_from(value).is_err() => {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN FLENGTH literal must fit a signed fullword".into(),
            ));
        }
        ("LENGTH", HirCicsValue::Data(reference))
            if reference.usage != CobolUsage::Binary
                || reference.length != 2
                || reference.scale != 0
                || reference.signed =>
        {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN LENGTH requires unsigned halfword binary storage".into(),
            ));
        }
        ("LENGTH", HirCicsValue::Integer(value)) if !(0..=65_520).contains(&value) => {
            return Err(ResolutionFailure::Invalid(
                "CICS GETMAIN LENGTH literal must be between 0 and 65520".into(),
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
    if operation == HirCicsOperation::Freemain {
        let (name, tokens) = if let Some(tokens) = clauses.get("DATA") {
            (HirCicsOperandName::DataArea, tokens)
        } else {
            (HirCicsOperandName::DataPointer, &clauses["DATAPOINTER"])
        };
        return Ok(vec![HirCicsNamedOperand {
            name,
            value: HirCicsValue::Data(freemain_reference(
                if name == HirCicsOperandName::DataArea {
                    "DATA"
                } else {
                    "DATAPOINTER"
                },
                tokens,
                semantic,
            )?),
        }]);
    } else if operation != HirCicsOperation::Getmain {
        return Ok(Vec::new());
    }
    let (name, tokens) = if let Some(tokens) = clauses.get("FLENGTH") {
        (HirCicsOperandName::Flength, tokens)
    } else {
        (HirCicsOperandName::Length, &clauses["LENGTH"])
    };
    let mut operands = vec![HirCicsNamedOperand {
        name,
        value: cics_integer_value(tokens, semantic)?,
    }];
    if let Some(tokens) = clauses.get("INITIMG") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::InitImage,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    Ok(operands)
}

fn freemain_reference(
    name: &str,
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<HirDataReference> {
    match complete_data_reference(tokens, semantic) {
        Err(ResolutionFailure::Unsupported) => Err(ResolutionFailure::Invalid(format!(
            "CICS FREEMAIN {name} requires a data reference"
        ))),
        result => result,
    }
}
