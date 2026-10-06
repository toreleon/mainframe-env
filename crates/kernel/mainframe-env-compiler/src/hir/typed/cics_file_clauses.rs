//! CICS READ/REWRITE file operand and record-length clause lowering.
//!
//! Split out of `typed.rs` to keep it under its ADR-0010 module-review
//! budget (`conformance/subsystems/cics/application/inventory/module-budgets.json`). Builds the
//! `FILE`/`DATASET`/`FROM`/`RIDFLD` named operands plus the `LENGTH` and
//! `KEYLENGTH` operands (halfword binary data items or `LENGTH OF`), and
//! computes the `READ ... LENGTH` output write-back binding.

use super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsOutputBinding,
    HirCicsOutputName, HirCicsValue, Resolution, ResolutionFailure, complete_data_reference,
    numeric_literal, require_writable,
};
use crate::{DataCategory, SemanticModel};
use std::collections::BTreeMap;

/// Build the `FILE`/`DATASET`/`FROM`/`RIDFLD`/`LENGTH`/`KEYLENGTH` operands
/// present in `clauses`, plus the `READ ... LENGTH` output binding when the
/// command is a `READ` whose `LENGTH` target is a writable data item.
pub(super) fn cics_operands(
    clauses: &BTreeMap<String, Vec<String>>,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<(Vec<HirCicsNamedOperand>, Option<HirCicsOutputBinding>)> {
    let mut operands = Vec::new();
    for (name, identity) in [
        ("FILE", HirCicsOperandName::File),
        ("DATASET", HirCicsOperandName::Dataset),
        ("FROM", HirCicsOperandName::From),
        ("RIDFLD", HirCicsOperandName::Ridfld),
    ] {
        if let Some(value) = clauses.get(name) {
            operands.push(HirCicsNamedOperand {
                name: identity,
                value: cics_value(value, semantic)?,
            });
        }
    }
    let mut length_output = None;
    for (name, identity) in [
        ("LENGTH", HirCicsOperandName::Length),
        ("KEYLENGTH", HirCicsOperandName::KeyLength),
    ] {
        if let Some(value) = clauses.get(name) {
            let value = cics_numeric_value(value, semantic)?;
            if operation == HirCicsOperation::Read
                && identity == HirCicsOperandName::Length
                && let HirCicsValue::Data(target) = &value
            {
                require_writable(target)?;
                length_output = Some(target.clone());
            }
            operands.push(HirCicsNamedOperand {
                name: identity,
                value,
            });
        }
    }
    Ok((
        operands,
        length_output.map(|target| HirCicsOutputBinding {
            name: HirCicsOutputName::Length,
            target,
        }),
    ))
}

fn cics_value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if let [value] = tokens
        && value.len() >= 2
        && value.starts_with(['\'', '"'])
        && value.as_bytes().first() == value.as_bytes().last()
    {
        return Ok(HirCicsValue::Literal(value[1..value.len() - 1].into()));
    }
    if matches!(tokens, [value] if numeric_literal(value).is_some()) {
        return Err(ResolutionFailure::Unsupported);
    }
    complete_data_reference(tokens, semantic).map(HirCicsValue::Data)
}

fn cics_numeric_value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
        && tokens
            .get(1)
            .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
    {
        return complete_data_reference(&tokens[2..], semantic).map(HirCicsValue::LengthOf);
    }
    let reference = complete_data_reference(tokens, semantic)?;
    if reference.category != DataCategory::Binary || reference.length != 2 {
        return Err(ResolutionFailure::Invalid(format!(
            "{} is not a halfword binary data item",
            reference.qualified_name
        )));
    }
    Ok(HirCicsValue::Data(reference))
}
