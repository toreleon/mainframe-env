use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    require_writable,
};
use super::{Clauses, cics_value, complete_data_reference};
use crate::SemanticModel;

pub(super) fn resolve(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let browse = matches!(
        operation,
        HirCicsOperation::StartBrowse | HirCicsOperation::ReadNext | HirCicsOperation::ReadPrev
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
            let reference = complete_data_reference(tokens, semantic)?;
            require_writable(&reference)?;
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
