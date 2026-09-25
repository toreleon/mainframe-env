//! COBOL forms of the source-pinned conversation extraction commands.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName as I, HirCicsOperation as P, HirCicsOutputBinding,
    HirCicsOutputName as O, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const fn is_operation(operation: P) -> bool {
    matches!(
        operation,
        P::ExtractAttach
            | P::ExtractAttributes
            | P::GdsExtractAttributes
            | P::ExtractLogonMsg
            | P::ExtractProcess
            | P::GdsExtractProcess
            | P::ExtractTct
            | P::Point
    )
}

pub(super) const fn allowed_clauses(operation: P) -> &'static [&'static str] {
    match operation {
        P::ExtractAttach => &[
            "ATTACHID",
            "CONVID",
            "SESSION",
            "PROCESS",
            "RESOURCE",
            "RPROCESS",
            "RRESOURCE",
            "QUEUE",
            "IUTYPE",
            "DATASTR",
            "RECFM",
            "RESP",
            "RESP2",
        ],
        P::ExtractAttributes => &["CONVID", "SESSION", "STATE", "RESP", "RESP2"],
        P::GdsExtractAttributes => &["CONVID", "STATE", "CONVDATA", "RETCODE", "RESP", "RESP2"],
        P::ExtractLogonMsg => &["INTO", "SET", "LENGTH", "RESP", "RESP2"],
        P::ExtractProcess | P::GdsExtractProcess => &[
            "CONVID",
            "SESSION",
            "PROCNAME",
            "PROCLENGTH",
            "MAXPROCLEN",
            "SYNCLEVEL",
            "PIPLIST",
            "PIPLENGTH",
            "RETCODE",
            "RESP",
            "RESP2",
        ],
        P::ExtractTct => &["NETNAME", "SYSID", "TERMID", "RESP", "RESP2"],
        P::Point => &["CONVID", "SESSION", "RESP", "RESP2"],
        _ => &[],
    }
}

pub(super) fn validate(clauses: &Clauses, operation: P) -> Resolution<()> {
    if !is_operation(operation) {
        return Ok(());
    }
    if matches!(operation, P::GdsExtractAttributes | P::GdsExtractProcess) {
        return invalid("GDS extraction is defined only for assembler and C programs");
    }
    let selectors = ["ATTACHID", "CONVID", "SESSION"]
        .iter()
        .filter(|name| clauses.contains_key(**name))
        .count();
    if selectors > 1 {
        return invalid("one ATTACHID, CONVID, or SESSION selector is allowed");
    }
    let has = |name| clauses.contains_key(name);
    let malformed = match operation {
        P::ExtractAttributes => !has("STATE"),
        P::GdsExtractAttributes => !has("CONVID") || !has("CONVDATA") || !has("RETCODE"),
        P::ExtractLogonMsg => !has("LENGTH") || has("INTO") == has("SET"),
        P::ExtractProcess | P::GdsExtractProcess => {
            has("PROCNAME") && !has("PROCLENGTH")
                || has("MAXPROCLEN") && !has("PROCNAME")
                || has("PIPLIST") != has("PIPLENGTH")
                || operation == P::GdsExtractProcess && (!has("CONVID") || !has("RETCODE"))
                || operation == P::ExtractProcess && has("RETCODE")
        }
        P::ExtractTct => !has("NETNAME") || has("SYSID") == has("TERMID"),
        _ => false,
    };
    if malformed {
        invalid("required conversation extraction options or option pairing are missing")
    } else {
        Ok(())
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: P,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_operation(operation) {
        return Ok(Vec::new());
    }
    let mut operands = Vec::new();
    for (name, identity, maximum, exact) in [
        ("ATTACHID", I::ConversationAttachId, 8, false),
        ("CONVID", I::ConversationConvid, 4, true),
        ("SESSION", I::ConversationSession, 4, false),
        ("NETNAME", I::ConversationNetName, 8, true),
    ] {
        if let Some(tokens) = clauses.get(name) {
            let value = cics_value(tokens, semantic)?;
            let length = match &value {
                HirCicsValue::Literal(value) => value.len(),
                HirCicsValue::Data(value) => value.length,
                _ => return invalid("conversation name requires character storage or a literal"),
            };
            if length == 0 || length > maximum || exact && length != maximum {
                return invalid("conversation name has an invalid source-defined width");
            }
            operands.push(HirCicsNamedOperand {
                name: identity,
                value,
            });
        }
    }
    if let Some(tokens) = clauses.get("MAXPROCLEN") {
        let value = cics_integer_value(tokens, semantic)?;
        if matches!(value, HirCicsValue::Integer(number) if !(1..=64).contains(&number)) {
            return invalid("MAXPROCLEN must be 1 through 64");
        }
        operands.push(HirCicsNamedOperand {
            name: I::ConversationMaxProcLen,
            value,
        });
    }
    Ok(operands)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: P,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_operation(operation) {
        return Ok(Vec::new());
    }
    let mut outputs = Vec::new();
    for (name, identity, width, pointer) in [
        ("PROCESS", O::AttachProcess, 1, false),
        ("RESOURCE", O::AttachResource, 1, false),
        ("RPROCESS", O::AttachReturnProcess, 1, false),
        ("RRESOURCE", O::AttachReturnResource, 1, false),
        ("QUEUE", O::AttachQueue, 1, false),
        ("IUTYPE", O::AttachIuType, 2, false),
        ("DATASTR", O::AttachDataStream, 2, false),
        ("RECFM", O::AttachRecordFormat, 2, false),
        ("STATE", O::ConversationState, 4, false),
        ("CONVDATA", O::ConversationData, 24, false),
        ("RETCODE", O::ConversationRetCode, 6, false),
        ("INTO", O::LogonInto, 256, false),
        ("SET", O::LogonSet, 4, true),
        ("LENGTH", O::LogonLength, 2, false),
        ("PROCNAME", O::ProcessName, 1, false),
        ("PROCLENGTH", O::ProcessLength, 2, false),
        ("SYNCLEVEL", O::SyncLevel, 2, false),
        ("PIPLIST", O::PipList, 4, true),
        ("PIPLENGTH", O::PipLength, 2, false),
        ("SYSID", O::TctSysId, 4, false),
        ("TERMID", O::TctTermId, 4, false),
    ] {
        if let Some(tokens) = clauses.get(name) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if pointer {
                if !matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32) {
                    return invalid("conversation pointer output requires POINTER or POINTER-32");
                }
            } else if identity == O::ProcessName {
                if !(1..=64).contains(&target.length) {
                    return invalid("PROCNAME requires 1–64 bytes");
                }
            } else if target.length < width {
                return invalid(
                    "conversation output area is shorter than the source-defined width",
                );
            }
            if matches!(
                identity,
                O::AttachIuType
                    | O::AttachDataStream
                    | O::AttachRecordFormat
                    | O::ConversationState
                    | O::LogonLength
                    | O::ProcessLength
                    | O::SyncLevel
                    | O::PipLength
            ) && target.category != DataCategory::Binary
            {
                return invalid("conversation numeric output requires binary storage");
            }
            outputs.push(HirCicsOutputBinding {
                name: identity,
                target,
            });
        }
    }
    if let Some(HirCicsValue::Integer(max)) = operands(clauses, operation, semantic)?
        .into_iter()
        .find(|operand| operand.name == I::ConversationMaxProcLen)
        .map(|operand| operand.value)
        && outputs
            .iter()
            .find(|output| output.name == O::ProcessName)
            .is_some_and(|output| output.target.length < max as usize)
    {
        return invalid("PROCNAME area is shorter than MAXPROCLEN");
    }
    Ok(outputs)
}

fn invalid<T>(message: &str) -> Resolution<T> {
    Err(ResolutionFailure::Invalid(format!("CICS {message}")))
}
