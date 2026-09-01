use crate::JclBundle;
use mainframe_env_diagnostics::{
    Completeness, Diagnostic, DiagnosticCode, DiagnosticLimits, FailureCategory, Phase, Redaction,
    Severity, SourceSpan,
};
use mainframe_env_source::{
    FileId, LibraryProblem, LogicalPath, SourceBundle, SourceEncoding, SourceFile, SourceFormat,
    SourceLibrary, SourceLimits, SourceRange,
};
use rowan::{GreenNode, GreenNodeBuilder, Language};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

/// Stable identity for the owned, lossless JCL concrete-syntax contract.
pub const JCL_SYNTAX_CONTRACT: &str = "mainframe-env.jcl-syntax@1";

/// Bounds applied before or while constructing JCL syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JclSyntaxLimits {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_source_bytes: usize,
    pub max_lines: usize,
    pub max_record_bytes: usize,
    pub max_tokens: usize,
    pub max_inline_bytes: usize,
    pub max_path_bytes: usize,
}

impl Default for JclSyntaxLimits {
    fn default() -> Self {
        Self {
            max_files: 2_049,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_source_bytes: 64 * 1024 * 1024,
            max_lines: 65_536,
            max_record_bytes: 80,
            max_tokens: 1_000_000,
            max_inline_bytes: 64 * 1024 * 1024,
            max_path_bytes: 512,
        }
    }
}

/// Lossless token and record nodes used by the Rowan-style JCL tree.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum JclSyntaxKind {
    Root,
    StatementRecord,
    ContinuationRecord,
    CommentRecord,
    NullRecord,
    InStreamDataRecord,
    DelimiterRecord,
    JeclRecord,
    ErrorRecord,
    Prefix,
    Name,
    Operation,
    Operand,
    Whitespace,
    Sequence,
    Data,
    Newline,
    Error,
}

impl From<JclSyntaxKind> for rowan::SyntaxKind {
    fn from(kind: JclSyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum JclLanguage {}

impl Language for JclLanguage {
    type Kind = JclSyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        match raw.0 {
            0 => JclSyntaxKind::Root,
            1 => JclSyntaxKind::StatementRecord,
            2 => JclSyntaxKind::ContinuationRecord,
            3 => JclSyntaxKind::CommentRecord,
            4 => JclSyntaxKind::NullRecord,
            5 => JclSyntaxKind::InStreamDataRecord,
            6 => JclSyntaxKind::DelimiterRecord,
            7 => JclSyntaxKind::JeclRecord,
            8 => JclSyntaxKind::ErrorRecord,
            9 => JclSyntaxKind::Prefix,
            10 => JclSyntaxKind::Name,
            11 => JclSyntaxKind::Operation,
            12 => JclSyntaxKind::Operand,
            13 => JclSyntaxKind::Whitespace,
            14 => JclSyntaxKind::Sequence,
            15 => JclSyntaxKind::Data,
            16 => JclSyntaxKind::Newline,
            _ => JclSyntaxKind::Error,
        }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JclRecordKind {
    Statement,
    Continuation,
    Comment,
    Null,
    InStreamData,
    Delimiter,
    Jecl,
    Error,
}

impl JclRecordKind {
    const fn syntax(self) -> JclSyntaxKind {
        match self {
            Self::Statement => JclSyntaxKind::StatementRecord,
            Self::Continuation => JclSyntaxKind::ContinuationRecord,
            Self::Comment => JclSyntaxKind::CommentRecord,
            Self::Null => JclSyntaxKind::NullRecord,
            Self::InStreamData => JclSyntaxKind::InStreamDataRecord,
            Self::Delimiter => JclSyntaxKind::DelimiterRecord,
            Self::Jecl => JclSyntaxKind::JeclRecord,
            Self::Error => JclSyntaxKind::ErrorRecord,
        }
    }
}

/// One physical record. The exact byte span is authoritative; line and column
/// values are deterministic display projections over that span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JclRecord {
    kind: JclRecordKind,
    span: SourceRange,
    line: usize,
    content_bytes: Range<usize>,
    terminator_bytes: Range<usize>,
}

impl JclRecord {
    #[must_use]
    pub const fn kind(&self) -> JclRecordKind {
        self.kind
    }

    #[must_use]
    pub fn span(&self) -> &SourceRange {
        &self.span
    }

    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    #[must_use]
    pub fn content_bytes(&self) -> &Range<usize> {
        &self.content_bytes
    }

    #[must_use]
    pub fn terminator_bytes(&self) -> &Range<usize> {
        &self.terminator_bytes
    }
}

#[derive(Clone, Debug)]
pub struct JclLosslessSyntax {
    green: GreenNode,
    text: String,
    records: Vec<JclRecord>,
    token_count: usize,
}

impl JclLosslessSyntax {
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn records(&self) -> &[JclRecord] {
        &self.records
    }

    #[must_use]
    pub const fn token_count(&self) -> usize {
        self.token_count
    }
}

#[derive(Clone, Debug)]
pub struct JclSyntaxAnalysis {
    source: SourceBundle,
    syntax: JclLosslessSyntax,
    diagnostics: Vec<Diagnostic>,
}

impl JclSyntaxAnalysis {
    #[must_use]
    pub fn source(&self) -> &SourceBundle {
        &self.source
    }

    #[must_use]
    pub fn syntax(&self) -> &JclLosslessSyntax {
        &self.syntax
    }

    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JclSyntaxProblem {
    SourceBundle(String),
    PrimaryMissing,
    SourceNotUtf8,
    FileLimitExceeded,
    TotalSourceLimitExceeded,
    LineLimitExceeded,
    TokenLimitExceeded,
    InlineDataLimitExceeded,
}

impl fmt::Display for JclSyntaxProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "JCL syntax failed: {self:?}")
    }
}

impl std::error::Error for JclSyntaxProblem {}

#[derive(Clone, Debug)]
struct PhysicalRecord {
    content: Range<usize>,
    terminator: Range<usize>,
}

#[derive(Clone, Debug)]
struct InStreamState {
    delimiter: String,
    jcl_terminates: bool,
    bytes: usize,
    definition_span: Range<usize>,
}

#[derive(Clone, Debug)]
struct StatementFields {
    name: Range<usize>,
    operation: Range<usize>,
    operands: Range<usize>,
}

/// Builds the exact bounded source closure, then performs recoverable,
/// lossless, fixed-record JCL lexical analysis.
pub fn analyze_jcl_syntax(
    bundle: &JclBundle,
    limits: JclSyntaxLimits,
) -> Result<JclSyntaxAnalysis, JclSyntaxProblem> {
    let source = source_bundle(bundle, limits)?;
    let primary = source
        .file(source.primary())
        .ok_or(JclSyntaxProblem::PrimaryMissing)?;
    let text = std::str::from_utf8(primary.bytes())
        .map_err(|_| JclSyntaxProblem::SourceNotUtf8)?
        .to_string();
    let physical = physical_records(&text);
    if physical.len() > limits.max_lines {
        return Err(JclSyntaxProblem::LineLimitExceeded);
    }

    let mut builder = GreenNodeBuilder::new();
    builder.start_node(JclSyntaxKind::Root.into());
    let mut records = Vec::with_capacity(physical.len());
    let mut diagnostics = Vec::new();
    let mut inline = None::<InStreamState>;
    let mut continuation_expected = false;
    let mut token_count = 0usize;

    for (index, physical) in physical.iter().enumerate() {
        let raw = &text[physical.content.clone()];
        let line = index + 1;
        let file = source.primary();
        let mut kind = JclRecordKind::Error;

        if let Some(state) = inline.as_mut() {
            let semantic = statement_area(raw);
            let delimiter = semantic.trim_end() == state.delimiter;
            if delimiter {
                kind = JclRecordKind::Delimiter;
                inline = None;
            } else if state.jcl_terminates && raw.starts_with("//") {
                inline = None;
            } else {
                state.bytes = state
                    .bytes
                    .checked_add(raw.len() + physical.terminator.len())
                    .ok_or(JclSyntaxProblem::InlineDataLimitExceeded)?;
                if state.bytes > limits.max_inline_bytes {
                    return Err(JclSyntaxProblem::InlineDataLimitExceeded);
                }
                kind = JclRecordKind::InStreamData;
            }
        }

        if kind == JclRecordKind::Error {
            kind = classify_control_record(raw, continuation_expected);
            if raw.len() > limits.max_record_bytes {
                diagnostics.push(diagnostic(
                    "MEJCL0701",
                    "JCL physical record exceeds the configured record width",
                    file,
                    physical.content.start + limits.max_record_bytes..physical.content.end,
                ));
                kind = JclRecordKind::Error;
            } else if !raw.is_ascii() {
                diagnostics.push(diagnostic(
                    "MEJCL0702",
                    "JCL UTF-8 input contains a non-ASCII physical record",
                    file,
                    physical.content.clone(),
                ));
                kind = JclRecordKind::Error;
            } else if matches!(kind, JclRecordKind::Error) {
                diagnostics.push(diagnostic(
                    "MEJCL0703",
                    "JCL control record must begin with // or a JES2 /* control prefix",
                    file,
                    physical.content.clone(),
                ));
            }

            if matches!(kind, JclRecordKind::Statement | JclRecordKind::Continuation)
                && let Some(fields) = statement_fields(raw)
            {
                let operation = &raw[fields.operation.clone()];
                let operands = &raw[fields.operands.clone()];
                if operation.eq_ignore_ascii_case("DD")
                    && let Some(state) = inline_state(operands, physical.content.clone())
                {
                    inline = Some(state);
                }
                continuation_expected = statement_continues(operands);
            } else {
                continuation_expected = false;
            }
        }

        builder.start_node(kind.syntax().into());
        token_count += emit_record_tokens(&mut builder, raw, kind);
        if !physical.terminator.is_empty() {
            builder.token(
                JclSyntaxKind::Newline.into(),
                &text[physical.terminator.clone()],
            );
            token_count += 1;
        }
        builder.finish_node();
        if token_count > limits.max_tokens {
            return Err(JclSyntaxProblem::TokenLimitExceeded);
        }

        records.push(JclRecord {
            kind,
            span: SourceRange {
                file,
                bytes: physical.content.start..physical.terminator.end,
            },
            line,
            content_bytes: physical.content.clone(),
            terminator_bytes: physical.terminator.clone(),
        });
    }

    if let Some(state) = inline {
        diagnostics.push(diagnostic(
            "MEJCL0704",
            "in-stream data is missing its declared delimiter",
            source.primary(),
            state.definition_span,
        ));
    }
    builder.finish_node();
    let green = builder.finish();
    debug_assert_eq!(green.to_string(), text);
    Ok(JclSyntaxAnalysis {
        source,
        syntax: JclLosslessSyntax {
            green,
            text,
            records,
            token_count,
        },
        diagnostics,
    })
}

fn source_bundle(
    bundle: &JclBundle,
    limits: JclSyntaxLimits,
) -> Result<SourceBundle, JclSyntaxProblem> {
    let source_limits = SourceLimits {
        max_files: limits.max_files,
        max_file_bytes: limits.max_file_bytes,
        max_total_bytes: limits.max_total_source_bytes,
        max_path_bytes: limits.max_path_bytes,
        max_options: 8,
        max_option_bytes: 128,
        max_provenance_edges: limits.max_lines,
    };
    let primary_path = LogicalPath::new("jcl/primary.jcl", limits.max_path_bytes)
        .map_err(|problem| JclSyntaxProblem::SourceBundle(problem.to_string()))?;
    let mut files = vec![source_file(
        primary_path.as_str(),
        bundle.primary.as_bytes(),
        source_limits,
    )?];
    let mut include_members = Vec::new();
    let mut procedure_members = Vec::new();
    for (name, bytes) in &bundle.includes {
        let path = member_path("jcl/includes", name, limits)?;
        include_members.push(path.clone());
        files.push(source_file(path.as_str(), bytes.as_bytes(), source_limits)?);
    }
    for (name, bytes) in &bundle.cataloged_procedures {
        let path = member_path("jcl/procedures", name, limits)?;
        procedure_members.push(path.clone());
        files.push(source_file(path.as_str(), bytes.as_bytes(), source_limits)?);
    }
    if files.len() > limits.max_files {
        return Err(JclSyntaxProblem::FileLimitExceeded);
    }
    let mut libraries = Vec::new();
    if !include_members.is_empty() {
        libraries.push(
            SourceLibrary::new("jcl-includes", include_members, source_limits)
                .map_err(library_problem)?,
        );
    }
    if !procedure_members.is_empty() {
        libraries.push(
            SourceLibrary::new("jcl-procedures", procedure_members, source_limits)
                .map_err(library_problem)?,
        );
    }
    SourceBundle::with_libraries(
        &primary_path,
        files,
        libraries,
        BTreeMap::from([("jcl.syntax-contract".into(), JCL_SYNTAX_CONTRACT.into())]),
        Vec::new(),
        source_limits,
    )
    .map_err(library_problem)
}

fn source_file(
    path: &str,
    bytes: &[u8],
    limits: SourceLimits,
) -> Result<SourceFile, JclSyntaxProblem> {
    SourceFile::input(
        path,
        bytes.to_vec(),
        SourceFormat::Fixed,
        SourceEncoding::Utf8,
        limits,
    )
    .map_err(|problem| JclSyntaxProblem::SourceBundle(problem.to_string()))
}

fn member_path(
    directory: &str,
    name: &str,
    limits: JclSyntaxLimits,
) -> Result<LogicalPath, JclSyntaxProblem> {
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'#' | b'@' | b'-' | b'_')
        })
    {
        return Err(JclSyntaxProblem::SourceBundle(format!(
            "invalid JCL member name {name:?}"
        )));
    }
    LogicalPath::new(
        format!("{directory}/{}.jcl", name.to_ascii_uppercase()),
        limits.max_path_bytes,
    )
    .map_err(|problem| JclSyntaxProblem::SourceBundle(problem.to_string()))
}

fn library_problem(problem: LibraryProblem) -> JclSyntaxProblem {
    JclSyntaxProblem::SourceBundle(problem.to_string())
}

fn physical_records(text: &str) -> Vec<PhysicalRecord> {
    let bytes = text.as_bytes();
    let mut records = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
            end += 1;
        }
        let mut terminator_end = end;
        if terminator_end < bytes.len() {
            if bytes[terminator_end] == b'\r' && bytes.get(terminator_end + 1) == Some(&b'\n') {
                terminator_end += 2;
            } else {
                terminator_end += 1;
            }
        }
        records.push(PhysicalRecord {
            content: start..end,
            terminator: end..terminator_end,
        });
        start = terminator_end;
    }
    records
}

fn statement_area(raw: &str) -> &str {
    &raw[..raw.len().min(72)]
}

fn classify_control_record(raw: &str, continuation_expected: bool) -> JclRecordKind {
    if raw.starts_with("//*") {
        JclRecordKind::Comment
    } else if raw == "//" || raw.get(2..).is_some_and(|value| value.trim().is_empty()) {
        JclRecordKind::Null
    } else if raw.starts_with("//") {
        if continuation_expected && raw.as_bytes().get(2).is_some_and(u8::is_ascii_whitespace) {
            JclRecordKind::Continuation
        } else {
            JclRecordKind::Statement
        }
    } else if raw.starts_with("/*") {
        if raw.get(2..).is_none_or(|value| value.trim().is_empty()) {
            JclRecordKind::Delimiter
        } else {
            JclRecordKind::Jecl
        }
    } else {
        JclRecordKind::Error
    }
}

fn statement_fields(raw: &str) -> Option<StatementFields> {
    let area = statement_area(raw);
    if !area.starts_with("//") || area.starts_with("//*") {
        return None;
    }
    let bytes = area.as_bytes();
    let mut cursor = 2usize;
    let name_start = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let name = name_start..cursor;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let operation_start = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    let operation = operation_start..cursor;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    Some(StatementFields {
        name,
        operation,
        operands: cursor..area.len(),
    })
}

fn inline_state(operands: &str, definition_span: Range<usize>) -> Option<InStreamState> {
    let first = top_level_operands(operands).into_iter().next()?;
    let first = first.trim();
    let jcl_terminates = first == "*";
    if !jcl_terminates && !first.eq_ignore_ascii_case("DATA") && !first.starts_with("DATA=") {
        return None;
    }
    let delimiter = top_level_operands(operands)
        .into_iter()
        .filter_map(|operand| operand.split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("DLM"))
        .map(|(_, value)| value.trim().trim_matches(['\'', '"']).to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/*".into());
    Some(InStreamState {
        delimiter,
        jcl_terminates,
        bytes: 0,
        definition_span,
    })
}

fn statement_continues(operands: &str) -> bool {
    let trimmed = operands.trim_end();
    if trimmed.ends_with(',') {
        return true;
    }
    let mut quote = None;
    let mut depth = 0usize;
    for byte in trimmed.bytes() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'(' if quote.is_none() => depth += 1,
            b')' if quote.is_none() => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    quote.is_some() || depth > 0
}

fn top_level_operands(value: &str) -> Vec<&str> {
    let bytes = value.as_bytes();
    let mut values = Vec::new();
    let mut start = 0usize;
    let mut quote = None;
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        match byte {
            b'\'' | b'"' if quote == Some(byte) => quote = None,
            b'\'' | b'"' if quote.is_none() => quote = Some(byte),
            b'(' if quote.is_none() => depth += 1,
            b')' if quote.is_none() => depth = depth.saturating_sub(1),
            b',' if quote.is_none() && depth == 0 => {
                values.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    values.push(&value[start..]);
    values
}

fn emit_record_tokens(builder: &mut GreenNodeBuilder<'_>, raw: &str, kind: JclRecordKind) -> usize {
    match kind {
        JclRecordKind::Statement | JclRecordKind::Continuation => {
            emit_statement_tokens(builder, raw)
        }
        JclRecordKind::InStreamData => {
            builder.token(JclSyntaxKind::Data.into(), raw);
            usize::from(!raw.is_empty())
        }
        JclRecordKind::Error => {
            builder.token(JclSyntaxKind::Error.into(), raw);
            usize::from(!raw.is_empty())
        }
        _ => {
            builder.token(JclSyntaxKind::Data.into(), raw);
            usize::from(!raw.is_empty())
        }
    }
}

fn emit_statement_tokens(builder: &mut GreenNodeBuilder<'_>, raw: &str) -> usize {
    let area_end = raw.len().min(72);
    let Some(fields) = statement_fields(raw) else {
        builder.token(JclSyntaxKind::Error.into(), raw);
        return usize::from(!raw.is_empty());
    };
    let mut count = 0usize;
    emit(builder, JclSyntaxKind::Prefix, &raw[..2], &mut count);
    emit(
        builder,
        JclSyntaxKind::Name,
        &raw[fields.name.clone()],
        &mut count,
    );
    emit(
        builder,
        JclSyntaxKind::Whitespace,
        &raw[fields.name.end..fields.operation.start],
        &mut count,
    );
    emit(
        builder,
        JclSyntaxKind::Operation,
        &raw[fields.operation.clone()],
        &mut count,
    );
    emit(
        builder,
        JclSyntaxKind::Whitespace,
        &raw[fields.operation.end..fields.operands.start],
        &mut count,
    );
    emit(
        builder,
        JclSyntaxKind::Operand,
        &raw[fields.operands.start..area_end],
        &mut count,
    );
    emit(
        builder,
        JclSyntaxKind::Sequence,
        &raw[area_end..],
        &mut count,
    );
    count
}

fn emit(builder: &mut GreenNodeBuilder<'_>, kind: JclSyntaxKind, value: &str, count: &mut usize) {
    if !value.is_empty() {
        builder.token(kind.into(), value);
        *count += 1;
    }
}

fn diagnostic(code: &str, message: &str, file: FileId, bytes: Range<usize>) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::new(code).expect("static JCL diagnostic code"),
        Severity::Error,
        Phase::Parse,
        FailureCategory::MalformedInput,
        Completeness::Incomplete,
        message,
        Some(SourceSpan::new(file, bytes).expect("validated physical source span")),
        Redaction::Public,
        DiagnosticLimits::default(),
    )
    .expect("bounded static JCL diagnostic")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rowan::NodeOrToken;

    fn analyze(source: &str) -> JclSyntaxAnalysis {
        analyze_jcl_syntax(
            &JclBundle {
                primary: source.into(),
                ..JclBundle::default()
            },
            JclSyntaxLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn lossless_tree_preserves_exact_records_sequence_columns_and_crlf() {
        let source = "//J JOB CLASS=A                                                         00000001\r\n//S EXEC PGM=IEFBR14                                                    00000002\r\n";
        let analysis = analyze(source);
        assert!(analysis.is_complete());
        assert_eq!(analysis.syntax().text(), source);
        assert_eq!(analysis.syntax().green().to_string(), source);
        assert_eq!(analysis.syntax().records().len(), 2);
        assert_eq!(analysis.syntax().records()[0].span().bytes, 0..82);
        let root = rowan::SyntaxNode::<JclLanguage>::new_root(analysis.syntax().green().clone());
        assert!(root.descendants_with_tokens().any(|element| {
            matches!(element, NodeOrToken::Token(token) if token.kind() == JclSyntaxKind::Sequence && token.text() == "00000001")
        }));
    }

    #[test]
    fn continuation_and_custom_delimited_data_are_classified_without_losing_bytes() {
        let source = "//J JOB CLASS=A\n//S EXEC PGM=IEBGENER,\n//             PARM='A,B'\n//IN DD DATA,DLM=@@\n//THIS IS DATA\n/*ALSO DATA\n@@\n//OUT DD SYSOUT=*\n";
        let analysis = analyze(source);
        assert!(analysis.is_complete());
        assert_eq!(
            analysis
                .syntax()
                .records()
                .iter()
                .map(JclRecord::kind)
                .collect::<Vec<_>>(),
            vec![
                JclRecordKind::Statement,
                JclRecordKind::Statement,
                JclRecordKind::Continuation,
                JclRecordKind::Statement,
                JclRecordKind::InStreamData,
                JclRecordKind::InStreamData,
                JclRecordKind::Delimiter,
                JclRecordKind::Statement,
            ]
        );
        assert_eq!(analysis.syntax().green().to_string(), source);
    }

    #[test]
    fn asterisk_data_stops_before_the_next_jcl_statement() {
        let source = "//J JOB\n//S EXEC PGM=IEBGENER\n//IN DD *\nDATA\n//OUT DD SYSOUT=*\n";
        let analysis = analyze(source);
        assert_eq!(
            analysis.syntax().records()[3].kind(),
            JclRecordKind::InStreamData
        );
        assert_eq!(
            analysis.syntax().records()[4].kind(),
            JclRecordKind::Statement
        );
    }

    #[test]
    fn malformed_records_recover_with_shared_source_diagnostics() {
        let source = "//J JOB\nBROKEN\n//S EXEC PGM=IEFBR14\n";
        let analysis = analyze(source);
        assert!(!analysis.is_complete());
        assert_eq!(analysis.diagnostics().len(), 1);
        assert_eq!(analysis.diagnostics()[0].code().as_str(), "MEJCL0703");
        assert_eq!(analysis.diagnostics()[0].primary().unwrap().bytes, 8..14);
        assert_eq!(
            analysis.syntax().records()[2].kind(),
            JclRecordKind::Statement
        );
    }

    #[test]
    fn missing_inline_delimiter_points_to_the_defining_dd() {
        let analysis = analyze("//J JOB\n//S EXEC PGM=IEBGENER\n//IN DD DATA,DLM=@@\nDATA\n");
        assert_eq!(analysis.diagnostics().len(), 1);
        assert_eq!(analysis.diagnostics()[0].code().as_str(), "MEJCL0704");
        assert_eq!(analysis.diagnostics()[0].primary().unwrap().bytes, 30..49);
    }

    #[test]
    fn source_identity_covers_include_and_procedure_bytes_and_ordered_libraries() {
        let first = analyze_jcl_syntax(
            &JclBundle {
                primary: "//J JOB\n".into(),
                includes: BTreeMap::from([("A".into(), "//*A\n".into())]),
                cataloged_procedures: BTreeMap::from([("P".into(), "//P PROC\n// PEND\n".into())]),
            },
            JclSyntaxLimits::default(),
        )
        .unwrap();
        let second = analyze_jcl_syntax(
            &JclBundle {
                primary: "//J JOB\n".into(),
                includes: BTreeMap::from([("A".into(), "//*CHANGED\n".into())]),
                cataloged_procedures: BTreeMap::from([("P".into(), "//P PROC\n// PEND\n".into())]),
            },
            JclSyntaxLimits::default(),
        )
        .unwrap();
        assert_ne!(first.source().id(), second.source().id());
        assert_eq!(first.source().libraries().len(), 2);
    }

    #[test]
    fn lexical_resources_are_bounded_before_tree_growth() {
        let result = analyze_jcl_syntax(
            &JclBundle {
                primary: "//J JOB\n//S EXEC PGM=IEFBR14\n".into(),
                ..JclBundle::default()
            },
            JclSyntaxLimits {
                max_tokens: 1,
                ..JclSyntaxLimits::default()
            },
        );
        assert!(matches!(result, Err(JclSyntaxProblem::TokenLimitExceeded)));
    }
}
