use super::super::{HirCicsValue, Resolution, ResolutionFailure};
use super::cics_value;
use crate::{DataCategory, SemanticModel};

pub(super) fn value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            matches!(value.len(), 1..=4)
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        }
        HirCicsValue::Data(reference) => {
            matches!(reference.length, 1..=4)
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
        Err(ResolutionFailure::Invalid(
            "CICS RETURN TRANSID requires a 1-4 character name".into(),
        ))
    }
}
