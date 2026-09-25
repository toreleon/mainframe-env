//! Source-bounded lowering of full-BMS ROUTE.

use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure,
};
use super::{Clauses, cics_integer_value, cics_value, complete_data_reference};
use crate::{CobolUsage, DataCategory, SemanticModel};

pub(super) const ALLOWED_CLAUSES: &[&str] = &[
    "INTERVAL", "TIME", "HOURS", "MINUTES", "SECONDS", "ERRTERM", "TITLE", "LIST", "OPCLASS",
    "REQID", "LDC", "RESP", "RESP2",
];
pub(super) const ALLOWED_OPTIONS: &[&str] = &["AFTER", "AT", "NLEOM", "NOHANDLE"];

pub(super) fn validate_constraints(
    clauses: &Clauses,
    options: &[String],
    operation: HirCicsOperation,
) -> Resolution<()> {
    if operation != HirCicsOperation::Route {
        return Ok(());
    }
    let after = options.iter().any(|option| option == "AFTER");
    let at = options.iter().any(|option| option == "AT");
    let components = ["HOURS", "MINUTES", "SECONDS"]
        .iter()
        .filter(|name| clauses.contains_key(**name))
        .count();
    if usize::from(after)
        + usize::from(at)
        + usize::from(clauses.contains_key("INTERVAL"))
        + usize::from(clauses.contains_key("TIME"))
        > 1
        || components > 0 && !after && !at
        || (after || at) && components == 0
    {
        return Err(ResolutionFailure::Invalid(
            "CICS ROUTE requires one unambiguous time form".into(),
        ));
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::Route {
        return Ok(Vec::new());
    }
    let mut operands = Vec::new();
    for (clause, name, min, max) in [
        ("ERRTERM", HirCicsOperandName::Errterm, 4, 4),
        ("REQID", HirCicsOperandName::ReqId, 2, 2),
        ("LDC", HirCicsOperandName::Ldc, 2, 2),
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
                    "CICS ROUTE {clause} requires {min}-{max} text bytes"
                )));
            }
            operands.push(HirCicsNamedOperand { name, value });
        }
    }
    for (clause, name, min, max) in [
        ("TITLE", HirCicsOperandName::RouteTitle, 1, 256),
        ("LIST", HirCicsOperandName::RouteList, 16, 32_752),
        ("OPCLASS", HirCicsOperandName::Opclass, 3, 3),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let reference = complete_data_reference(tokens, semantic)?;
            if !(min..=max).contains(&reference.length)
                || clause == "LIST" && reference.length % 16 != 0
                || !matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS ROUTE {clause} has an invalid bounded data area"
                )));
            }
            operands.push(HirCicsNamedOperand {
                name,
                value: HirCicsValue::Data(reference),
            });
        }
    }
    for (clause, name) in [
        ("INTERVAL", HirCicsOperandName::Interval),
        ("TIME", HirCicsOperandName::StartTime),
        ("HOURS", HirCicsOperandName::Hours),
        ("MINUTES", HirCicsOperandName::Minutes),
        ("SECONDS", HirCicsOperandName::Seconds),
    ] {
        if let Some(tokens) = clauses.get(clause) {
            let value = cics_integer_value(tokens, semantic)?;
            if matches!(clause, "HOURS" | "MINUTES" | "SECONDS")
                && let HirCicsValue::Data(reference) = &value
                && (reference.usage != CobolUsage::Binary
                    || reference.length != 4
                    || reference.scale != 0)
            {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS ROUTE {clause} requires fullword binary storage"
                )));
            }
            operands.push(HirCicsNamedOperand { name, value });
        }
    }
    Ok(operands)
}
