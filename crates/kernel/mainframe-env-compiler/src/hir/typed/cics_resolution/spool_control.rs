use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOption, HirCicsValue,
    Resolution, ResolutionFailure,
};
use super::{Clauses, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn allowed_clauses(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::SpoolClose => &["TOKEN", "RESP", "RESP2"],
        _ => panic!("non-spool CICS operation reached spool clause validation"),
    }
}

pub(super) fn allowed_options(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::SpoolClose => &["DELETE", "KEEP", "NOHANDLE"],
        _ => panic!("non-spool CICS operation reached spool option validation"),
    }
}

pub(super) fn required(operation: HirCicsOperation) -> &'static [&'static str] {
    match operation {
        HirCicsOperation::SpoolClose => &["TOKEN"],
        _ => panic!("non-spool CICS operation reached spool required validation"),
    }
}

pub(super) fn option(name: &str) -> Option<HirCicsOption> {
    match name {
        "KEEP" => Some(HirCicsOption::SpoolKeep),
        "DELETE" => Some(HirCicsOption::SpoolDelete),
        _ => None,
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::SpoolClose {
        return Ok(Vec::new());
    }
    if !clauses.contains_key("RESP") && !options.iter().any(|option| option == "NOHANDLE") {
        return Err(ResolutionFailure::Invalid(
            "CICS SPOOLCLOSE requires RESP or NOHANDLE".into(),
        ));
    }
    if options
        .iter()
        .filter(|option| matches!(option.as_str(), "KEEP" | "DELETE"))
        .count()
        > 1
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SPOOLCLOSE accepts at most one of KEEP or DELETE".into(),
        ));
    }
    let token = complete_data_reference(&clauses["TOKEN"], semantic)?;
    if token.length != 8
        || !matches!(
            token.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS SPOOLCLOSE TOKEN requires an 8-character data area".into(),
        ));
    }
    Ok(vec![HirCicsNamedOperand {
        name: HirCicsOperandName::SpoolToken,
        value: HirCicsValue::Data(token),
    }])
}
