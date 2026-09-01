mod directives;

pub use directives::{
    CompilerDirectingNode, CompilerDirectiveNode, CompilerOption, CompilerOptionSet,
};

use crate::generated::cobol_language::CompilerDirectingKind;
use mainframe_env_encoding::CodePage;
use mainframe_env_source::{
    LibraryProblem, LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat,
};
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
    pub max_directives: usize,
    pub max_directive_nesting: usize,
    pub max_compilation_variables: usize,
    pub max_replacements: usize,
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
            max_directives: 65_536,
            max_directive_nesting: 256,
            max_compilation_variables: 4_096,
            max_replacements: 4_096,
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
    CompilerDirective,
    PseudoText,
    BooleanLiteral,
    HexLiteral,
    NationalLiteral,
    Utf8Literal,
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
            8 => CobolSyntaxKind::Error,
            9 => CobolSyntaxKind::CompilerDirective,
            10 => CobolSyntaxKind::PseudoText,
            11 => CobolSyntaxKind::BooleanLiteral,
            12 => CobolSyntaxKind::HexLiteral,
            13 => CobolSyntaxKind::NationalLiteral,
            14 => CobolSyntaxKind::Utf8Literal,
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
    lossless_origins: Vec<SourceOrigin>,
    expansions: Vec<Expansion>,
    semantic_origins: Vec<SourceOrigin>,
    copy_directive_origins: Vec<Vec<SourceSpan>>,
    tokens: Vec<SyntaxToken>,
    compiler_directing: Vec<CompilerDirectingNode>,
    compiler_directives: Vec<CompilerDirectiveNode>,
    compiler_options: CompilerOptionSet,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyntaxTokenId(u32);

impl SyntaxTokenId {
    fn from_index(index: usize) -> Result<Self, SyntaxProblem> {
        u32::try_from(index)
            .map(Self)
            .map_err(|_| SyntaxProblem::TokenLimitExceeded)
    }

    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxToken {
    pub id: SyntaxTokenId,
    pub kind: CobolSyntaxKind,
    pub bytes: Range<usize>,
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
    pub fn lossless_origins(&self) -> &[SourceOrigin] {
        &self.lossless_origins
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
    #[must_use]
    pub fn tokens(&self) -> &[SyntaxToken] {
        &self.tokens
    }
    #[must_use]
    pub fn compiler_directing_statements(&self) -> &[CompilerDirectingNode] {
        &self.compiler_directing
    }
    #[must_use]
    pub fn compiler_directives(&self) -> &[CompilerDirectiveNode] {
        &self.compiler_directives
    }
    #[must_use]
    pub fn compiler_options(&self) -> &CompilerOptionSet {
        &self.compiler_options
    }
}

#[derive(Clone, Debug, Default)]
struct NormalizedSource {
    text: String,
    origins: Vec<SourceOrigin>,
}

#[derive(Clone, Debug)]
struct DecodedSource {
    text: String,
    source_offsets: Option<Vec<usize>>,
}

impl DecodedSource {
    fn input_offset(&self, decoded_offset: usize) -> Result<usize, SyntaxProblem> {
        if let Some(offsets) = &self.source_offsets {
            offsets
                .get(decoded_offset)
                .copied()
                .ok_or(SyntaxProblem::InvalidEncoding)
        } else if decoded_offset <= self.text.len() {
            Ok(decoded_offset)
        } else {
            Err(SyntaxProblem::InvalidEncoding)
        }
    }

    fn lossless_origins(&self, path: &LogicalPath) -> Vec<SourceOrigin> {
        if self.text.is_empty() {
            return Vec::new();
        }
        if self.source_offsets.is_none() {
            return vec![SourceOrigin {
                output_start: 0,
                output_end: self.text.len(),
                source: path.clone(),
                source_start: 0,
                source_end: self.text.len(),
            }];
        }
        let mut origins = Vec::<SourceOrigin>::new();
        for (input, (output_start, character)) in self.text.char_indices().enumerate() {
            let output_end = output_start + character.len_utf8();
            if character.len_utf8() == 1
                && let Some(previous) = origins.last_mut()
                && previous.output_end == output_start
                && previous.source_end == input
                && previous.output_end - previous.output_start
                    == previous.source_end - previous.source_start
            {
                previous.output_end = output_end;
                previous.source_end = input + 1;
            } else {
                origins.push(SourceOrigin {
                    output_start,
                    output_end,
                    source: path.clone(),
                    source_start: input,
                    source_end: input + 1,
                });
            }
        }
        origins
    }
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
    let debugging = bundle
        .options()
        .get("cobol.debugging-mode")
        .is_some_and(|value| value == "true");
    let mut artifacts = directives::DirectiveArtifacts::default();
    let normalized =
        directives::apply_basis(&decoded, file, bundle, debugging, &mut artifacts, limits)?
            .map_or_else(
                || normalize_decoded_source(&decoded, file, debugging, limits),
                Ok,
            )?;
    let prepared = directives::prepare_source(&normalized, true, &mut artifacts, limits)?;
    let mut directive_state = directives::DirectiveState::new(bundle, &artifacts.options);
    let expanded = expand_copies(
        &prepared,
        bundle,
        limits,
        &mut Vec::new(),
        &mut directive_state,
        &mut artifacts,
        false,
        file.format(),
    )?;
    let expanded = if bundle
        .options()
        .get("cobol.sql-precompile")
        .is_some_and(|value| value == "true")
    {
        let expanded = expand_sql_includes(
            &expanded,
            bundle,
            limits,
            &mut Vec::new(),
            &mut directive_state,
            &mut artifacts,
        )?;
        hoist_sql_cursor_declarations(&expanded, limits)?
    } else {
        expanded
    };
    let expanded = process_replaces(expanded, &mut artifacts, limits)?;
    let mut syntax = lex(&decoded.text, limits)?;
    syntax.lossless_origins = decoded.lossless_origins(file.path());
    syntax.semantic_text = expanded.text;
    syntax.expansions = expanded.expansions;
    syntax.semantic_origins = expanded.origins;
    syntax.copy_directive_origins = expanded.copy_directive_origins;
    syntax.compiler_directing = artifacts.directing;
    syntax.compiler_directives = artifacts.directives;
    syntax.compiler_options = artifacts.options;
    Ok(syntax)
}

fn normalize_source(
    source: &str,
    path: &LogicalPath,
    format: SourceFormat,
    debugging: bool,
    limits: SyntaxLimits,
) -> Result<NormalizedSource, SyntaxProblem> {
    let lines = physical_lines(source);
    if lines.len() > limits.max_lines {
        return Err(SyntaxProblem::LineLimitExceeded);
    }
    if format == SourceFormat::Free {
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
        let bounded_end = if format == SourceFormat::Variable {
            bytes.len()
        } else {
            bytes.len().min(72)
        };
        if !physical.is_char_boundary(bounded_end) {
            return Err(SyntaxProblem::InvalidEncoding);
        }
        let bounded = &physical[..bounded_end];
        let leading = bounded.len() - bounded.trim_start().len();
        let special_process = leading < 7
            && bounded[leading..]
                .split_whitespace()
                .next()
                .is_some_and(|word| {
                    word.eq_ignore_ascii_case("PROCESS") || word.eq_ignore_ascii_case("CBL")
                });
        let indicator = bytes.get(6).copied().unwrap_or(b' ');
        let special_control = bytes.len() > 6
            && indicator == b'*'
            && bounded[6..].split_whitespace().next().is_some_and(|word| {
                word.eq_ignore_ascii_case("*CONTROL") || word.eq_ignore_ascii_case("*CBL")
            });
        if !special_process
            && !special_control
            && (matches!(indicator, b'*' | b'/') || matches!(indicator, b'D' | b'd') && !debugging)
        {
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
            let code_end = bounded_end;
            if !physical.is_char_boundary(code_start) || !physical.is_char_boundary(code_end) {
                return Err(SyntaxProblem::InvalidEncoding);
            }
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
        let start = if special_process {
            leading
        } else if special_control {
            6
        } else {
            bytes.len().min(7)
        };
        if !physical.is_char_boundary(start) {
            return Err(SyntaxProblem::InvalidEncoding);
        }
        if !special_process && !special_control && !matches!(indicator, b' ' | b'-' | b'D' | b'd') {
            return Err(SyntaxProblem::InvalidIndicator(indicator));
        }
        let end = bounded_end;
        let code = &physical[start..end];
        if !special_process && !special_control && indicator == b'-' {
            if pending.text.is_empty() {
                return Err(SyntaxProblem::InvalidContinuation);
            }
            if directives::is_noncontinuable_directing(&pending.text) {
                return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
            }
            if code
                .get(..code.len().min(4))
                .is_some_and(|area_a| area_a.bytes().any(|byte| !byte.is_ascii_whitespace()))
            {
                return Err(SyntaxProblem::InvalidContinuationArea);
            }
            let quote = unclosed_quote(&pending.text);
            if quote.is_none() {
                trim_pending_end(&mut pending);
            }
            let leading = code.len() - code.trim_start().len();
            let mut continued = &code[leading..];
            let mut source_start = line.content.start + start + leading;
            if let Some(quote) = quote {
                if continued.as_bytes().first() != Some(&quote) {
                    return Err(SyntaxProblem::InvalidLiteralContinuation);
                }
                continued = &continued[1..];
                source_start += 1;
            }
            if pending.text.contains(">>") || continued.contains(">>") {
                return Err(SyntaxProblem::InvalidDirectiveContinuation);
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

fn normalize_decoded_source(
    source: &DecodedSource,
    file: &SourceFile,
    debugging: bool,
    limits: SyntaxLimits,
) -> Result<NormalizedSource, SyntaxProblem> {
    let normalized = normalize_source(&source.text, file.path(), file.format(), debugging, limits)?;
    remap_decoded_origins(normalized, source, file.path())
}

fn remap_decoded_origins(
    mut normalized: NormalizedSource,
    decoded: &DecodedSource,
    path: &LogicalPath,
) -> Result<NormalizedSource, SyntaxProblem> {
    if decoded.source_offsets.is_none() {
        return Ok(normalized);
    }
    let decoded_origins = decoded.lossless_origins(path);
    let mut remapped = Vec::with_capacity(normalized.origins.len());
    for origin in &normalized.origins {
        let source_range = origin.source_start..origin.source_end;
        if origin.output_end - origin.output_start == source_range.end - source_range.start {
            remapped.extend(slice_origins(
                &decoded_origins,
                source_range,
                origin.output_start,
            ));
            continue;
        }
        let mapped = decoded_origins
            .iter()
            .filter(|raw| {
                raw.output_start < source_range.end && raw.output_end > source_range.start
            })
            .collect::<Vec<_>>();
        let (Some(first), Some(last)) = (mapped.first(), mapped.last()) else {
            return Err(SyntaxProblem::InvalidEncoding);
        };
        remapped.push(SourceOrigin {
            output_start: origin.output_start,
            output_end: origin.output_end,
            source: path.clone(),
            source_start: first.source_start,
            source_end: last.source_end,
        });
    }
    normalized.origins = remapped;
    Ok(normalized)
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

fn decode_file(file: &SourceFile, limits: SyntaxLimits) -> Result<DecodedSource, SyntaxProblem> {
    match file.encoding() {
        SourceEncoding::Utf8 => Ok(DecodedSource {
            text: String::from_utf8(file.bytes().to_vec())
                .map_err(|_| SyntaxProblem::InvalidEncoding)?,
            source_offsets: None,
        }),
        SourceEncoding::Ebcdic(ccsid) => {
            let text = CodePage::from_ccsid(ccsid)
                .map_err(|_| SyntaxProblem::UnsupportedEncoding)?
                .decode(file.bytes(), limits.max_expanded_bytes)
                .map_err(|_| SyntaxProblem::InvalidEncoding)?;
            let mut source_offsets = vec![0usize; text.len() + 1];
            for (input, (output, character)) in text.char_indices().enumerate() {
                source_offsets[output..output + character.len_utf8()].fill(input);
                source_offsets[output + character.len_utf8()] = input + 1;
            }
            Ok(DecodedSource {
                text,
                source_offsets: Some(source_offsets),
            })
        }
    }
}

fn debugging_mode(bundle: &SourceBundle) -> bool {
    bundle
        .options()
        .get("cobol.debugging-mode")
        .is_some_and(|value| value == "true")
}

fn expand_copies(
    source: &NormalizedSource,
    bundle: &SourceBundle,
    limits: SyntaxLimits,
    stack: &mut Vec<String>,
    directive_state: &mut directives::DirectiveState,
    artifacts: &mut directives::DirectiveArtifacts,
    replacing_active: bool,
    format: SourceFormat,
) -> Result<ExpandedSource, SyntaxProblem> {
    let initial_depth = directive_state.depth();
    let mut output = ExpandedSource::default();
    let mut cursor = 0usize;
    loop {
        let next_directive = directives::next_directive_line(source, cursor);
        let boundary = next_directive
            .as_ref()
            .map_or(source.text.len(), |line| line.range.start);
        let next_copy = if directive_state.active() {
            find_copy_directive(&source.text[..boundary], cursor)?
        } else {
            None
        };
        let Some(directive) = next_copy else {
            append_conditional_slice(&mut output, source, cursor..boundary, directive_state);
            let Some(line) = next_directive else {
                break;
            };
            directives::process_directive_line(
                source,
                &line,
                format,
                directive_state,
                artifacts,
                limits,
            )?;
            append_inactive_newlines(&mut output, &source.text[line.range.clone()]);
            cursor = line.range.end;
            continue;
        };
        append_conditional_slice(
            &mut output,
            source,
            cursor..directive.start,
            directive_state,
        );
        let parsed = parse_copy_directive(&source.text[directive.start..directive.end], limits)?;
        let name = parsed.name;
        if replacing_active && !parsed.replacements.is_empty() {
            return Err(SyntaxProblem::NestedCopyReplacing);
        }
        let mut copy_operands = vec![name.clone()];
        if let Some(library) = &parsed.library {
            copy_operands.push(format!("IN:{library}"));
        }
        if parsed.suppress {
            copy_operands.push("SUPPRESS".into());
        }
        if !parsed.replacements.is_empty() {
            copy_operands.push(format!("REPLACING:{}", parsed.replacements.len()));
        }
        directives::record_directing(
            artifacts,
            CompilerDirectingKind::Copy,
            copy_operands,
            source_spans(&source.origins, directive.start..directive.end),
            true,
            limits,
        )?;
        let stack_key = format!("{}:{name}", parsed.library.as_deref().unwrap_or("*"));
        if stack.contains(&stack_key) {
            return Err(SyntaxProblem::CircularCopy(name));
        }
        if stack.len() >= limits.max_copy_depth {
            return Err(SyntaxProblem::CopyDepthExceeded);
        }
        let dependency = resolve_copy_member(bundle, &name, parsed.library.as_deref())?;
        stack.push(stack_key);
        let decoded = decode_file(dependency, limits)?;
        let normalized =
            normalize_decoded_source(&decoded, dependency, debugging_mode(bundle), limits)?;
        let prepared = directives::prepare_source(&normalized, false, artifacts, limits)?;
        let mut expanded = expand_copies(
            &prepared,
            bundle,
            limits,
            stack,
            directive_state,
            artifacts,
            replacing_active || !parsed.replacements.is_empty(),
            dependency.format(),
        )?;
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
    if directive_state.depth() != initial_depth {
        return Err(SyntaxProblem::UnterminatedCompilerDirective);
    }
    if output.text.len() > limits.max_expanded_bytes {
        return Err(SyntaxProblem::SourceLimitExceeded);
    }
    Ok(output)
}

fn resolve_copy_member<'a>(
    bundle: &'a SourceBundle,
    name: &str,
    library: Option<&str>,
) -> Result<&'a SourceFile, SyntaxProblem> {
    let Some(library_name) = library else {
        return bundle
            .resolve_library_member(name)
            .map_err(|problem| match problem {
                LibraryProblem::MissingMember(_) => SyntaxProblem::CopyNotFound(name.into()),
                LibraryProblem::AmbiguousMember { .. } => SyntaxProblem::AmbiguousCopy(name.into()),
                _ => SyntaxProblem::InvalidCopy,
            });
    };
    let Some(library) = bundle
        .libraries()
        .iter()
        .find(|library| library.name().eq_ignore_ascii_case(library_name))
    else {
        return Err(SyntaxProblem::CopyNotFound(name.into()));
    };
    let matches = library
        .members()
        .iter()
        .filter_map(|path| bundle.files().iter().find(|file| file.path() == path))
        .filter(|file| {
            file.path()
                .as_str()
                .rsplit('/')
                .next()
                .unwrap_or(file.path().as_str())
                .split('.')
                .next()
                .is_some_and(|stem| stem.eq_ignore_ascii_case(name))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [file] => Ok(*file),
        [] => Err(SyntaxProblem::CopyNotFound(name.into())),
        _ => Err(SyntaxProblem::AmbiguousCopy(name.into())),
    }
}

fn append_conditional_slice(
    output: &mut ExpandedSource,
    source: &NormalizedSource,
    range: Range<usize>,
    directive_state: &mut directives::DirectiveState,
) {
    if range.start == range.end {
        return;
    }
    if directive_state.active() {
        append_normalized_slice(output, source, range.clone());
        directive_state.observe_text(&source.text[range]);
    } else {
        append_inactive_newlines(output, &source.text[range]);
    }
}

fn append_inactive_newlines(output: &mut ExpandedSource, text: &str) {
    output
        .text
        .extend(text.bytes().filter(|byte| *byte == b'\n').map(|_| '\n'));
}

#[derive(Clone, Debug)]
struct SqlIncludeDirective {
    start: usize,
    end: usize,
    name: String,
}

fn expand_sql_includes(
    source: &ExpandedSource,
    bundle: &SourceBundle,
    limits: SyntaxLimits,
    stack: &mut Vec<String>,
    directive_state: &mut directives::DirectiveState,
    artifacts: &mut directives::DirectiveArtifacts,
) -> Result<ExpandedSource, SyntaxProblem> {
    let mut output = ExpandedSource::default();
    let mut cursor = 0usize;
    while let Some(directive) = find_sql_include(&source.text, cursor)? {
        append_expanded_slice(&mut output, source, cursor..directive.start);
        if stack.contains(&directive.name) {
            return Err(SyntaxProblem::CircularPrecompilerInclude(directive.name));
        }
        if stack.len() >= limits.max_copy_depth {
            return Err(SyntaxProblem::PrecompilerIncludeDepthExceeded);
        }
        let dependency = bundle
            .resolve_library_member(&directive.name)
            .map_err(|problem| match problem {
                LibraryProblem::MissingMember(_) => {
                    SyntaxProblem::PrecompilerIncludeNotFound(directive.name.clone())
                }
                LibraryProblem::AmbiguousMember { .. } => {
                    SyntaxProblem::AmbiguousPrecompilerInclude(directive.name.clone())
                }
                _ => SyntaxProblem::InvalidPrecompilerInclude,
            })?;
        stack.push(directive.name.clone());
        let decoded = decode_file(dependency, limits)?;
        let normalized =
            normalize_decoded_source(&decoded, dependency, debugging_mode(bundle), limits)?;
        let prepared = directives::prepare_source(&normalized, false, artifacts, limits)?;
        let copied = expand_copies(
            &prepared,
            bundle,
            limits,
            &mut Vec::new(),
            directive_state,
            artifacts,
            false,
            dependency.format(),
        )?;
        let mut expanded =
            expand_sql_includes(&copied, bundle, limits, stack, directive_state, artifacts)?;
        stack.pop();
        let output_start = output.text.len();
        append_expanded(&mut output, std::mem::take(&mut expanded));
        let output_end = output.text.len();
        output.expansions.push(Expansion {
            output_start,
            output_end,
            source: dependency.path().clone(),
        });
        if output.text.len() > limits.max_expanded_bytes {
            return Err(SyntaxProblem::SourceLimitExceeded);
        }
        cursor = directive.end;
    }
    append_expanded_slice(&mut output, source, cursor..source.text.len());
    output
        .copy_directive_origins
        .extend(source.copy_directive_origins.iter().cloned());
    output
        .expansions
        .sort_by_key(|expansion| (expansion.output_start, expansion.output_end));
    if output.text.len() > limits.max_expanded_bytes {
        return Err(SyntaxProblem::SourceLimitExceeded);
    }
    Ok(output)
}

fn find_sql_include(
    source: &str,
    from: usize,
) -> Result<Option<SqlIncludeDirective>, SyntaxProblem> {
    let mut cursor = from;
    while let Some((start, end)) = next_semantic_word(source, &mut cursor)? {
        if !source[start..end].eq_ignore_ascii_case("EXEC") {
            continue;
        }
        let mut probe = cursor;
        let Some((sql_start, sql_end)) = next_semantic_word(source, &mut probe)? else {
            continue;
        };
        if !source[sql_start..sql_end].eq_ignore_ascii_case("SQL") {
            continue;
        }
        let Some((include_start, include_end)) = next_semantic_word(source, &mut probe)? else {
            continue;
        };
        if !source[include_start..include_end].eq_ignore_ascii_case("INCLUDE") {
            continue;
        }
        let (name_start, name_end) = next_semantic_word(source, &mut probe)?
            .ok_or(SyntaxProblem::InvalidPrecompilerInclude)?;
        let name = source[name_start..name_end].to_ascii_uppercase();
        let (terminal_start, terminal_end) = next_semantic_word(source, &mut probe)?
            .ok_or(SyntaxProblem::InvalidPrecompilerInclude)?;
        if !source[terminal_start..terminal_end].eq_ignore_ascii_case("END-EXEC") {
            return Err(SyntaxProblem::InvalidPrecompilerInclude);
        }
        let end = terminal_end + usize::from(source.as_bytes().get(terminal_end) == Some(&b'.'));
        return Ok(Some(SqlIncludeDirective { start, end, name }));
    }
    Ok(None)
}

fn hoist_sql_cursor_declarations(
    source: &ExpandedSource,
    limits: SyntaxLimits,
) -> Result<ExpandedSource, SyntaxProblem> {
    let procedure = source
        .text
        .to_ascii_uppercase()
        .find("PROCEDURE DIVISION")
        .ok_or(SyntaxProblem::InvalidPrecompilerInclude)?;
    let mut declarations = Vec::new();
    let mut cursor = 0usize;
    while let Some(range) = find_sql_cursor_declaration(&source.text, cursor)? {
        cursor = range.end;
        if range.start < procedure {
            declarations.push(range);
        }
    }
    if declarations.is_empty() {
        return Ok(source.clone());
    }
    let mut without = ExpandedSource::default();
    cursor = 0;
    for range in &declarations {
        append_expanded_slice(&mut without, source, cursor..range.start);
        cursor = range.end;
    }
    append_expanded_slice(&mut without, source, cursor..source.text.len());
    let upper = without.text.to_ascii_uppercase();
    let procedure = upper
        .find("PROCEDURE DIVISION")
        .ok_or(SyntaxProblem::InvalidPrecompilerInclude)?;
    let insert = procedure
        + without.text[procedure..]
            .find('.')
            .ok_or(SyntaxProblem::InvalidPrecompilerInclude)?
        + 1;
    let mut output = ExpandedSource::default();
    append_expanded_slice(&mut output, &without, 0..insert);
    output.text.push('\n');
    for range in declarations {
        append_expanded_slice(&mut output, source, range);
        output.text.push('\n');
    }
    append_expanded_slice(&mut output, &without, insert..without.text.len());
    output
        .copy_directive_origins
        .extend(source.copy_directive_origins.iter().cloned());
    if output.text.len() > limits.max_expanded_bytes {
        return Err(SyntaxProblem::SourceLimitExceeded);
    }
    Ok(output)
}

fn find_sql_cursor_declaration(
    source: &str,
    from: usize,
) -> Result<Option<Range<usize>>, SyntaxProblem> {
    let mut cursor = from;
    while let Some((start, end)) = next_semantic_word(source, &mut cursor)? {
        if !source[start..end].eq_ignore_ascii_case("EXEC") {
            continue;
        }
        let mut probe = cursor;
        let Some((sql_start, sql_end)) = next_semantic_word(source, &mut probe)? else {
            continue;
        };
        if !source[sql_start..sql_end].eq_ignore_ascii_case("SQL") {
            continue;
        }
        let Some((declare_start, declare_end)) = next_semantic_word(source, &mut probe)? else {
            continue;
        };
        if !source[declare_start..declare_end].eq_ignore_ascii_case("DECLARE") {
            continue;
        }
        let _cursor_name = next_semantic_word(source, &mut probe)?
            .ok_or(SyntaxProblem::InvalidPrecompilerInclude)?;
        let Some((kind_start, kind_end)) = next_semantic_word(source, &mut probe)? else {
            return Err(SyntaxProblem::InvalidPrecompilerInclude);
        };
        if !source[kind_start..kind_end].eq_ignore_ascii_case("CURSOR") {
            continue;
        }
        loop {
            let Some((terminal_start, terminal_end)) = next_semantic_word(source, &mut probe)?
            else {
                return Err(SyntaxProblem::InvalidPrecompilerInclude);
            };
            if source[terminal_start..terminal_end].eq_ignore_ascii_case("END-EXEC") {
                let end =
                    terminal_end + usize::from(source.as_bytes().get(terminal_end) == Some(&b'.'));
                return Ok(Some(start..end));
            }
        }
    }
    Ok(None)
}

fn next_semantic_word(
    source: &str,
    cursor: &mut usize,
) -> Result<Option<(usize, usize)>, SyntaxProblem> {
    let bytes = source.as_bytes();
    while *cursor < bytes.len() {
        if source[*cursor..].starts_with("*>") {
            *cursor = source[*cursor..]
                .find('\n')
                .map_or(bytes.len(), |offset| *cursor + offset + 1);
            continue;
        }
        if matches!(bytes[*cursor], b'\'' | b'"') {
            *cursor = skip_quoted(source, *cursor)?;
            continue;
        }
        if is_word_byte(bytes[*cursor]) {
            let start = *cursor;
            while bytes.get(*cursor).is_some_and(|byte| is_word_byte(*byte)) {
                *cursor += 1;
            }
            return Ok(Some((start, *cursor)));
        }
        *cursor += source[*cursor..].chars().next().map_or(1, char::len_utf8);
    }
    Ok(None)
}

#[derive(Clone, Debug)]
struct CopyDirective {
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct ParsedCopyDirective {
    name: String,
    library: Option<String>,
    suppress: bool,
    replacements: Vec<TextReplacement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplacementMode {
    Exact,
    Leading,
    Trailing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextReplacement {
    mode: ReplacementMode,
    from: String,
    to: String,
}

fn process_replaces(
    source: ExpandedSource,
    artifacts: &mut directives::DirectiveArtifacts,
    limits: SyntaxLimits,
) -> Result<ExpandedSource, SyntaxProblem> {
    let mut output = ExpandedSource::default();
    let mut cursor = 0usize;
    let mut replacements = Vec::<TextReplacement>::new();
    while let Some(directive) = find_replace_directive(&source.text, cursor)? {
        if semantic_last_byte(&source.text[..directive.start]).is_some_and(|byte| byte != b'.') {
            return Err(SyntaxProblem::InvalidReplace);
        }
        append_replaced_segment(
            &mut output,
            &source,
            cursor..directive.start,
            &replacements,
            limits,
        )?;
        let parsed = parse_replace_directive(&source.text[directive.start..directive.end], limits)?;
        directives::record_directing(
            artifacts,
            CompilerDirectingKind::Replace,
            if parsed.is_none() {
                vec!["OFF".into()]
            } else {
                vec![format!(
                    "{} replacement-pair(s)",
                    parsed.as_ref().map_or(0, Vec::len)
                )]
            },
            source_spans(&source.origins, directive.start..directive.end),
            true,
            limits,
        )?;
        replacements = parsed.unwrap_or_default();
        for _ in source.text[directive.start..directive.end]
            .bytes()
            .filter(|byte| *byte == b'\n')
        {
            output.text.push('\n');
        }
        cursor = directive.end;
    }
    append_replaced_segment(
        &mut output,
        &source,
        cursor..source.text.len(),
        &replacements,
        limits,
    )?;
    output
        .copy_directive_origins
        .extend(source.copy_directive_origins);
    Ok(output)
}

fn semantic_last_byte(source: &str) -> Option<u8> {
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut last = None;
    while cursor < bytes.len() {
        if source[cursor..].starts_with("*>") {
            cursor = source[cursor..]
                .find('\n')
                .map_or(bytes.len(), |offset| cursor + offset + 1);
            continue;
        }
        if !bytes[cursor].is_ascii_whitespace() {
            last = Some(bytes[cursor]);
        }
        cursor += source[cursor..].chars().next().map_or(1, char::len_utf8);
    }
    last
}

fn append_replaced_segment(
    output: &mut ExpandedSource,
    source: &ExpandedSource,
    range: Range<usize>,
    replacements: &[TextReplacement],
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    if range.start == range.end {
        return Ok(());
    }
    if replacements.is_empty() {
        append_expanded_slice(output, source, range);
        return Ok(());
    }
    let mut segment = ExpandedSource::default();
    append_expanded_slice(&mut segment, source, range);
    append_expanded(
        output,
        apply_replacing(segment, replacements, limits.max_expanded_bytes)?,
    );
    Ok(())
}

fn find_replace_directive(
    source: &str,
    from: usize,
) -> Result<Option<CopyDirective>, SyntaxProblem> {
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
        if source[index..].starts_with("==") {
            index = source[index + 2..]
                .find("==")
                .map_or(bytes.len(), |relative| index + relative + 4);
            continue;
        }
        if is_word_byte(bytes[index]) {
            let start = index;
            while index < bytes.len() && is_word_byte(bytes[index]) {
                index += 1;
            }
            if source[start..index].eq_ignore_ascii_case("REPLACE") {
                return find_copy_end(source, start, index).map(Some);
            }
            continue;
        }
        index += source[index..].chars().next().map_or(1, char::len_utf8);
    }
    Ok(None)
}

fn parse_replace_directive(
    command: &str,
    limits: SyntaxLimits,
) -> Result<Option<Vec<TextReplacement>>, SyntaxProblem> {
    let body = command
        .strip_suffix('.')
        .ok_or(SyntaxProblem::InvalidReplace)?;
    let mut parser = CopyParser::new(body);
    parser
        .expect_word("REPLACE")
        .map_err(|_| SyntaxProblem::InvalidReplace)?;
    parser.whitespace();
    let checkpoint = parser.cursor;
    if parser.expect_word("OFF").is_ok() && parser.done() {
        return Ok(None);
    }
    parser.cursor = checkpoint;
    let mut replacements = Vec::new();
    while !parser.done() {
        if replacements.len() >= limits.max_replacements {
            return Err(SyntaxProblem::ReplacementLimitExceeded);
        }
        let mode = if parser.try_word("LEADING") {
            ReplacementMode::Leading
        } else if parser.try_word("TRAILING") {
            ReplacementMode::Trailing
        } else {
            ReplacementMode::Exact
        };
        let from = parser
            .operand(mode != ReplacementMode::Exact)
            .map_err(|_| SyntaxProblem::InvalidReplace)?
            .trim()
            .to_string();
        parser
            .expect_word("BY")
            .map_err(|_| SyntaxProblem::InvalidReplace)?;
        let to = parser
            .operand(mode != ReplacementMode::Exact)
            .map_err(|_| SyntaxProblem::InvalidReplace)?
            .trim()
            .to_string();
        if from.is_empty() {
            return Err(SyntaxProblem::InvalidReplace);
        }
        replacements.push(TextReplacement { mode, from, to });
    }
    if replacements.is_empty() {
        Err(SyntaxProblem::InvalidReplace)
    } else {
        Ok(Some(replacements))
    }
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
        if source[index..].starts_with("==") {
            index = source[index + 2..]
                .find("==")
                .map_or(bytes.len(), |relative| index + relative + 4);
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

fn parse_copy_directive(
    command: &str,
    limits: SyntaxLimits,
) -> Result<ParsedCopyDirective, SyntaxProblem> {
    let body = command
        .strip_suffix('.')
        .ok_or(SyntaxProblem::InvalidCopy)?;
    let mut parser = CopyParser::new(body);
    parser.expect_word("COPY")?;
    let name = parser.name()?.to_ascii_uppercase();
    validate_copy_name(&name)?;
    let mut library = None;
    let mut suppress = false;
    if parser.try_word("OF") || parser.try_word("IN") {
        let value = parser.name()?.to_ascii_uppercase();
        validate_copy_name(&value)?;
        library = Some(value);
    }
    if parser.try_word("SUPPRESS") {
        suppress = true;
    }
    let mut replacements = Vec::new();
    let replacing = parser.try_word("REPLACING");
    if replacing {
        while !parser.done() {
            if replacements.len() >= limits.max_replacements {
                return Err(SyntaxProblem::ReplacementLimitExceeded);
            }
            let mode = if parser.try_word("LEADING") {
                ReplacementMode::Leading
            } else if parser.try_word("TRAILING") {
                ReplacementMode::Trailing
            } else {
                ReplacementMode::Exact
            };
            let from = parser
                .operand(mode != ReplacementMode::Exact)?
                .trim()
                .to_string();
            parser.expect_word("BY")?;
            let to = parser
                .operand(mode != ReplacementMode::Exact)?
                .trim()
                .to_string();
            if from.is_empty() {
                return Err(SyntaxProblem::InvalidCopy);
            }
            replacements.push(TextReplacement { mode, from, to });
        }
        if replacements.is_empty() {
            return Err(SyntaxProblem::InvalidCopy);
        }
    } else if !parser.done() {
        return Err(SyntaxProblem::InvalidCopy);
    }
    Ok(ParsedCopyDirective {
        name,
        library,
        suppress,
        replacements,
    })
}

fn validate_copy_name(name: &str) -> Result<(), SyntaxProblem> {
    if name.is_empty()
        || name.len() > 160
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains('_')
    {
        Err(SyntaxProblem::InvalidCopy)
    } else {
        Ok(())
    }
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

    fn try_word(&mut self, expected: &str) -> bool {
        let checkpoint = self.cursor;
        if self.expect_word(expected).is_ok() {
            true
        } else {
            self.cursor = checkpoint;
            false
        }
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

    fn operand(&mut self, partial: bool) -> Result<String, SyntaxProblem> {
        self.whitespace();
        if self.source[self.cursor..].starts_with("==") {
            let value = self.pseudotext()?.to_string();
            if partial && value.split_whitespace().count() > 1 {
                return Err(SyntaxProblem::InvalidCopy);
            }
            return Ok(value);
        }
        if partial {
            return Err(SyntaxProblem::InvalidCopy);
        }
        let start = self.cursor;
        let bytes = self.source.as_bytes();
        let mut quote = None;
        let mut parentheses = 0usize;
        while self.cursor < bytes.len() {
            let byte = bytes[self.cursor];
            if matches!(byte, b'\'' | b'"') {
                if quote == Some(byte) {
                    if bytes.get(self.cursor + 1) == Some(&byte) {
                        self.cursor += 2;
                        continue;
                    }
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(byte);
                }
            } else if quote.is_none() && byte == b'(' {
                parentheses += 1;
            } else if quote.is_none() && byte == b')' {
                parentheses = parentheses.saturating_sub(1);
            } else if quote.is_none() && parentheses == 0 && byte.is_ascii_whitespace() {
                break;
            }
            self.cursor += 1;
        }
        if start == self.cursor || quote.is_some() || parentheses != 0 {
            return Err(SyntaxProblem::InvalidCopy);
        }
        let value = self.source[start..self.cursor].to_string();
        if value.eq_ignore_ascii_case("COPY") {
            Err(SyntaxProblem::InvalidCopy)
        } else {
            Ok(value)
        }
    }
}

fn apply_replacing(
    source: ExpandedSource,
    replacements: &[TextReplacement],
    max_expanded_bytes: usize,
) -> Result<ExpandedSource, SyntaxProblem> {
    let mut output = ExpandedSource::default();
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    while cursor < source.text.len() {
        let matched = replacements.iter().find_map(|replacement| {
            replacement_match_len(&source.text, cursor, replacement)
                .map(|length| (replacement, length))
        });
        if let Some((replacement, matched_length)) = matched {
            if output
                .text
                .len()
                .checked_add(replacement.to.len())
                .is_none_or(|length| length > max_expanded_bytes)
            {
                return Err(SyntaxProblem::SourceLimitExceeded);
            }
            let output_start = output.text.len();
            output.text.push_str(&replacement.to);
            let spans = source_spans(&source.origins, cursor..cursor + matched_length);
            for span in spans {
                output.origins.push(SourceOrigin {
                    output_start,
                    output_end: output.text.len(),
                    source: span.source,
                    source_start: span.source_start,
                    source_end: span.source_end,
                });
            }
            segments.push(TransformSegment {
                input: cursor..cursor + matched_length,
                output: output_start..output.text.len(),
            });
            cursor += matched_length;
            if replacement.mode == ReplacementMode::Leading {
                let remainder_start = cursor;
                while source
                    .text
                    .as_bytes()
                    .get(cursor)
                    .is_some_and(|byte| is_word_byte(*byte))
                {
                    cursor += 1;
                }
                if cursor > remainder_start {
                    let output_start = output.text.len();
                    output.text.push_str(&source.text[remainder_start..cursor]);
                    output.origins.extend(slice_origins(
                        &source.origins,
                        remainder_start..cursor,
                        output_start,
                    ));
                    segments.push(TransformSegment {
                        input: remainder_start..cursor,
                        output: output_start..output.text.len(),
                    });
                }
            }
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
        let output_start = output.text.len();
        output.text.push_str(&source.text[cursor..cursor + length]);
        output.origins.extend(slice_origins(
            &source.origins,
            cursor..cursor + length,
            output_start,
        ));
        segments.push(TransformSegment {
            input: cursor..cursor + length,
            output: output_start..output.text.len(),
        });
        cursor += length;
    }
    for expansion in &source.expansions {
        output.expansions.push(Expansion {
            output_start: map_transformed_position(&segments, expansion.output_start, false),
            output_end: map_transformed_position(&segments, expansion.output_end, true),
            source: expansion.source.clone(),
        });
    }
    output
        .copy_directive_origins
        .extend(source.copy_directive_origins);
    Ok(output)
}

#[derive(Clone, Debug)]
struct TransformSegment {
    input: Range<usize>,
    output: Range<usize>,
}

fn map_transformed_position(segments: &[TransformSegment], position: usize, ending: bool) -> usize {
    for segment in segments {
        if position == segment.input.start {
            return segment.output.start;
        }
        if position == segment.input.end {
            return segment.output.end;
        }
        if position > segment.input.start && position < segment.input.end {
            if segment.input.len() == segment.output.len() {
                return segment.output.start + position - segment.input.start;
            }
            return if ending {
                segment.output.end
            } else {
                segment.output.start
            };
        }
    }
    segments.last().map_or(0, |segment| segment.output.end)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PseudoPart {
    Exact(String),
    Separator,
}

fn pseudotext_parts(pattern: &str) -> Vec<PseudoPart> {
    let pattern = pattern.trim();
    if matches!(pattern, "," | ";") {
        return vec![PseudoPart::Exact(pattern.into())];
    }
    let bytes = pattern.as_bytes();
    let mut parts = Vec::new();
    let mut exact = String::new();
    let mut cursor = 0usize;
    let mut quote = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if matches!(byte, b'\'' | b'"') {
            exact.push(char::from(byte));
            if quote == Some(byte) {
                if bytes.get(cursor + 1) == Some(&byte) {
                    exact.push(char::from(byte));
                    cursor += 2;
                    continue;
                }
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
            cursor += 1;
            continue;
        }
        if quote.is_none() && (byte.is_ascii_whitespace() || matches!(byte, b',' | b';')) {
            if !exact.is_empty() {
                parts.push(PseudoPart::Exact(std::mem::take(&mut exact)));
            }
            if !matches!(parts.last(), Some(PseudoPart::Separator)) {
                parts.push(PseudoPart::Separator);
            }
            cursor += 1;
            continue;
        }
        let character = pattern[cursor..].chars().next().expect("bounded character");
        exact.push(character);
        cursor += character.len_utf8();
    }
    if !exact.is_empty() {
        parts.push(PseudoPart::Exact(exact));
    }
    while matches!(parts.first(), Some(PseudoPart::Separator)) {
        parts.remove(0);
    }
    while matches!(parts.last(), Some(PseudoPart::Separator)) {
        parts.pop();
    }
    parts
}

fn pseudotext_match_len(source: &str, cursor: usize, pattern: &str) -> Option<usize> {
    let parts = pseudotext_parts(pattern);
    if parts.is_empty() {
        return None;
    }
    let first_word = match parts.first() {
        Some(PseudoPart::Exact(value)) => value.as_bytes().first().copied(),
        _ => None,
    };
    if first_word.is_some_and(is_word_byte)
        && cursor
            .checked_sub(1)
            .and_then(|index| source.as_bytes().get(index))
            .is_some_and(|byte| is_word_byte(*byte))
    {
        return None;
    }
    let mut position = cursor;
    for part in parts {
        match part {
            PseudoPart::Exact(value) => {
                if !source[position..].starts_with(&value) {
                    return None;
                }
                position += value.len();
            }
            PseudoPart::Separator => {
                let start = position;
                loop {
                    while source.as_bytes().get(position).is_some_and(|byte| {
                        byte.is_ascii_whitespace() || matches!(byte, b',' | b';')
                    }) {
                        position += 1;
                    }
                    if source[position..].starts_with("*>") {
                        position = source[position..]
                            .find('\n')
                            .map_or(source.len(), |offset| position + offset + 1);
                        continue;
                    }
                    break;
                }
                if position == start {
                    return None;
                }
            }
        }
    }
    let last_word = pattern
        .trim()
        .as_bytes()
        .last()
        .copied()
        .is_some_and(is_word_byte);
    if last_word
        && source
            .as_bytes()
            .get(position)
            .is_some_and(|byte| is_word_byte(*byte))
    {
        return None;
    }
    Some(position - cursor)
}

fn replacement_match_len(
    source: &str,
    cursor: usize,
    replacement: &TextReplacement,
) -> Option<usize> {
    if replacement.from.is_empty() {
        return None;
    }
    let bytes = source.as_bytes();
    let left_boundary = cursor
        .checked_sub(1)
        .and_then(|index| bytes.get(index))
        .is_none_or(|byte| !is_word_byte(*byte));
    let right_boundary = bytes
        .get(cursor + replacement.from.len())
        .is_none_or(|byte| !is_word_byte(*byte));
    match replacement.mode {
        ReplacementMode::Exact => pseudotext_match_len(source, cursor, &replacement.from),
        ReplacementMode::Leading => (source[cursor..].starts_with(&replacement.from)
            && left_boundary
            && replacement.from.bytes().all(is_word_byte))
        .then_some(replacement.from.len()),
        ReplacementMode::Trailing => (source[cursor..].starts_with(&replacement.from)
            && right_boundary
            && replacement.from.bytes().all(is_word_byte))
        .then_some(replacement.from.len()),
    }
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
        .extend(slice_origins(&source.origins, range.clone(), output_start));
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
        .extend(slice_origins(&source.origins, range.clone(), output_start));
    for expansion in source.expansions.iter().filter(|expansion| {
        expansion.output_start >= range.start && expansion.output_end <= range.end
    }) {
        let mut expansion = expansion.clone();
        expansion.output_start = output_start + expansion.output_start - range.start;
        expansion.output_end = output_start + expansion.output_end - range.start;
        output.expansions.push(expansion);
    }
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

pub(crate) fn source_spans(origins: &[SourceOrigin], range: Range<usize>) -> Vec<SourceSpan> {
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
    let mut tokens = Vec::new();
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
        tokens.push(SyntaxToken {
            id: SyntaxTokenId::from_index(count)?,
            kind,
            bytes: offset..offset + length,
        });
        offset += length;
        count += 1;
    }
    builder.finish_node();
    Ok(LosslessSyntax {
        green: builder.finish(),
        text: text.to_string(),
        semantic_text: text.to_string(),
        token_count: count,
        lossless_origins: Vec::new(),
        expansions: Vec::new(),
        semantic_origins: Vec::new(),
        copy_directive_origins: Vec::new(),
        tokens,
        compiler_directing: Vec::new(),
        compiler_directives: Vec::new(),
        compiler_options: CompilerOptionSet::default(),
    })
}

fn next_token(text: &str) -> (CobolSyntaxKind, usize) {
    let bytes = text.as_bytes();
    if bytes[0] == b'\n' || bytes[0] == b'\r' {
        return (
            CobolSyntaxKind::Newline,
            usize::from(text.starts_with("\r\n")) + 1,
        );
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
    if text.starts_with(">>") {
        let length = 2 + bytes[2..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            .count();
        return (CobolSyntaxKind::CompilerDirective, length);
    }
    if let Some(stripped) = text.strip_prefix("==") {
        return match stripped.find("==") {
            Some(relative) => (CobolSyntaxKind::PseudoText, relative + 4),
            None => (CobolSyntaxKind::Error, text.len()),
        };
    }
    for (prefix, kind) in [
        ("BX", CobolSyntaxKind::HexLiteral),
        ("NX", CobolSyntaxKind::NationalLiteral),
        ("B", CobolSyntaxKind::BooleanLiteral),
        ("X", CobolSyntaxKind::HexLiteral),
        ("N", CobolSyntaxKind::NationalLiteral),
        ("U", CobolSyntaxKind::Utf8Literal),
        ("G", CobolSyntaxKind::String),
        ("Z", CobolSyntaxKind::String),
    ] {
        if text.len() > prefix.len()
            && bytes
                .get(..prefix.len())
                .is_some_and(|value| value.eq_ignore_ascii_case(prefix.as_bytes()))
            && matches!(bytes.get(prefix.len()), Some(b'\'' | b'"'))
        {
            return (
                kind,
                quoted_token_length(text, prefix.len()).unwrap_or(text.len()),
            );
        }
    }
    if matches!(bytes[0], b'\'' | b'"') {
        return match quoted_token_length(text, 0) {
            Some(length) => (CobolSyntaxKind::String, length),
            None => (CobolSyntaxKind::Error, text.len()),
        };
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
    if text.chars().next().is_some_and(char::is_alphabetic) {
        return (
            CobolSyntaxKind::Word,
            text.char_indices()
                .take_while(|(_, character)| {
                    character.is_alphanumeric() || matches!(character, '-' | '_')
                })
                .last()
                .map_or(1, |(index, character)| index + character.len_utf8()),
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

fn quoted_token_length(text: &str, prefix: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let quote = *bytes.get(prefix)?;
    let mut index = prefix + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return Some(index + 1);
        }
        index += text[index..].chars().next().map_or(1, char::len_utf8);
    }
    None
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
    AmbiguousCopy(String),
    CircularCopy(String),
    CopyDepthExceeded,
    NestedCopyReplacing,
    InvalidCopy,
    InvalidReplace,
    ReplacementLimitExceeded,
    InvalidContinuation,
    InvalidContinuationArea,
    InvalidLiteralContinuation,
    InvalidDirectiveContinuation,
    InvalidIndicator(u8),
    PrecompilerIncludeNotFound(String),
    AmbiguousPrecompilerInclude(String),
    CircularPrecompilerInclude(String),
    PrecompilerIncludeDepthExceeded,
    InvalidPrecompilerInclude,
    DirectiveLimitExceeded,
    DirectiveNestingExceeded,
    CompilationVariableLimitExceeded,
    InvalidCompilerOption,
    InvalidCompilerDirective,
    UnknownCompilerDirective(String),
    InvalidDirectiveContext,
    UnterminatedCompilerDirective,
    UnmatchedCompilerDirective,
    DuplicateCompilerBranch,
    InvalidCompilationVariable(String),
    CompilationVariableRedefinition(String),
    UndefinedCompilationVariable(String),
    InvalidConditionalExpression,
    ConditionalArithmeticOverflow,
    InvalidCompilerDirectingStatement,
    BasisNotFound(String),
    AmbiguousBasis(String),
    InvalidBasis,
    InvalidBasisSequence,
    InvalidDelete,
    InvalidInsert,
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
    use crate::CompilerDirectiveKind;
    use mainframe_env_source::{LogicalPath, SourceFile, SourceLibrary, SourceLimits};
    use std::collections::BTreeMap;
    fn bundle(text: &str, format: SourceFormat) -> SourceBundle {
        bundle_with_options(text, format, BTreeMap::new())
    }
    fn bundle_with_options(
        text: &str,
        format: SourceFormat,
        options: BTreeMap<String, String>,
    ) -> SourceBundle {
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
        SourceBundle::new(&path, vec![file], options, Vec::new(), limits).unwrap()
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
    fn bundle_with_basis(text: &str, basis_name: &str, basis_text: &str) -> SourceBundle {
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
                format!("basis/{basis_name}.cbl"),
                basis_text.as_bytes().to_vec(),
                SourceFormat::Fixed,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ];
        SourceBundle::new(&path, files, BTreeMap::new(), Vec::new(), limits).unwrap()
    }

    fn bundle_with_named_copies(text: &str) -> SourceBundle {
        let limits = SourceLimits::default();
        let primary = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let first = LogicalPath::new("first/MEMBER.cpy", limits.max_path_bytes).unwrap();
        let second = LogicalPath::new("second/MEMBER.cpy", limits.max_path_bytes).unwrap();
        let files = vec![
            SourceFile::input(
                primary.as_str(),
                text.as_bytes().to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
            SourceFile::input(
                first.as_str(),
                b"01 FROM-FIRST PIC X.\n".to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
            SourceFile::input(
                second.as_str(),
                b"01 FROM-SECOND PIC X.\n".to_vec(),
                SourceFormat::Free,
                SourceEncoding::Utf8,
                limits,
            )
            .unwrap(),
        ];
        SourceBundle::with_libraries(
            &primary,
            files,
            vec![
                SourceLibrary::new("LIBA", vec![first], limits).unwrap(),
                SourceLibrary::new("LIBB", vec![second], limits).unwrap(),
            ],
            BTreeMap::new(),
            Vec::new(),
            limits,
        )
        .unwrap()
    }
    fn ebcdic_bundle(text: &str) -> SourceBundle {
        let limits = SourceLimits::default();
        let path = LogicalPath::new("main.cbl", limits.max_path_bytes).unwrap();
        let bytes = CodePage::Cp037.encode(text, limits.max_file_bytes).unwrap();
        let file = SourceFile::input(
            path.as_str(),
            bytes,
            SourceFormat::Free,
            SourceEncoding::Ebcdic(37),
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
    fn ebcdic_multibyte_decoding_retains_exact_input_byte_provenance() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. CAFé.\n";
        let bundle = ebcdic_bundle(source);
        let byte_len = bundle.file(bundle.primary()).unwrap().bytes().len();
        let syntax = decode_and_lex(&bundle, SyntaxLimits::default()).unwrap();
        assert_eq!(syntax.text(), source);
        let decoded = syntax.text();
        let decoded_start = decoded.find('é').unwrap();
        let lossless = syntax
            .lossless_origins()
            .iter()
            .find(|origin| {
                origin.output_start <= decoded_start && origin.output_end > decoded_start
            })
            .unwrap();
        assert_eq!(lossless.source_end - lossless.source_start, 1);
        let semantic_start = syntax.semantic_text().find('é').unwrap();
        let semantic = syntax
            .semantic_origins()
            .iter()
            .find(|origin| {
                origin.output_start <= semantic_start && origin.output_end > semantic_start
            })
            .unwrap();
        assert_eq!(semantic.source_end - semantic.source_start, 1);
        assert!(
            syntax
                .semantic_origins()
                .iter()
                .all(|origin| origin.source_end <= byte_len)
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
    fn copy_replacing_normalizes_separators_and_partial_word_boundaries() {
        let exact = decode_and_lex(
            &bundle_with_copy(
                "       COPY ACTUAL REPLACING == A B ==\n              BY == X Y ==.\n",
                "ACTUAL",
                "       MOVE A   B TO RESULT.\n",
            ),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(exact.semantic_text().contains("MOVE X Y TO RESULT"));

        let partial = decode_and_lex(
            &bundle_with_copy(
                "       COPY ACTUAL REPLACING LEADING == DEPT ==\n              BY == PAYROLL == TRAILING == GROSS-PAY ==\n              BY == NET-PAY ==.\n",
                "ACTUAL",
                "       01 DEPT-WEEK PIC X.\n       01 OTHER-GROSS-PAY PIC X.\n       01 DEPT-GROSS-PAY PIC X.\n",
            ),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(partial.semantic_text().contains("PAYROLL-WEEK"));
        assert!(partial.semantic_text().contains("OTHER-NET-PAY"));
        assert!(partial.semantic_text().contains("PAYROLL-GROSS-PAY"));
    }

    #[test]
    fn copy_of_selects_the_explicit_ordered_library() {
        let syntax = decode_and_lex(
            &bundle_with_named_copies("COPY MEMBER OF LIBB.\n"),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(syntax.semantic_text().contains("FROM-SECOND"));
        assert!(!syntax.semantic_text().contains("FROM-FIRST"));
        assert_eq!(syntax.expansions()[0].source.as_str(), "second/MEMBER.cpy");
    }
    #[test]
    fn copy_in_strings_and_free_form_comments_is_ignored() {
        let source = "IDENTIFICATION DIVISION.\n*> COPY COMMENTED.\nPROCEDURE DIVISION.\nDISPLAY 'COPY QUOTED.'.\n";
        let syntax =
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default()).unwrap();
        assert!(syntax.expansions().is_empty());
    }
    #[test]
    fn copy_words_inside_replace_pseudotext_are_not_expanded() {
        let source = "REPLACE ==COPY MISSING.== BY ==DISPLAY 'OK'.==.\n";
        let syntax =
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default()).unwrap();
        assert!(syntax.expansions().is_empty());
        assert!(
            syntax
                .compiler_directing_statements()
                .iter()
                .any(|node| { node.kind == CompilerDirectingKind::Replace })
        );
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
    fn basis_delete_and_insert_preserve_selected_source_origins() {
        let primary = "       BASIS BASE\n\
       DELETE 000200\n\
000250 PROGRAM-ID. NEW.\n\
       INSERT 000300\n\
000350     DISPLAY 'NEW'.\n\
000360     DELETE SOME-FILE RECORD.\n";
        let basis = "000100 IDENTIFICATION DIVISION.\n\
000200 PROGRAM-ID. OLD.\n\
000300 PROCEDURE DIVISION.\n\
000400     DISPLAY 'OLD'.\n\
000500     STOP RUN.\n";
        let syntax = decode_and_lex(
            &bundle_with_basis(primary, "BASE", basis),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(syntax.semantic_text().contains("PROGRAM-ID. NEW."));
        assert!(!syntax.semantic_text().contains("PROGRAM-ID. OLD."));
        assert!(syntax.semantic_text().contains("DISPLAY 'NEW'."));
        assert!(syntax.semantic_text().contains("DELETE SOME-FILE RECORD."));
        let kinds = syntax
            .compiler_directing_statements()
            .iter()
            .map(|node| node.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                CompilerDirectingKind::Basis,
                CompilerDirectingKind::Delete,
                CompilerDirectingKind::Insert,
            ]
        );
        let paths = syntax
            .semantic_origins()
            .iter()
            .map(|origin| origin.source.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            paths,
            std::collections::BTreeSet::from(["basis/BASE.cbl", "main.cbl"])
        );
    }

    #[test]
    fn basis_modification_sequence_rules_fail_closed() {
        let basis = "000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. BASE.\n";
        let out_of_order = "       BASIS BASE\n       INSERT 000200\n000250 PROGRAM-ID. NEW.\n       DELETE 000100\n";
        assert_eq!(
            decode_and_lex(
                &bundle_with_basis(out_of_order, "BASE", basis),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::InvalidBasisSequence
        );

        let missing_insert_text = "       BASIS BASE\n       INSERT 000100\n";
        assert_eq!(
            decode_and_lex(
                &bundle_with_basis(missing_insert_text, "BASE", basis),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::InvalidInsert
        );

        let unsequenced_basis = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. BASE.\n";
        let modifying = "       BASIS BASE\n       DELETE 000100\n";
        assert_eq!(
            decode_and_lex(
                &bundle_with_basis(modifying, "BASE", unsequenced_basis),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::InvalidBasisSequence
        );
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

    #[test]
    fn all_directive_groups_are_typed_and_conditional_text_is_selected() {
        let source = "PROCESS LP(64),DLL\n\
>>JAVA-CALLABLE\n\
IDENTIFICATION DIVISION.\nPROGRAM-ID. DIRECTIV.\n\
DATA DIVISION.\nWORKING-STORAGE SECTION.\n\
>>DATA 31\n\
>>JAVA-SHAREABLE ON\n01 FLAG PIC X.\n>>JAVA-SHAREABLE OFF\n\
PROCEDURE DIVISION.\n\
>>CALLINTERFACE DYNAMIC\n\
>>INLINE OFF\n\
>>DEFINE SELECTED B'1'\n\
>>IF SELECTED\nDISPLAY 'INCLUDED'.\n>>ELSE\nDISPLAY 'OMITTED'.\n>>END-IF\n\
STOP RUN.\n";
        let syntax =
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default()).unwrap();
        assert_eq!(syntax.compiler_directing_statements().len(), 1);
        assert_eq!(
            syntax.compiler_directing_statements()[0].kind,
            CompilerDirectingKind::Process
        );
        assert_eq!(syntax.compiler_options().value("LP"), Some(Some("(64)")));
        let groups = syntax
            .compiler_directives()
            .iter()
            .map(|directive| directive.group)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            groups,
            std::collections::BTreeSet::from([
                crate::CompilerDirectiveGroup::Callinterface,
                crate::CompilerDirectiveGroup::Data,
                crate::CompilerDirectiveGroup::Inline,
                crate::CompilerDirectiveGroup::Conditional,
                crate::CompilerDirectiveGroup::JavaInterop,
            ])
        );
        assert!(syntax.semantic_text().contains("DISPLAY 'INCLUDED'"));
        assert!(!syntax.semantic_text().contains("DISPLAY 'OMITTED'"));
        assert!(
            syntax
                .compiler_directives()
                .iter()
                .all(|directive| !directive.source.is_empty())
        );
    }

    #[test]
    fn conditional_parameters_aliases_and_inactive_directives_are_bounded() {
        let source = ">>DEFINE EXTERNAL AS PARAMETER\n\
>>IF EXTERNAL = 7 AND ARCH = 10\nDISPLAY 'PARAMETER'.\n>>END-IF\n\
>>IF B'0'\n>>CALLINTERFACE STATIC\n>>END-IF\n\
IDENTIFICATION DIVISION.\nPROGRAM-ID. PARAMS.\nPROCEDURE DIVISION.\nSTOP RUN.\n";
        let syntax = decode_and_lex(
            &bundle_with_options(
                source,
                SourceFormat::Free,
                BTreeMap::from([("cobol.define.external".into(), "7".into())]),
            ),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(syntax.semantic_text().contains("DISPLAY 'PARAMETER'"));
        assert!(syntax.compiler_directives().iter().any(|directive| {
            directive.kind == CompilerDirectiveKind::Callinterface && !directive.active
        }));
    }

    #[test]
    fn source_context_keywords_inside_literals_do_not_authorize_directives() {
        let problem = decode_and_lex(
            &bundle(
                "DISPLAY 'PROCEDURE DIVISION'.\n>>CALLINTERFACE STATIC\n",
                SourceFormat::Free,
            ),
            SyntaxLimits::default(),
        )
        .unwrap_err();
        assert_eq!(problem, SyntaxProblem::InvalidDirectiveContext);
    }

    #[test]
    fn variable_debugging_and_literal_continuation_reference_rules_are_distinct() {
        let extended = format!("{:<72}TAIL\n", "000100 IDENTIFICATION DIVISION.");
        let fixed = decode_and_lex(
            &bundle(&extended, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        let variable = decode_and_lex(
            &bundle(&extended, SourceFormat::Variable),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(!fixed.semantic_text().contains("TAIL"));
        assert!(variable.semantic_text().contains("TAIL"));

        let debugging = "000100D    DISPLAY 'DEBUG'.\n";
        let suppressed = decode_and_lex(
            &bundle(debugging, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        let enabled = decode_and_lex(
            &bundle_with_options(
                debugging,
                SourceFormat::Fixed,
                BTreeMap::from([("cobol.debugging-mode".into(), "true".into())]),
            ),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(suppressed.semantic_text().starts_with("*>"));
        assert!(enabled.semantic_text().contains("DISPLAY 'DEBUG'"));

        let literal = format!(
            "000100 {:<65}\n000200-    'B'.\n",
            "01 TEXT-X PIC X(50) VALUE 'A"
        );
        let syntax = decode_and_lex(
            &bundle(&literal, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        let value = syntax.semantic_text();
        let start = value.find("'A").unwrap();
        let end = value.find("B'").unwrap();
        assert!(value[start + 2..end].bytes().all(|byte| byte == b' '));
        assert!(end > start + 2);
    }

    #[test]
    fn fixed_compiler_directing_lines_cannot_continue() {
        for source in [
            "PROCESS LP(64)\n000100-    DLL\n",
            "      *CONTROL SOURCE\n000100-    LIST\n",
        ] {
            assert_eq!(
                decode_and_lex(
                    &bundle(source, SourceFormat::Fixed),
                    SyntaxLimits::default()
                )
                .unwrap_err(),
                SyntaxProblem::InvalidCompilerDirectingStatement
            );
        }
        assert_eq!(
            decode_and_lex(
                &bundle(
                    "IDENTIFICATION DIVISION.\nPROCESS LP(64)\n",
                    SourceFormat::Free,
                ),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::InvalidDirectiveContext
        );
    }

    #[test]
    fn inactive_conditional_copy_is_not_resolved() {
        let source = ">>IF B'0'\nCOPY MISSING.\n>>END-IF\n\
IDENTIFICATION DIVISION. PROGRAM-ID. NOCOPY. PROCEDURE DIVISION. STOP RUN.\n";
        let syntax =
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default()).unwrap();
        assert!(syntax.expansions().is_empty());
        assert!(!syntax.semantic_text().contains("MISSING"));
    }

    #[test]
    fn compilation_variables_defined_in_copy_text_affect_following_source() {
        let source = "       COPY ACTUAL.\n       >>IF FROM-COPY\n           DISPLAY 'YES'.\n       >>ELSE\n           DISPLAY 'NO'.\n       >>END-IF\n";
        let syntax = decode_and_lex(
            &bundle_with_copy(source, "ACTUAL", "       >>DEFINE FROM-COPY B'1'\n"),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(syntax.semantic_text().contains("DISPLAY 'YES'"));
        assert!(!syntax.semantic_text().contains("DISPLAY 'NO'"));
    }

    #[test]
    fn malformed_directive_scope_indicator_and_continuation_fail_closed() {
        assert_eq!(
            decode_and_lex(
                &bundle(">>IF B'1'\nDISPLAY 'X'.\n", SourceFormat::Free),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::UnterminatedCompilerDirective
        );
        assert_eq!(
            decode_and_lex(
                &bundle("      >>INLINE ON\n", SourceFormat::Fixed),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::InvalidIndicator(b'>')
        );
        assert_eq!(
            decode_and_lex(
                &bundle(
                    "           >>INLINE\n      -       ON\n",
                    SourceFormat::Fixed,
                ),
                SyntaxLimits::default(),
            )
            .unwrap_err(),
            SyntaxProblem::InvalidDirectiveContinuation
        );
    }

    #[test]
    fn fixed_directives_are_accepted_in_area_a_and_area_b() {
        let source = "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. DIRECT.\n       PROCEDURE DIVISION.\n       >>INLINE ON\n           >>INLINE OFF\n           STOP RUN.\n";
        let syntax = decode_and_lex(
            &bundle(source, SourceFormat::Fixed),
            SyntaxLimits::default(),
        )
        .unwrap();
        assert_eq!(syntax.compiler_directives().len(), 2);
    }

    #[test]
    fn contextual_lexer_preserves_directive_pseudotext_and_typed_literals() {
        let source =
            ">>DEFINE B1 B'1'\nREPLACE ==A B== BY ==C==.\nN'X' U'Y' X'F1' '漢' 漢字-NAME\n";
        let syntax =
            decode_and_lex(&bundle(source, SourceFormat::Free), SyntaxLimits::default()).unwrap();
        let kinds = syntax
            .tokens()
            .iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&CobolSyntaxKind::CompilerDirective));
        assert!(kinds.contains(&CobolSyntaxKind::PseudoText));
        assert!(kinds.contains(&CobolSyntaxKind::BooleanLiteral));
        assert!(kinds.contains(&CobolSyntaxKind::NationalLiteral));
        assert!(kinds.contains(&CobolSyntaxKind::Utf8Literal));
        assert!(kinds.contains(&CobolSyntaxKind::HexLiteral));
        assert!(
            syntax
                .tokens()
                .iter()
                .enumerate()
                .all(|(index, token)| token.id.index() == index)
        );
    }
}
