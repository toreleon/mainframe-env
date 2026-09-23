use super::super::{HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, Resolution};
use super::{Clauses, cics_value, shape::CommandShape};
use crate::SemanticModel;

pub(super) fn shape(operation: HirCicsOperation) -> Option<CommandShape> {
    match operation {
        HirCicsOperation::DefineInputEvent => Some(CommandShape {
            clauses: &["EVENT", "RESP", "RESP2"],
            options: &["NOHANDLE"],
            required: &["EVENT"],
        }),
        _ => None,
    }
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::DefineInputEvent {
        return Ok(Vec::new());
    }
    Ok(vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Event,
        value: cics_value(&clauses["EVENT"], semantic)?,
    }])
}
