use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const fn is_counter(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::DefineCounter
            | HirCicsOperation::DefineDCounter
            | HirCicsOperation::DeleteCounter
            | HirCicsOperation::DeleteDCounter
            | HirCicsOperation::GetCounter
            | HirCicsOperation::GetDCounter
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
        HirCicsOperation::GetCounter => &[
            "COUNTER",
            "POOL",
            "VALUE",
            "INCREMENT",
            "COMPAREMIN",
            "COMPAREMAX",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::GetDCounter => &[
            "DCOUNTER",
            "POOL",
            "VALUE",
            "INCREMENT",
            "COMPAREMIN",
            "COMPAREMAX",
            "RESP",
            "RESP2",
        ],
        _ => unreachable!("counter clause contract requested for another operation"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    if matches!(
        operation,
        HirCicsOperation::GetCounter | HirCicsOperation::GetDCounter
    ) {
        &["NOSUSPEND", "NOHANDLE", "REDUCE", "WRAP"]
    } else {
        &["NOSUSPEND", "NOHANDLE"]
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::DefineCounter | HirCicsOperation::DeleteCounter => &["COUNTER"],
        HirCicsOperation::DefineDCounter | HirCicsOperation::DeleteDCounter => &["DCOUNTER"],
        HirCicsOperation::GetCounter => &["COUNTER", "VALUE"],
        HirCicsOperation::GetDCounter => &["DCOUNTER", "VALUE"],
        _ => unreachable!("counter required clause contract requested for another operation"),
    }
}

pub(super) fn option(operation: HirCicsOperation, option: &str) -> Option<HirCicsOption> {
    match option {
        "NOSUSPEND" if is_counter(operation) => Some(HirCicsOption::CounterNoSuspend),
        "REDUCE"
            if matches!(
                operation,
                HirCicsOperation::GetCounter | HirCicsOperation::GetDCounter
            ) =>
        {
            Some(HirCicsOption::CounterReduce)
        }
        "WRAP"
            if matches!(
                operation,
                HirCicsOperation::GetCounter | HirCicsOperation::GetDCounter
            ) =>
        {
            Some(HirCicsOption::CounterWrap)
        }
        _ => None,
    }
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
    let numeric: &[(&str, HirCicsOperandName)] = if matches!(
        operation,
        HirCicsOperation::GetCounter | HirCicsOperation::GetDCounter
    ) {
        &[
            ("INCREMENT", HirCicsOperandName::CounterIncrement),
            ("COMPAREMIN", HirCicsOperandName::CounterCompareMin),
            ("COMPAREMAX", HirCicsOperandName::CounterCompareMax),
        ]
    } else {
        &[
            ("VALUE", HirCicsOperandName::CounterValue),
            ("MINIMUM", HirCicsOperandName::CounterMinimum),
            ("MAXIMUM", HirCicsOperandName::CounterMaximum),
        ]
    };
    for &(label, name) in numeric {
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        let value = cics_integer_value(tokens, semantic)?;
        validate_number(&value, operation, label)?;
        operands.push(HirCicsNamedOperand { name, value });
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !matches!(
        operation,
        HirCicsOperation::GetCounter | HirCicsOperation::GetDCounter
    ) {
        return Ok(Vec::new());
    }
    let target = complete_data_reference(&clauses["VALUE"], semantic)?;
    require_writable(&target)?;
    let doubleword = operation == HirCicsOperation::GetDCounter;
    if target.usage != CobolUsage::Binary
        || target.length != if doubleword { 8 } else { 4 }
        || target.scale != 0
        || target.signed == doubleword
    {
        return Err(ResolutionFailure::Invalid(
            "CICS GET counter VALUE requires matching signed fullword or unsigned doubleword storage"
                .into(),
        ));
    }
    Ok(vec![HirCicsOutputBinding {
        name: HirCicsOutputName::CounterValue,
        target,
    }])
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
    let doubleword = matches!(
        operation,
        HirCicsOperation::DefineDCounter | HirCicsOperation::GetDCounter
    );
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
