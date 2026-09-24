//! Source-bounded APPC/MRO conversation-control lowering.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const fn is_conversation(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::AllocateConversation
            | HirCicsOperation::GdsAllocateConversation
            | HirCicsOperation::GdsAssignConversation
            | HirCicsOperation::BuildAttach
            | HirCicsOperation::ConnectProcess
            | HirCicsOperation::GdsConnectProcess
            | HirCicsOperation::Converse
            | HirCicsOperation::FreeConversation
            | HirCicsOperation::GdsFreeConversation
    )
}

pub(super) const fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::AllocateConversation => {
            &["SYSID", "PARTNER", "PROFILE", "STATE", "RESP", "RESP2"]
        }
        HirCicsOperation::GdsAllocateConversation => &[
            "SYSID", "PARTNER", "MODENAME", "CONVID", "RETCODE", "STATE", "RESP", "RESP2",
        ],
        HirCicsOperation::GdsAssignConversation => {
            &["PRINCONVID", "PRINSYSID", "RETCODE", "RESP", "RESP2"]
        }
        HirCicsOperation::BuildAttach => &[
            "ATTACHID",
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
        HirCicsOperation::ConnectProcess => &[
            "CONVID",
            "SESSION",
            "PROCNAME",
            "PROCLENGTH",
            "PARTNER",
            "PIPLIST",
            "PIPLENGTH",
            "SYNCLEVEL",
            "STATE",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::GdsConnectProcess => &[
            "CONVID",
            "PROCNAME",
            "PROCLENGTH",
            "PARTNER",
            "PIPLIST",
            "PIPLENGTH",
            "SYNCLEVEL",
            "CONVDATA",
            "RETCODE",
            "STATE",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::Converse => &[
            "CONVID",
            "SESSION",
            "ATTACHID",
            "FROM",
            "FROMLENGTH",
            "FROMFLENGTH",
            "INTO",
            "SET",
            "TOLENGTH",
            "TOFLENGTH",
            "MAXLENGTH",
            "MAXFLENGTH",
            "STATE",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::FreeConversation => &["CONVID", "SESSION", "STATE", "RESP", "RESP2"],
        HirCicsOperation::GdsFreeConversation => {
            &["CONVID", "CONVDATA", "RETCODE", "STATE", "RESP", "RESP2"]
        }
        _ => &[],
    }
}

pub(super) const fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::AllocateConversation | HirCicsOperation::GdsAllocateConversation => {
            &["NOQUEUE", "NOHANDLE"]
        }
        HirCicsOperation::Converse => &["NOTRUNCATE", "DEFRESP", "FMH", "NOHANDLE"],
        _ => &["NOHANDLE"],
    }
}

pub(super) const fn required_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::GdsAllocateConversation => &["CONVID", "RETCODE"],
        HirCicsOperation::GdsAssignConversation => &["RETCODE"],
        HirCicsOperation::BuildAttach => &["ATTACHID"],
        HirCicsOperation::GdsConnectProcess => &["CONVID", "RETCODE"],
        HirCicsOperation::GdsFreeConversation => &["CONVID", "RETCODE"],
        _ => &[],
    }
}

pub(super) fn validate(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if !is_conversation(operation) {
        return Ok(());
    }
    if matches!(
        operation,
        HirCicsOperation::GdsAllocateConversation
            | HirCicsOperation::GdsAssignConversation
            | HirCicsOperation::GdsConnectProcess
            | HirCicsOperation::GdsFreeConversation
    ) {
        return Err(ResolutionFailure::Invalid(
            "CICS GDS commands are available to assembler and C programs only".into(),
        ));
    }
    let has = |name: &str| clauses.contains_key(name);
    let xor = |a: &str, b: &str| has(a) != has(b);
    let invalid = match operation {
        HirCicsOperation::AllocateConversation => !xor("SYSID", "PARTNER"),
        HirCicsOperation::GdsAllocateConversation => {
            !xor("SYSID", "PARTNER") || !has("CONVID") || !has("RETCODE")
        }
        HirCicsOperation::GdsAssignConversation => !has("RETCODE"),
        HirCicsOperation::BuildAttach => !has("ATTACHID"),
        HirCicsOperation::ConnectProcess | HirCicsOperation::GdsConnectProcess => {
            !xor("PROCNAME", "PARTNER")
                || !xor("CONVID", "SESSION") && operation == HirCicsOperation::ConnectProcess
                || !has("CONVID") && operation == HirCicsOperation::GdsConnectProcess
                || has("PROCNAME") != has("PROCLENGTH")
                || has("PIPLIST") != has("PIPLENGTH")
                || operation == HirCicsOperation::GdsConnectProcess && !has("RETCODE")
        }
        HirCicsOperation::Converse => {
            !has("FROM") && !has("ATTACHID")
                || !xor("INTO", "SET")
                || !xor("TOLENGTH", "TOFLENGTH")
                || has("FROMLENGTH") && has("FROMFLENGTH")
                || has("MAXLENGTH") && has("MAXFLENGTH")
                || (has("FROMLENGTH") || has("FROMFLENGTH")) && !has("FROM")
                || options.iter().any(|option| option == "FMH") && !has("ATTACHID")
        }
        HirCicsOperation::FreeConversation => has("CONVID") && has("SESSION"),
        HirCicsOperation::GdsFreeConversation => !has("CONVID") || !has("RETCODE"),
        _ => false,
    };
    if invalid {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} has an invalid conversation option combination"
        )));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_conversation(operation) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name, min, max) in [
        ("SYSID", HirCicsOperandName::ConversationSysid, 1, 4),
        ("PARTNER", HirCicsOperandName::ConversationPartner, 1, 8),
        ("PROFILE", HirCicsOperandName::ConversationProfile, 1, 8),
        ("SESSION", HirCicsOperandName::ConversationSession, 1, 4),
        ("MODENAME", HirCicsOperandName::ConversationModeName, 1, 8),
        ("CONVID", HirCicsOperandName::ConversationConvid, 4, 4),
        ("ATTACHID", HirCicsOperandName::ConversationAttachId, 1, 8),
        ("PROCESS", HirCicsOperandName::ConversationProcess, 1, 64),
        ("RESOURCE", HirCicsOperandName::ConversationResource, 1, 64),
        (
            "RPROCESS",
            HirCicsOperandName::ConversationReturnProcess,
            1,
            64,
        ),
        (
            "RRESOURCE",
            HirCicsOperandName::ConversationReturnResource,
            1,
            64,
        ),
        ("QUEUE", HirCicsOperandName::ConversationQueue, 1, 64),
        ("PROCNAME", HirCicsOperandName::ConversationProcName, 1, 64),
        ("PIPLIST", HirCicsOperandName::ConversationPipList, 4, 763),
        ("FROM", HirCicsOperandName::ConversationFrom, 1, 1_048_576),
    ] {
        if clause == "CONVID" && operation == HirCicsOperation::GdsAllocateConversation {
            continue;
        }
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_value(tokens, semantic)?;
            let length = match &value {
                HirCicsValue::Literal(text) => text.len(),
                HirCicsValue::Data(reference) => {
                    if !matches!(clause, "CONVID" | "PIPLIST" | "FROM")
                        && !matches!(
                            reference.category,
                            DataCategory::Alphabetic | DataCategory::Alphanumeric
                        )
                    {
                        return Err(ResolutionFailure::Invalid(format!(
                            "CICS {operation:?} {clause} requires character storage"
                        )));
                    }
                    reference.length
                }
                _ => 0,
            };
            if !(min..=max).contains(&length) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} {clause} requires {min}..={max} bytes"
                )));
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    for (clause, name, width, min, max) in [
        ("IUTYPE", HirCicsOperandName::ConversationIuType, 2, 0, 127),
        (
            "DATASTR",
            HirCicsOperandName::ConversationDataStream,
            2,
            0,
            255,
        ),
        (
            "RECFM",
            HirCicsOperandName::ConversationRecordFormat,
            2,
            0,
            255,
        ),
        (
            "PROCLENGTH",
            HirCicsOperandName::ConversationProcLength,
            2,
            1,
            64,
        ),
        (
            "PIPLENGTH",
            HirCicsOperandName::ConversationPipLength,
            2,
            4,
            763,
        ),
        (
            "SYNCLEVEL",
            HirCicsOperandName::ConversationSyncLevel,
            2,
            0,
            2,
        ),
        (
            "FROMLENGTH",
            HirCicsOperandName::ConversationFromLength,
            2,
            0,
            32767,
        ),
        (
            "FROMFLENGTH",
            HirCicsOperandName::ConversationFromFullLength,
            4,
            0,
            1048576,
        ),
        (
            "MAXLENGTH",
            HirCicsOperandName::ConversationMaxLength,
            2,
            0,
            32767,
        ),
        (
            "MAXFLENGTH",
            HirCicsOperandName::ConversationMaxFullLength,
            4,
            0,
            1048576,
        ),
        (
            "TOLENGTH",
            HirCicsOperandName::ConversationToLength,
            2,
            0,
            32767,
        ),
        (
            "TOFLENGTH",
            HirCicsOperandName::ConversationToFullLength,
            4,
            0,
            1048576,
        ),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_integer_value(tokens, semantic)?;
            match &value {
                HirCicsValue::Integer(number)
                    if (min..=max).contains(number) && valid_attach_number(clause, *number) => {}
                HirCicsValue::Data(reference)
                    if reference.category == DataCategory::Binary
                        && reference.length == width
                        && reference.scale == 0
                        && matches!(
                            reference.usage,
                            CobolUsage::Binary | CobolUsage::NativeBinary
                        ) => {}
                _ => {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS {operation:?} {clause} requires a bounded {width}-byte binary value"
                    )));
                }
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    Ok(result)
}

fn valid_attach_number(clause: &str, number: i64) -> bool {
    match clause {
        "IUTYPE" => number & !0x13 == 0 && number & 0x03 <= 1,
        "DATASTR" => {
            matches!(number >> 4, 0 | 0xc | 0xd | 0xe | 0xf)
                && (number >> 4 == 0 || number & 0x0f == 0)
        }
        "RECFM" => matches!(number, 1 | 4),
        _ => true,
    }
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_conversation(operation) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name, size) in [
        ("STATE", HirCicsOutputName::ConversationState, 4),
        ("RETCODE", HirCicsOutputName::ConversationRetcode, 6),
        ("PRINCONVID", HirCicsOutputName::ConversationPrinConvid, 4),
        ("PRINSYSID", HirCicsOutputName::ConversationPrinSysid, 4),
        ("CONVDATA", HirCicsOutputName::ConversationConvData, 24),
        ("TOLENGTH", HirCicsOutputName::ConversationToLength, 2),
        ("TOFLENGTH", HirCicsOutputName::ConversationToFullLength, 4),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            let numeric = matches!(
                name,
                HirCicsOutputName::ConversationState
                    | HirCicsOutputName::ConversationToLength
                    | HirCicsOutputName::ConversationToFullLength
            );
            if target.length != size
                || numeric && (target.category != DataCategory::Binary || target.scale != 0)
                || !numeric
                    && !matches!(
                        target.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} {clause} requires {size}-byte writable storage"
                )));
            }
            result.push(HirCicsOutputBinding { name, target });
        }
    }
    if operation == HirCicsOperation::GdsAllocateConversation {
        let target = complete_data_reference(&clauses["CONVID"], semantic)?;
        require_writable(&target)?;
        if target.length != 4 {
            return Err(ResolutionFailure::Invalid(
                "CICS GDS ALLOCATE CONVID requires four writable bytes".into(),
            ));
        }
        result.push(HirCicsOutputBinding {
            name: HirCicsOutputName::ConversationConvid,
            target,
        });
    }
    if operation == HirCicsOperation::Converse {
        if let Some(tokens) = clauses.get("INTO") {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if target.length == 0 || target.length > 1_048_576 {
                return Err(ResolutionFailure::Invalid(
                    "CICS CONVERSE INTO exceeds the bounded receiving area".into(),
                ));
            }
            result.push(HirCicsOutputBinding {
                name: HirCicsOutputName::ConversationInto,
                target,
            });
        }
        if let Some(tokens) = clauses.get("SET") {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            if !matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32) {
                return Err(ResolutionFailure::Invalid(
                    "CICS CONVERSE SET requires a writable pointer".into(),
                ));
            }
            result.push(HirCicsOutputBinding {
                name: HirCicsOutputName::ConversationSet,
                target,
            });
        }
    }
    Ok(result)
}

pub(super) fn option(operation: HirCicsOperation, option: &str) -> Option<HirCicsOption> {
    if !is_conversation(operation) {
        return None;
    }
    match option {
        "NOQUEUE" => Some(HirCicsOption::ConversationNoQueue),
        "NOTRUNCATE" => Some(HirCicsOption::ConversationNotruncate),
        "DEFRESP" => Some(HirCicsOption::ConversationDefresp),
        "FMH" => Some(HirCicsOption::ConversationFmh),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_forms_reject_ambiguous_selectors_and_device_flags() {
        let mut clauses = Clauses::new();
        clauses.insert("SYSID".into(), vec!["'SYS1'".into()]);
        assert!(validate(&clauses, &[], HirCicsOperation::AllocateConversation).is_ok());
        clauses.insert("PARTNER".into(), vec!["'REMOTE'".into()]);
        assert!(validate(&clauses, &[], HirCicsOperation::AllocateConversation).is_err());
        assert!(!allowed_options(HirCicsOperation::Converse).contains(&"ERASE"));
        assert!(allowed_options(HirCicsOperation::Converse).contains(&"NOTRUNCATE"));
        assert!(valid_attach_number("IUTYPE", 0x10));
        assert!(!valid_attach_number("IUTYPE", 0x02));
        assert!(valid_attach_number("DATASTR", 0xd0));
        assert!(!valid_attach_number("DATASTR", 0xd1));
        assert!(valid_attach_number("RECFM", 4));
        assert!(!valid_attach_number("RECFM", 3));
    }

    #[test]
    fn cobol_rejects_all_basic_gds_conversation_commands() {
        for operation in [
            HirCicsOperation::GdsAllocateConversation,
            HirCicsOperation::GdsAssignConversation,
            HirCicsOperation::GdsConnectProcess,
            HirCicsOperation::GdsFreeConversation,
        ] {
            assert!(matches!(
                validate(&Clauses::new(), &[], operation),
                Err(ResolutionFailure::Invalid(message))
                    if message.contains("assembler and C programs only")
            ));
        }
    }
}
