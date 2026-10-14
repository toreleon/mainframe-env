//! Syntactic intrinsic argument boundaries; classification never evaluates operands.

use super::{MachineProblem, ReferenceMachine, matching_close};

pub(super) fn split_function_arguments<'a>(
    machine: &ReferenceMachine,
    tokens: &'a [String],
) -> Result<Vec<&'a [String]>, MachineProblem> {
    if tokens.is_empty() {
        return Ok(Vec::new());
    }
    let mut arguments = Vec::new();
    let mut start = 0usize;
    let mut operand_start = 0usize;
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if depth == 0 {
            if matches!(token.as_str(), "," | ";") {
                if start == index {
                    return Err(MachineProblem::InvalidOperation);
                }
                arguments.push(&tokens[start..index]);
                start = index + 1;
                operand_start = start;
                continue;
            }
            if index > start && space_separates_arguments(machine, tokens, operand_start, index) {
                arguments.push(&tokens[start..index]);
                start = index;
                operand_start = index;
            }
        }
        match token.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(MachineProblem::InvalidOperation)?;
            }
            "+" | "-" | "*" | "/" if depth == 0 => operand_start = index + 1,
            _ => {}
        }
    }
    if depth != 0 || start == tokens.len() {
        return Err(MachineProblem::InvalidOperation);
    }
    arguments.push(&tokens[start..]);
    Ok(arguments)
}

fn space_separates_arguments(
    machine: &ReferenceMachine,
    tokens: &[String],
    operand_start: usize,
    next: usize,
) -> bool {
    let prefix = &tokens[operand_start..next];
    if prefix.is_empty()
        || matches!(
            prefix.last().map(String::as_str),
            Some("FUNCTION" | "OF" | "IN")
        )
        || matches!(tokens[next].as_str(), "+" | "-" | "*" | "/" | "OF" | "IN")
    {
        return false;
    }
    if tokens[next] != "(" {
        return true;
    }
    if ((prefix[0] == "FUNCTION" || prefix == ["DFHRESP"])
        && !prefix.iter().any(|token| token == "("))
        || has_reference_modifier(tokens, next)
    {
        return false;
    }
    // COBOL 6.5 rlrefsub: only an OCCURS item or its descendant consumes a
    // subscript. Otherwise parentheses begin the next arithmetic argument.
    // Inspect declared metadata only, including for variable-length tables.
    let reference = match prefix {
        [kind, of, rest @ ..] if matches!(kind.as_str(), "LENGTH" | "ADDRESS") && of == "OF" => {
            rest
        }
        _ => prefix,
    };
    let mut layout = machine.layout_qualified(reference);
    while let Some(item) = layout {
        if item.occurs_clause {
            return false;
        }
        layout = item
            .parent
            .as_ref()
            .and_then(|parent| machine.layouts.get(parent));
    }
    true
}

fn has_reference_modifier(tokens: &[String], open: usize) -> bool {
    let Some(close) = matching_close(tokens, open) else {
        return false;
    };
    let mut depth = 0usize;
    for token in &tokens[open + 1..close] {
        match token.as_str() {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            _ if depth == 0
                && !matches!(token.as_bytes().first(), Some(b'\'' | b'"'))
                && token.contains(':') =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}
