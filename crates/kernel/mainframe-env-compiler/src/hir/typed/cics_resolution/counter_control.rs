use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsValue,
    Resolution, ResolutionFailure,
};
use super::{Clauses, cics_integer_value, cics_value};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const fn is_counter(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::DefineCounter
            | HirCicsOperation::DefineDCounter
            | HirCicsOperation::DeleteCounter
            | HirCicsOperation::DeleteDCounter
    )
}

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::DefineCounter => &[
            "COUNTER", "POOL", "VALUE", "MINIMUM", "MAXIMUM", "RESP", "RESP2",
        ],
        HirCicsOperation::DefineDCounter => &[
            "DCOUNTER", "POOL", "VALUE", "MINIMUM", "MAXIMUM", "RESP", "RESP2",
        ],
        HirCicsOperation::DeleteCounter => &["COUNTER", "POOL", "RESP", "RESP2"],
        HirCicsOperation::DeleteDCounter => &["DCOUNTER", "POOL", "RESP", "RESP2"],
        _ => unreachable!("counter clause contract requested for another operation"),
    }
}

pub(super) fn allowed_options() -> &'static [&'static str] {
    &["NOSUSPEND", "NOHANDLE"]
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::DefineCounter | HirCicsOperation::DeleteCounter => &["COUNTER"],
        HirCicsOperation::DefineDCounter | HirCicsOperation::DeleteDCounter => &["DCOUNTER"],
        _ => unreachable!("counter required clause contract requested for another operation"),
    }
}

pub(super) fn option(operation: HirCicsOperation, option: &str) -> Option<HirCicsOption> {
    (is_counter(operation) && option == "NOSUSPEND").then_some(HirCicsOption::CounterNoSuspend)
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_counter(operation) {
        return Ok(Vec::new());
    }
    if matches!(
        operation,
        HirCicsOperation::DefineCounter | HirCicsOperation::DefineDCounter
    ) && clauses.contains_key("MINIMUM")
        && !clauses.contains_key("VALUE")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DEFINE COUNTER MINIMUM requires VALUE".into(),
        ));
    }
    let selector = required(operation)[0];
    let name = cics_value(&clauses[selector], semantic)?;
    validate_text(&name, 16, true, selector)?;
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::CounterName,
        value: name,
    }];
    if let Some(tokens) = clauses.get("POOL") {
        let value = cics_value(tokens, semantic)?;
        validate_text(&value, 8, false, "POOL")?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::CounterPool,
            value,
        });
    }
    for (label, name) in [
        ("VALUE", HirCicsOperandName::CounterValue),
        ("MINIMUM", HirCicsOperandName::CounterMinimum),
        ("MAXIMUM", HirCicsOperandName::CounterMaximum),
    ] {
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        let value = cics_integer_value(tokens, semantic)?;
        validate_number(&value, operation, label)?;
        operands.push(HirCicsNamedOperand { name, value });
    }
    Ok(operands)
}

fn validate_text(
    value: &HirCicsValue,
    length: usize,
    counter: bool,
    label: &str,
) -> Resolution<()> {
    let valid = match value {
        HirCicsValue::Literal(text) => {
            let bytes = text.as_bytes();
            let trimmed = bytes.trim_ascii_end();
            bytes.len() <= length
                && (!counter || !trimmed.is_empty())
                && (!counter || !matches!(trimmed[0], b'0'..=b'9' | b'_'))
                && trimmed.iter().all(|byte| {
                    byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"_$#@".contains(byte)
                })
        }
        HirCicsValue::Data(reference) => {
            reference.length == length
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS counter {label} requires a valid {length}-character name"
        )))
    }
}

fn validate_number(
    value: &HirCicsValue,
    operation: HirCicsOperation,
    label: &str,
) -> Resolution<()> {
    let doubleword = operation == HirCicsOperation::DefineDCounter;
    let valid = match value {
        HirCicsValue::Integer(value) => {
            *value >= 0 && (doubleword || *value <= i64::from(i32::MAX))
        }
        HirCicsValue::Data(reference) => {
            reference.usage == CobolUsage::Binary
                && reference.length == if doubleword { 8 } else { 4 }
                && reference.scale == 0
                && reference.signed != doubleword
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} {label} requires {} binary value",
            if doubleword {
                "unsigned doubleword"
            } else {
                "signed fullword"
            }
        )))
    }
}
