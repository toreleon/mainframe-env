use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOperation, Resolution, ResolutionFailure,
};
use super::{Clauses, cics_value};
use crate::SemanticModel;

pub(super) struct TransformShape {
    pub(super) clauses: &'static [&'static str],
    pub(super) options: &'static [&'static str],
    pub(super) required: &'static [&'static str],
}

pub(super) fn shape(operation: HirCicsOperation) -> Option<TransformShape> {
    if operation != HirCicsOperation::TransformDataToJson {
        return None;
    }
    Some(TransformShape {
        clauses: &[
            "CHANNEL",
            "INCONTAINER",
            "OUTCONTAINER",
            "TRANSFORMER",
            "RESP",
            "RESP2",
        ],
        options: &["NOHANDLE"],
        required: &["CHANNEL", "INCONTAINER", "TRANSFORMER"],
    })
}

pub(super) fn validate_constraints(
    clauses: &Clauses,
    operation: HirCicsOperation,
) -> Resolution<()> {
    let Some(shape) = shape(operation) else {
        return Ok(());
    };
    for required in shape.required {
        if !clauses.contains_key(*required) {
            return Err(ResolutionFailure::Invalid(format!(
                "CICS TRANSFORM DATATOJSON requires {required}"
            )));
        }
    }
    Ok(())
}

pub(super) fn operands(
    clauses: &Clauses,
    operation: HirCicsOperation,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    if operation != HirCicsOperation::TransformDataToJson {
        return Ok(Vec::new());
    }
    [
        ("CHANNEL", HirCicsOperandName::Channel),
        ("INCONTAINER", HirCicsOperandName::InContainer),
        ("OUTCONTAINER", HirCicsOperandName::OutContainer),
        ("TRANSFORMER", HirCicsOperandName::Transformer),
    ]
    .into_iter()
    .filter_map(|(name, identity)| clauses.get(name).map(|value| (identity, value)))
    .map(|(name, value)| {
        Ok(HirCicsNamedOperand {
            name,
            value: cics_value(value, semantic)?,
        })
    })
    .collect()
}
