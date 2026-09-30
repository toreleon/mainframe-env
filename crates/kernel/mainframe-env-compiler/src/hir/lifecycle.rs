use super::*;

// Reuse the language lexer so a PROGRAM-ID inside a literal/comment is not a
// nested program and INITIAL in procedure/data text is not a program attribute.
pub(super) fn installed_lifecycle(text: &str, semantic: &SemanticModel) -> &'static str {
    use crate::syntax::CobolSyntaxKind as K;
    let mut remaining = text;
    let mut tokens = Vec::new();
    while !remaining.is_empty() {
        let (kind, length) = crate::syntax::next_token(remaining);
        if length == 0 {
            return "unsupported@1";
        }
        if !matches!(kind, K::Whitespace | K::Newline | K::Comment) {
            tokens.push((kind, remaining[..length].to_ascii_uppercase()));
        }
        remaining = &remaining[length..];
    }
    let ids: Vec<_> = tokens
        .iter()
        .enumerate()
        .filter_map(|(i, (kind, text))| (*kind == K::Word && text == "PROGRAM-ID").then_some(i))
        .collect();
    if ids.len() != 1
        || semantic.layouts.iter().any(|layout| {
            layout.external_name.is_some()
                || layout.global
                || layout.typedef
                || !layout.allocated
                    && !matches!(
                        layout.category,
                        crate::DataCategory::Condition | crate::DataCategory::Rename
                    )
        })
    {
        return "unsupported@1";
    }
    let mut at = ids[0] + 1;
    if tokens.get(at).is_some_and(|(_, text)| text == ".") {
        at += 1;
    }
    if tokens.get(at).is_none() {
        return "unsupported@1";
    }
    at += 1; // program name, including a quoted name
    let attributes: Vec<_> = tokens[at..]
        .iter()
        .take_while(|(_, text)| text != ".")
        .map(|(_, text)| text.as_str())
        .collect();
    match attributes.as_slice() {
        [] => "retained@1",
        ["INITIAL"] | ["INITIAL", "PROGRAM"] | ["IS", "INITIAL"] | ["IS", "INITIAL", "PROGRAM"] => {
            "initial@1"
        }
        _ => "unsupported@1",
    }
}

pub(super) fn procedure_text(source: &str) -> Option<(usize, &str)> {
    let upper = source.to_ascii_uppercase();
    let start = upper.find("PROCEDURE DIVISION")?;
    let rest = &source[start..];
    let dot = rest.find('.')?;
    Some((start + dot + 1, &rest[dot + 1..]))
}

pub(super) fn entry_formals(
    source: &str,
    semantic: &SemanticModel,
) -> Result<Vec<String>, HirProblem> {
    let upper = source_text::blank(source).to_ascii_uppercase();
    let start = upper
        .find("PROCEDURE DIVISION")
        .ok_or(HirProblem::MissingProcedure)?;
    let header = upper[start + "PROCEDURE DIVISION".len()..]
        .split('.')
        .next()
        .ok_or(HirProblem::MissingProcedure)?;
    let words = header
        .split(|ch: char| ch.is_whitespace() || ch == ',')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let Some(using) = words.iter().position(|word| *word == "USING") else {
        return Ok(Vec::new());
    };
    let mut formals = Vec::new();
    let mut index = using + 1;
    while index < words.len() && !matches!(words[index], "RETURNING" | "GIVING") {
        if words[index] == "BY" {
            index += 2;
            continue;
        }
        let layout = semantic
            .resolve(words[index])
            .map_err(|_| HirProblem::InvalidLayout)?;
        if layout.section != crate::StorageSection::Linkage || layout.parent.is_some() {
            return Err(HirProblem::InvalidLayout);
        }
        formals.push(layout.qualified_name.clone());
        index += 1;
    }
    Ok(formals)
}
