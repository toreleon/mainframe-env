use super::super::{HirCicsValue, Resolution, ResolutionFailure};
use super::cics_value;
use crate::{DataCategory, SemanticModel};

pub(super) fn value(
    tokens: &[String],
    semantic: &SemanticModel,
    command: &str,
) -> Resolution<HirCicsValue> {
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            matches!(value.len(), 1..=8) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }
        HirCicsValue::Data(reference) => {
            matches!(reference.length, 1..=8)
                && matches!(
                    reference.category,
                    DataCategory::Alphabetic | DataCategory::Alphanumeric
                )
        }
        HirCicsValue::Integer(_) => false,
    };
    if valid {
        Ok(value)
    } else {
        Err(ResolutionFailure::Invalid(format!(
            "CICS {command} PROGRAM requires a 1-8 character name"
        )))
    }
}
