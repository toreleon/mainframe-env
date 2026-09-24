//! Source-selected conversation data and wait forms.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const fn is_data_wait(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::ReceiveConversation
            | HirCicsOperation::GdsReceiveConversation
            | HirCicsOperation::SendConversation
            | HirCicsOperation::GdsWaitConversation
            | HirCicsOperation::WaitConvid
            | HirCicsOperation::WaitSignal
            | HirCicsOperation::WaitTerminal
    )
}

pub(super) const fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::ReceiveConversation => &[
            "CONVID",
            "SESSION",
            "INTO",
            "SET",
            "LENGTH",
            "FLENGTH",
            "MAXLENGTH",
            "MAXFLENGTH",
            "STATE",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::GdsReceiveConversation => &[
            "CONVID",
            "INTO",
            "SET",
            "FLENGTH",
            "MAXFLENGTH",
            "CONVDATA",
            "RETCODE",
            "STATE",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::SendConversation => &[
            "CONVID", "SESSION", "ATTACHID", "FROM", "LENGTH", "FLENGTH", "STATE", "RESP", "RESP2",
        ],
        HirCicsOperation::GdsWaitConversation => {
            &["CONVID", "CONVDATA", "RETCODE", "STATE", "RESP", "RESP2"]
        }
        HirCicsOperation::WaitConvid => &["CONVID", "STATE", "RESP", "RESP2"],
        HirCicsOperation::WaitSignal => &["RESP", "RESP2"],
        HirCicsOperation::WaitTerminal => &["CONVID", "SESSION", "RESP", "RESP2"],
        _ => &[],
    }
}

pub(super) const fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::ReceiveConversation => &["NOTRUNCATE", "NOHANDLE"],
        HirCicsOperation::GdsReceiveConversation => &["BUFFER", "LLID", "NOHANDLE"],
        HirCicsOperation::SendConversation => &[
            "INVITE", "LAST", "CONFIRM", "WAIT", "FMH", "DEFRESP", "NOHANDLE",
        ],
        _ => &["NOHANDLE"],
    }
}

pub(super) const fn required_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::GdsReceiveConversation => &["CONVID", "FLENGTH", "MAXFLENGTH", "RETCODE"],
        HirCicsOperation::SendConversation => &["FROM"],
        HirCicsOperation::GdsWaitConversation => &["RETCODE"],
        HirCicsOperation::WaitConvid => &["CONVID"],
        _ => &[],
    }
}

pub(super) fn validate(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if !is_data_wait(operation) {
        return Ok(());
    }
    let has = |name: &str| clauses.contains_key(name);
    let conflict = has("CONVID") && has("SESSION");
    let wrong = match operation {
        HirCicsOperation::ReceiveConversation => {
            conflict
                || has("INTO") == has("SET")
                || has("LENGTH") && has("FLENGTH")
                || has("MAXLENGTH") && has("MAXFLENGTH")
                || has("INTO")
                    && !has("LENGTH")
                    && !has("FLENGTH")
                    && !has("MAXLENGTH")
                    && !has("MAXFLENGTH")
        }
        HirCicsOperation::GdsReceiveConversation => {
            !has("CONVID")
                || has("INTO") == has("SET")
                || options.iter().any(|option| option == "BUFFER")
                    && options.iter().any(|option| option == "LLID")
        }
        HirCicsOperation::SendConversation => {
            conflict
                || !has("FROM")
                || has("LENGTH") == has("FLENGTH")
                || options.iter().any(|option| option == "CONFIRM")
                    && options.iter().any(|option| option == "DEFRESP")
        }
        HirCicsOperation::GdsWaitConversation => false,
        HirCicsOperation::WaitConvid => !has("CONVID"),
        HirCicsOperation::WaitSignal => false,
        HirCicsOperation::WaitTerminal => conflict,
        _ => false,
    };
    if wrong {
        return Err(ResolutionFailure::Invalid(format!(
            "CICS {operation:?} has an invalid data or wait form"
        )));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_data_wait(operation) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name, min, max) in [
        ("CONVID", HirCicsOperandName::ConversationDataConvid, 4, 4),
        ("SESSION", HirCicsOperandName::ConversationDataSession, 1, 4),
        (
            "ATTACHID",
            HirCicsOperandName::ConversationDataAttachId,
            1,
            8,
        ),
        (
            "FROM",
            HirCicsOperandName::ConversationDataFrom,
            1,
            1_048_576,
        ),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_value(tokens, semantic)?;
            let size = match &value {
                HirCicsValue::Literal(bytes) => bytes.len(),
                HirCicsValue::Data(reference) => reference.length,
                _ => 0,
            };
            if !(min..=max).contains(&size) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} {clause} requires {min}..={max} bytes"
                )));
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    for (clause, name, width) in [
        ("LENGTH", HirCicsOperandName::ConversationDataLength, 2),
        ("FLENGTH", HirCicsOperandName::ConversationDataFullLength, 4),
        (
            "MAXLENGTH",
            HirCicsOperandName::ConversationDataMaxLength,
            2,
        ),
        (
            "MAXFLENGTH",
            HirCicsOperandName::ConversationDataMaxFullLength,
            4,
        ),
    ] {
        if operation == HirCicsOperation::GdsReceiveConversation && clause == "FLENGTH" {
            continue;
        }
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_integer_value(tokens, semantic)?;
            let maximum = if operation == HirCicsOperation::GdsReceiveConversation {
                32_767
            } else {
                1_048_576
            };
            match &value {
                HirCicsValue::Integer(number) if (0..=maximum).contains(number) => {}
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
                        "CICS {operation:?} {clause} requires bounded {width}-byte binary data"
                    )));
                }
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    Ok(result)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_data_wait(operation) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name, size) in [
        ("STATE", HirCicsOutputName::ConversationDataState, 4),
        ("RETCODE", HirCicsOutputName::ConversationDataRetcode, 6),
        ("CONVDATA", HirCicsOutputName::ConversationDataConvData, 24),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let target = complete_data_reference(tokens, semantic)?;
            require_writable(&target)?;
            let valid = target.length == size
                && if clause == "STATE" {
                    target.category == DataCategory::Binary && target.scale == 0
                } else {
                    matches!(
                        target.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
                };
            if !valid {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {operation:?} {clause} requires {size}-byte writable storage"
                )));
            }
            result.push(HirCicsOutputBinding { name, target });
        }
    }
    if matches!(
        operation,
        HirCicsOperation::ReceiveConversation | HirCicsOperation::GdsReceiveConversation
    ) {
        for (clause, name) in [
            ("INTO", HirCicsOutputName::ConversationDataInto),
            ("SET", HirCicsOutputName::ConversationDataSet),
        ] {
            if let Some(tokens) = clauses.get(clause) {
                let target = complete_data_reference(tokens, semantic)?;
                require_writable(&target)?;
                if clause == "SET"
                    && !matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32)
                    || clause == "INTO" && (target.length == 0 || target.length > 1_048_576)
                {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS {operation:?} {clause} has an invalid receiving area"
                    )));
                }
                result.push(HirCicsOutputBinding { name, target });
            }
        }
        for (clause, name, width) in [
            ("LENGTH", HirCicsOutputName::ConversationDataLength, 2),
            ("FLENGTH", HirCicsOutputName::ConversationDataFullLength, 4),
        ] {
            if let Some(tokens) = clauses.get(clause) {
                let target = complete_data_reference(tokens, semantic)?;
                require_writable(&target)?;
                if target.category != DataCategory::Binary
                    || target.scale != 0
                    || target.length != width
                {
                    return Err(ResolutionFailure::Invalid(format!(
                        "CICS {operation:?} {clause} requires writable {width}-byte binary storage"
                    )));
                }
                result.push(HirCicsOutputBinding { name, target });
            }
        }
    }
    Ok(result)
}

pub(super) fn option(operation: HirCicsOperation, name: &str) -> Option<HirCicsOption> {
    if !is_data_wait(operation) {
        return None;
    }
    match name {
        "NOTRUNCATE" => Some(HirCicsOption::ConversationDataNotruncate),
        "BUFFER" => Some(HirCicsOption::ConversationDataBuffer),
        "LLID" => Some(HirCicsOption::ConversationDataLlid),
        "INVITE" => Some(HirCicsOption::ConversationDataInvite),
        "LAST" => Some(HirCicsOption::ConversationDataLast),
        "CONFIRM" => Some(HirCicsOption::ConversationDataConfirm),
        "WAIT" => Some(HirCicsOption::ConversationDataWait),
        "FMH" => Some(HirCicsOption::ConversationDataFmh),
        "DEFRESP" => Some(HirCicsOption::ConversationDataDefresp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_wait_forms_reject_ambiguous_buffers_and_selectors() {
        let mut receive = Clauses::new();
        receive.insert("CONVID".into(), vec!["'ABCD'".into()]);
        receive.insert("INTO".into(), vec!["IN-BUFFER".into()]);
        receive.insert("LENGTH".into(), vec!["IN-LENGTH".into()]);
        assert!(validate(&receive, &[], HirCicsOperation::ReceiveConversation).is_ok());
        receive.insert("SET".into(), vec!["IN-POINTER".into()]);
        assert!(validate(&receive, &[], HirCicsOperation::ReceiveConversation).is_err());
        receive.remove("SET");
        receive.insert("FLENGTH".into(), vec!["IN-FULL-LENGTH".into()]);
        assert!(validate(&receive, &[], HirCicsOperation::ReceiveConversation).is_err());

        let mut send = Clauses::new();
        send.insert("FROM".into(), vec!["OUT-BUFFER".into()]);
        send.insert("LENGTH".into(), vec!["OUT-LENGTH".into()]);
        assert!(validate(&send, &[], HirCicsOperation::SendConversation).is_ok());
        send.insert("SESSION".into(), vec!["'S1'".into()]);
        send.insert("CONVID".into(), vec!["'ABCD'".into()]);
        assert!(validate(&send, &[], HirCicsOperation::SendConversation).is_err());

        assert_eq!(required_clauses(HirCicsOperation::WaitConvid), &["CONVID"]);
        assert_eq!(
            allowed_clauses(HirCicsOperation::WaitSignal),
            &["RESP", "RESP2"]
        );
        assert!(!allowed_options(HirCicsOperation::WaitTerminal).contains(&"INVITE"));
    }
}
