use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, require_writable,
};
use super::{Clauses, cics_value, complete_data_reference, numeric_literal};
use crate::{DataCategory, SemanticModel};

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    let required: &[&str] = match operation {
        HirCicsOperation::StartBrowse => &["RIDFLD"],
        HirCicsOperation::Delete | HirCicsOperation::EndBrowse => &[],
        HirCicsOperation::ReadNext | HirCicsOperation::ReadPrev | HirCicsOperation::Read => {
            &["RIDFLD", "INTO"]
        }
        HirCicsOperation::Write => &["FROM", "RIDFLD"],
        HirCicsOperation::Rewrite => &["FROM"],
        _ => return Ok(()),
    };
    let resources =
        usize::from(clauses.contains_key("FILE")) + usize::from(clauses.contains_key("DATASET"));
    if resources != 1 {
        return Err(ResolutionFailure::Invalid(
            "CICS file command requires exactly one FILE or DATASET".into(),
        ));
    }
    for name in required {
        if !clauses.contains_key(*name) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS {operation:?} requires {name}"
            )));
        }
    }
    Ok(())
}

pub(super) fn resolve(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if !matches!(
        operation,
        HirCicsOperation::StartBrowse
            | HirCicsOperation::ReadNext
            | HirCicsOperation::ReadPrev
            | HirCicsOperation::EndBrowse
            | HirCicsOperation::Delete
            | HirCicsOperation::Write
            | HirCicsOperation::Read
            | HirCicsOperation::Rewrite
    ) {
        return Ok(Vec::new());
    }
    let browse = matches!(
        operation,
        HirCicsOperation::StartBrowse | HirCicsOperation::ReadNext | HirCicsOperation::ReadPrev
    );
    let stored_file_input = matches!(
        operation,
        HirCicsOperation::Delete | HirCicsOperation::Write
    );
    let mut operands = Vec::new();
    for (name, identity) in [
        ("FILE", HirCicsOperandName::File),
        ("DATASET", HirCicsOperandName::Dataset),
        ("FROM", HirCicsOperandName::From),
        ("RIDFLD", HirCicsOperandName::Ridfld),
    ] {
        let Some(tokens) = clauses.get(name) else {
            continue;
        };
        let value = if browse && name == "RIDFLD" {
            let HirCicsValue::Data(reference) = cics_value(tokens, semantic)? else {
                return Err(ResolutionFailure::Invalid(
                    "CICS RIDFLD requires a data area".into(),
                ));
            };
            require_writable(&reference)?;
            HirCicsValue::Data(reference)
        } else if stored_file_input && matches!(name, "FROM" | "RIDFLD") {
            let HirCicsValue::Data(reference) = cics_value(tokens, semantic)? else {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS {name} requires a data area"
                )));
            };
            HirCicsValue::Data(reference)
        } else {
            cics_value(tokens, semantic)?
        };
        operands.push(HirCicsNamedOperand {
            name: identity,
            value,
        });
    }
    for (name, identity) in [
        ("LENGTH", HirCicsOperandName::Length),
        ("KEYLENGTH", HirCicsOperandName::KeyLength),
    ] {
        if let Some(tokens) = clauses.get(name) {
            if matches!(tokens.as_slice(), [token] if numeric_literal(token).is_some()) {
                return Err(ResolutionFailure::Invalid(format!(
                    "CICS typed lowering is unready for {name} numeric literal"
                )));
            }
            operands.push(HirCicsNamedOperand {
                name: identity,
                value: numeric_length_value(tokens, semantic)?,
            });
        }
    }
    Ok(operands)
}

fn numeric_length_value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
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
