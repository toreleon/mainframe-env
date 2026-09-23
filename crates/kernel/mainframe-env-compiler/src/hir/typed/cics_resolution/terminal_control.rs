use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_value, complete_data_reference, numeric_value::cics_integer_value};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if operation == HirCicsOperation::ReceivePartn {
        if !clauses.contains_key("PARTN")
            || clauses.contains_key("INTO") && clauses.contains_key("SET")
            || clauses.contains_key("INTO") != clauses.contains_key("LENGTH")
        {
            return Err(ResolutionFailure::Invalid(
                "CICS RECEIVE PARTN requires PARTN and pairs INTO with LENGTH; INTO and SET are exclusive"
                    .into(),
            ));
        }
    }
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
    if operation == HirCicsOperation::SendMap
        && clauses.contains_key("LENGTH")
        && !clauses.contains_key("FROM")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SEND MAP LENGTH requires an explicit FROM data area".into(),
        ));
    }
    if operation == HirCicsOperation::ReceiveMap
        && clauses.contains_key("LENGTH")
        && !clauses.contains_key("FROM")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS RECEIVE MAP LENGTH requires an explicit FROM data area".into(),
        ));
    }
    if operation == HirCicsOperation::ReceiveMap
        && options.iter().any(|option| option == "TERMINAL")
        && clauses.contains_key("FROM")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS RECEIVE MAP TERMINAL does not accept FROM".into(),
        ));
    }
    if operation == HirCicsOperation::SendMap
        && options.iter().any(|option| option == "DATAONLY")
        && options.iter().any(|option| option == "MAPONLY")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SEND MAP DATAONLY and MAPONLY are mutually exclusive".into(),
        ));
    }
    if operation == HirCicsOperation::SendMap
        && options.iter().any(|option| option == "MAPONLY")
        && (clauses.contains_key("FROM") || clauses.contains_key("LENGTH"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SEND MAP MAPONLY does not accept FROM or LENGTH".into(),
        ));
    }
    if operation == HirCicsOperation::SendMap
        && options.iter().any(|option| option == "DATAONLY")
        && !clauses.contains_key("FROM")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SEND MAP DATAONLY requires an explicit FROM data area".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation == HirCicsOperation::ReceivePartn {
        let Some(tokens) = clauses.get("LENGTH") else {
            return Ok(Vec::new());
        };
        let HirCicsValue::Data(reference) = cics_value(tokens, semantic)? else {
            return Err(ResolutionFailure::Invalid(
                "CICS RECEIVE PARTN LENGTH requires a halfword binary data area".into(),
            ));
        };
        if reference.usage != CobolUsage::Binary || reference.length != 2 || reference.scale != 0 {
            return Err(ResolutionFailure::Invalid(
                "CICS RECEIVE PARTN LENGTH requires a halfword binary data area".into(),
            ));
        }
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: HirCicsValue::Data(reference),
        }]);
    }
    if operation == HirCicsOperation::SendPartnset {
        let Some(tokens) = clauses.get("PARTNSET") else {
            return Ok(Vec::new());
        };
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(value) => {
                matches!(value.len(), 1..=8)
                    && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            HirCicsValue::Data(reference) => {
                matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                ) && matches!(reference.length, 1..=8)
            }
            HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS SEND PARTNSET requires a 1-8 character partition-set name".into(),
            ));
        }
        return Ok(vec![HirCicsNamedOperand {
            name: HirCicsOperandName::Partnset,
            value,
        }]);
    }
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
    if let Some(tokens) = clauses.get("LENGTH") {
        let value = if tokens
            .first()
            .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
            && tokens
                .get(1)
                .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
        {
            HirCicsValue::LengthOf(complete_data_reference(&tokens[2..], semantic)?)
        } else if matches!(
            operation,
            HirCicsOperation::ReceiveMap | HirCicsOperation::SendMap | HirCicsOperation::SendText
        ) {
            let value = cics_integer_value(tokens, semantic)?;
            match &value {
                HirCicsValue::Data(reference)
                    if reference.usage != CobolUsage::Binary
                        || reference.length != 2
                        || reference.scale != 0 =>
                {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS {operation:?} LENGTH requires halfword binary storage"
                    )));
                }
                HirCicsValue::Integer(value) if !(0..=32_767).contains(value) => {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS {operation:?} LENGTH literal must be between 0 and 32767"
                    )));
                }
                _ => {}
            }
            value
        } else {
            unreachable!("LENGTH is admitted only for RECEIVE MAP, SEND MAP, or SEND TEXT")
        };
        if operation == HirCicsOperation::ReceiveMap
            && let HirCicsValue::LengthOf(length) = &value
            && !operands.iter().any(|operand| {
                operand.name == HirCicsOperandName::From
                    && matches!(&operand.value, HirCicsValue::Data(from) if from == length)
            })
        {
            return Err(ResolutionFailure::Invalid(
                "CICS ReceiveMap LENGTH OF must name the FROM data area".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value,
        });
    }
    Ok(operands)
}
