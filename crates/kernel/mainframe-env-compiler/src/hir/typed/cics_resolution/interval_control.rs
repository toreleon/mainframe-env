use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, require_numeric, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    match operation {
        HirCicsOperation::Delay => {
            let relative = options.iter().any(|option| option == "FOR");
            let absolute = options.iter().any(|option| option == "UNTIL");
            let units = ["HOURS", "MINUTES", "SECONDS", "MILLISECS"]
                .into_iter()
                .any(|name| clauses.contains_key(name));
            let schedules = usize::from(clauses.contains_key("INTERVAL"))
                + usize::from(clauses.contains_key("TIME"))
                + usize::from(relative)
                + usize::from(absolute);
            if schedules > 1 || (relative || absolute) != units {
                return Err(ResolutionFailure::Invalid(
                    "CICS DELAY accepts one INTERVAL, TIME, or FOR/UNTIL explicit-unit schedule"
                        .into(),
                ));
            }
            if absolute && clauses.contains_key("MILLISECS") {
                return Err(ResolutionFailure::Invalid(
                    "CICS DELAY MILLISECS is valid only with FOR".into(),
                ));
            }
            if clauses.contains_key("REQID")
                && !relative
                && !absolute
                && !clauses.contains_key("TIME")
                && clauses.get("INTERVAL").is_none_or(|tokens| {
                    tokens.first().and_then(|value| value.parse::<i64>().ok()) == Some(0)
                })
            {
                return Err(ResolutionFailure::Invalid(
                    "typed CICS DELAY REQID requires a positive schedule".into(),
                ));
            }
        }
        HirCicsOperation::Post => {
            let after = options.iter().any(|option| option == "AFTER");
            let at = options.iter().any(|option| option == "AT");
            let units = ["HOURS", "MINUTES", "SECONDS"]
                .into_iter()
                .any(|name| clauses.contains_key(name));
            let schedules = usize::from(clauses.contains_key("INTERVAL"))
                + usize::from(clauses.contains_key("TIME"))
                + usize::from(after)
                + usize::from(at);
            if schedules > 1 || (after || at) != units {
                return Err(ResolutionFailure::Invalid(
                    "CICS POST accepts one INTERVAL, TIME, AFTER, or AT schedule".into(),
                ));
            }
        }
        HirCicsOperation::Start => {
            let after = options.iter().any(|option| option == "AFTER");
            let at = options.iter().any(|option| option == "AT");
            let units = ["HOURS", "MINUTES", "SECONDS"]
                .into_iter()
                .any(|name| clauses.contains_key(name));
            let schedule_selectors = usize::from(clauses.contains_key("INTERVAL"))
                + usize::from(clauses.contains_key("TIME"))
                + usize::from(after)
                + usize::from(at);
            if schedule_selectors > 1 {
                return Err(ResolutionFailure::Invalid(
                    "CICS START accepts exactly one INTERVAL, TIME, AFTER, or AT schedule".into(),
                ));
            }
            if (after || at) != units {
                return Err(ResolutionFailure::Invalid(
                    "CICS START AFTER or AT requires at least one explicit time unit".into(),
                ));
            }
            if clauses.contains_key("LENGTH") && !clauses.contains_key("FROM") {
                return Err(ResolutionFailure::Invalid(
                    "CICS START LENGTH requires FROM".into(),
                ));
            }
            if options.iter().any(|option| option == "FMH") && !clauses.contains_key("FROM") {
                return Err(ResolutionFailure::Invalid(
                    "CICS START FMH requires FROM".into(),
                ));
            }
        }
        HirCicsOperation::Retrieve
            if clauses.contains_key("INTO") == clauses.contains_key("SET")
                || !clauses.contains_key("LENGTH") =>
        {
            return Err(ResolutionFailure::Invalid(
                "typed CICS RETRIEVE requires exactly one of INTO or SET plus LENGTH".into(),
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
        HirCicsOperation::Delay => delay_operands(clauses, semantic, "DELAY"),
        HirCicsOperation::Post => delay_operands(clauses, semantic, "POST"),
        HirCicsOperation::Start | HirCicsOperation::StartAttach => {
            start_operands(clauses, semantic)
        }
        HirCicsOperation::Retrieve => retrieve_operands(clauses, semantic),
        _ => Ok(Vec::new()),
    }
}

fn delay_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
    operation: &str,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = clauses
        .get("INTERVAL")
        .map(|value| {
            Ok::<_, ResolutionFailure>(vec![HirCicsNamedOperand {
                name: HirCicsOperandName::Interval,
                value: cics_integer_value(value, semantic)?,
            }])
        })
        .unwrap_or_else(|| Ok(Vec::new()))?;
    if let Some(value) = clauses.get("TIME") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::StartTime,
            value: cics_integer_value(value, semantic)?,
        });
    }
    if let Some(request_id) = clauses.get("REQID") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::ReqId,
            value: bounded_name(request_id, semantic, 8, operation, "REQID")?,
        });
    }
    for (clause, name) in [
        ("HOURS", HirCicsOperandName::Hours),
        ("MINUTES", HirCicsOperandName::Minutes),
        ("SECONDS", HirCicsOperandName::Seconds),
        ("MILLISECS", HirCicsOperandName::Milliseconds),
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
    if clauses.contains_key("TERMID") && clauses.contains_key("USERID") {
        return Err(ResolutionFailure::Unsupported);
    }
    let transaction = bounded_name(&clauses["TRANSID"], semantic, 4, "START", "TRANSID")?;
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::TransId,
        value: transaction,
    }];
    if let Some(source) = clauses.get("FROM") {
        let HirCicsValue::Data(from) = cics_value(source, semantic)? else {
            return Err(ResolutionFailure::Invalid(
                "CICS START FROM requires a data area".into(),
            ));
        };
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::From,
            value: HirCicsValue::Data(from),
        });
    }
    if let Some(request_id) = clauses.get("REQID") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::ReqId,
            value: bounded_name(request_id, semantic, 8, "START", "REQID")?,
        });
    }
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
    for (clause, name) in [
        ("HOURS", HirCicsOperandName::Hours),
        ("MINUTES", HirCicsOperandName::Minutes),
        ("SECONDS", HirCicsOperandName::Seconds),
    ] {
        if let Some(value) = clauses.get(clause) {
            operands.push(HirCicsNamedOperand {
                name,
                value: cics_integer_value(value, semantic)?,
            });
        }
    }
    for (clause, name, max) in [
        ("TERMID", HirCicsOperandName::TermId, 4),
        ("RTRANSID", HirCicsOperandName::ReturnTransId, 4),
        ("RTERMID", HirCicsOperandName::ReturnTermId, 4),
        ("QUEUE", HirCicsOperandName::Queue, 8),
        ("USERID", HirCicsOperandName::UserId, 8),
    ] {
        if let Some(value) = clauses.get(clause) {
            operands.push(HirCicsNamedOperand {
                name,
                value: bounded_name(value, semantic, max, "START", clause)?,
            });
        }
    }
    Ok(operands)
}

fn retrieve_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    for (name, length) in [("RTRANSID", 4), ("RTERMID", 4), ("QUEUE", 8)] {
        if let Some(value) = clauses.get(name) {
            let reference = complete_data_reference(value, semantic)?;
            require_writable(&reference)?;
            if reference.length != length
                || !matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS RETRIEVE {name} requires an exact {length}-character writable area"
                )));
            }
        }
    }
    let reference = complete_data_reference(&clauses["LENGTH"], semantic)?;
    require_numeric(&reference)?;
    require_writable(&reference)?;
    Ok(clauses
        .contains_key("INTO")
        .then_some(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: HirCicsValue::Data(reference),
        })
        .into_iter()
        .collect())
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
