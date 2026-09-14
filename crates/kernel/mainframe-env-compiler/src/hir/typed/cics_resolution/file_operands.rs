use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, require_writable,
};
use super::{Clauses, cics_value};
use crate::SemanticModel;

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    let required: &[&str] = match operation {
        HirCicsOperation::StartBrowse | HirCicsOperation::Delete => &["RIDFLD"],
        HirCicsOperation::ReadNext | HirCicsOperation::ReadPrev | HirCicsOperation::Read => {
            &["RIDFLD", "INTO"]
        }
        HirCicsOperation::Write => &["FROM", "RIDFLD"],
        HirCicsOperation::Rewrite => &["FROM"],
        HirCicsOperation::EndBrowse => &[],
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
    Ok(operands)
}
