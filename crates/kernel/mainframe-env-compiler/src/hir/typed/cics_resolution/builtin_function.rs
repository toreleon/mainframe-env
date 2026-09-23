use super::super::{
    HirCicsNamedOperand, HirCicsOperandName, HirCicsOutputBinding, HirCicsOutputName, HirCicsValue,
    Resolution, ResolutionFailure, require_writable,
};
use super::{Clauses, cics_integer_value, complete_data_reference};
use crate::{DataCategory, SemanticModel};

pub(super) fn deedit_operands(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<Vec<HirCicsNamedOperand>> {
    let field = field(clauses, semantic)?;
    let mut operands = vec![HirCicsNamedOperand {
        name: HirCicsOperandName::Field,
        value: HirCicsValue::Data(field),
    }];
    if let Some(tokens) = clauses.get("LENGTH") {
        operands.push(HirCicsNamedOperand {
            name: HirCicsOperandName::Length,
            value: cics_integer_value(tokens, semantic).map_err(|problem| match problem {
                ResolutionFailure::Unsupported => ResolutionFailure::Invalid(
                    "CICS BIF DEEDIT LENGTH requires an integer value".into(),
                ),
                other => other,
            })?,
        });
    }
    Ok(operands)
}

pub(super) fn deedit_output(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<HirCicsOutputBinding> {
    Ok(HirCicsOutputBinding {
        name: HirCicsOutputName::Field,
        target: field(clauses, semantic)?,
    })
}

fn field(
    clauses: &Clauses,
    semantic: &SemanticModel,
) -> Resolution<super::super::HirDataReference> {
    let field =
        complete_data_reference(&clauses["FIELD"], semantic).map_err(|problem| match problem {
            ResolutionFailure::Unsupported => {
                ResolutionFailure::Invalid("CICS BIF DEEDIT FIELD requires a data area".into())
            }
            other => other,
        })?;
    require_writable(&field)?;
    if field.length == 0
        || !matches!(
            field.category,
            DataCategory::Alphabetic | DataCategory::Alphanumeric
        )
    {
        return Err(ResolutionFailure::Invalid(
            "CICS BIF DEEDIT FIELD requires writable character storage".into(),
        ));
    }
    Ok(field)
}
