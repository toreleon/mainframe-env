use mainframe_env_encoding::CodePage;
use mainframe_env_source::{LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat};
use rowan::{GreenNode, GreenNodeBuilder, Language};
use std::fmt;
use std::ops::Range;

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
    semantic_origins: Vec<SourceOrigin>,
    copy_directive_origins: Vec<Vec<SourceSpan>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expansion {
    pub output_start: usize,
    pub output_end: usize,
    pub source: LogicalPath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceOrigin {
    pub output_start: usize,
    pub output_end: usize,
    pub source: LogicalPath,
    pub source_start: usize,
    pub source_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub source: LogicalPath,
    pub source_start: usize,
    pub source_end: usize,
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
    #[must_use]
    pub fn semantic_origins(&self) -> &[SourceOrigin] {
        &self.semantic_origins
    }
    #[must_use]
    pub fn copy_directive_origins(&self) -> &[Vec<SourceSpan>] {
        &self.copy_directive_origins
    }
}

#[derive(Clone, Debug, Default)]
struct NormalizedSource {
    text: String,
    origins: Vec<SourceOrigin>,
}

#[derive(Clone, Debug, Default)]
struct ExpandedSource {
    text: String,
    origins: Vec<SourceOrigin>,
    expansions: Vec<Expansion>,
    copy_directive_origins: Vec<Vec<SourceSpan>>,
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
    let normalized = normalize_source(&decoded, file.path(), file.format(), limits)?;
    let expanded = expand_copies(&normalized, bundle, limits, &mut Vec::new())?;
    let mut syntax = lex(&decoded, limits)?;
    syntax.semantic_text = expanded.text;
    syntax.expansions = expanded.expansions;
    syntax.semantic_origins = expanded.origins;
    syntax.copy_directive_origins = expanded.copy_directive_origins;
    Ok(syntax)
}

fn normalize_source(
    source: &str,
    path: &LogicalPath,
    format: SourceFormat,
    limits: SyntaxLimits,
) -> Result<NormalizedSource, SyntaxProblem> {
    let lines = physical_lines(source);
    if lines.len() > limits.max_lines {
        return Err(SyntaxProblem::LineLimitExceeded);
    }
    if format != SourceFormat::Fixed {
        let mut normalized = NormalizedSource::default();
        for line in lines {
            let output_start = normalized.text.len();
            normalized.text.push_str(&source[line.content.clone()]);
            push_origin(
                &mut normalized.origins,
                output_start..normalized.text.len(),
                path,
                line.content,
            );
            if line.terminator.end > line.terminator.start {
                normalized.text.push('\n');
            }
        }
        return Ok(normalized);
    }
    let mut output = NormalizedSource::default();
    let mut pending = NormalizedSource::default();
    for line in lines {
        let physical = &source[line.content.clone()];
        let bytes = physical.as_bytes();
        let indicator = bytes.get(6).copied().unwrap_or(b' ');
        if matches!(indicator, b'*' | b'/') {
            flush_logical_line(&mut output, &mut pending);
            let prefix_start = output.text.len();
            output.text.push_str("*>");
            if bytes.len() > 6 {
                push_origin(
                    &mut output.origins,
                    prefix_start..output.text.len(),
                    path,
                    line.content.start + 6..line.content.start + 7,
                );
            }
            let code_start = bytes.len().min(7);
            let code_end = bytes.len().min(72);
            let emitted_start = output.text.len();
            output.text.push_str(&physical[code_start..code_end]);
            push_origin(
                &mut output.origins,
                emitted_start..output.text.len(),
                path,
                line.content.start + code_start..line.content.start + code_end,
            );
            output.text.push('\n');
            continue;
        }
        let start = bytes.len().min(7);
        let end = bytes.len().min(72);
        let code = &physical[start..end];
        if indicator == b'-' {
            if pending.text.is_empty() {
                return Err(SyntaxProblem::InvalidContinuation);
            }
            trim_pending_end(&mut pending);
            let leading = code.len() - code.trim_start().len();
            let mut continued = &code[leading..];
            let mut source_start = line.content.start + start + leading;
            if let Some(quote) = unclosed_quote(&pending.text)
                && continued.as_bytes().first() == Some(&quote)
            {
                continued = &continued[1..];
                source_start += 1;
            }
            let output_start = pending.text.len();
            pending.text.push_str(continued);
            push_origin(
                &mut pending.origins,
                output_start..pending.text.len(),
                path,
                source_start..line.content.start + end,
            );
        } else {
            flush_logical_line(&mut output, &mut pending);
            pending.text.push_str(code);
            push_origin(
                &mut pending.origins,
                0..pending.text.len(),
                path,
                line.content.start + start..line.content.start + end,
            );
        }
        if output.text.len() + pending.text.len() > limits.max_source_bytes.saturating_mul(2) {
            return Err(SyntaxProblem::SourceLimitExceeded);
        }
    }
    flush_logical_line(&mut output, &mut pending);
    Ok(output)
}

#[derive(Clone, Debug)]
struct PhysicalLine {
    content: Range<usize>,
    terminator: Range<usize>,
}

fn physical_lines(source: &str) -> Vec<PhysicalLine> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
            end += 1;
        }
        let mut terminator_end = end;
        if terminator_end < bytes.len() {
            terminator_end += 1;
            if bytes[end] == b'\r' && terminator_end < bytes.len() && bytes[terminator_end] == b'\n'
            {
                terminator_end += 1;
            }
        }
        lines.push(PhysicalLine {
            content: start..end,
            terminator: end..terminator_end,
        });
        start = terminator_end;
    }
    lines
}

fn flush_logical_line(output: &mut NormalizedSource, pending: &mut NormalizedSource) {
    if pending.text.is_empty() {
        return;
    }
    let offset = output.text.len();
    output.text.push_str(&pending.text);
    for mut origin in pending.origins.drain(..) {
        origin.output_start += offset;
        origin.output_end += offset;
        output.origins.push(origin);
    }
    output.text.push('\n');
    pending.text.clear();
}

fn trim_pending_end(pending: &mut NormalizedSource) {
    let trimmed = pending.text.trim_end().len();
    pending.text.truncate(trimmed);
    for origin in &mut pending.origins {
        if origin.output_start >= trimmed {
            origin.output_end = origin.output_start;
        } else if origin.output_end > trimmed {
            let removed = origin.output_end - trimmed;
            origin.output_end = trimmed;
            origin.source_end = origin.source_end.saturating_sub(removed);
        }
    }
    pending
        .origins
        .retain(|origin| origin.output_start < origin.output_end);
}

fn unclosed_quote(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    let mut quote = None;
    let mut index = 0usize;
    while index < bytes.len() {
        if matches!(bytes[index], b'\'' | b'"') {
            if quote == Some(bytes[index]) {
                if bytes.get(index + 1) == Some(&bytes[index]) {
                    index += 2;
                    continue;
                }
                quote = None;
            } else if quote.is_none() {
                quote = Some(bytes[index]);
            }
        }
        index += 1;
    }
    quote
}

fn push_origin(
    origins: &mut Vec<SourceOrigin>,
    output: Range<usize>,
    source: &LogicalPath,
    input: Range<usize>,
) {
    if output.start == output.end {
        return;
    }
    origins.push(SourceOrigin {
        output_start: output.start,
        output_end: output.end,
        source: source.clone(),
        source_start: input.start,
        source_end: input.end,
    });
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
    source: &NormalizedSource,
    bundle: &SourceBundle,
    limits: SyntaxLimits,
    stack: &mut Vec<String>,
) -> Result<ExpandedSource, SyntaxProblem> {
    let mut output = ExpandedSource::default();
    let mut cursor = 0usize;
    while let Some(directive) = find_copy_directive(&source.text, cursor)? {
        append_normalized_slice(&mut output, source, cursor..directive.start);
        let parsed = parse_copy_directive(&source.text[directive.start..directive.end])?;
        let name = parsed.name;
        if stack.contains(&name) {
            return Err(SyntaxProblem::CircularCopy(name));
        }
        if stack.len() >= limits.max_copy_depth {
            return Err(SyntaxProblem::CopyDepthExceeded);
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
            .ok_or_else(|| SyntaxProblem::CopyNotFound(name.clone()))?;
        stack.push(name.clone());
        let decoded = decode_file(dependency, limits)?;
        let normalized =
            normalize_source(&decoded, dependency.path(), dependency.format(), limits)?;
        let mut expanded = expand_copies(&normalized, bundle, limits, stack)?;
        stack.pop();
        if !parsed.replacements.is_empty() {
            expanded = apply_replacing(expanded, &parsed.replacements, limits.max_expanded_bytes)?;
        }
        let output_start = output.text.len();
        let mut nested = std::mem::take(&mut expanded.expansions);
        let nested_origins = std::mem::take(&mut expanded.copy_directive_origins);
        append_expanded(&mut output, expanded);
        let output_end = output.text.len();
        output.expansions.push(Expansion {
            output_start,
            output_end,
            source: dependency.path().clone(),
        });
        output.copy_directive_origins.push(source_spans(
            &source.origins,
            directive.start..directive.end,
        ));
        for expansion in &mut nested {
            expansion.output_start += output_start;
            expansion.output_end += output_start;
        }
        output.expansions.extend(nested);
        output.copy_directive_origins.extend(nested_origins);
        if output.text.len() > limits.max_expanded_bytes {
            return Err(SyntaxProblem::SourceLimitExceeded);
        }
        cursor = directive.end;
    }
    append_normalized_slice(&mut output, source, cursor..source.text.len());
    if output.text.len() > limits.max_expanded_bytes {
        return Err(SyntaxProblem::SourceLimitExceeded);
    }
    Ok(output)
}

#[derive(Clone, Debug)]
struct CopyDirective {
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct ParsedCopyDirective {
    name: String,
    replacements: Vec<(String, String)>,
}

fn find_copy_directive(source: &str, from: usize) -> Result<Option<CopyDirective>, SyntaxProblem> {
    let bytes = source.as_bytes();
    let mut index = from;
    while index < bytes.len() {
        if source[index..].starts_with("*>") {
            index = source[index..]
                .find('\n')
                .map_or(bytes.len(), |relative| index + relative + 1);
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"') {
            index = skip_quoted(source, index)?;
            continue;
        }
        if is_word_byte(bytes[index]) {
            let start = index;
            while index < bytes.len() && is_word_byte(bytes[index]) {
                index += 1;
            }
            if source[start..index].eq_ignore_ascii_case("COPY") {
                return find_copy_end(source, start, index).map(Some);
            }
            continue;
        }
        index += source[index..].chars().next().map_or(1, char::len_utf8);
    }
    Ok(None)
}

fn find_copy_end(
    source: &str,
    start: usize,
    mut index: usize,
) -> Result<CopyDirective, SyntaxProblem> {
    let bytes = source.as_bytes();
    let mut pseudotext = false;
    while index < bytes.len() {
        if !pseudotext && source[index..].starts_with("*>") {
            index = source[index..]
                .find('\n')
                .map_or(bytes.len(), |relative| index + relative + 1);
            continue;
        }
        if source[index..].starts_with("==") {
            pseudotext = !pseudotext;
            index += 2;
            continue;
        }
        if !pseudotext && matches!(bytes[index], b'\'' | b'"') {
            index = skip_quoted(source, index)?;
            continue;
        }
        if !pseudotext && bytes[index] == b'.' {
            return Ok(CopyDirective {
                start,
                end: index + 1,
            });
        }
        index += source[index..].chars().next().map_or(1, char::len_utf8);
    }
    Err(SyntaxProblem::InvalidCopy)
}

fn skip_quoted(source: &str, start: usize) -> Result<usize, SyntaxProblem> {
    let bytes = source.as_bytes();
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return Ok(index + 1);
        }
        index += source[index..].chars().next().map_or(1, char::len_utf8);
    }
    Err(SyntaxProblem::InvalidCopy)
}

const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn parse_copy_directive(command: &str) -> Result<ParsedCopyDirective, SyntaxProblem> {
    let body = command
        .strip_suffix('.')
        .ok_or(SyntaxProblem::InvalidCopy)?;
    let mut parser = CopyParser::new(body);
    parser.expect_word("COPY")?;
    let name = parser.name()?.to_ascii_uppercase();
    parser.whitespace();
    if parser.done() {
        return Ok(ParsedCopyDirective {
            name,
            replacements: Vec::new(),
        });
    }
    parser.expect_word("REPLACING")?;
    let mut replacements = Vec::new();
    loop {
        parser.whitespace();
        if parser.done() {
            break;
        }
        let from = parser.pseudotext()?;
        parser.expect_word("BY")?;
        let to = parser.pseudotext()?;
        if from.is_empty() {
            return Err(SyntaxProblem::InvalidCopy);
        }
        replacements.push((from.to_string(), to.to_string()));
    }
    if replacements.is_empty() {
        return Err(SyntaxProblem::InvalidCopy);
    }
    Ok(ParsedCopyDirective { name, replacements })
}

struct CopyParser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> CopyParser<'a> {
    const fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn whitespace(&mut self) {
        while self
            .source
            .as_bytes()
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }

    fn done(&mut self) -> bool {
        self.whitespace();
        self.cursor == self.source.len()
    }

    fn expect_word(&mut self, expected: &str) -> Result<(), SyntaxProblem> {
        self.whitespace();
        let start = self.cursor;
        while self
            .source
            .as_bytes()
            .get(self.cursor)
            .is_some_and(|byte| is_word_byte(*byte))
        {
            self.cursor += 1;
        }
        if start == self.cursor || !self.source[start..self.cursor].eq_ignore_ascii_case(expected) {
            return Err(SyntaxProblem::InvalidCopy);
        }
        Ok(())
    }

    fn name(&mut self) -> Result<&'a str, SyntaxProblem> {
        self.whitespace();
        let bytes = self.source.as_bytes();
        if matches!(bytes.get(self.cursor), Some(b'\'' | b'"')) {
            let start = self.cursor;
            let end = skip_quoted(self.source, start)?;
            self.cursor = end;
            let name = &self.source[start + 1..end - 1];
            if name.is_empty() || !name.bytes().all(is_word_byte) {
                return Err(SyntaxProblem::InvalidCopy);
            }
            return Ok(name);
        }
        let start = self.cursor;
        while bytes
            .get(self.cursor)
            .is_some_and(|byte| is_word_byte(*byte))
        {
            self.cursor += 1;
        }
        if start == self.cursor {
            Err(SyntaxProblem::InvalidCopy)
        } else {
            Ok(&self.source[start..self.cursor])
        }
    }

    fn pseudotext(&mut self) -> Result<&'a str, SyntaxProblem> {
        self.whitespace();
        if !self.source[self.cursor..].starts_with("==") {
            return Err(SyntaxProblem::InvalidCopy);
        }
        self.cursor += 2;
        let start = self.cursor;
        let relative = self.source[start..]
            .find("==")
            .ok_or(SyntaxProblem::InvalidCopy)?;
        let end = start + relative;
        self.cursor = end + 2;
        Ok(&self.source[start..end])
    }
}

fn apply_replacing(
    source: ExpandedSource,
    replacements: &[(String, String)],
    max_expanded_bytes: usize,
) -> Result<ExpandedSource, SyntaxProblem> {
    if !source.expansions.is_empty() {
        return Err(SyntaxProblem::InvalidCopy);
    }
    let mut output = ExpandedSource::default();
    let mut cursor = 0usize;
    while cursor < source.text.len() {
        let matched = replacements
            .iter()
            .find(|(from, _)| source.text[cursor..].starts_with(from));
        if let Some((from, to)) = matched {
            if output
                .text
                .len()
                .checked_add(to.len())
                .is_none_or(|length| length > max_expanded_bytes)
            {
                return Err(SyntaxProblem::SourceLimitExceeded);
            }
            let output_start = output.text.len();
            output.text.push_str(to);
            let spans = source_spans(&source.origins, cursor..cursor + from.len());
            for span in spans {
                output.origins.push(SourceOrigin {
                    output_start,
                    output_end: output.text.len(),
                    source: span.source,
                    source_start: span.source_start,
                    source_end: span.source_end,
                });
            }
            cursor += from.len();
            continue;
        }
        let length = source.text[cursor..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        if output
            .text
            .len()
            .checked_add(length)
            .is_none_or(|total| total > max_expanded_bytes)
        {
            return Err(SyntaxProblem::SourceLimitExceeded);
        }
        append_expanded_slice(&mut output, &source, cursor..cursor + length);
        cursor += length;
    }
    Ok(output)
}

fn append_normalized_slice(
    output: &mut ExpandedSource,
    source: &NormalizedSource,
    range: Range<usize>,
) {
    let output_start = output.text.len();
    output.text.push_str(&source.text[range.clone()]);
    output
        .origins
        .extend(slice_origins(&source.origins, range, output_start));
}

fn append_expanded_slice(
    output: &mut ExpandedSource,
    source: &ExpandedSource,
    range: Range<usize>,
) {
    let output_start = output.text.len();
    output.text.push_str(&source.text[range.clone()]);
    output
        .origins
        .extend(slice_origins(&source.origins, range, output_start));
}

fn append_expanded(output: &mut ExpandedSource, mut source: ExpandedSource) {
    let offset = output.text.len();
    output.text.push_str(&source.text);
    for origin in &mut source.origins {
        origin.output_start += offset;
        origin.output_end += offset;
    }
    for expansion in &mut source.expansions {
        expansion.output_start += offset;
        expansion.output_end += offset;
    }
    output.origins.extend(source.origins);
    output.expansions.extend(source.expansions);
    output
        .copy_directive_origins
        .extend(source.copy_directive_origins);
}

fn slice_origins(
    origins: &[SourceOrigin],
    range: Range<usize>,
    output_start: usize,
) -> Vec<SourceOrigin> {
    origins
        .iter()
        .filter_map(|origin| {
            let start = origin.output_start.max(range.start);
            let end = origin.output_end.min(range.end);
            if start >= end {
                return None;
            }
            let equal_width =
                origin.output_end - origin.output_start == origin.source_end - origin.source_start;
            let (source_start, source_end) = if equal_width {
                (
                    origin.source_start + (start - origin.output_start),
                    origin.source_start + (end - origin.output_start),
                )
            } else {
                (origin.source_start, origin.source_end)
            };
            Some(SourceOrigin {
                output_start: output_start + (start - range.start),
                output_end: output_start + (end - range.start),
                source: origin.source.clone(),
                source_start,
                source_end,
            })
        })
        .collect()
}

fn source_spans(origins: &[SourceOrigin], range: Range<usize>) -> Vec<SourceSpan> {
    slice_origins(origins, range, 0)
        .into_iter()
        .map(|origin| SourceSpan {
            source: origin.source,
            source_start: origin.source_start,
            source_end: origin.source_end,
        })
        .collect()
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
        semantic_origins: Vec::new(),
        copy_directive_origins: Vec::new(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SyntaxProblem {
    PrimaryMissing,
    SourceLimitExceeded,
    LineLimitExceeded,
    TokenLimitExceeded,
    InvalidEncoding,
    UnsupportedEncoding,
    CopyNotFound(String),
    CircularCopy(String),
    CopyDepthExceeded,
    InvalidCopy,
    InvalidContinuation,
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
    fn bundle_with_copy(text: &str, copy_name: &str, copy_text: &str) -> SourceBundle {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let files = vec![
            SourceFile::input(
                "main.cbl",
                text.as_bytes().to_vec(),
                SourceFormat::Fixed,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
            SourceFile::input(
                format!("copy/{copy_name}.cpy"),
                copy_text.as_bytes().to_vec(),
                SourceFormat::Fixed,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ];
        SourceBundle::new(&path, files, BTreeMap::new(), Vec::new(), limits).unwrap()
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
    fn carddemo_license_comment_is_not_a_copy_directive() {
        let source = "      * You may obtain a copy of the License at                         \n       COPY ACTUAL.\n";
        let syntax = decode_and_lex(
            &bundle_with_copy(source, "ACTUAL", "       01 ACTUAL PIC X.\n"),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert_eq!(syntax.expansions().len(), 1);
        assert_eq!(syntax.expansions()[0].source.as_str(), "copy/ACTUAL.cpy");
        assert!(syntax.copy_directive_origins()[0].iter().any(|span| {
            span.source.as_str() == "main.cbl"
                && source[span.source_start..span.source_end].contains("COPY ACTUAL.")
        }));
        assert!(
            syntax
                .semantic_origins()
                .iter()
                .any(|origin| origin.source.as_str() == "copy/ACTUAL.cpy")
        );
    }
    #[test]
    fn fixed_continuation_columns_and_exact_origins_are_preserved() {
        let source = format!(
            "000100 01 TEXT-VALUE PIC X(20) VALUE 'ABC-\r\n000200-{:<65}00000002\r\n",
            "             'DEF'."
        );
        let syntax = decode_and_lex(
            &bundle(&source, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert_eq!(syntax.text(), source);
        assert!(syntax.semantic_text().contains("'ABC-DEF'"));
        assert!(!syntax.semantic_text().contains("00000002"));
        assert!(syntax.semantic_origins().iter().all(|origin| {
            origin.source.as_str() == "main.cbl"
                && origin.source_start < origin.source_end
                && origin.source_end <= source.len()
        }));
    }
    #[test]
    fn fixed_comments_and_sequence_area_copy_text_are_ignored() {
        let source = format!(
            "      *COPY COMMENTED.\n000100 {:<65}COPY SEQUENCE.\n",
            "IDENTIFICATION DIVISION."
        );
        let syntax = decode_and_lex(
            &bundle(&source, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(syntax.expansions().is_empty());
        assert!(!syntax.semantic_text().contains("COPY SEQUENCE"));
    }
    #[test]
    fn copy_replacing_applies_all_reached_pseudotext_pairs_deterministically() {
        let source = "       COPY ACTUAL REPLACING\n         ==(TESTVAR1)== BY ==ACCOUNT-STATUS==\n         ==(SCRNVAR2)== BY ==ACSTTUS==\n         ==(MAPNAME3)== BY ==CACTUPA== .\n";
        let bundle = bundle_with_copy(
            source,
            "ACTUAL",
            "       MOVE (TESTVAR1) TO (SCRNVAR2) OF (MAPNAME3).\n",
        );
        let first = decode_and_lex(&bundle, SyntaxLimits::default()).unwrap();
        let second = decode_and_lex(&bundle, SyntaxLimits::default()).unwrap();
        assert_eq!(first.semantic_text(), second.semantic_text());
        assert_eq!(first.semantic_origins(), second.semantic_origins());
        assert!(
            first
                .semantic_text()
                .contains("MOVE ACCOUNT-STATUS TO ACSTTUS OF CACTUPA")
        );
        assert!(!first.semantic_text().contains("TESTVAR1"));
        assert_eq!(first.expansions().len(), 1);
    }
    #[test]
    fn copy_in_strings_and_free_form_comments_is_ignored() {
        let source = "IDENTIFICATION DIVISION.\n*> COPY COMMENTED.\nPROCEDURE DIVISION.\nDISPLAY 'COPY QUOTED.'.\n";
        let syntax =
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default()).unwrap();
        assert!(syntax.expansions().is_empty());
    }
    #[test]
    fn copy_failure_categories_are_stable_and_named() {
        let missing = decode_and_lex(
            &bundle("       COPY MISSING.\n", SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(missing, SyntaxProblem::CopyNotFound("MISSING".into()));

        let circular = decode_and_lex(
            &bundle_with_copy("       COPY ACTUAL.\n", "ACTUAL", "       COPY ACTUAL.\n"),
            SyntaxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(circular, SyntaxProblem::CircularCopy("ACTUAL".into()));

        let malformed = decode_and_lex(
            &bundle_with_copy(
                "       COPY ACTUAL REPLACING ==A== BY.\n",
                "ACTUAL",
                "       A\n",
            ),
            SyntaxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(malformed, SyntaxProblem::InvalidCopy);

        let limited = decode_and_lex(
            &bundle_with_copy("       COPY ACTUAL.\n", "ACTUAL", "       01 A PIC X.\n"),
            SyntaxLimits {
                max_copy_depth: 0,
                ..SyntaxLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(limited, SyntaxProblem::CopyDepthExceeded);

        let expanded = decode_and_lex(
            &bundle_with_copy("       COPY ACTUAL.\n", "ACTUAL", "       01 A PIC X.\n"),
            SyntaxLimits {
                max_expanded_bytes: 1,
                ..SyntaxLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(expanded, SyntaxProblem::SourceLimitExceeded);

        let continuation = decode_and_lex(
            &bundle("      -BROKEN\n", SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(continuation, SyntaxProblem::InvalidContinuation);
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
