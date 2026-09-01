use super::{
    DecodedSource, NormalizedSource, SourceOrigin, SourceSpan, SyntaxLimits, SyntaxProblem,
    decode_file, normalize_source, physical_lines, slice_origins, source_spans,
};
use crate::generated::cobol_language::{
    CompilerDirectingKind, CompilerDirectiveGroup, CompilerDirectiveKind,
    compiler_directive_descriptor,
};
use mainframe_env_source::{LibraryProblem, SourceBundle, SourceFile, SourceFormat};
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerDirectingNode {
    pub kind: CompilerDirectingKind,
    pub operands: Vec<String>,
    pub source: Vec<SourceSpan>,
    pub active: bool,
    pub declarative_section: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerDirectiveNode {
    pub kind: CompilerDirectiveKind,
    pub group: CompilerDirectiveGroup,
    pub operands: Vec<String>,
    pub source: Vec<SourceSpan>,
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerOption {
    pub name: String,
    pub value: Option<String>,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerOptionSet {
    ordered: Vec<CompilerOption>,
    effective: BTreeMap<String, Option<String>>,
}

impl CompilerOptionSet {
    #[must_use]
    pub fn ordered(&self) -> &[CompilerOption] {
        &self.ordered
    }

    #[must_use]
    pub fn value(&self, name: &str) -> Option<Option<&str>> {
        self.effective
            .get(&name.to_ascii_uppercase())
            .map(|value| value.as_deref())
    }

    #[must_use]
    pub fn enabled(&self, name: &str) -> bool {
        let name = name.to_ascii_uppercase();
        if self.effective.contains_key(&format!("NO{name}")) {
            return false;
        }
        self.effective.contains_key(&name)
    }

    fn insert(
        &mut self,
        option: CompilerOption,
        limits: SyntaxLimits,
    ) -> Result<(), SyntaxProblem> {
        if self.ordered.len() >= limits.max_directives {
            return Err(SyntaxProblem::DirectiveLimitExceeded);
        }
        if let Some(enabled) = option.name.strip_prefix("NO") {
            self.effective.remove(enabled);
        } else {
            self.effective.remove(&format!("NO{}", option.name));
        }
        self.effective
            .insert(option.name.clone(), option.value.clone());
        self.ordered.push(option);
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct DirectiveArtifacts {
    pub directing: Vec<CompilerDirectingNode>,
    pub directives: Vec<CompilerDirectiveNode>,
    pub options: CompilerOptionSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CompileValue {
    Integer(i128),
    Text(String),
    Boolean(bool),
}

#[derive(Clone, Debug)]
enum ConditionalFrame {
    If {
        parent_active: bool,
        condition: bool,
        in_else: bool,
    },
    Evaluate {
        parent_active: bool,
        subject: Option<CompileValue>,
        matched: bool,
        branch_active: bool,
        other_seen: bool,
    },
}

impl ConditionalFrame {
    fn active(&self) -> bool {
        match self {
            Self::If {
                parent_active,
                condition,
                in_else,
            } => *parent_active && if *in_else { !condition } else { *condition },
            Self::Evaluate {
                parent_active,
                branch_active,
                ..
            } => *parent_active && *branch_active,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceContext {
    BeforeIdentification,
    Identification,
    Environment,
    Data,
    WorkingStorage,
    Procedure,
}

pub(super) struct DirectiveState {
    variables: BTreeMap<String, Option<CompileValue>>,
    parameters: BTreeMap<String, CompileValue>,
    frames: Vec<ConditionalFrame>,
    context: SourceContext,
    lp: u8,
    java_callable_seen: bool,
}

#[derive(Clone, Debug)]
pub(super) struct DirectiveLine {
    pub range: std::ops::Range<usize>,
    pub content: std::ops::Range<usize>,
    pub leading: usize,
}

pub(super) struct DirectiveCursor {
    lines: Vec<DirectiveLine>,
    position: usize,
}

impl DirectiveCursor {
    pub(super) fn new(source: &NormalizedSource) -> Self {
        let lines = physical_lines(&source.text)
            .into_iter()
            .filter_map(|line| {
                let text = &source.text[line.content.clone()];
                text.trim_start().starts_with(">>").then(|| DirectiveLine {
                    range: line.content.start..line.terminator.end,
                    content: line.content,
                    leading: text.len() - text.trim_start().len(),
                })
            })
            .collect();
        Self { lines, position: 0 }
    }

    pub(super) fn next_from(&mut self, from: usize) -> Option<DirectiveLine> {
        while self
            .lines
            .get(self.position)
            .is_some_and(|line| line.content.start < from)
        {
            self.position += 1;
        }
        self.lines.get(self.position).cloned()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectiveCompilerOptions {
    lp: u8,
    arithmetic_mode: EffectiveArithmeticMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveArithmeticMode {
    Compatible,
    Extended,
}

impl EffectiveArithmeticMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Extended => "extended",
        }
    }
}

impl EffectiveCompilerOptions {
    pub(super) const DEFAULT: Self = Self {
        lp: 32,
        arithmetic_mode: EffectiveArithmeticMode::Extended,
    };

    pub(super) fn resolve(
        bundle: &SourceBundle,
        options: &CompilerOptionSet,
    ) -> Result<Self, SyntaxProblem> {
        let source_lp = options
            .value("LP")
            .map(|value| {
                value
                    .and_then(lp_value)
                    .ok_or(SyntaxProblem::InvalidCompilerOption)
            })
            .transpose()?;
        let bundle_lp = bundle
            .options()
            .get("cobol.lp")
            .map(|value| lp_value(value).ok_or(SyntaxProblem::InvalidCompilerOption))
            .transpose()?;
        if source_lp
            .zip(bundle_lp)
            .is_some_and(|(source, bundle)| source != bundle)
        {
            return Err(SyntaxProblem::ConflictingCompilerOption("LP".into()));
        }
        let source_arithmetic = options
            .value("ARITH")
            .map(|value| {
                value
                    .and_then(arithmetic_mode_value)
                    .ok_or(SyntaxProblem::InvalidCompilerOption)
            })
            .transpose()?;
        let bundle_arithmetic = bundle
            .options()
            .get("cobol.arith")
            .map(|value| arithmetic_mode_value(value).ok_or(SyntaxProblem::InvalidCompilerOption))
            .transpose()?;
        if source_arithmetic
            .zip(bundle_arithmetic)
            .is_some_and(|(source, bundle)| source != bundle)
        {
            return Err(SyntaxProblem::ConflictingCompilerOption("ARITH".into()));
        }
        Ok(Self {
            lp: source_lp.or(bundle_lp).unwrap_or(32),
            arithmetic_mode: source_arithmetic
                .or(bundle_arithmetic)
                .unwrap_or(EffectiveArithmeticMode::Extended),
        })
    }

    #[must_use]
    pub const fn lp(self) -> u8 {
        self.lp
    }

    #[must_use]
    pub const fn pointer_bytes(self) -> usize {
        if self.lp == 64 { 8 } else { 4 }
    }

    #[must_use]
    pub const fn arithmetic_mode(self) -> EffectiveArithmeticMode {
        self.arithmetic_mode
    }
}

impl DirectiveState {
    pub(super) fn new(
        bundle: &SourceBundle,
        options: &CompilerOptionSet,
        effective: EffectiveCompilerOptions,
    ) -> Self {
        let mut state = Self {
            variables: BTreeMap::new(),
            parameters: BTreeMap::new(),
            frames: Vec::new(),
            context: SourceContext::BeforeIdentification,
            lp: effective.lp(),
            java_callable_seen: false,
        };
        state.install_predefined(bundle, options);
        state.install_parameters(bundle);
        state
    }

    fn install_predefined(&mut self, bundle: &SourceBundle, options: &CompilerOptionSet) {
        let integers = [
            (
                "IGY-ARCH",
                option_integer(bundle, options, "ARCH").unwrap_or(10),
            ),
            ("IGY-COMPILER-VRM", 60_500),
            ("IGY-LP", i128::from(self.lp)),
            (
                "IGY-OPTIMIZE",
                option_integer(bundle, options, "OPTIMIZE").unwrap_or(0),
            ),
        ];
        for (name, value) in integers {
            self.variables
                .insert(name.into(), Some(CompileValue::Integer(value)));
        }
        for (name, enabled) in [
            ("IGY-CICS", option_enabled(bundle, options, "CICS")),
            ("IGY-DLL", option_enabled(bundle, options, "DLL")),
            ("IGY-DYNAM", option_enabled(bundle, options, "DYNAM")),
            (
                "IGY-JAVAIOP-JAVA64",
                option_value(bundle, options, "JAVAIOP")
                    .is_some_and(|value| value.to_ascii_uppercase().contains("JAVA64")),
            ),
            ("IGY-SQL", option_enabled(bundle, options, "SQL")),
            ("IGY-SQLIMS", option_enabled(bundle, options, "SQLIMS")),
            ("IGY-THREAD", option_enabled(bundle, options, "THREAD")),
        ] {
            self.variables
                .insert(name.into(), Some(CompileValue::Boolean(enabled)));
        }
        for name in [
            "ARCH",
            "COMPILER-VRM",
            "LP",
            "OPTIMIZE",
            "CICS",
            "DLL",
            "DYNAM",
            "JAVAIOP-JAVA64",
            "SQL",
            "SQLIMS",
            "THREAD",
        ] {
            if let Some(value) = self.variables.get(&format!("IGY-{name}")).cloned() {
                self.variables.insert(name.into(), value);
            }
        }
    }

    fn install_parameters(&mut self, bundle: &SourceBundle) {
        for (key, value) in bundle.options() {
            let Some(name) = key.strip_prefix("cobol.define.") else {
                continue;
            };
            let name = name.to_ascii_uppercase();
            if let Ok(value) = atom(value, &BTreeMap::new()) {
                self.parameters.insert(name, value);
            }
        }
    }

    pub(super) fn active(&self) -> bool {
        self.frames.last().is_none_or(ConditionalFrame::active)
    }

    pub(super) fn depth(&self) -> usize {
        self.frames.len()
    }

    pub(super) fn observe_text(&mut self, text: &str) {
        observe_source_context(text, &mut self.context, None);
    }
}

pub(super) fn process_directive_line(
    source: &NormalizedSource,
    line: &DirectiveLine,
    format: SourceFormat,
    state: &mut DirectiveState,
    artifacts: &mut DirectiveArtifacts,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    process_directive(
        source.text[line.content.clone()].trim_start(),
        line.leading,
        format,
        state,
        artifacts,
        source_spans(&source.origins, line.content.clone()),
        limits,
    )
}

#[derive(Clone, Debug)]
enum BasisChange {
    Delete {
        ranges: Vec<(u32, u32)>,
        records: Vec<usize>,
    },
    Insert {
        target: u32,
        records: Vec<usize>,
    },
}

impl BasisChange {
    fn records_mut(&mut self) -> &mut Vec<usize> {
        match self {
            Self::Delete { records, .. } | Self::Insert { records, .. } => records,
        }
    }

    fn records(&self) -> &[usize] {
        match self {
            Self::Delete { records, .. } | Self::Insert { records, .. } => records,
        }
    }

    fn first_reference(&self) -> u32 {
        match self {
            Self::Delete { ranges, .. } => ranges[0].0,
            Self::Insert { target, .. } => *target,
        }
    }

    fn last_reference(&self) -> u32 {
        match self {
            Self::Delete { ranges, .. } => ranges.last().expect("nonempty ranges").1,
            Self::Insert { target, .. } => *target,
        }
    }
}

/// Applies the extended source-library deck before ordinary reference-format
/// normalization. Every emitted byte is remapped to either the BASIS member or
/// the modifying primary source.
pub(super) fn apply_basis(
    primary_text: &DecodedSource,
    primary: &SourceFile,
    bundle: &SourceBundle,
    debugging: bool,
    artifacts: &mut DirectiveArtifacts,
    limits: SyntaxLimits,
) -> Result<Option<NormalizedSource>, SyntaxProblem> {
    let primary_source = &primary_text.text;
    let primary_lines = physical_lines(primary_source);
    let Some(basis_line_index) = primary_lines
        .iter()
        .position(|line| !primary_source[line.content.clone()].trim().is_empty())
    else {
        return Ok(None);
    };
    let basis_line = &primary_lines[basis_line_index];
    let basis_body = extended_body(primary_source, basis_line, primary.format());
    if !basis_body
        .body
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("BASIS"))
    {
        return Ok(None);
    }
    let basis_words = directive_words(basis_body.body)?;
    if basis_words.len() != 2 || !basis_words[0].eq_ignore_ascii_case("BASIS") {
        return Err(SyntaxProblem::InvalidBasis);
    }
    validate_directing_name(&basis_words[1], 128).map_err(|_| SyntaxProblem::InvalidBasis)?;
    let basis_name = basis_words[1].trim_matches(['\'', '"']).to_string();
    let basis = bundle
        .resolve_library_member(&basis_name)
        .map_err(|problem| match problem {
            LibraryProblem::MissingMember(_) => SyntaxProblem::BasisNotFound(basis_name.clone()),
            LibraryProblem::AmbiguousMember { .. } => {
                SyntaxProblem::AmbiguousBasis(basis_name.clone())
            }
            _ => SyntaxProblem::InvalidBasis,
        })?;
    if basis.format() != primary.format() {
        return Err(SyntaxProblem::InvalidBasis);
    }
    record_raw_directing(
        artifacts,
        CompilerDirectingKind::Basis,
        vec![basis_name],
        primary_text,
        primary,
        basis_line.content.clone(),
        limits,
    )?;

    let mut changes = Vec::<BasisChange>::new();
    for (index, line) in primary_lines.iter().enumerate().skip(basis_line_index + 1) {
        let extended = extended_body(primary_source, line, primary.format());
        if extended.body.is_empty() {
            if let Some(change) = changes.last_mut() {
                change.records_mut().push(index);
            }
            continue;
        }
        match parse_basis_change(extended.body) {
            Ok(Some(change)) => {
                let (kind, operands) = match &change {
                    BasisChange::Delete { ranges, .. } => (
                        CompilerDirectingKind::Delete,
                        ranges
                            .iter()
                            .map(|(start, end)| {
                                if start == end {
                                    format!("{start:06}")
                                } else {
                                    format!("{start:06}-{end:06}")
                                }
                            })
                            .collect(),
                    ),
                    BasisChange::Insert { target, .. } => {
                        (CompilerDirectingKind::Insert, vec![format!("{target:06}")])
                    }
                };
                record_raw_directing(
                    artifacts,
                    kind,
                    operands,
                    primary_text,
                    primary,
                    line.content.clone(),
                    limits,
                )?;
                changes.push(change);
            }
            Ok(None) => {
                if let Some(change) = changes.last_mut() {
                    change.records_mut().push(index);
                } else {
                    return Err(SyntaxProblem::InvalidBasis);
                }
            }
            Err(problem) => {
                // A DELETE in Area B with non-sequence operands is the COBOL
                // DELETE statement, not an extended source-library statement.
                if extended.column >= 12
                    && extended
                        .body
                        .split_whitespace()
                        .next()
                        .is_some_and(|word| word.eq_ignore_ascii_case("DELETE"))
                {
                    if let Some(change) = changes.last_mut() {
                        change.records_mut().push(index);
                    } else {
                        return Err(SyntaxProblem::InvalidBasis);
                    }
                } else {
                    return Err(problem);
                }
            }
        }
    }
    validate_change_order(&changes)?;
    for change in &changes {
        if matches!(change, BasisChange::Insert { .. })
            && !change.records().iter().any(|index| {
                !extended_body(primary_source, &primary_lines[*index], primary.format())
                    .body
                    .is_empty()
            })
        {
            return Err(SyntaxProblem::InvalidInsert);
        }
    }

    let basis_text = decode_file(basis, limits)?;
    let basis_lines = physical_lines(&basis_text.text);
    let mut sequences = Vec::with_capacity(basis_lines.len());
    if !changes.is_empty() {
        let mut previous = None;
        for line in &basis_lines {
            let sequence = basis_sequence(&basis_text.text[line.content.clone()])
                .ok_or(SyntaxProblem::InvalidBasisSequence)?;
            if previous.is_some_and(|value| value >= sequence) {
                return Err(SyntaxProblem::InvalidBasisSequence);
            }
            previous = Some(sequence);
            sequences.push(sequence);
        }
    }

    let positions = sequences
        .iter()
        .enumerate()
        .map(|(index, sequence)| (*sequence, index))
        .collect::<BTreeMap<_, _>>();
    let mut deleted = vec![false; basis_lines.len()];
    let mut insertions = BTreeMap::<usize, Vec<usize>>::new();
    for change in &changes {
        match change {
            BasisChange::Delete { ranges, records } => {
                for (start, end) in ranges {
                    if !positions.contains_key(start) || !positions.contains_key(end) {
                        return Err(SyntaxProblem::InvalidDelete);
                    }
                    for (index, sequence) in sequences.iter().enumerate() {
                        if (*start..=*end).contains(sequence) {
                            deleted[index] = true;
                        }
                    }
                }
                let anchor = positions[&ranges.last().expect("nonempty ranges").1];
                insertions
                    .entry(anchor)
                    .or_default()
                    .extend(records.iter().copied());
            }
            BasisChange::Insert { target, records } => {
                let anchor = *positions.get(target).ok_or(SyntaxProblem::InvalidInsert)?;
                insertions
                    .entry(anchor)
                    .or_default()
                    .extend(records.iter().copied());
            }
        }
    }

    let mut combined = NormalizedSource::default();
    for (index, line) in basis_lines.iter().enumerate() {
        if !deleted[index] {
            append_raw_record(&mut combined, &basis_text, basis, line);
        }
        if let Some(records) = insertions.get(&index) {
            for record in records {
                append_raw_record(
                    &mut combined,
                    primary_text,
                    primary,
                    &primary_lines[*record],
                );
            }
        }
    }
    if combined.text.len() > limits.max_expanded_bytes {
        return Err(SyntaxProblem::SourceLimitExceeded);
    }
    let normalized = normalize_source(
        &combined.text,
        primary.path(),
        primary.format(),
        debugging,
        limits,
    )?;
    Ok(Some(remap_basis_origins(normalized, &combined)?))
}

struct ExtendedBody<'a> {
    body: &'a str,
    column: usize,
}

fn extended_body<'a>(
    source: &'a str,
    line: &super::PhysicalLine,
    format: SourceFormat,
) -> ExtendedBody<'a> {
    let physical = &source[line.content.clone()];
    let end = if format == SourceFormat::Free {
        physical.len()
    } else {
        physical.len().min(72)
    };
    if !physical.is_char_boundary(end) {
        return ExtendedBody {
            body: "",
            column: 1,
        };
    }
    let bounded = &physical[..end];
    let bytes = bounded.as_bytes();
    let start = if bytes.len() >= 7
        && bytes[..6].iter().all(u8::is_ascii_digit)
        && bytes[6].is_ascii_whitespace()
    {
        7
    } else {
        bounded.len() - bounded.trim_start().len()
    };
    if !bounded.is_char_boundary(start) {
        return ExtendedBody {
            body: "",
            column: 1,
        };
    }
    let body = bounded[start..].trim();
    let leading = bounded[start..].len() - bounded[start..].trim_start().len();
    ExtendedBody {
        body,
        column: start + leading + 1,
    }
}

fn parse_basis_change(body: &str) -> Result<Option<BasisChange>, SyntaxProblem> {
    let Some((head, operands)) = body.split_once(char::is_whitespace) else {
        return if body.eq_ignore_ascii_case("DELETE") {
            Err(SyntaxProblem::InvalidDelete)
        } else if body.eq_ignore_ascii_case("INSERT") {
            Err(SyntaxProblem::InvalidInsert)
        } else {
            Ok(None)
        };
    };
    if head.eq_ignore_ascii_case("DELETE") {
        let ranges = parse_sequence_ranges(operands.trim())?;
        return Ok(Some(BasisChange::Delete {
            ranges,
            records: Vec::new(),
        }));
    }
    if head.eq_ignore_ascii_case("INSERT") {
        let operand = operands.trim();
        if !is_sequence_number(operand) {
            return Err(SyntaxProblem::InvalidInsert);
        }
        return Ok(Some(BasisChange::Insert {
            target: operand.parse().map_err(|_| SyntaxProblem::InvalidInsert)?,
            records: Vec::new(),
        }));
    }
    Ok(None)
}

fn parse_sequence_ranges(operands: &str) -> Result<Vec<(u32, u32)>, SyntaxProblem> {
    if operands.is_empty() || operands.contains(',') && !operands.contains(", ") {
        return Err(SyntaxProblem::InvalidDelete);
    }
    let mut ranges = Vec::new();
    let mut previous = None;
    for entry in operands.split(", ") {
        if entry.contains(',') || entry.matches('-').count() > 1 {
            return Err(SyntaxProblem::InvalidDelete);
        }
        let (start, end) = entry.split_once('-').unwrap_or((entry, entry));
        if !is_sequence_number(start) || !is_sequence_number(end) {
            return Err(SyntaxProblem::InvalidDelete);
        }
        let start = start
            .parse::<u32>()
            .map_err(|_| SyntaxProblem::InvalidDelete)?;
        let end = end
            .parse::<u32>()
            .map_err(|_| SyntaxProblem::InvalidDelete)?;
        if start > end || previous.is_some_and(|value| value >= start) {
            return Err(SyntaxProblem::InvalidDelete);
        }
        previous = Some(end);
        ranges.push((start, end));
    }
    if ranges.is_empty() {
        Err(SyntaxProblem::InvalidDelete)
    } else {
        Ok(ranges)
    }
}

fn validate_change_order(changes: &[BasisChange]) -> Result<(), SyntaxProblem> {
    let mut previous = None;
    for change in changes {
        if previous.is_some_and(|value| value >= change.first_reference()) {
            return Err(SyntaxProblem::InvalidBasisSequence);
        }
        previous = Some(change.last_reference());
    }
    Ok(())
}

fn basis_sequence(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    if bytes.len() < 6 || !bytes[..6].iter().all(u8::is_ascii_digit) {
        return None;
    }
    line[..6].parse().ok()
}

fn append_raw_record(
    output: &mut NormalizedSource,
    source: &DecodedSource,
    file: &SourceFile,
    line: &super::PhysicalLine,
) {
    let range = line.content.start..line.terminator.end;
    let output_start = output.text.len();
    output.text.push_str(&source.text[range.clone()]);
    output.origins.extend(slice_origins(
        &source.lossless_origins(file.path()),
        range,
        output_start,
    ));
    if line.terminator.start == line.terminator.end {
        output.text.push('\n');
    }
}

fn remap_basis_origins(
    mut normalized: NormalizedSource,
    combined: &NormalizedSource,
) -> Result<NormalizedSource, SyntaxProblem> {
    let mut remapped = Vec::with_capacity(normalized.origins.len());
    for origin in &normalized.origins {
        let source_range = origin.source_start..origin.source_end;
        if origin.output_end - origin.output_start == source_range.end - source_range.start {
            remapped.extend(slice_origins(
                &combined.origins,
                source_range,
                origin.output_start,
            ));
            continue;
        }
        let mapped = combined
            .origins
            .iter()
            .filter(|raw| {
                raw.output_start < source_range.end && raw.output_end > source_range.start
            })
            .collect::<Vec<_>>();
        let (Some(first), Some(last)) = (mapped.first(), mapped.last()) else {
            return Err(SyntaxProblem::InvalidBasis);
        };
        if first.source != last.source {
            return Err(SyntaxProblem::InvalidBasis);
        }
        remapped.push(SourceOrigin {
            output_start: origin.output_start,
            output_end: origin.output_end,
            source: first.source.clone(),
            source_start: first.source_start,
            source_end: last.source_end,
        });
    }
    normalized.origins = remapped;
    Ok(normalized)
}

fn record_raw_directing(
    artifacts: &mut DirectiveArtifacts,
    kind: CompilerDirectingKind,
    operands: Vec<String>,
    decoded: &DecodedSource,
    file: &SourceFile,
    range: Range<usize>,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    push_directing(
        artifacts,
        kind,
        operands,
        vec![SourceSpan {
            source: file.path().clone(),
            source_start: decoded.input_offset(range.start)?,
            source_end: decoded.input_offset(range.end)?,
        }],
        true,
        None,
        limits,
    )
}

fn option_value<'a>(
    bundle: &'a SourceBundle,
    options: &'a CompilerOptionSet,
    name: &str,
) -> Option<&'a str> {
    options.value(name).flatten().or_else(|| {
        bundle
            .options()
            .get(&format!("cobol.{}", name.to_ascii_lowercase()))
            .map(String::as_str)
    })
}

fn option_integer(bundle: &SourceBundle, options: &CompilerOptionSet, name: &str) -> Option<i128> {
    option_value(bundle, options, name).and_then(option_integer_value)
}

fn option_integer_value(value: &str) -> Option<i128> {
    value
        .trim_matches(['(', ')'])
        .split(',')
        .next()
        .and_then(|value| value.parse().ok())
}

fn lp_value(value: &str) -> Option<u8> {
    let trimmed = value.trim();
    let value = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        trimmed.get(1..trimmed.len().checked_sub(1)?)?.trim()
    } else {
        trimmed
    };
    match value {
        "32" => Some(32),
        "64" => Some(64),
        _ => None,
    }
}

fn arithmetic_mode_value(value: &str) -> Option<EffectiveArithmeticMode> {
    let trimmed = value.trim();
    let value = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        trimmed.get(1..trimmed.len().checked_sub(1)?)?.trim()
    } else {
        trimmed
    };
    match value.to_ascii_uppercase().as_str() {
        "COMPAT" | "COMPATIBLE" => Some(EffectiveArithmeticMode::Compatible),
        "EXTEND" | "EXTENDED" => Some(EffectiveArithmeticMode::Extended),
        _ => None,
    }
}

fn option_enabled(bundle: &SourceBundle, options: &CompilerOptionSet, name: &str) -> bool {
    options.enabled(name)
        || bundle
            .options()
            .get(&format!("cobol.{}", name.to_ascii_lowercase()))
            .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "true" | "on" | "1"))
}

pub(super) fn prepare_source(
    source: &NormalizedSource,
    primary: bool,
    artifacts: &mut DirectiveArtifacts,
    limits: SyntaxLimits,
) -> Result<NormalizedSource, SyntaxProblem> {
    let mut output = NormalizedSource::default();
    let mut control_prefix = primary;
    let mut context = SourceContext::BeforeIdentification;
    let mut in_declaratives = false;
    let mut declarative_section = None;
    for line in physical_lines(&source.text) {
        let whole = line.content.start..line.terminator.end;
        let text = &source.text[line.content.clone()];
        let trimmed = text.trim();
        let upper = trimmed.trim_end_matches('.').to_ascii_uppercase();
        let head = upper.split_whitespace().next().unwrap_or("");
        if primary && !control_prefix && starts_control_option(&upper) {
            return Err(SyntaxProblem::InvalidDirectiveContext);
        }
        if primary && head == "BASIS" {
            return Err(SyntaxProblem::InvalidDirectiveContext);
        }
        if matches!(upper.as_str(), "DECLARATIVES" | "END DECLARATIVES") {
            observe_source_context(
                &source.text[line.content.clone()],
                &mut context,
                Some(&mut in_declaratives),
            );
            if upper == "END DECLARATIVES" {
                declarative_section = None;
                let at = upper
                    .find("DECLARATIVES")
                    .ok_or(SyntaxProblem::InvalidDirectiveContext)?;
                let leading = text.len().saturating_sub(text.trim_start().len());
                append_slice(
                    &mut output,
                    source,
                    line.content.start + leading + at..whole.end,
                );
            } else {
                append_newline(&mut output, source, &line);
            }
            continue;
        }
        let kind = if primary && control_prefix && starts_control_option(&upper) {
            Some(CompilerDirectingKind::Process)
        } else if upper.starts_with("*CONTROL") || upper.starts_with("*CBL") {
            Some(CompilerDirectingKind::Control)
        } else if head == "EJECT" {
            Some(CompilerDirectingKind::Eject)
        } else if matches!(head, "SKIP1" | "SKIP2" | "SKIP3") {
            Some(CompilerDirectingKind::Skip)
        } else if upper.starts_with("TITLE ") {
            Some(CompilerDirectingKind::Title)
        } else if upper.starts_with("ENTER ") {
            Some(CompilerDirectingKind::Enter)
        } else if matches!(head, "READY" | "RESET") {
            Some(CompilerDirectingKind::Trace)
        } else if upper.starts_with("SERVICE LABEL") {
            Some(CompilerDirectingKind::ServiceLabel)
        } else if upper.starts_with("SERVICE RELOAD ") {
            Some(CompilerDirectingKind::ServiceReload)
        } else if upper.starts_with("USE ") {
            Some(CompilerDirectingKind::Use)
        } else {
            None
        };
        if let Some(kind) = kind {
            let spans = source_spans(&source.origins, line.content.clone());
            let operands = directing_operands(kind, trimmed)?;
            validate_directing_context(kind, context, in_declaratives)?;
            if kind == CompilerDirectingKind::Process {
                parse_options(trimmed, &spans, &mut artifacts.options, limits)?;
            } else {
                control_prefix = false;
            }
            push_directing(
                artifacts,
                kind,
                operands,
                spans,
                true,
                (kind == CompilerDirectingKind::Use)
                    .then(|| declarative_section.clone())
                    .flatten(),
                limits,
            )?;
            if matches!(
                kind,
                CompilerDirectingKind::Process
                    | CompilerDirectingKind::Control
                    | CompilerDirectingKind::Eject
                    | CompilerDirectingKind::Skip
                    | CompilerDirectingKind::Title
                    | CompilerDirectingKind::Use
            ) {
                append_newline(&mut output, source, &line);
            } else {
                append_slice(&mut output, source, whole);
            }
            continue;
        }
        if control_prefix {
            control_prefix = false;
        }
        observe_source_context(
            &source.text[line.content.clone()],
            &mut context,
            Some(&mut in_declaratives),
        );
        let words = upper.split_whitespace().collect::<Vec<_>>();
        if words.as_slice() == ["END", "DECLARATIVES"] {
            declarative_section = None;
        } else if in_declaratives && words.len() == 2 && words[1] == "SECTION" {
            declarative_section = Some(words[0].to_string());
        }
        append_slice(&mut output, source, whole);
    }
    Ok(output)
}

fn starts_control_option(upper: &str) -> bool {
    upper
        .split_whitespace()
        .next()
        .is_some_and(|word| matches!(word, "PROCESS" | "CBL"))
}

fn observe_source_context(
    text: &str,
    context: &mut SourceContext,
    mut in_declaratives: Option<&mut bool>,
) {
    let words = source_words(text);
    for index in 0..words.len() {
        let pair = words.get(index..index + 2);
        match pair {
            Some([first, second]) if first == "IDENTIFICATION" && second == "DIVISION" => {
                *context = SourceContext::Identification;
            }
            Some([first, second]) if first == "ENVIRONMENT" && second == "DIVISION" => {
                *context = SourceContext::Environment;
            }
            Some([first, second]) if first == "DATA" && second == "DIVISION" => {
                *context = SourceContext::Data;
            }
            Some([first, second]) if first == "WORKING-STORAGE" && second == "SECTION" => {
                *context = SourceContext::WorkingStorage;
            }
            Some([first, second]) if first == "PROCEDURE" && second == "DIVISION" => {
                *context = SourceContext::Procedure;
            }
            _ => {}
        }
        if let Some(value) = in_declaratives.as_deref_mut()
            && words.get(index).is_some_and(|word| word == "DECLARATIVES")
        {
            *value = index == 0 || words.get(index - 1).is_none_or(|word| word != "END");
        }
    }
}

fn source_words(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut words = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if text[cursor..].starts_with("*>") {
            cursor = text[cursor..]
                .find('\n')
                .map_or(bytes.len(), |offset| cursor + offset + 1);
            continue;
        }
        if matches!(bytes[cursor], b'\'' | b'"') {
            let quote = bytes[cursor];
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == quote {
                    cursor += 1;
                    if bytes.get(cursor) == Some(&quote) {
                        cursor += 1;
                        continue;
                    }
                    break;
                }
                cursor += 1;
            }
            continue;
        }
        if bytes[cursor].is_ascii_alphabetic() {
            let start = cursor;
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            {
                cursor += 1;
            }
            words.push(text[start..cursor].to_ascii_uppercase());
        } else {
            cursor += text[cursor..].chars().next().map_or(1, char::len_utf8);
        }
    }
    words
}

pub(super) fn is_noncontinuable_directing(text: &str) -> bool {
    let upper = text.trim_start().to_ascii_uppercase();
    let head = upper.split_whitespace().next().unwrap_or("");
    matches!(
        head,
        "PROCESS" | "CBL" | "*CONTROL" | "*CBL" | "EJECT" | "SKIP1" | "SKIP2" | "SKIP3" | "TITLE"
    )
}

fn validate_directing_context(
    kind: CompilerDirectingKind,
    context: SourceContext,
    in_declaratives: bool,
) -> Result<(), SyntaxProblem> {
    let valid = match kind {
        CompilerDirectingKind::Enter
        | CompilerDirectingKind::Trace
        | CompilerDirectingKind::ServiceReload => context == SourceContext::Procedure,
        CompilerDirectingKind::ServiceLabel => {
            context == SourceContext::Procedure && !in_declaratives
        }
        CompilerDirectingKind::Use => context == SourceContext::Procedure && in_declaratives,
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(SyntaxProblem::InvalidDirectiveContext)
    }
}

fn parse_options(
    statement: &str,
    source: &[SourceSpan],
    options: &mut CompilerOptionSet,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    let initial_count = options.ordered.len();
    let body = statement
        .trim()
        .trim_end_matches('.')
        .split_once(char::is_whitespace)
        .map(|(_, body)| body)
        .ok_or(SyntaxProblem::InvalidCompilerOption)?;
    validate_option_separators(body)?;
    let mut cursor = 0usize;
    let bytes = body.as_bytes();
    while cursor < bytes.len() {
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        let start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            cursor += 1;
        }
        if start == cursor {
            return Err(SyntaxProblem::InvalidCompilerOption);
        }
        let name = body[start..cursor].to_ascii_uppercase();
        let value = if bytes.get(cursor) == Some(&b'(') {
            let value_start = cursor;
            let mut depth = 0usize;
            let mut quote = None;
            while cursor < bytes.len() {
                let byte = bytes[cursor];
                if matches!(byte, b'\'' | b'"') {
                    if quote == Some(byte) {
                        quote = None;
                    } else if quote.is_none() {
                        quote = Some(byte);
                    }
                } else if quote.is_none() && byte == b'(' {
                    depth += 1;
                } else if quote.is_none() && byte == b')' {
                    depth = depth
                        .checked_sub(1)
                        .ok_or(SyntaxProblem::InvalidCompilerOption)?;
                    if depth == 0 {
                        cursor += 1;
                        break;
                    }
                }
                cursor += 1;
            }
            if depth != 0 || quote.is_some() {
                return Err(SyntaxProblem::InvalidCompilerOption);
            }
            Some(body[value_start..cursor].to_string())
        } else {
            None
        };
        options.insert(
            CompilerOption {
                name,
                value,
                source: source.to_vec(),
            },
            limits,
        )?;
    }
    if options.ordered.len() == initial_count {
        return Err(SyntaxProblem::InvalidCompilerOption);
    }
    Ok(())
}

fn validate_option_separators(body: &str) -> Result<(), SyntaxProblem> {
    let bytes = body.as_bytes();
    let mut depth = 0usize;
    let mut quote = None;
    let mut top_level_comma = false;
    let mut saw_token = false;
    for byte in bytes {
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(*byte) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(*byte);
            }
        } else if quote.is_none() && *byte == b'(' {
            depth += 1;
            saw_token = true;
        } else if quote.is_none() && *byte == b')' {
            depth = depth.saturating_sub(1);
            saw_token = true;
        } else if quote.is_none() && depth == 0 && *byte == b',' {
            if !saw_token || top_level_comma {
                return Err(SyntaxProblem::InvalidCompilerOption);
            }
            top_level_comma = true;
            saw_token = false;
        } else if !byte.is_ascii_whitespace() {
            saw_token = true;
            top_level_comma = false;
        }
    }
    if top_level_comma {
        Err(SyntaxProblem::InvalidCompilerOption)
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn process_directive(
    line: &str,
    leading: usize,
    format: SourceFormat,
    state: &mut DirectiveState,
    artifacts: &mut DirectiveArtifacts,
    source: Vec<SourceSpan>,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    if line.contains('\n') || line.contains('\r') {
        return Err(SyntaxProblem::InvalidCompilerDirective);
    }
    let body = strip_inline_comment(line[2..].trim())?;
    let mut words = directive_words(body)?;
    let head = words
        .first()
        .ok_or(SyntaxProblem::InvalidCompilerDirective)?
        .to_ascii_uppercase();
    let (kind, consumed) = match head.as_str() {
        "CALLINTERFACE" => (CompilerDirectiveKind::Callinterface, 1),
        "CALLINT" => (CompilerDirectiveKind::Callint, 1),
        "DATA" => (CompilerDirectiveKind::Data, 1),
        "INLINE" => (CompilerDirectiveKind::Inline, 1),
        "DEFINE" => (CompilerDirectiveKind::Define, 1),
        "EVALUATE" => (CompilerDirectiveKind::Evaluate, 1),
        "WHEN"
            if words
                .get(1)
                .is_some_and(|word| word.eq_ignore_ascii_case("OTHER")) =>
        {
            (CompilerDirectiveKind::WhenOther, 2)
        }
        "WHEN" => (CompilerDirectiveKind::When, 1),
        "END-EVALUATE" => (CompilerDirectiveKind::EndEvaluate, 1),
        "IF" => (CompilerDirectiveKind::If, 1),
        "ELSE" => (CompilerDirectiveKind::Else, 1),
        "END-IF" => (CompilerDirectiveKind::EndIf, 1),
        "JAVA-CALLABLE" => (CompilerDirectiveKind::JavaCallable, 1),
        "JAVA-SHAREABLE" => (CompilerDirectiveKind::JavaShareable, 1),
        _ => return Err(SyntaxProblem::UnknownCompilerDirective(head)),
    };
    let descriptor = compiler_directive_descriptor(kind);
    if artifacts.directives.len() >= limits.max_directives {
        return Err(SyntaxProblem::DirectiveLimitExceeded);
    }
    let was_active = state.active();
    validate_directive_area(kind, leading, format)?;
    let operands = words.split_off(consumed);
    let conditional = matches!(descriptor.group, CompilerDirectiveGroup::Conditional);
    if !was_active && !conditional {
        artifacts.directives.push(CompilerDirectiveNode {
            kind,
            group: descriptor.group,
            operands,
            source,
            active: false,
        });
        return Ok(());
    }
    match kind {
        CompilerDirectiveKind::Define => {
            if was_active {
                apply_define(&operands, state, limits)?;
            }
        }
        CompilerDirectiveKind::If => {
            if state.frames.len() >= limits.max_directive_nesting {
                return Err(SyntaxProblem::DirectiveNestingExceeded);
            }
            let parent_active = was_active;
            let condition = if parent_active {
                evaluate_condition(&operands, &state.variables)?
            } else {
                false
            };
            state.frames.push(ConditionalFrame::If {
                parent_active,
                condition,
                in_else: false,
            });
        }
        CompilerDirectiveKind::Else => {
            if !operands.is_empty() {
                return Err(SyntaxProblem::InvalidCompilerDirective);
            }
            let Some(ConditionalFrame::If { in_else, .. }) = state.frames.last_mut() else {
                return Err(SyntaxProblem::UnmatchedCompilerDirective);
            };
            if *in_else {
                return Err(SyntaxProblem::DuplicateCompilerBranch);
            }
            *in_else = true;
        }
        CompilerDirectiveKind::EndIf => {
            if !operands.is_empty()
                || !matches!(state.frames.last(), Some(ConditionalFrame::If { .. }))
            {
                return Err(SyntaxProblem::UnmatchedCompilerDirective);
            }
            state.frames.pop();
        }
        CompilerDirectiveKind::Evaluate => {
            if state.frames.len() >= limits.max_directive_nesting || operands.is_empty() {
                return Err(SyntaxProblem::InvalidCompilerDirective);
            }
            let parent_active = was_active;
            let subject = if operands.len() == 1 && operands[0].eq_ignore_ascii_case("TRUE") {
                None
            } else if parent_active {
                Some(evaluate_value(&operands, &state.variables)?)
            } else {
                Some(CompileValue::Boolean(false))
            };
            state.frames.push(ConditionalFrame::Evaluate {
                parent_active,
                subject,
                matched: false,
                branch_active: false,
                other_seen: false,
            });
        }
        CompilerDirectiveKind::When | CompilerDirectiveKind::WhenOther => {
            apply_when(kind, &operands, state)?;
        }
        CompilerDirectiveKind::EndEvaluate => {
            if !operands.is_empty()
                || !matches!(state.frames.last(), Some(ConditionalFrame::Evaluate { .. }))
            {
                return Err(SyntaxProblem::UnmatchedCompilerDirective);
            }
            state.frames.pop();
        }
        CompilerDirectiveKind::Callinterface | CompilerDirectiveKind::Callint => {
            if state.context != SourceContext::Procedure
                || operands.len() > 1
                || operands.first().is_some_and(|value| {
                    !matches!(
                        value.to_ascii_uppercase().as_str(),
                        "DLL" | "DYNAMIC" | "STATIC"
                    )
                })
            {
                return Err(SyntaxProblem::InvalidDirectiveContext);
            }
        }
        CompilerDirectiveKind::Data => {
            if state.context != SourceContext::WorkingStorage
                || state.lp != 64
                || operands.len() > 1
                || operands
                    .first()
                    .is_some_and(|value| !matches!(value.as_str(), "31" | "64"))
            {
                return Err(SyntaxProblem::InvalidDirectiveContext);
            }
        }
        CompilerDirectiveKind::Inline => {
            if operands.len() != 1
                || !matches!(operands[0].to_ascii_uppercase().as_str(), "ON" | "OFF")
            {
                return Err(SyntaxProblem::InvalidCompilerDirective);
            }
        }
        CompilerDirectiveKind::JavaCallable => {
            if !operands.is_empty()
                || state.context == SourceContext::Procedure
                || state.java_callable_seen
            {
                return Err(SyntaxProblem::InvalidDirectiveContext);
            }
            state.java_callable_seen = true;
        }
        CompilerDirectiveKind::JavaShareable => {
            if state.context != SourceContext::WorkingStorage
                || operands.len() != 1
                || !matches!(operands[0].to_ascii_uppercase().as_str(), "ON" | "OFF")
            {
                return Err(SyntaxProblem::InvalidDirectiveContext);
            }
        }
    }
    artifacts.directives.push(CompilerDirectiveNode {
        kind,
        group: descriptor.group,
        operands,
        source,
        active: was_active,
    });
    Ok(())
}

fn validate_directive_area(
    _kind: CompilerDirectiveKind,
    _leading: usize,
    _format: SourceFormat,
) -> Result<(), SyntaxProblem> {
    // Fixed/variable normalization has already removed columns 1-7. A
    // directive that reaches this point therefore starts in Area A or Area B;
    // a directive beginning in the indicator column fails normalization.
    Ok(())
}

fn strip_inline_comment(value: &str) -> Result<&str, SyntaxProblem> {
    let bytes = value.as_bytes();
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
        } else if quote.is_none() && bytes.get(index..index + 2) == Some(b"*>") {
            if index > 0 && !bytes[index - 1].is_ascii_whitespace() {
                return Err(SyntaxProblem::InvalidCompilerDirective);
            }
            return Ok(value[..index].trim_end());
        }
        index += 1;
    }
    if quote.is_some() {
        Err(SyntaxProblem::InvalidCompilerDirective)
    } else {
        Ok(value.trim_end())
    }
}

fn directive_words(value: &str) -> Result<Vec<String>, SyntaxProblem> {
    let mut words = Vec::new();
    let bytes = value.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        if matches!(bytes[cursor], b'\'' | b'"')
            || (matches!(
                bytes[cursor],
                b'B' | b'b' | b'X' | b'x' | b'N' | b'n' | b'G' | b'g'
            ) && matches!(bytes.get(cursor + 1), Some(b'\'' | b'"')))
        {
            let start = cursor;
            if !matches!(bytes[cursor], b'\'' | b'"') {
                cursor += 1;
            }
            let quote = bytes[cursor];
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == quote {
                    if bytes.get(cursor + 1) == Some(&quote) {
                        cursor += 2;
                        continue;
                    }
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            if cursor > bytes.len() || !matches!(bytes.get(cursor - 1), Some(b'\'' | b'"')) {
                return Err(SyntaxProblem::InvalidCompilerDirective);
            }
            words.push(value[start..cursor].to_string());
            continue;
        }
        let start = cursor;
        if bytes[cursor] == b'*' && bytes.get(cursor + 1).is_some_and(u8::is_ascii_alphabetic) {
            cursor += 1;
            while bytes
                .get(cursor)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                cursor += 1;
            }
        } else if matches!(
            bytes[cursor],
            b'(' | b')' | b'+' | b'-' | b'*' | b'/' | b'=' | b'<' | b'>'
        ) {
            cursor += 1;
            if (matches!(bytes[start], b'<' | b'>') && bytes.get(cursor) == Some(&b'='))
                || (bytes[start] == b'<' && bytes.get(cursor) == Some(&b'>'))
            {
                cursor += 1;
            }
        } else {
            while bytes.get(cursor).is_some_and(|byte| {
                !byte.is_ascii_whitespace()
                    && !matches!(byte, b'(' | b')' | b'+' | b'*' | b'/' | b'=' | b'<' | b'>')
            }) {
                cursor += 1;
            }
        }
        words.push(value[start..cursor].to_string());
    }
    Ok(words)
}

fn apply_define(
    operands: &[String],
    state: &mut DirectiveState,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    let Some(name) = operands.first().map(|name| name.to_ascii_uppercase()) else {
        return Err(SyntaxProblem::InvalidCompilerDirective);
    };
    if name.starts_with("IGY-")
        || matches!(
            name.as_str(),
            "DEFINE" | "EVALUATE" | "WHEN" | "END-EVALUATE" | "IF" | "ELSE" | "END-IF"
        )
        || name.len() > 31
        || name.starts_with('-')
        || name.ends_with('-')
        || !name.bytes().any(|byte| byte.is_ascii_alphabetic())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(SyntaxProblem::InvalidCompilationVariable(name));
    }
    if state.variables.len() >= limits.max_compilation_variables
        && !state.variables.contains_key(&name)
    {
        return Err(SyntaxProblem::CompilationVariableLimitExceeded);
    }
    let mut rest = &operands[1..];
    if rest
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("AS"))
    {
        rest = &rest[1..];
    }
    let override_value = rest
        .last()
        .is_some_and(|word| word.eq_ignore_ascii_case("OVERRIDE"));
    if override_value {
        rest = &rest[..rest.len() - 1];
    }
    let value = if rest.len() == 1 && rest[0].eq_ignore_ascii_case("OFF") {
        None
    } else if rest.len() == 1 && rest[0].eq_ignore_ascii_case("PARAMETER") {
        state.parameters.get(&name).cloned()
    } else {
        Some(evaluate_value(rest, &state.variables)?)
    };
    if !override_value
        && state
            .variables
            .get(&name)
            .is_some_and(|existing| existing.is_some() && existing != &value)
    {
        return Err(SyntaxProblem::CompilationVariableRedefinition(name));
    }
    state.variables.insert(name, value);
    Ok(())
}

fn apply_when(
    kind: CompilerDirectiveKind,
    operands: &[String],
    state: &mut DirectiveState,
) -> Result<(), SyntaxProblem> {
    let variables = state.variables.clone();
    let Some(ConditionalFrame::Evaluate {
        parent_active,
        subject,
        matched,
        branch_active,
        other_seen,
    }) = state.frames.last_mut()
    else {
        return Err(SyntaxProblem::UnmatchedCompilerDirective);
    };
    if *other_seen {
        return Err(SyntaxProblem::DuplicateCompilerBranch);
    }
    if kind == CompilerDirectiveKind::WhenOther {
        if !operands.is_empty() {
            return Err(SyntaxProblem::InvalidCompilerDirective);
        }
        *other_seen = true;
        *branch_active = *parent_active && !*matched;
        *matched = true;
        return Ok(());
    }
    if operands.is_empty() {
        return Err(SyntaxProblem::InvalidCompilerDirective);
    }
    let selected = if subject.is_none() {
        evaluate_condition(operands, &variables)?
    } else if let Some(index) = operands
        .iter()
        .position(|word| matches!(word.to_ascii_uppercase().as_str(), "THRU" | "THROUGH"))
    {
        let lower = evaluate_value(&operands[..index], &variables)?;
        let upper = evaluate_value(&operands[index + 1..], &variables)?;
        compare_range(subject.as_ref().expect("subject"), &lower, &upper)?
    } else {
        subject.as_ref().expect("subject") == &evaluate_value(operands, &variables)?
    };
    *branch_active = *parent_active && !*matched && selected;
    *matched |= selected;
    Ok(())
}

fn compare_range(
    subject: &CompileValue,
    lower: &CompileValue,
    upper: &CompileValue,
) -> Result<bool, SyntaxProblem> {
    match (subject, lower, upper) {
        (
            CompileValue::Integer(value),
            CompileValue::Integer(lower),
            CompileValue::Integer(upper),
        ) => Ok(value >= lower && value <= upper),
        _ => Err(SyntaxProblem::InvalidConditionalExpression),
    }
}

fn evaluate_condition(
    words: &[String],
    variables: &BTreeMap<String, Option<CompileValue>>,
) -> Result<bool, SyntaxProblem> {
    let mut parser = ExpressionParser::new(words, variables);
    let value = parser.condition()?;
    if parser.cursor != words.len() {
        return Err(SyntaxProblem::InvalidConditionalExpression);
    }
    Ok(value)
}

fn evaluate_value(
    words: &[String],
    variables: &BTreeMap<String, Option<CompileValue>>,
) -> Result<CompileValue, SyntaxProblem> {
    if words.is_empty() {
        return Err(SyntaxProblem::InvalidConditionalExpression);
    }
    if words.len() == 1 {
        return atom(&words[0], variables);
    }
    let mut parser = ExpressionParser::new(words, variables);
    let value = parser.arithmetic()?;
    if parser.cursor != words.len() {
        return Err(SyntaxProblem::InvalidConditionalExpression);
    }
    Ok(CompileValue::Integer(value))
}

struct ExpressionParser<'a> {
    words: &'a [String],
    variables: &'a BTreeMap<String, Option<CompileValue>>,
    cursor: usize,
}

impl<'a> ExpressionParser<'a> {
    const fn new(
        words: &'a [String],
        variables: &'a BTreeMap<String, Option<CompileValue>>,
    ) -> Self {
        Self {
            words,
            variables,
            cursor: 0,
        }
    }

    fn condition(&mut self) -> Result<bool, SyntaxProblem> {
        let mut value = self.conjunction()?;
        while self.take("OR") {
            value |= self.conjunction()?;
        }
        Ok(value)
    }

    fn conjunction(&mut self) -> Result<bool, SyntaxProblem> {
        let mut value = self.negation()?;
        while self.take("AND") {
            value &= self.negation()?;
        }
        Ok(value)
    }

    fn negation(&mut self) -> Result<bool, SyntaxProblem> {
        if self.take("NOT") {
            return Ok(!self.negation()?);
        }
        if self.take("(") {
            let value = self.condition()?;
            self.expect(")")?;
            return Ok(value);
        }
        self.simple_condition()
    }

    fn simple_condition(&mut self) -> Result<bool, SyntaxProblem> {
        let start = self.cursor;
        let first = self
            .next()
            .ok_or(SyntaxProblem::InvalidConditionalExpression)?;
        let upper = first.to_ascii_uppercase();
        if self.peek("IS") {
            self.cursor += 1;
            let not = self.take("NOT");
            if self.take("DEFINED") {
                let defined = self.variables.get(&upper).is_some_and(Option::is_some);
                return Ok(if not { !defined } else { defined });
            }
            if not {
                self.cursor = self.cursor.saturating_sub(2);
            }
        }
        self.cursor = start;
        let left = self.value_operand()?;
        let Some(operator) = self.relation_operator() else {
            return match left {
                CompileValue::Boolean(value) => Ok(value),
                _ => Err(SyntaxProblem::InvalidConditionalExpression),
            };
        };
        let right = self.value_operand()?;
        compare_values(&left, operator.as_str(), &right)
    }

    fn relation_operator(&mut self) -> Option<String> {
        if self.take("IS") {
            let not = self.take("NOT");
            for (word, operator) in [("EQUAL", "="), ("GREATER", ">"), ("LESS", "<")] {
                if self.take(word) {
                    let _ = self.take("THAN") || self.take("TO");
                    return Some(if not {
                        format!("!{operator}")
                    } else {
                        operator.into()
                    });
                }
            }
            if self.take("=") {
                return Some(if not { "!=".into() } else { "=".into() });
            }
            return None;
        }
        self.words.get(self.cursor).and_then(|word| {
            matches!(word.as_str(), "=" | "<" | ">" | "<=" | ">=" | "<>").then(|| {
                self.cursor += 1;
                word.clone()
            })
        })
    }

    fn value_operand(&mut self) -> Result<CompileValue, SyntaxProblem> {
        let start = self.cursor;
        match self.arithmetic() {
            Ok(value) if self.cursor > start => Ok(CompileValue::Integer(value)),
            _ => {
                self.cursor = start;
                let value = self
                    .next()
                    .ok_or(SyntaxProblem::InvalidConditionalExpression)?;
                atom(value, self.variables)
            }
        }
    }

    fn arithmetic(&mut self) -> Result<i128, SyntaxProblem> {
        let mut value = self.term()?;
        loop {
            if self.take("+") {
                value = value
                    .checked_add(self.term()?)
                    .ok_or(SyntaxProblem::ConditionalArithmeticOverflow)?;
            } else if self.take("-") {
                value = value
                    .checked_sub(self.term()?)
                    .ok_or(SyntaxProblem::ConditionalArithmeticOverflow)?;
            } else {
                break;
            }
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<i128, SyntaxProblem> {
        let mut value = self.factor()?;
        loop {
            if self.take("*") {
                value = value
                    .checked_mul(self.factor()?)
                    .ok_or(SyntaxProblem::ConditionalArithmeticOverflow)?;
            } else if self.take("/") {
                let divisor = self.factor()?;
                value = value
                    .checked_div(divisor)
                    .ok_or(SyntaxProblem::ConditionalArithmeticOverflow)?;
            } else {
                break;
            }
        }
        Ok(value)
    }

    fn factor(&mut self) -> Result<i128, SyntaxProblem> {
        let negative = self.take("-");
        let value = if self.take("(") {
            let value = self.arithmetic()?;
            self.expect(")")?;
            value
        } else {
            let word = self
                .next()
                .ok_or(SyntaxProblem::InvalidConditionalExpression)?;
            match atom(word, self.variables)? {
                CompileValue::Integer(value) => value,
                _ => return Err(SyntaxProblem::InvalidConditionalExpression),
            }
        };
        if negative {
            value
                .checked_neg()
                .ok_or(SyntaxProblem::ConditionalArithmeticOverflow)
        } else {
            Ok(value)
        }
    }

    fn next(&mut self) -> Option<&'a str> {
        let value = self.words.get(self.cursor)?;
        self.cursor += 1;
        Some(value)
    }

    fn peek(&self, expected: &str) -> bool {
        self.words
            .get(self.cursor)
            .is_some_and(|word| word.eq_ignore_ascii_case(expected))
    }

    fn take(&mut self, expected: &str) -> bool {
        if self.peek(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &str) -> Result<(), SyntaxProblem> {
        if self.take(expected) {
            Ok(())
        } else {
            Err(SyntaxProblem::InvalidConditionalExpression)
        }
    }
}

fn atom(
    word: &str,
    variables: &BTreeMap<String, Option<CompileValue>>,
) -> Result<CompileValue, SyntaxProblem> {
    let upper = word.to_ascii_uppercase();
    if let Some(value) = variables.get(&upper) {
        return value
            .clone()
            .ok_or(SyntaxProblem::UndefinedCompilationVariable(upper));
    }
    if upper == "TRUE" || upper == "B'1'" || upper == "B\"1\"" {
        return Ok(CompileValue::Boolean(true));
    }
    if upper == "FALSE" || upper == "B'0'" || upper == "B\"0\"" {
        return Ok(CompileValue::Boolean(false));
    }
    if let Ok(value) = word.parse::<i128>() {
        return Ok(CompileValue::Integer(value));
    }
    if (word.starts_with('\'') && word.ends_with('\''))
        || (word.starts_with('"') && word.ends_with('"'))
    {
        return Ok(CompileValue::Text(
            word[1..word.len() - 1]
                .replace("''", "'")
                .replace("\"\"", "\""),
        ));
    }
    if (upper.starts_with("X'") && upper.ends_with('\''))
        || (upper.starts_with("X\"") && upper.ends_with('"'))
    {
        let hex = &word[2..word.len() - 1];
        if !hex.len().is_multiple_of(2) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SyntaxProblem::InvalidConditionalExpression);
        }
        return Ok(CompileValue::Text(upper));
    }
    Err(SyntaxProblem::UndefinedCompilationVariable(upper))
}

fn compare_values(
    left: &CompileValue,
    operator: &str,
    right: &CompileValue,
) -> Result<bool, SyntaxProblem> {
    let ordering = match (left, right) {
        (CompileValue::Integer(left), CompileValue::Integer(right)) => left.cmp(right),
        (CompileValue::Text(left), CompileValue::Text(right)) => {
            left.as_bytes().cmp(right.as_bytes())
        }
        (CompileValue::Boolean(left), CompileValue::Boolean(right)) => left.cmp(right),
        _ => return Err(SyntaxProblem::InvalidConditionalExpression),
    };
    Ok(match operator {
        "=" => ordering.is_eq(),
        "!=" | "<>" => !ordering.is_eq(),
        ">" => ordering.is_gt(),
        "<" => ordering.is_lt(),
        ">=" => ordering.is_ge(),
        "<=" => ordering.is_le(),
        "!>" => !ordering.is_gt(),
        "!<" => !ordering.is_lt(),
        _ => return Err(SyntaxProblem::InvalidConditionalExpression),
    })
}

fn directing_operands(
    kind: CompilerDirectingKind,
    statement: &str,
) -> Result<Vec<String>, SyntaxProblem> {
    let mut words = directive_words(statement.trim_end_matches('.'))?;
    match kind {
        CompilerDirectingKind::Basis
        | CompilerDirectingKind::Process
        | CompilerDirectingKind::Delete
        | CompilerDirectingKind::Enter
        | CompilerDirectingKind::Insert
        | CompilerDirectingKind::Trace
        | CompilerDirectingKind::ServiceLabel
        | CompilerDirectingKind::ServiceReload
        | CompilerDirectingKind::Use
        | CompilerDirectingKind::Control
        | CompilerDirectingKind::Eject
        | CompilerDirectingKind::Skip
        | CompilerDirectingKind::Title => {
            if words.is_empty() {
                return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
            }
            words.remove(0);
            if kind == CompilerDirectingKind::Control {
                words = parse_control_operands(statement)?;
            }
            if matches!(kind, CompilerDirectingKind::Trace) {
                if words
                    .first()
                    .is_some_and(|word| word.eq_ignore_ascii_case("TRACE"))
                {
                    // READY TRACE loses only the READY/RESET head above.
                } else {
                    return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
                }
            }
            if matches!(kind, CompilerDirectingKind::Eject) && !words.is_empty()
                || matches!(kind, CompilerDirectingKind::Skip) && !words.is_empty()
                || matches!(kind, CompilerDirectingKind::Title) && words.len() != 1
                || matches!(kind, CompilerDirectingKind::Basis) && words.len() != 1
                || matches!(kind, CompilerDirectingKind::Insert) && words.len() != 1
                || matches!(kind, CompilerDirectingKind::Enter) && !(1..=2).contains(&words.len())
                || matches!(kind, CompilerDirectingKind::Trace) && words.as_slice() != ["TRACE"]
                || matches!(kind, CompilerDirectingKind::ServiceLabel)
                    && words.as_slice() != ["LABEL"]
                || matches!(kind, CompilerDirectingKind::ServiceReload) && words.len() != 2
                || matches!(kind, CompilerDirectingKind::Use) && words.is_empty()
            {
                return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
            }
            match kind {
                CompilerDirectingKind::Basis => validate_directing_name(&words[0], 30)?,
                CompilerDirectingKind::Delete => validate_sequence_fields(&words)?,
                CompilerDirectingKind::Insert => {
                    if !is_sequence_number(&words[0]) {
                        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
                    }
                }
                CompilerDirectingKind::Control => {
                    let allowed = ["SOURCE", "NOSOURCE", "LIST", "NOLIST", "MAP", "NOMAP"];
                    if words.is_empty()
                        || words
                            .iter()
                            .any(|word| !allowed.contains(&word.to_ascii_uppercase().as_str()))
                    {
                        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
                    }
                }
                CompilerDirectingKind::Enter => {
                    for word in &words {
                        validate_directing_name(word, 30)?;
                    }
                }
                CompilerDirectingKind::ServiceReload => {
                    if !words[0].eq_ignore_ascii_case("RELOAD") {
                        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
                    }
                    validate_directing_name(&words[1], 30)?;
                }
                CompilerDirectingKind::Title => {
                    let literal = &words[0];
                    let figurative = matches!(
                        literal.to_ascii_uppercase().as_str(),
                        "SPACE" | "SPACES" | "ZERO" | "ZEROS" | "ZEROES" | "QUOTE" | "QUOTES"
                    );
                    if figurative
                        || !(literal.starts_with(['\'', '"', 'N', 'n', 'G', 'g'])
                            && (literal.ends_with('\'') || literal.ends_with('"')))
                    {
                        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
                    }
                }
                CompilerDirectingKind::Use => validate_use_operands(&words)?,
                _ => {}
            }
            Ok(words)
        }
        _ => Err(SyntaxProblem::InvalidCompilerDirectingStatement),
    }
}

fn parse_control_operands(statement: &str) -> Result<Vec<String>, SyntaxProblem> {
    let statement = statement.trim().trim_end_matches('.');
    let body = statement
        .split_once(char::is_whitespace)
        .map(|(_, body)| body.trim())
        .ok_or(SyntaxProblem::InvalidCompilerDirectingStatement)?;
    if body.is_empty() {
        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
    }
    let bytes = body.as_bytes();
    let mut words = Vec::new();
    let mut cursor = 0usize;
    let mut require_word = true;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        if bytes[cursor] == b',' {
            if require_word {
                return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
            }
            require_word = true;
            cursor += 1;
            continue;
        }
        let start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        {
            cursor += 1;
        }
        if start == cursor || !require_word && !bytes[start - 1].is_ascii_whitespace() {
            return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
        }
        words.push(body[start..cursor].to_string());
        require_word = false;
    }
    if require_word {
        Err(SyntaxProblem::InvalidCompilerDirectingStatement)
    } else {
        Ok(words)
    }
}

fn validate_directing_name(value: &str, max: usize) -> Result<(), SyntaxProblem> {
    let value = value.trim_matches(['\'', '"']);
    if value.is_empty()
        || value.len() > max
        || value.starts_with('-')
        || value.ends_with('-')
        || !value.bytes().any(|byte| byte.is_ascii_alphabetic())
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'@' | b'#' | b'$')
        })
    {
        Err(SyntaxProblem::InvalidCompilerDirectingStatement)
    } else {
        Ok(())
    }
}

fn is_sequence_number(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_sequence_fields(words: &[String]) -> Result<(), SyntaxProblem> {
    if words.is_empty() {
        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
    }
    let joined = words.join(" ");
    let mut previous = None;
    for entry in joined.split(',').map(str::trim) {
        let (start, end) = entry.split_once('-').unwrap_or((entry, entry));
        if !is_sequence_number(start) || !is_sequence_number(end) || start > end {
            return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
        }
        if previous.is_some_and(|previous: &str| previous >= start) {
            return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
        }
        previous = Some(end);
    }
    Ok(())
}

fn validate_use_operands(words: &[String]) -> Result<(), SyntaxProblem> {
    let upper = words
        .iter()
        .map(|word| word.to_ascii_uppercase())
        .collect::<Vec<_>>();
    if upper.starts_with(&["FOR".into(), "DEBUGGING".into(), "ON".into()]) {
        let targets = &upper[3..];
        if targets == ["ALL", "PROCEDURES"] {
            return Ok(());
        }
        if targets.is_empty() {
            return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
        }
        let mut seen = std::collections::BTreeSet::new();
        for target in targets {
            validate_directing_name(target, 30)?;
            if !seen.insert(target) {
                return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
            }
        }
        return Ok(());
    }
    let mut cursor = usize::from(upper.first().is_some_and(|word| word == "GLOBAL"));
    if upper.get(cursor).is_none_or(|word| word != "AFTER") {
        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
    }
    cursor += 1;
    if upper.get(cursor).is_some_and(|word| word == "STANDARD") {
        cursor += 1;
    }
    if upper
        .get(cursor)
        .is_none_or(|word| !matches!(word.as_str(), "EXCEPTION" | "ERROR"))
    {
        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
    }
    cursor += 1;
    if upper.get(cursor).is_some_and(|word| word == "PROCEDURE") {
        cursor += 1;
    }
    if upper.get(cursor).is_none_or(|word| word != "ON") || cursor + 1 >= upper.len() {
        return Err(SyntaxProblem::InvalidCompilerDirectingStatement);
    }
    let targets = &upper[cursor + 1..];
    for target in targets {
        if !matches!(target.as_str(), "INPUT" | "OUTPUT" | "I-O" | "EXTEND") {
            validate_directing_name(target, 30)?;
        }
    }
    Ok(())
}

pub(super) fn record_directing(
    artifacts: &mut DirectiveArtifacts,
    kind: CompilerDirectingKind,
    operands: Vec<String>,
    source: Vec<SourceSpan>,
    active: bool,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    push_directing(artifacts, kind, operands, source, active, None, limits)
}

fn push_directing(
    artifacts: &mut DirectiveArtifacts,
    kind: CompilerDirectingKind,
    operands: Vec<String>,
    source: Vec<SourceSpan>,
    active: bool,
    declarative_section: Option<String>,
    limits: SyntaxLimits,
) -> Result<(), SyntaxProblem> {
    if artifacts.directing.len() >= limits.max_directives {
        return Err(SyntaxProblem::DirectiveLimitExceeded);
    }
    artifacts.directing.push(CompilerDirectingNode {
        kind,
        operands,
        source,
        active,
        declarative_section,
    });
    Ok(())
}

fn append_slice(
    output: &mut NormalizedSource,
    source: &NormalizedSource,
    range: std::ops::Range<usize>,
) {
    let output_start = output.text.len();
    output.text.push_str(&source.text[range.clone()]);
    output
        .origins
        .extend(slice_origins(&source.origins, range, output_start));
}

fn append_newline(
    output: &mut NormalizedSource,
    _source: &NormalizedSource,
    line: &super::PhysicalLine,
) {
    if line.terminator.start < line.terminator.end {
        output.text.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_expression_supports_defined_boolean_relation_and_arithmetic() {
        let variables = BTreeMap::from([
            ("A".into(), Some(CompileValue::Integer(10))),
            ("B".into(), Some(CompileValue::Boolean(true))),
        ]);
        assert!(
            evaluate_condition(&directive_words("A + 2 = 12 AND B").unwrap(), &variables).unwrap()
        );
        assert!(evaluate_condition(&directive_words("A IS DEFINED").unwrap(), &variables).unwrap());
        assert!(
            evaluate_condition(
                &directive_words("MISSING IS NOT DEFINED").unwrap(),
                &variables
            )
            .unwrap()
        );
    }

    #[test]
    fn compiler_option_parser_preserves_nested_values() {
        let mut options = CompilerOptionSet::default();
        parse_options(
            "PROCESS LP(64),DEFINE(A=1),JAVAIOP(JAVA64,OUTPATH('/tmp/x'))",
            &[],
            &mut options,
            SyntaxLimits::default(),
        )
        .unwrap();
        assert_eq!(options.value("LP"), Some(Some("(64)")));
        assert!(options.enabled("DEFINE"));
        assert!(
            options
                .value("JAVAIOP")
                .flatten()
                .unwrap()
                .contains("JAVA64")
        );

        parse_options(
            "PROCESS NODLL,DLL",
            &[],
            &mut options,
            SyntaxLimits::default(),
        )
        .unwrap();
        assert!(options.enabled("DLL"));
        parse_options("PROCESS NODLL", &[], &mut options, SyntaxLimits::default()).unwrap();
        assert!(!options.enabled("DLL"));
        assert_eq!(
            parse_options(
                "PROCESS LP(64),,DLL",
                &[],
                &mut options,
                SyntaxLimits::default(),
            ),
            Err(SyntaxProblem::InvalidCompilerOption)
        );
    }
}
