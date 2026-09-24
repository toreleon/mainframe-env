//! Source-shaped COBOL lowering for mapped and device ISSUE commands.
//! GDS ISSUE commands are assembler/C-only and never enter this HIR.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn is_issue(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::IssueAbend
            | HirCicsOperation::IssueConfirmation
            | HirCicsOperation::IssueCopy
            | HirCicsOperation::IssueDisconnect
            | HirCicsOperation::IssueEndfile
            | HirCicsOperation::IssueEndoutput
            | HirCicsOperation::IssueEods
            | HirCicsOperation::IssueEraseAup
            | HirCicsOperation::IssueError
            | HirCicsOperation::IssueLoad
            | HirCicsOperation::IssuePass
            | HirCicsOperation::IssuePrepare
            | HirCicsOperation::IssuePrint
            | HirCicsOperation::IssueReset
            | HirCicsOperation::IssueSignal
    )
}

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::IssueAbend
        | HirCicsOperation::IssueConfirmation
        | HirCicsOperation::IssueError
        | HirCicsOperation::IssuePrepare => &["CONVID", "STATE", "RESP", "RESP2"],
        HirCicsOperation::IssueSignal => &["CONVID", "SESSION", "STATE", "RESP", "RESP2"],
        HirCicsOperation::IssueCopy => &["TERMID", "CTLCHAR", "RESP", "RESP2"],
        HirCicsOperation::IssueDisconnect => &["SESSION", "RESP", "RESP2"],
        HirCicsOperation::IssueLoad => &["PROGRAM", "RESP", "RESP2"],
        HirCicsOperation::IssuePass => &["LUNAME", "FROM", "LENGTH", "LOGMODE", "RESP", "RESP2"],
        HirCicsOperation::IssueEndfile
        | HirCicsOperation::IssueEndoutput
        | HirCicsOperation::IssueEods
        | HirCicsOperation::IssueEraseAup
        | HirCicsOperation::IssuePrint
        | HirCicsOperation::IssueReset => &["RESP", "RESP2"],
        _ => &[],
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::IssueCopy | HirCicsOperation::IssueEraseAup => &["WAIT", "NOHANDLE"],
        HirCicsOperation::IssueEndfile => &["ENDOUTPUT", "NOHANDLE"],
        HirCicsOperation::IssueEndoutput => &["ENDFILE", "NOHANDLE"],
        HirCicsOperation::IssueLoad => &["CONVERSE", "NOHANDLE"],
        HirCicsOperation::IssuePass => &["LOGONLOGMODE", "NOQUIESCE", "NOHANDLE"],
        op if is_issue(op) => &["NOHANDLE"],
        _ => &[],
    }
}

pub(super) fn validate(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if !is_issue(operation) {
        return Ok(());
    }
    let present = |name: &str| options.iter().any(|option| option == name);
    if matches!(operation, HirCicsOperation::IssueCopy) && !clauses.contains_key("TERMID")
        || matches!(operation, HirCicsOperation::IssueLoad) && !clauses.contains_key("PROGRAM")
        || matches!(operation, HirCicsOperation::IssuePass)
            && (!clauses.contains_key("LUNAME")
                || clauses.contains_key("FROM") != clauses.contains_key("LENGTH")
                || clauses.contains_key("LOGMODE") && present("LOGONLOGMODE"))
        || clauses.contains_key("CONVID") && clauses.contains_key("SESSION")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE required operands or option combination is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !is_issue(operation) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name, min, max) in [
        ("CONVID", HirCicsOperandName::IssueConvid, 4, 4),
        ("SESSION", HirCicsOperandName::IssueSession, 1, 4),
        ("TERMID", HirCicsOperandName::IssueTermId, 1, 4),
        ("PROGRAM", HirCicsOperandName::IssueProgram, 1, 8),
        ("LUNAME", HirCicsOperandName::IssueLuName, 1, 8),
        ("LOGMODE", HirCicsOperandName::IssueLogMode, 1, 8),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_value(tokens, semantic)?;
            let valid = match &value {
                HirCicsValue::Literal(text) => (min..=max).contains(&text.len()),
                HirCicsValue::Data(reference) => {
                    (min..=max).contains(&reference.length)
                        && matches!(
                            reference.category,
                            DataCategory::Alphabetic | DataCategory::Alphanumeric
                        )
                }
                _ => false,
            };
            if !valid {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS ISSUE {clause} requires {min}-{max} text bytes"
                )));
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    for (clause, name, maximum) in [
        ("CTLCHAR", HirCicsOperandName::IssueCtlChar, 1),
        ("FROM", HirCicsOperandName::IssueFrom, 255),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let reference = complete_data_reference(tokens, semantic)?;
            if reference.length == 0 || reference.length > maximum {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS ISSUE {clause} data length is outside 1-{maximum}"
                )));
            }
            result.push(HirCicsNamedOperand {
                name,
                value: HirCicsValue::Data(reference),
            });
        }
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::IssueLength,
            value: cics_integer_value(tokens, semantic)?,
        });
    }
    Ok(result)
}

pub(super) fn outputs(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsOutputBinding>> {
    if !is_issue(operation) || !clauses.contains_key("STATE") {
        return Ok(Vec::new());
    }
    let target = complete_data_reference(&clauses["STATE"], semantic)?;
    require_writable(&target)?;
    if target.length != 4 {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE STATE requires a four-byte CVDA receiver".into(),
        ));
    }
    Ok(vec![HirCicsOutputBinding {
        name: HirCicsOutputName::IssueState,
        target,
    }])
}
