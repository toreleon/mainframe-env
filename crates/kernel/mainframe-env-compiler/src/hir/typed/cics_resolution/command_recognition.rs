//! Top-level command-head tokens carried into valued CICS clauses.

use super::{builtin_function, compatibility_alias_target, journal_control, task_wait};
use crate::SemanticModel;
use mainframe_env_ir::{CicsApplicationOptionValueShape, CicsApplicationRegistryDescriptor};

pub(super) fn clause_tokens(
    body: &[String],
    head: &[&str],
    descriptor: &CicsApplicationRegistryDescriptor,
) -> Vec<String> {
    let remainder = &body[head.len()..];
    let mut tokens = Vec::with_capacity(remainder.len() + 1);
    if remainder.first().is_some_and(|token| token == "(")
        && let Some(last) = head.last()
        && (descriptor.options.iter().any(|option| option.name == *last)
            || super::journal_control::synthetic_selector(descriptor, last))
    {
        tokens.push((*last).into());
    }
    tokens.extend_from_slice(remainder);
    tokens
}

pub(super) fn option_value_shape(
    descriptor: &CicsApplicationRegistryDescriptor,
    name: &str,
) -> Option<CicsApplicationOptionValueShape> {
    if matches!(
        descriptor.label_tokens,
        ["ISSUE", "ABORT" | "END" | "SEND" | "WAIT"]
    ) && matches!(name, "WPMEDIA2" | "WPMEDIA3" | "WPMEDIA4")
    {
        // The pinned prose explicitly lists all four media; the projected
        // syntax diagram currently materializes only WPMEDIA1.
        return Some(CicsApplicationOptionValueShape::Flag);
    }
    builtin_function::option_value_shape(descriptor, name)
        .or_else(|| task_wait::option_value_shape(descriptor, name))
        .or_else(|| journal_control::option_value_shape(descriptor, name))
        .or_else(|| {
            descriptor
                .options
                .iter()
                .find(|option| option.name == name)
                .map(|option| option.value_shape)
                .or_else(|| {
                    compatibility_alias_target(descriptor, name).and_then(|canonical| {
                        descriptor
                            .options
                            .iter()
                            .find(|option| option.name == canonical)
                            .map(|option| option.value_shape)
                    })
                })
        })
}

pub(super) fn statically_known_value_bytes(
    tokens: &[String],
    semantic: &SemanticModel,
) -> Option<usize> {
    if let [literal] = tokens
        && literal.len() >= 2
        && let Some(quote) = literal.chars().next()
        && matches!(quote, '\'' | '"')
        && literal.ends_with(quote)
    {
        let contents = &literal[quote.len_utf8()..literal.len() - quote.len_utf8()];
        let escaped = format!("{quote}{quote}");
        return Some(contents.replace(&escaped, &quote.to_string()).len());
    }
    semantic
        .resolve(&tokens.join(" "))
        .ok()
        .map(|layout| layout.length)
}
