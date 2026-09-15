use super::super::{HirCicsValue, Resolution, ResolutionFailure};
use super::cics_value;
use crate::{DataCategory, SemanticModel};

fn valid_program_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'@' | b'#')
}

pub(super) fn value(
    tokens: &[String],
    semantic: &SemanticModel,
    command: &str,
) -> Resolution<HirCicsValue> {
    let value = cics_value(tokens, semantic)?;
    let valid = match &value {
        HirCicsValue::Literal(value) => {
            matches!(value.len(), 1..=8) && value.bytes().all(valid_program_character)
        }
        HirCicsValue::Data(reference) => {
            reference.length == 8
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
            "CICS {command} PROGRAM requires a 1-8 character literal name or an 8-byte alphanumeric data area; names use A-Z, 0-9, $, @, or #"
        )))
    }
}
