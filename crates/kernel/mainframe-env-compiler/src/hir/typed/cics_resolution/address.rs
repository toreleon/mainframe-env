use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsValue, HirDataReference, Resolution,
    ResolutionFailure, require_writable,
};
use super::{Clauses, complete_data_reference};
use crate::{CobolUsage, SemanticModel, StorageSection};

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let target = complete_data_reference(&clauses["COMMAREA"], semantic)?;
    require_writable(&target)?;
    if !matches!(target.usage, CobolUsage::Pointer | CobolUsage::Pointer32) || target.length != 4 {
        return Err(ResolutionFailure::Invalid(
            "CICS ADDRESS COMMAREA requires a four-byte POINTER or POINTER-32 reference".into(),
        ));
    }
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::CommareaPointer,
        value: HirCicsValue::Data(target),
    }];
    if let Some(layout) = semantic.layout("DFHCOMMAREA") {
        if layout.section != StorageSection::Linkage {
            return Err(ResolutionFailure::Invalid(
                "CICS ADDRESS COMMAREA requires DFHCOMMAREA in LINKAGE SECTION".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::UsingAddress,
            value: HirCicsValue::Data(HirDataReference::from(layout)),
        });
    }
    Ok(operands)
}
