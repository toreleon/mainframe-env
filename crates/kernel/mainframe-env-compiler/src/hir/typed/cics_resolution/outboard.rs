//! Typed lowering for CICS outboard data interchange commands.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) fn is_issue(operation: HirCicsOperation) -> bool {
    matches!(
        operation,
        HirCicsOperation::IssueAbort
            | HirCicsOperation::IssueAdd
            | HirCicsOperation::IssueEnd
            | HirCicsOperation::IssueErase
            | HirCicsOperation::IssueNote
            | HirCicsOperation::IssueQuery
            | HirCicsOperation::IssueReceive
            | HirCicsOperation::IssueReplace
            | HirCicsOperation::IssueSend
            | HirCicsOperation::IssueWait
    )
}

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::IssueReceive => &["INTO", "SET", "LENGTH", "RESP", "RESP2"],
        HirCicsOperation::IssueAdd | HirCicsOperation::IssueReplace => &[
            "DESTID",
            "DESTIDLENG",
            "VOLUME",
            "VOLUMELENG",
            "FROM",
            "LENGTH",
            "NUMREC",
            "RIDFLD",
            "KEYLENGTH",
            "KEYNUMBER",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::IssueErase => &[
            "DESTID",
            "DESTIDLENG",
            "VOLUME",
            "VOLUMELENG",
            "NUMREC",
            "RIDFLD",
            "KEYLENGTH",
            "KEYNUMBER",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::IssueNote => &[
            "DESTID",
            "DESTIDLENG",
            "VOLUME",
            "VOLUMELENG",
            "RIDFLD",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::IssueQuery => &[
            "DESTID",
            "DESTIDLENG",
            "VOLUME",
            "VOLUMELENG",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::IssueSend => &[
            "DESTID",
            "DESTIDLENG",
            "VOLUME",
            "VOLUMELENG",
            "SUBADDR",
            "FROM",
            "LENGTH",
            "RESP",
            "RESP2",
        ],
        HirCicsOperation::IssueAbort | HirCicsOperation::IssueEnd | HirCicsOperation::IssueWait => {
            &[
                "DESTID",
                "DESTIDLENG",
                "VOLUME",
                "VOLUMELENG",
                "SUBADDR",
                "RESP",
                "RESP2",
            ]
        }
        _ => &[],
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::IssueAdd
        | HirCicsOperation::IssueErase
        | HirCicsOperation::IssueReplace => &["RRN", "DEFRESP", "NOWAIT", "NOHANDLE"],
        HirCicsOperation::IssueNote => &["RRN", "NOHANDLE"],
        HirCicsOperation::IssueSend => &[
            "CONSOLE", "PRINT", "CARD", "WPMEDIA1", "WPMEDIA2", "WPMEDIA3", "WPMEDIA4", "DEFRESP",
            "NOWAIT", "NOHANDLE",
        ],
        HirCicsOperation::IssueAbort | HirCicsOperation::IssueEnd | HirCicsOperation::IssueWait => {
            &[
                "CONSOLE", "PRINT", "CARD", "WPMEDIA1", "WPMEDIA2", "WPMEDIA3", "WPMEDIA4",
                "NOHANDLE",
            ]
        }
        HirCicsOperation::IssueQuery | HirCicsOperation::IssueReceive => &["NOHANDLE"],
        _ => &[],
    }
}

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if allowed_clauses(operation).is_empty() {
        return Ok(());
    }
    let present = |name: &str| options.iter().any(|option| option == name);
    let media_count = [
        "CONSOLE", "PRINT", "CARD", "WPMEDIA1", "WPMEDIA2", "WPMEDIA3", "WPMEDIA4",
    ]
    .iter()
    .filter(|name| present(name))
    .count();
    if media_count > 1
        || media_count > 0 && clauses.contains_key("DESTID")
        || clauses.contains_key("DESTIDLENG") && !clauses.contains_key("DESTID")
        || clauses.contains_key("VOLUMELENG") && !clauses.contains_key("VOLUME")
        || clauses.contains_key("SUBADDR") && media_count == 0
        || present("RRN")
            && (clauses.contains_key("KEYLENGTH") || clauses.contains_key("KEYNUMBER"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE selection or key options conflict".into(),
        ));
    }
    if matches!(
        operation,
        HirCicsOperation::IssueAdd | HirCicsOperation::IssueReplace | HirCicsOperation::IssueSend
    ) && (!clauses.contains_key("FROM") || !clauses.contains_key("LENGTH"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE write requires FROM and LENGTH".into(),
        ));
    }
    if matches!(
        operation,
        HirCicsOperation::IssueErase | HirCicsOperation::IssueReplace
    ) && !clauses.contains_key("RIDFLD")
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE record update requires RIDFLD".into(),
        ));
    }
    if operation == HirCicsOperation::IssueNote
        && (!clauses.contains_key("RIDFLD") || !present("RRN"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE NOTE requires RIDFLD and RRN".into(),
        ));
    }
    if operation == HirCicsOperation::IssueReceive
        && (clauses.contains_key("INTO") == clauses.contains_key("SET")
            || !clauses.contains_key("LENGTH"))
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ISSUE RECEIVE requires one of INTO or SET and LENGTH".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if allowed_clauses(operation).is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for (clause, name, min, max) in [
        ("DESTID", HirCicsOperandName::DestId, 1, 8),
        ("VOLUME", HirCicsOperandName::Volume, 1, 6),
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
                    "CICS ISSUE {clause} requires 1-{max} text bytes"
                )));
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    for (clause, name) in [
        ("DESTIDLENG", HirCicsOperandName::DestIdLength),
        ("VOLUMELENG", HirCicsOperandName::VolumeLength),
        ("SUBADDR", HirCicsOperandName::Subaddress),
        ("NUMREC", HirCicsOperandName::NumRec),
        ("KEYNUMBER", HirCicsOperandName::KeyNumber),
        ("KEYLENGTH", HirCicsOperandName::KeyLength),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_integer_value(tokens, semantic)?;
            if let HirCicsValue::Data(reference) = &value
                && (reference.usage != CobolUsage::Binary
                    || reference.length != 2
                    || reference.scale != 0)
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS ISSUE {clause} requires halfword binary storage"
                )));
            }
            result.push(HirCicsNamedOperand { name, value });
        }
    }
    if let Some(tokens) = clauses.get("FROM") {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::From,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    if let Some(tokens) = clauses.get("RIDFLD")
        && operation != HirCicsOperation::IssueNote
    {
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Ridfld,
            value: HirCicsValue::Data(complete_data_reference(tokens, semantic)?),
        });
    }
    if let Some(tokens) = clauses.get("LENGTH") {
        let value = if operation == HirCicsOperation::IssueReceive {
            let reference = complete_data_reference(tokens, semantic)?;
            require_writable(&reference)?;
            if reference.usage != CobolUsage::Binary
                || reference.length != 2
                || reference.scale != 0
            {
                return Err(ResolutionFailure::Invalid(
                    "CICS ISSUE RECEIVE LENGTH requires writable halfword binary storage".into(),
                ));
            }
            HirCicsValue::Data(reference)
        } else {
            cics_integer_value(tokens, semantic)?
        };
        result.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value,
        });
    }
    Ok(result)
}
