use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, require_numeric, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    match operation {
        HirCicsOperation::Start => {
            if clauses.contains_key("INTERVAL") && clauses.contains_key("TIME") {
                return Err(ResolutionFailure::Invalid(
                    "CICS START INTERVAL and TIME are mutually exclusive".into(),
                ));
            }
            if clauses.contains_key("LENGTH") && !clauses.contains_key("FROM") {
                return Err(ResolutionFailure::Invalid(
                    "CICS START LENGTH requires FROM".into(),
                ));
            }
        }
        HirCicsOperation::Retrieve
            if !clauses.contains_key("INTO") || !clauses.contains_key("LENGTH") =>
        {
            return Err(ResolutionFailure::Invalid(
                "typed CICS RETRIEVE requires INTO and LENGTH".into(),
            ));
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    match operation {
        HirCicsOperation::Cancel => cancel_operands(clauses, semantic),
        HirCicsOperation::Start => start_operands(clauses, semantic),
        HirCicsOperation::Retrieve => retrieve_operands(clauses, semantic),
        _ => Ok(Vec::new()),
    }
}

fn cancel_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::ReqId,
        value: bounded_name(&clauses["REQID"], semantic, 8, "CANCEL", "REQID")?,
    }];
    if let Some(transaction) = clauses.get("TRANSID") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TransId,
            value: bounded_name(transaction, semantic, 4, "CANCEL", "TRANSID")?,
        });
    }
    Ok(operands)
}

fn start_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let transaction = bounded_name(&clauses["TRANSID"], semantic, 4, "START", "TRANSID")?;
    let request_id = bounded_name(&clauses["REQID"], semantic, 8, "START", "REQID")?;
    let HirCicsValue::Data(from) = cics_value(&clauses["FROM"], semantic)? else {
        return Err(ResolutionFailure::Invalid(
            "CICS START FROM requires a data area".into(),
        ));
    };
    let mut operands = vec![
        HirCicsNamedOperand {
            name: HirCicsOperandName::TransId,
            value: transaction,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::ReqId,
            value: request_id,
        },
        HirCicsNamedOperand {
            name: HirCicsOperandName::From,
            value: HirCicsValue::Data(from),
        },
    ];
    if let Some(length) = clauses.get("LENGTH") {
        let value = if length
            .first()
            .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
            && length
                .get(1)
                .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
        {
            HirCicsValue::LengthOf(complete_data_reference(&length[2..], semantic)?)
        } else {
            cics_integer_value(length, semantic)?
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value,
        });
    }
    for (clause, name) in [
        ("INTERVAL", HirCicsOperandName::Interval),
        ("TIME", HirCicsOperandName::StartTime),
    ] {
        if let Some(value) = clauses.get(clause) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_integer_value(value, semantic)?,
            });
        }
    }
    Ok(operands)
}

fn retrieve_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let length = &clauses["LENGTH"];
    let reference = complete_data_reference(length, semantic)?;
    require_numeric(&reference)?;
    require_writable(&reference)?;
    Ok(vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Length,
        value: HirCicsValue::Data(reference),
    }])
}

fn bounded_name(
    tokens: &[String],
    semantic: &SemanticModel,
    max: usize,
    command: &str,
    label: &str,
) -> Resolution<HirCicsValue> {
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            matches!(value.len(), 1..=8)
                && value.len() <= max
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'$' | b'#' | b'@')
                })
        }
        HirCicsValue::Data(reference) => {
            matches!(reference.length, 1..=8)
                && reference.length <= max
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        HirCicsValue::Integer(_) | HirCicsValue::LengthOf(_) => false,
    };
    if valid {
        Ok(value)
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {command} {label} requires a 1-{max} character name"
        )))
    }
}
