//! Top-level command-head tokens carried into valued CICS clauses.

use mainframe_env_ir::CicsApplicationRegistryDescriptor;

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
