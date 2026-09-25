use super::{
    HirDataReference, Resolution, ResolutionFailure, data_reference_at, is_add_corresponding_group,
};
use crate::SemanticModel;

pub(super) fn corresponding_group_reference_at(
    tokens: &[String],
    start: usize,
    semantic: &SemanticModel,
) -> Resolution<(HirDataReference, usize)> {
    let mut suffix = start + 1;
    while tokens
        .get(suffix)
        .is_some_and(|token| matches!(token.as_str(), "OF" | "IN"))
    {
        suffix += 2;
    }
    let has_subscript = tokens.get(suffix).is_some_and(|token| token == "(");
    if has_subscript {
        let close = matching_close(tokens, suffix)?;
        if tokens[suffix + 1..close]
            .iter()
            .any(|token| token.contains(':'))
        {
            return Err(ResolutionFailure::Invalid(
                "ADD CORRESPONDING group operands cannot be reference modified".into(),
            ));
        }
    }
    let spelling = tokens
        .get(start..suffix)
        .ok_or_else(|| ResolutionFailure::Invalid("data reference is missing".into()))?
        .join(" ");
    let layout = semantic.resolve(&spelling).map_err(|problem| {
        ResolutionFailure::Invalid(format!("cannot resolve {spelling}: {problem:?}"))
    })?;
    if !is_add_corresponding_group(layout.category) {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING operands must be alphanumeric or national groups".into(),
        ));
    }
    let requires_subscript = std::iter::successors(Some(layout), |layout| {
        layout
            .parent
            .as_deref()
            .and_then(|parent| semantic.layout(parent))
    })
    .any(|layout| layout.occurs_clause);
    if requires_subscript && !has_subscript {
        return Err(ResolutionFailure::Invalid(
            "ADD CORRESPONDING table group operand requires subscripting".into(),
        ));
    }
    data_reference_at(tokens, start, semantic)
}

fn matching_close(tokens: &[String], open: usize) -> Resolution<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1).ok_or(ResolutionFailure::Unsupported)?;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err(ResolutionFailure::Unsupported)
}
