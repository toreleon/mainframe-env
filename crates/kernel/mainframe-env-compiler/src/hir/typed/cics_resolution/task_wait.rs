use super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, HirCicsValue, Resolution,
    ResolutionFailure, cics_value, complete_data_reference,
};
use crate::{CobolUsage, DataCategory, SemanticModel};
use std::collections::BTreeMap;

pub(super) fn operands(
    clauses: &BTreeMap<String, Vec<String>>,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::WaitEvent {
        return Ok(Vec::new());
    }
    let pointer = complete_data_reference(&clauses["ECADDR"], semantic)?;
    if pointer.usage != CobolUsage::Pointer32 || pointer.length != 4 {
        return Err(ResolutionFailure::Invalid(
            "CICS WAIT EVENT ECADDR requires a four-byte POINTER-32 reference".into(),
        ));
    }
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::EventControlAddress,
        value: HirCicsValue::Data(pointer),
    }];
    if let Some(tokens) = clauses.get("NAME") {
        let value = cics_value(tokens, semantic)?;
        let valid = match &value {
            HirCicsValue::Literal(value) => {
                matches!(value.len(), 1..=8)
                    && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            }
            HirCicsValue::Data(reference) => {
                matches!(reference.length, 1..=8)
                    && matches!(
                        reference.category,
                        DataCategory::Alphabetic | DataCategory::Alphanumeric
                    )
            }
            _ => false,
        };
        if !valid {
            return Err(ResolutionFailure::Invalid(
                "CICS WAIT EVENT NAME must be 1-8 alphanumeric characters".into(),
            ));
        }
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::WaitName,
            value,
        });
    }
    Ok(operands)
}
