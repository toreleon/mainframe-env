use mainframe_env_encoding::CodePage;
use mainframe_env_source::{LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat};
use rowan::{GreenNode, GreenNodeBuilder, Language};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxLimits {
    pub max_source_bytes: usize,
    pub max_tokens: usize,
    pub max_token_bytes: usize,
    pub max_lines: usize,
    pub max_expanded_bytes: usize,
    pub max_copy_depth: usize,
}
impl Default for SyntaxLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 4 * 1024 * 1024,
            max_tokens: 1_000_000,
            max_token_bytes: 65_536,
            max_lines: 200_000,
            max_expanded_bytes: 16 * 1024 * 1024,
            max_copy_depth: 32,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum CobolSyntaxKind {
    Root,
    Word,
    Number,
    String,
    Punctuation,
    Whitespace,
    Newline,
    Comment,
    Error,
}

impl From<CobolSyntaxKind> for rowan::SyntaxKind {
    fn from(kind: CobolSyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CobolLanguage {}
impl Language for CobolLanguage {
    type Kind = CobolSyntaxKind;
    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        match raw.0 {
            0 => CobolSyntaxKind::Root,
            1 => CobolSyntaxKind::Word,
            2 => CobolSyntaxKind::Number,
            3 => CobolSyntaxKind::String,
            4 => CobolSyntaxKind::Punctuation,
            5 => CobolSyntaxKind::Whitespace,
            6 => CobolSyntaxKind::Newline,
            7 => CobolSyntaxKind::Comment,
            _ => CobolSyntaxKind::Error,
        }
    }
    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

#[derive(Clone, Debug)]
pub struct LosslessSyntax {
    green: GreenNode,
    text: String,
    semantic_text: String,
    token_count: usize,
    expansions: Vec<Expansion>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expansion {
    pub output_start: usize,
    pub output_end: usize,
    pub source: LogicalPath,
}
impl LosslessSyntax {
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
    #[must_use]
    pub(crate) fn semantic_text(&self) -> &str {
        &self.semantic_text
    }
    #[must_use]
    pub const fn token_count(&self) -> usize {
        self.token_count
    }
    #[must_use]
    pub fn expansions(&self) -> &[Expansion] {
        &self.expansions
    }
}

pub(crate) fn decode_and_lex(
    bundle: &SourceBundle,
    limits: SyntaxLimits,
) -> Result<LosslessSyntax, SyntaxProblem> {
    let file = bundle
        .file(bundle.primary())
        .ok_or(SyntaxProblem::PrimaryMissing)?;
    if file.bytes().len() > limits.max_source_bytes {
        return Err(SyntaxProblem::SourceLimitExceeded);
    }
    let decoded = decode_file(file, limits)?;
    let normalized = normalize_source(&decoded, file.format(), limits)?;
    let (semantic_text, expansions) = expand_copies(&normalized, bundle, limits, &mut Vec::new())?;
    let mut syntax = lex(&decoded, limits)?;
    syntax.semantic_text = semantic_text;
    syntax.expansions = expansions;
    Ok(syntax)
}

fn normalize_source(
    source: &str,
    format: SourceFormat,
    limits: SyntaxLimits,
) -> Result<String, SyntaxProblem> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.lines().count() > limits.max_lines {
        return Err(SyntaxProblem::LineLimitExceeded);
    }
    if format != SourceFormat::Fixed {
        return Ok(normalized);
    }
    let mut output = String::new();
    let mut continuing = false;
    for line in normalized.lines() {
        let bytes = line.as_bytes();
        let indicator = bytes.get(6).copied().unwrap_or(b' ');
        if matches!(indicator, b'*' | b'/') {
            output.push_str("*> ");
            output.push_str(line);
            output.push('\n');
            continuing = false;
            continue;
        }
        let start = bytes.len().min(7);
        let end = bytes.len().min(72);
        let code = &line[start..end];
        if indicator == b'-' && continuing {
            output.push_str(code.trim_start());
        } else {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(code);
        }
        output.push('\n');
        continuing = !code.trim_end().ends_with('.');
        if output.len() > limits.max_source_bytes.saturating_mul(2) {
            return Err(SyntaxProblem::SourceLimitExceeded);
        }
    }
    Ok(output)
}

fn decode_file(file: &SourceFile, limits: SyntaxLimits) -> Result<String, SyntaxProblem> {
    match file.encoding() {
        SourceEncoding::Utf8 => {
            String::from_utf8(file.bytes().to_vec()).map_err(|_| SyntaxProblem::InvalidEncoding)
        }
        SourceEncoding::Ebcdic(ccsid) => CodePage::from_ccsid(ccsid)
            .map_err(|_| SyntaxProblem::UnsupportedEncoding)?
            .decode(file.bytes(), limits.max_expanded_bytes)
            .map_err(|_| SyntaxProblem::InvalidEncoding),
    }
}

fn expand_copies(
    source: &str,
    bundle: &SourceBundle,
    limits: SyntaxLimits,
    stack: &mut Vec<String>,
) -> Result<(String, Vec<Expansion>), SyntaxProblem> {
    if stack.len() >= limits.max_copy_depth {
        return Err(SyntaxProblem::CopyDepthExceeded);
    }
    let mut output = String::new();
    let mut expansions = Vec::new();
    let mut cursor = 0usize;
    loop {
        let upper = source[cursor..].to_ascii_uppercase();
        let Some(relative) = upper.find("COPY ") else {
            output.push_str(&source[cursor..]);
            break;
        };
        let start = cursor + relative;
        output.push_str(&source[cursor..start]);
        let end = source[start..]
            .find('.')
            .map(|value| start + value + 1)
            .ok_or(SyntaxProblem::InvalidCopy)?;
        let command = source[start..end].trim_end_matches('.').trim();
        let words: Vec<_> = command.split_whitespace().collect();
        let name = words
            .get(1)
            .ok_or(SyntaxProblem::InvalidCopy)?
            .trim_matches(['\'', '"'])
            .to_ascii_uppercase();
        if stack.contains(&name) {
            return Err(SyntaxProblem::CircularCopy);
        }
        let dependency = bundle
            .files()
            .iter()
            .find(|file| {
                let leaf = file
                    .path()
                    .as_str()
                    .rsplit('/')
                    .next()
                    .unwrap_or(file.path().as_str());
                leaf.split('.')
                    .next()
                    .is_some_and(|stem| stem.eq_ignore_ascii_case(&name))
            })
            .ok_or(SyntaxProblem::CopyNotFound)?;
        stack.push(name);
        let decoded = decode_file(dependency, limits)?;
        let normalized = normalize_source(&decoded, dependency.format(), limits)?;
        let (mut expanded, nested) = expand_copies(&normalized, bundle, limits, stack)?;
        stack.pop();
        if let Some(replacing) = command.to_ascii_uppercase().find(" REPLACING ") {
            apply_replacing(&mut expanded, &command[replacing + " REPLACING ".len()..])?;
        }
        let output_start = output.len();
        output.push_str(&expanded);
        let output_end = output.len();
        expansions.push(Expansion {
            output_start,
            output_end,
            source: dependency.path().clone(),
        });
        for mut item in nested {
            item.output_start += output_start;
            item.output_end += output_start;
            expansions.push(item);
        }
        if output.len() > limits.max_expanded_bytes {
            return Err(SyntaxProblem::SourceLimitExceeded);
        }
        cursor = end;
    }
    Ok((output, expansions))
}

fn apply_replacing(source: &mut String, clause: &str) -> Result<(), SyntaxProblem> {
    let mut parts = clause.split("==");
    let _ = parts.next();
    let from = parts.next().ok_or(SyntaxProblem::InvalidCopy)?;
    let between = parts.next().ok_or(SyntaxProblem::InvalidCopy)?;
    if !between.trim().eq_ignore_ascii_case("BY") {
        return Err(SyntaxProblem::InvalidCopy);
    }
    let to = parts.next().ok_or(SyntaxProblem::InvalidCopy)?;
    *source = source.replace(from, to);
    Ok(())
}

fn lex(text: &str, limits: SyntaxLimits) -> Result<LosslessSyntax, SyntaxProblem> {
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(CobolSyntaxKind::Root.into());
    let mut offset = 0usize;
    let mut count = 0usize;
    while offset < text.len() {
        if count >= limits.max_tokens {
            return Err(SyntaxProblem::TokenLimitExceeded);
        }
        let rest = &text[offset..];
        let (kind, length) = next_token(rest);
        if length == 0 || length > limits.max_token_bytes {
            return Err(SyntaxProblem::TokenLimitExceeded);
        }
        builder.token(kind.into(), &rest[..length]);
        offset += length;
        count += 1;
    }
    builder.finish_node();
    Ok(LosslessSyntax {
        green: builder.finish(),
        text: text.to_string(),
        semantic_text: text.to_string(),
        token_count: count,
        expansions: Vec::new(),
    })
}

fn next_token(text: &str) -> (CobolSyntaxKind, usize) {
    let bytes = text.as_bytes();
    if bytes[0] == b'\n' {
        return (CobolSyntaxKind::Newline, 1);
    }
    if bytes[0].is_ascii_whitespace() {
        return (
            CobolSyntaxKind::Whitespace,
            bytes
                .iter()
                .take_while(|byte| byte.is_ascii_whitespace() && **byte != b'\n')
                .count(),
        );
    }
    if text.starts_with("*>") {
        return (
            CobolSyntaxKind::Comment,
            text.find('\n').unwrap_or(text.len()),
        );
    }
    if matches!(bytes[0], b'\'' | b'"') {
        let quote = bytes[0];
        let mut index = 1;
        while index < bytes.len() {
            if bytes[index] == quote {
                if bytes.get(index + 1) == Some(&quote) {
                    index += 2;
                    continue;
                }
                return (CobolSyntaxKind::String, index + 1);
            }
            index += 1;
        }
        return (CobolSyntaxKind::Error, text.len());
    }
    if bytes[0].is_ascii_alphabetic() {
        return (
            CobolSyntaxKind::Word,
            bytes
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                .count(),
        );
    }
    if bytes[0].is_ascii_digit() {
        return (
            CobolSyntaxKind::Number,
            bytes
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count(),
        );
    }
    (
        CobolSyntaxKind::Punctuation,
        text.chars().next().map_or(1, char::len_utf8),
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyntaxProblem {
    PrimaryMissing,
    SourceLimitExceeded,
    LineLimitExceeded,
    TokenLimitExceeded,
    InvalidEncoding,
    UnsupportedEncoding,
    CopyNotFound,
    CircularCopy,
    CopyDepthExceeded,
    InvalidCopy,
}
impl fmt::Display for SyntaxProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "COBOL syntax failed: {self:?}")
    }
}
impl std::error::Error for SyntaxProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_source::{LogicalPath, SourceFile, SourceLimits};
    use std::collections::BTreeMap;
    fn bundle(text: &str, format: SourceFormat) -> SourceBundle {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let file = SourceFile::input(
            "main.cbl",
            text.as_bytes().to_vec(),
            format,
            SourceEncoding::Utf8,
            limits,
        )
        .unwrap();
        SourceBundle::new(&path, vec![file], BTreeMap::new(), Vec::new(), limits).unwrap()
    }
    #[test]
    fn lossless_free_form_roundtrips() {
        let source = "IDENTIFICATION DIVISION.\n*> note\nPROGRAM-ID. HELLO.\n";
        assert_eq!(
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default())
                .unwrap()
                .text(),
            source
        );
    }
    #[test]
    fn fixed_columns_and_comments_are_bounded() {
        let source = "000100 IDENTIFICATION DIVISION.                                         00000001\n000200*COMMENT\n";
        let syntax = decode_and_lex(
            &bundle(source, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(syntax.text().contains("IDENTIFICATION DIVISION"));
        assert!(syntax.text().contains("000200*COMMENT"));
        assert!(syntax.semantic_text().contains("*>"));
    }
    #[test]
    fn token_limit_is_enforced() {
        let limits = SyntaxLimits {
            max_tokens: 1,
            ..SyntaxLimits::default()
        };
        assert_eq!(
            decode_and_lex(&bundle("A B", SourceFormat::Free), limits).unwrap_err(),
            SyntaxProblem::TokenLimitExceeded
        );
    }
}
