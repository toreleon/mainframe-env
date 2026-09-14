use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOutputBinding, HirCicsOutputName, HirCicsValue,
    Resolution, require_writable,
};
use super::{Clauses, complete_data_reference, program_name};
use crate::SemanticModel;

pub(super) fn link_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Program,
        value: program_name::value(&clauses["PROGRAM"], semantic, "LINK")?,
    }];
    if let Some(tokens) = clauses.get("COMMAREA") {
        let reference = complete_data_reference(tokens, semantic)?;
        require_writable(&reference)?;
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Commarea,
            value: HirCicsValue::Data(reference),
        });
    }
    Ok(operands)
}

pub(super) fn link_commarea_output(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Option<HirCicsOutputBinding>> {
    let Some(tokens) = clauses.get("COMMAREA") else {
        return Ok(None);
    };
    let target = complete_data_reference(tokens, semantic)?;
    require_writable(&target)?;
    Ok(Some(HirCicsOutputBinding {
        name: HirCicsOutputName::Commarea,
        target,
    }))
}
