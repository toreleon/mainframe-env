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
