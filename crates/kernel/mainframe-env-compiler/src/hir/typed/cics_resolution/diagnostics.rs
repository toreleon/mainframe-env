use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
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
        HirCicsOperation::DumpTransaction => &[
            "DUMPCODE",
            "FROM",
            "LENGTH",
            "FLENGTH",
            "SEGMENTLIST",
            "LENGTHLIST",
            "NUMSEGMENTS",
            "DUMPID",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::Dump => &["DUMPCODE", "FROM", "LENGTH", "FLENGTH", "RESP", "RESP2"],
        HirCicsOperation::Trace => &["RESP", "RESP2"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &["EXCEPTION", "NOHANDLE"],
        HirCicsOperation::Monitor => &["NOHANDLE"],
        HirCicsOperation::DumpTransaction => &[
            "COMPLETE", "TASK", "STORAGE", "PROGRAM", "TERMINAL", "TABLES", "FCT", "PCT", "PPT",
            "SIT", "TCT", "TRT", "NOHANDLE",
        ],
        HirCicsOperation::Dump => &[
            "COMPLETE", "TASK", "STORAGE", "PROGRAM", "TERMINAL", "TABLES", "DCT", "FCT", "PCT",
            "PPT", "SIT", "TCT", "NOHANDLE",
        ],
        HirCicsOperation::Trace => &["ON", "OFF", "SYSTEM", "USER", "EI", "SINGLE", "NOHANDLE"],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::EnterTraceNum => &["TRACENUM"],
        HirCicsOperation::Monitor => &["POINT"],
        HirCicsOperation::DumpTransaction => &["DUMPCODE"],
        HirCicsOperation::Dump => &[],
        HirCicsOperation::Trace => &[],
        _ => unreachable!("non-diagnostic operation"),
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation == HirCicsOperation::Trace {
        validate_trace_options(options)?;
        return Ok(Vec::new());
    }
    if operation == HirCicsOperation::Monitor {
        return monitor_operands(clauses, semantic);
    }
    if matches!(
        operation,
        HirCicsOperation::Dump | HirCicsOperation::DumpTransaction
    ) {
        return dump_transaction_operands(clauses, operation, semantic);
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

pub(super) fn option(operation: HirCicsOperation, name: &str) -> Option<HirCicsOption> {
    if operation == HirCicsOperation::Trace {
        return Some(match name {
            "ON" => HirCicsOption::TraceOn,
            "OFF" => HirCicsOption::TraceOff,
            "SYSTEM" => HirCicsOption::TraceSystem,
            "USER" => HirCicsOption::TraceUser,
            "EI" => HirCicsOption::TraceEi,
            "SINGLE" => HirCicsOption::TraceSingle,
            _ => return None,
        });
    }
    if !matches!(
        operation,
        HirCicsOperation::Dump | HirCicsOperation::DumpTransaction
    ) {
        return None;
    }
    Some(match name {
        "COMPLETE" => HirCicsOption::DumpComplete,
        "TASK" => HirCicsOption::DumpTask,
        "STORAGE" => HirCicsOption::DumpStorage,
        "PROGRAM" => HirCicsOption::DumpProgram,
        "TERMINAL" => HirCicsOption::DumpTerminal,
        "TABLES" => HirCicsOption::DumpTables,
        "FCT" => HirCicsOption::DumpFct,
        "PCT" => HirCicsOption::DumpPct,
        "PPT" => HirCicsOption::DumpPpt,
        "SIT" => HirCicsOption::DumpSit,
        "TCT" => HirCicsOption::DumpTct,
        "TRT" => HirCicsOption::DumpTrt,
        "DCT" => HirCicsOption::DumpDct,
        _ => return None,
    })
}

fn validate_trace_options(options: &[String]) -> Resolution<()> {
    let on = options.iter().any(|option| option == "ON");
    let off = options.iter().any(|option| option == "OFF");
    let targets = ["USER", "SYSTEM", "EI", "SINGLE"]
        .iter()
        .filter(|name| options.iter().any(|option| option == **name))
        .count();
    if on == off || targets == 0 {
        return Err(ResolutionFailure::Invalid(
            "CICS TRACE requires exactly one of ON or OFF and at least one trace switch".into(),
        ));
    }
    Ok(())
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if operation != HirCicsOperation::DumpTransaction {
        return Ok(Vec::new());
    }
    let Some(tokens) = clauses.get("DUMPID") else {
        return Ok(Vec::new());
    };
    let target = complete_data_reference(tokens, semantic)?;
    require_writable(&target)?;
    if target.length != 9
        || !matches!(
            target.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DUMP TRANSACTION DUMPID requires nine-character writable storage".into(),
        ));
    }
    Ok(vec![HirCicsOutputBinding {
        name: HirCicsOutputName::DumpId,
        target,
    }])
}

fn dump_transaction_operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let code = clauses
        .get("DUMPCODE")
        .map(|tokens| cics_value(tokens, semantic))
        .transpose()?;
    let valid_code = code.as_ref().is_none_or(|code| match code {
        HirCicsValue::Literal(text) => (1..=4).contains(&text.len()),
        HirCicsValue::Data(reference) => {
            (1..=4).contains(&reference.length)
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        _ => false,
    });
    if !valid_code {
        return Err(ResolutionFailure::Invalid(
            "CICS DUMP TRANSACTION DUMPCODE requires one to four characters".into(),
        ));
    }
    if clauses.contains_key("LENGTH") && clauses.contains_key("FLENGTH") {
        return Err(ResolutionFailure::Invalid(
            "CICS DUMP TRANSACTION LENGTH and FLENGTH are mutually exclusive".into(),
        ));
    }
    if (clauses.contains_key("LENGTH") || clauses.contains_key("FLENGTH"))
        && !clauses.contains_key("FROM")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS DUMP TRANSACTION LENGTH or FLENGTH requires FROM".into(),
        ));
    }
    let segment_count = ["SEGMENTLIST", "LENGTHLIST", "NUMSEGMENTS"]
        .iter()
        .filter(|name| clauses.contains_key(**name))
        .count();
    if segment_count != 0 && segment_count != 3 {
        return Err(ResolutionFailure::Invalid(
            "CICS DUMP TRANSACTION SEGMENTLIST, LENGTHLIST, and NUMSEGMENTS must occur together"
                .into(),
        ));
    }
    let mut result = Vec::new();
    if let Some(code) = code {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::DumpCode,
            value: code,
        });
    }
    if let Some(tokens) = clauses.get("FROM") {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::DumpFrom,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    for (name, identity, width) in [
        ("LENGTH", HirCicsOperandName::DumpLength, 2),
        ("FLENGTH", HirCicsOperandName::DumpFlength, 4),
    ] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_integer_value(tokens, semantic)?;
            if let HirCicsValue::Data(reference) = &value {
                require_binary_width("DUMP TRANSACTION", name, reference, width)?;
            }
            result.push(HirCicsNamedOperand {
                name: identity,
                value,
            });
        }
    }
    if segment_count == 3 && operation == HirCicsOperation::DumpTransaction {
        for (name, identity) in [
            ("SEGMENTLIST", HirCicsOperandName::DumpSegmentList),
            ("LENGTHLIST", HirCicsOperandName::DumpLengthList),
        ] {
            let reference = complete_data_reference(&clauses[name], semantic)?;
            if reference.length == 0 || reference.length % 4 != 0 {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS DUMP TRANSACTION {name} requires a four-byte aligned list"
                )));
            }
            result.push(HirCicsNamedOperand {
                name: identity,
                value: HirCicsValue::Data(reference),
            });
        }
        let count = complete_data_reference(&clauses["NUMSEGMENTS"], semantic)?;
        require_binary_width("DUMP TRANSACTION", "NUMSEGMENTS", &count, 4)?;
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::DumpNumSegments,
            value: HirCicsValue::Data(count),
        });
    }
    Ok(result)
}

fn require_binary_width(
    command: &str,
    name: &str,
    reference: &super::super::HirDataReference,
    width: usize,
) -> Resolution<()> {
    if reference.length == width
        && reference.scale == 0
        && matches!(
            reference.usage,
            CobolUsage::Binary | CobolUsage::NativeBinary
        )
    {
        Ok(())
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {command} {name} requires {width}-byte binary storage"
        )))
    }
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
