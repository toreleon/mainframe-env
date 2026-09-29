use super::{
    HirCicsValue, Resolution, ResolutionFailure, complete_data_reference, numeric_literal,
    require_numeric,
};
use crate::SemanticModel;

pub(super) fn cics_integer_value(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<HirCicsValue> {
    if tokens
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("LENGTH"))
        && tokens
            .get(1)
            .is_some_and(|token| token.eq_ignore_ascii_case("OF"))
    {
        return complete_data_reference(&tokens[2..], semantic).map(HirCicsValue::LengthOf);
    }
    if let [value] = tokens
        && let Some(literal) = numeric_literal(value)
        && literal.scale == 0
    {
        let magnitude = literal.digits.parse::<i64>().map_err(|_| {
            ResolutionFailure::Invalid("CICS integer operand is out of range".into())
        })?;
        let value = if literal.negative {
            magnitude.checked_neg().ok_or_else(|| {
                ResolutionFailure::Invalid("CICS integer operand is out of range".into())
            })?
        } else {
            magnitude
        };
        return Ok(HirCicsValue::Integer(value));
    }
    let reference = complete_data_reference(tokens, semantic)?;
    require_numeric(&reference)?;
    Ok(HirCicsValue::Data(reference))
}

pub(super) fn cics_cvda_value(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<HirCicsValue> {
    if let [function, open, value, close] = tokens
        && function.eq_ignore_ascii_case("DFHVALUE")
        && open == "("
        && close == ")"
        && matches!(value.as_str(), "TASK" | "UOW" | "LUW" | "LOG" | "NOLOG")
    {
        return Ok(HirCicsValue::Integer(match value.as_str() {
            "TASK" => 233,
            "UOW" | "LUW" => 246,
            "LOG" => 2890,
            "NOLOG" => 2891,
            _ => unreachable!(),
        }));
    }
    cics_integer_value(tokens, semantic)
}
