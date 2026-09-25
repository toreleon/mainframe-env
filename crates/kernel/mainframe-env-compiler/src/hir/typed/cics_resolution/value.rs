use super::super::{
    HirCicsOutputBinding, HirCicsOutputName, HirCicsValue, HirDataReference, Resolution,
    ResolutionFailure, data_reference_at, numeric_literal,
};
use crate::SemanticModel;

pub(super) fn cics_value(tokens: &[String], semantic: &SemanticModel) -> Resolution<HirCicsValue> {
    if let [value] = tokens
        && value.len() >= 2
        && value.starts_with(['\'', '"'])
        && value.as_bytes().first() == value.as_bytes().last()
    {
        return Ok(HirCicsValue::Literal(value[1..value.len() - 1].into()));
    }
    if matches!(tokens, [value] if numeric_literal(value).is_some()) {
        return Err(ResolutionFailure::Unsupported);
    }
    complete_data_reference(tokens, semantic).map(HirCicsValue::Data)
}

pub(super) fn cics_address_value(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<(bool, HirDataReference)> {
    if tokens.len() > 2
        && tokens[0].eq_ignore_ascii_case("ADDRESS")
        && tokens[1].eq_ignore_ascii_case("OF")
    {
        complete_data_reference(&tokens[2..], semantic).map(|reference| (true, reference))
    } else {
        complete_data_reference(tokens, semantic).map(|reference| (false, reference))
    }
}

pub(super) fn complete_data_reference(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Resolution<HirDataReference> {
    let (reference, end) = data_reference_at(tokens, 0, semantic)?;
    if end == tokens.len() {
        Ok(reference)
    } else {
        Err(ResolutionFailure::Unsupported)
    }
}

pub(super) fn output(
    outputs: &[HirCicsOutputBinding],
    name: HirCicsOutputName,
) -> Option<&HirDataReference> {
    outputs
        .iter()
        .find(|output| output.name == name)
        .map(|output| &output.target)
}
