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
            | HirCicsOperation::QueryCounter
            | HirCicsOperation::QueryDCounter
            | HirCicsOperation::RewindCounter
            | HirCicsOperation::RewindDCounter
            | HirCicsOperation::UpdateCounter
            | HirCicsOperation::UpdateDCounter
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
        HirCicsOperation::QueryCounter => &[
            "COUNTER", "POOL", "VALUE", "MINIMUM", "MAXIMUM", "RESP", "RESP2",
        ],
        HirCicsOperation::QueryDCounter => &[
            "DCOUNTER", "POOL", "VALUE", "MINIMUM", "MAXIMUM", "RESP", "RESP2",
        ],
        HirCicsOperation::RewindCounter => &["COUNTER", "POOL", "INCREMENT", "RESP", "RESP2"],
        HirCicsOperation::RewindDCounter => &["DCOUNTER", "POOL", "INCREMENT", "RESP", "RESP2"],
        HirCicsOperation::UpdateCounter => &[
            "COUNTER",
            "POOL",
            "VALUE",
            "COMPAREMIN",
            "COMPAREMAX",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::UpdateDCounter => &[
            "DCOUNTER",
            "POOL",
            "VALUE",
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
        HirCicsOperation::QueryCounter => &["COUNTER"],
        HirCicsOperation::QueryDCounter => &["DCOUNTER"],
        HirCicsOperation::RewindCounter => &["COUNTER"],
        HirCicsOperation::RewindDCounter => &["DCOUNTER"],
        HirCicsOperation::UpdateCounter => &["COUNTER", "VALUE"],
        HirCicsOperation::UpdateDCounter => &["DCOUNTER", "VALUE"],
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
    } else if matches!(
        operation,
        HirCicsOperation::RewindCounter | HirCicsOperation::RewindDCounter
    ) {
        &[("INCREMENT", HirCicsOperandName::CounterIncrement)]
    } else if matches!(
        operation,
        HirCicsOperation::UpdateCounter | HirCicsOperation::UpdateDCounter
    ) {
        &[
            ("VALUE", HirCicsOperandName::CounterValue),
            ("COMPAREMIN", HirCicsOperandName::CounterCompareMin),
            ("COMPAREMAX", HirCicsOperandName::CounterCompareMax),
        ]
    } else if matches!(
        operation,
        HirCicsOperation::DefineCounter | HirCicsOperation::DefineDCounter
    ) {
        &[
            ("VALUE", HirCicsOperandName::CounterValue),
            ("MINIMUM", HirCicsOperandName::CounterMinimum),
            ("MAXIMUM", HirCicsOperandName::CounterMaximum),
        ]
    } else {
        &[]
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
        HirCicsOperation::GetCounter
            | HirCicsOperation::GetDCounter
            | HirCicsOperation::QueryCounter
            | HirCicsOperation::QueryDCounter
    ) {
        return Ok(Vec::new());
    }
    let doubleword = matches!(
        operation,
        HirCicsOperation::GetDCounter | HirCicsOperation::QueryDCounter
    );
    let labels: &[(&str, HirCicsOutputName)] = if matches!(
        operation,
        HirCicsOperation::GetCounter | HirCicsOperation::GetDCounter
    ) {
        &[("VALUE", HirCicsOutputName::CounterValue)]
    } else {
        &[
            ("VALUE", HirCicsOutputName::CounterValue),
            ("MINIMUM", HirCicsOutputName::CounterMinimum),
            ("MAXIMUM", HirCicsOutputName::CounterMaximum),
        ]
    };
    let mut outputs = Vec::new();
    for &(label, name) in labels {
        let Some(tokens) = clauses.get(label) else {
            continue;
        };
        let target = complete_data_reference(tokens, semantic)?;
        require_writable(&target)?;
        if target.usage != CobolUsage::Binary
            || target.length != if doubleword { 8 } else { 4 }
            || target.scale != 0
            || target.signed == doubleword
        {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS counter {label} requires matching signed fullword or unsigned doubleword storage"
            )));
        }
        outputs.push(HirCicsOutputBinding { name, target });
    }
    Ok(outputs)
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
        HirCicsOperation::DefineDCounter
            | HirCicsOperation::GetDCounter
            | HirCicsOperation::RewindDCounter
            | HirCicsOperation::UpdateDCounter
    );
    let valid = match value {
        HirCicsValue::Integer(value) => {
            (*value >= 0 && (doubleword || *value <= i64::from(i32::MAX)))
                || (!doubleword && label == "VALUE" && *value == i64::from(i32::MIN))
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
