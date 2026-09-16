//! Generic text handling shared by HIR statement construction: the plain
//! source-text scans `hir.rs` runs over a statement's header text, and the
//! comment blanking that must happen before any of that text is cut from
//! the procedure source.
//!
//! `blank`, `semantic_tokens`, `go_to_targets`, and `perform_targets` all
//! operate on plain `&str`/`String` with no dependency on HIR types; they
//! moved out of `hir.rs` together as one cohesive, self-contained group.

use super::statement_grammar::quoted_length;

/// Blank comment-line bytes out of HIR statement source before the
/// procedure grammar (`hir/statement_grammar.rs`) ever lexes it.
///
/// `token_range` in `statement_grammar.rs` builds statement, option and
/// branch text as a raw byte span between a statement's first and last
/// token (14 call sites). A fixed-format column-7 comment line normalizes
/// to a floating `*>` comment (see `normalize_source` in `syntax.rs`), and
/// a genuinely free-format `*>` comment looks the same; either way the
/// comment is never tokenized, so nothing removes its bytes from a span
/// that happens to straddle it -- for example a comment line between the
/// options of a multi-line `EXEC CICS ... END-EXEC` block. The comment
/// text then reaches the resolved operand/argument text, where CICS
/// top-level clause parsing rejects it (`toreleon/mainframe-env#176`).
///
/// `blank` replaces every `*>...` comment's bytes (up to but excluding the
/// newline) with ASCII spaces, preserving the exact byte length and every
/// other byte -- including quoted-literal content, which is skipped whole
/// via the same scan `lex` uses (`quoted_length`) so a literal containing
/// `*` or `*>` is never touched. Calling it once before lexing keeps every
/// `Range<usize>` computed downstream valid (offsets are unchanged) while
/// making every comment invisible to every later span slice, without
/// changing `token_range` or any of its call sites: a comment-free source
/// is returned unchanged, so resolved text for comment-free statements is
/// unaffected.
pub(super) fn blank(source: &str) -> String {
    let mut masked = source.as_bytes().to_vec();
    let mut index = 0usize;
    while index < source.len() {
        let rest = &source[index..];
        if matches!(rest.as_bytes()[0], b'\'' | b'"') {
            match quoted_length(rest, 0) {
                Some(length) => {
                    index += length;
                    continue;
                }
                None => break,
            }
        }
        if rest.starts_with("*>") {
            let length = rest.find('\n').unwrap_or(rest.len());
            masked[index..index + length].fill(b' ');
            index += length;
            continue;
        }
        index += rest.chars().next().map_or(1, char::len_utf8);
    }
    String::from_utf8(masked).expect("masking only replaces valid UTF-8 with ASCII spaces")
}

pub(super) fn perform_targets(text: &str) -> Option<(String, Option<String>)> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.trim_matches([',', '.']).to_ascii_uppercase())
        .collect();
    let target = words.get(1)?.clone();
    let through = words
        .windows(2)
        .find(|pair| matches!(pair[0].as_str(), "THRU" | "THROUGH"))
        .map(|pair| pair[1].clone());
    Some((target, through))
}

pub(super) fn go_to_targets(text: &str) -> Option<(Vec<String>, bool)> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.trim_matches([',', '.']).to_ascii_uppercase())
        .collect();
    let to = words.iter().position(|word| word == "TO")?;
    let depending = words.iter().position(|word| word == "DEPENDING");
    let end = depending.unwrap_or(words.len());
    let targets = words[to + 1..end]
        .iter()
        .filter(|word| word.as_str() != ",")
        .cloned()
        .collect::<Vec<_>>();
    (!targets.is_empty()).then_some((targets, depending.is_some()))
}

pub(super) fn semantic_tokens(sentence: &str, keyword_words: usize) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in sentence.chars() {
        if matches!(ch, '\'' | '"') {
            if quote == Some(ch) {
                current.push(ch);
                tokens.push(current.clone());
                current.clear();
                quote = None;
            } else if quote.is_none() {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                current.push(ch);
                quote = Some(ch);
            } else {
                current.push(ch);
            }
        } else if quote.is_some() {
            current.push(ch);
        } else if ch.is_whitespace() || matches!(ch, ',' | '(' | ')' | '=') {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            if matches!(ch, '=' | '(' | ')') {
                tokens.push(ch.to_string());
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
        .into_iter()
        .skip(keyword_words)
        .map(|token| {
            if token.starts_with(['\'', '"']) {
                token
            } else {
                token.to_ascii_uppercase()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::blank;

    #[test]
    fn comment_between_tokens_is_blanked_without_shifting_offsets() {
        let source = "COMMAREA (X)\n*>  LENGTH(LENGTH OF X)\nEND-EXEC";
        let masked = blank(source);
        assert_eq!(masked.len(), source.len());
        assert!(!masked.contains("LENGTH"));
        assert!(masked.contains("COMMAREA (X)"));
        assert!(masked.contains("END-EXEC"));
    }

    #[test]
    fn literal_containing_the_comment_marker_is_untouched() {
        let source = "MOVE '*>NOT-A-COMMENT' TO X";
        assert_eq!(blank(source), source);
    }

    #[test]
    fn source_without_a_comment_is_returned_unchanged() {
        let source = "EXEC CICS RETURN TRANSID (X) END-EXEC";
        assert_eq!(blank(source), source);
    }
}
