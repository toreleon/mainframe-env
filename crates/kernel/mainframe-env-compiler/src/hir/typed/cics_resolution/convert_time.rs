use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsValue, Resolution, ResolutionFailure,
};
use super::{Clauses, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let source = complete_data_reference(&clauses["DATESTRING"], semantic)?;
    if source.length != 64
        || !matches!(
            source.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS CONVERTTIME DATESTRING requires a 64-character data area".into(),
        ));
    }
    Ok(vec![HirCicsNamedOperand {
        name: HirCicsOperandName::DateString,
        value: HirCicsValue::Data(source),
    }])
}
