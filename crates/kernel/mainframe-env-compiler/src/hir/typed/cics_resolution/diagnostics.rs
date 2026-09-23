use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_value, complete_data_reference, numeric_value::cics_integer_value};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &[
            "TRACENUM",
            "FROM",
            "FROMLENGTH",
            "RESOURCE",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::Monitor => &["POINT", "DATA1", "DATA2", "ENTRYNAME", "RESP", "RESP2"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &["EXCEPTION", "NOHANDLE"],
        HirCicsOperation::Monitor => &["NOHANDLE"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &["TRACENUM"],
        HirCicsOperation::Monitor => &["POINT"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation == HirCicsOperation::Monitor {
        return monitor_operands(clauses, semantic);
    }
    if operation != HirCicsOperation::EnterTraceNum {
        return Ok(Vec::new());
    }
    let number = cics_integer_value(&clauses["TRACENUM"], semantic)?;
    if let HirCicsValue::Data(reference) = &number {
        require_halfword("ENTER TRACENUM", "TRACENUM", reference)?;
    }
    let mut result = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::TraceNum,
        value: number,
    }];
    if let Some(tokens) = clauses.get("FROM") {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TraceFrom,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    if let Some(tokens) = clauses.get("FROMLENGTH") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_halfword("ENTER TRACENUM", "FROMLENGTH", &reference)?;
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TraceFromLength,
            value: HirCicsValue::Data(reference),
        });
    }
    if let Some(tokens) = clauses.get("RESOURCE") {
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(text) => text.len() == 8,
            HirCicsValue::Data(reference) => {
                reference.length == 8
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS ENTER TRACENUM RESOURCE requires eight characters".into(),
            ));
        }
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::TraceResource,
            value,
        });
    }
    Ok(result)
}

fn monitor_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let point = cics_integer_value(&clauses["POINT"], semantic)?;
    if let HirCicsValue::Data(reference) = &point {
        require_halfword("MONITOR", "POINT", reference)?;
    }
    let mut result = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::MonitorPoint,
        value: point,
    }];
    if let Some(tokens) = clauses.get("ENTRYNAME") {
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(text) => text.len() == 8,
            HirCicsValue::Data(reference) => {
                reference.length == 8
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS MONITOR ENTRYNAME requires eight characters".into(),
            ));
        }
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::MonitorEntryName,
            value,
        });
    }
    for (name, identity) in [
        ("DATA1", HirCicsOperandName::MonitorData1),
        ("DATA2", HirCicsOperandName::MonitorData2),
    ] {
        if let Some(tokens) = clauses.get(name) {
            let reference = complete_data_reference(tokens, semantic)?;
            if reference.length != 4 {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS MONITOR {name} requires four-byte storage"
                )));
            }
            result.push(HirCicsNamedOperand {
                name: identity,
                value: HirCicsValue::Data(reference),
            });
        }
    }
    Ok(result)
}

fn require_halfword(
    command: &str,
    name: &str,
    reference: &super::super::HirDataReference,
) -> Resolution<()> {
    if reference.length == 2
        && reference.scale == 0
        && matches!(
            reference.usage,
            CobolUsage::Binary | CobolUsage::NativeBinary
        )
    {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {command} {name} requires halfword binary storage"
        )))
    }
}
