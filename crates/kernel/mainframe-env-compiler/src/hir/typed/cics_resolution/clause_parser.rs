use super::*;

pub(super) fn clauses(
    tokens: &[String],
    descriptor: Option<&CicsApplicationRegistryDescriptor>,
) -> Resolution<(Clauses, Vec<String>)> {
    let mut clauses = BTreeMap::new();
    let mut options = Vec::new();
    let mut seen = BTreeSet::new();
    let mut position = 0;
    while position < tokens.len() {
        let name = tokens[position].to_ascii_uppercase();
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(ResolutionFailure::Invalid(
                "CICS top-level clause is malformed".into(),
            ));
        }
        let has_operand = tokens.get(position + 1).is_some_and(|token| token == "(");
        if !seen.insert(name.clone()) {
            let exact_bare_flag_repeat = !has_operand
                && !clauses.contains_key(&name)
                && descriptor.is_some_and(|descriptor| {
                    matches!(
                        option_value_shape(descriptor, &name),
                        Some(CicsApplicationOptionValueShape::Flag)
                    )
                });
            if exact_bare_flag_repeat {
                position += 1;
                continue;
            }
            return Err(ResolutionFailure::Invalid(format!(
                "CICS top-level option {name} is duplicated"
            )));
        }
        if has_operand {
            let close = matching_close(tokens, position + 1)?;
            if close == position + 2 {
                return Err(ResolutionFailure::Invalid(
                    "CICS operand clause is empty".into(),
                ));
            }
            clauses.insert(name, tokens[position + 2..close].to_vec());
            position = close + 1;
        } else {
            options.push(name);
            position += 1;
        }
    }
    Ok((clauses, options))
}

pub(super) fn matching_close(tokens: &[String], open: usize) -> Resolution<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    ResolutionFailure::Invalid("CICS clause parentheses are malformed".into())
                })?;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err(ResolutionFailure::Invalid(
        "CICS clause parentheses are malformed".into(),
    ))
}
