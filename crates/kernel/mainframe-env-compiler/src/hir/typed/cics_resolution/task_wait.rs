use super::{
    Clauses, HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, cics_integer_value, cics_value, complete_data_reference,
};
use crate::{CobolUsage, DataCategory, SemanticModel};
use std::collections::BTreeMap;

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if operation != HirCicsOperation::WaitExternal {
        return Ok(());
    }
    let purge_selectors = usize::from(clauses.contains_key("PURGEABILITY"))
        + options
            .iter()
            .filter(|option| matches!(option.as_str(), "PURGEABLE" | "NOTPURGEABLE"))
            .count();
    if purge_selectors > 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS WAIT EXTERNAL accepts one PURGEABLE, NOTPURGEABLE, or PURGEABILITY selector"
                .into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &BTreeMap<String, Vec<String>>,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation == HirCicsOperation::WaitExternal {
        return external_operands(clauses, semantic);
    }
    if operation != HirCicsOperation::WaitEvent {
        return Ok(Vec::new());
    }
    let pointer = complete_data_reference(&clauses["ECADDR"], semantic)?;
    if !matches!(pointer.usage, CobolUsage::Pointer | CobolUsage::Pointer32) || pointer.length != 4
    {
        return Err(ResolutionFailure::Invalid(
            "CICS WAIT EVENT ECADDR requires a four-byte POINTER or POINTER-32 reference".into(),
        ));
    }
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::EventControlAddress,
        value: HirCicsValue::Data(pointer),
    }];
    if let Some(tokens) = clauses.get("NAME") {
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(value) => {
                matches!(value.len(), 1..=8)
                    && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            HirCicsValue::Data(reference) => {
                matches!(reference.length, 1..=8)
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT EVENT NAME must be 1-8 alphanumeric characters".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WaitName,
            value,
        });
    }
    Ok(operands)
}

fn external_operands(
    clauses: &BTreeMap<String, Vec<String>>,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let list = complete_data_reference(&clauses["ECBLIST"], semantic)?;
    if list.usage != CobolUsage::Pointer32 || list.length != 4 {
        return Err(ResolutionFailure::Invalid(
            "CICS WAIT EXTERNAL ECBLIST requires a four-byte POINTER-32 reference".into(),
        ));
    }
    let count = cics_integer_value(&clauses["NUMEVENTS"], semantic)?;
    match &count {
        HirCicsValue::Data(reference)
            if reference.usage != CobolUsage::Binary
                || reference.length != 4
                || reference.scale != 0 =>
        {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT EXTERNAL NUMEVENTS requires fullword binary storage".into(),
            ));
        }
        HirCicsValue::Integer(value) if i32::try_from(*value).is_err() => {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT EXTERNAL NUMEVENTS literal must fit a signed fullword".into(),
            ));
        }
        _ => {}
    }
    let mut operands = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::EcbList,
            value: HirCicsValue::Data(list),
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::NumEvents,
            value: count,
        },
    ];
    if let Some(tokens) = clauses.get("PURGEABILITY") {
        let value = if let [function, open, value, close] = tokens.as_slice()
            && function.eq_ignore_ascii_case("DFHVALUE")
            && open == "("
            && close == ")"
            && matches!(value.as_str(), "PURGEABLE" | "NOTPURGEABLE")
        {
            HirCicsValue::Literal(value.clone())
        } else {
            let reference = complete_data_reference(tokens, semantic)?;
            if reference.usage != CobolUsage::Binary
                || reference.length != 4
                || reference.scale != 0
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS WAIT EXTERNAL PURGEABILITY requires a fullword binary CVDA".into(),
                ));
            }
            HirCicsValue::Data(reference)
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Purgeability,
            value,
        });
    }
    if let Some(tokens) = clauses.get("NAME") {
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(value) => {
                matches!(value.len(), 1..=8)
                    && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            HirCicsValue::Data(reference) => {
                matches!(reference.length, 1..=8)
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT EXTERNAL NAME must be 1-8 alphanumeric characters".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WaitName,
            value,
        });
    }
    Ok(operands)
}
