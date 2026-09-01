use crate::{
    CobolFileBinding, CobolLayout, LosslessSyntax, ProcedureStatementKind, SemanticModel,
    SourceSpan,
};
use mainframe_env_ir::{
    Attribute, Effect, IrLimits, Module, ModuleBuilder, OperationCatalog, OperationIdentity,
    OperationSchema, StorageReference,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

mod statement_grammar;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StatementKind {
    Accept,
    Add,
    Allocate,
    Alter,
    Call,
    Cancel,
    Close,
    Compute,
    Continue,
    Delete,
    Display,
    Divide,
    DuplicateLabel,
    Entry,
    Evaluate,
    ExecCics,
    ExecDli,
    ExecSql,
    Exit,
    Free,
    GoBack,
    GoTo,
    If,
    Initialize,
    Inspect,
    Invoke,
    JsonGenerate,
    JsonParse,
    Merge,
    Move,
    Multiply,
    NextSentence,
    Open,
    Perform,
    Read,
    Release,
    Rewrite,
    ReturnStatement,
    Search,
    Set,
    Sort,
    Start,
    StopRun,
    StructuredControl,
    String,
    Subtract,
    Unstring,
    Write,
    XmlGenerate,
    XmlParse,
    Label,
    ProgramEnd,
}

impl StatementKind {
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Add => "add",
            Self::Allocate => "allocate",
            Self::Alter => "alter",
            Self::Call => "call",
            Self::Cancel => "cancel",
            Self::Close => "close",
            Self::Compute => "compute",
            Self::Continue => "continue",
            Self::Delete => "delete",
            Self::Display => "display",
            Self::Divide => "divide",
            Self::DuplicateLabel => "duplicate_label",
            Self::Entry => "entry",
            Self::Evaluate => "evaluate",
            Self::ExecCics => "exec_cics",
            Self::ExecDli => "exec_dli",
            Self::ExecSql => "exec_sql",
            Self::Exit => "exit",
            Self::Free => "free",
            Self::GoBack => "go_back",
            Self::GoTo => "go_to",
            Self::If => "if",
            Self::Initialize => "initialize",
            Self::Inspect => "inspect",
            Self::Invoke => "invoke",
            Self::JsonGenerate => "json_generate",
            Self::JsonParse => "json_parse",
            Self::Merge => "merge",
            Self::Move => "move",
            Self::Multiply => "multiply",
            Self::NextSentence => "next_sentence",
            Self::Open => "open",
            Self::Perform => "perform",
            Self::Read => "read",
            Self::Release => "release",
            Self::Rewrite => "rewrite",
            Self::ReturnStatement => "return_statement",
            Self::Search => "search",
            Self::Set => "set",
            Self::Sort => "sort",
            Self::Start => "start",
            Self::StopRun => "stop_run",
            Self::StructuredControl => "structured_control",
            Self::String => "string",
            Self::Subtract => "subtract",
            Self::Unstring => "unstring",
            Self::Write => "write",
            Self::XmlGenerate => "xml_generate",
            Self::XmlParse => "xml_parse",
            Self::Label => "label",
            Self::ProgramEnd => "program_end",
        }
    }

    #[must_use]
    pub const fn supported(self) -> bool {
        !matches!(self, Self::DuplicateLabel | Self::StructuredControl)
    }

    #[must_use]
    pub const fn official() -> [Self; 44] {
        [
            Self::Accept,
            Self::Add,
            Self::Allocate,
            Self::Alter,
            Self::Call,
            Self::Cancel,
            Self::Close,
            Self::Compute,
            Self::Continue,
            Self::Delete,
            Self::Display,
            Self::Divide,
            Self::Entry,
            Self::Evaluate,
            Self::Exit,
            Self::Free,
            Self::GoBack,
            Self::GoTo,
            Self::If,
            Self::Initialize,
            Self::Inspect,
            Self::Invoke,
            Self::JsonGenerate,
            Self::JsonParse,
            Self::Merge,
            Self::Move,
            Self::Multiply,
            Self::Open,
            Self::Perform,
            Self::Read,
            Self::Release,
            Self::ReturnStatement,
            Self::Rewrite,
            Self::Search,
            Self::Set,
            Self::Sort,
            Self::Start,
            Self::StopRun,
            Self::String,
            Self::Subtract,
            Self::Unstring,
            Self::Write,
            Self::XmlGenerate,
            Self::XmlParse,
        ]
    }

    #[must_use]
    pub const fn official_kind(self) -> Option<ProcedureStatementKind> {
        use ProcedureStatementKind as P;
        Some(match self {
            Self::Accept => P::Accept,
            Self::Add => P::Add,
            Self::Allocate => P::Allocate,
            Self::Alter => P::Alter,
            Self::Call => P::Call,
            Self::Cancel => P::Cancel,
            Self::Close => P::Close,
            Self::Compute => P::Compute,
            Self::Continue => P::Continue,
            Self::Delete => P::Delete,
            Self::Display => P::Display,
            Self::Divide => P::Divide,
            Self::Entry => P::Entry,
            Self::Evaluate => P::Evaluate,
            Self::Exit => P::Exit,
            Self::Free => P::Free,
            Self::GoBack => P::Goback,
            Self::GoTo => P::GoTo,
            Self::If => P::If,
            Self::Initialize => P::Initialize,
            Self::Inspect => P::Inspect,
            Self::Invoke => P::Invoke,
            Self::JsonGenerate => P::JsonGenerate,
            Self::JsonParse => P::JsonParse,
            Self::Merge => P::Merge,
            Self::Move => P::Move,
            Self::Multiply => P::Multiply,
            Self::Open => P::Open,
            Self::Perform => P::Perform,
            Self::Read => P::Read,
            Self::Release => P::Release,
            Self::ReturnStatement => P::Return,
            Self::Rewrite => P::Rewrite,
            Self::Search => P::Search,
            Self::Set => P::Set,
            Self::Sort => P::Sort,
            Self::Start => P::Start,
            Self::StopRun => P::Stop,
            Self::String => P::String,
            Self::Subtract => P::Subtract,
            Self::Unstring => P::Unstring,
            Self::Write => P::Write,
            Self::XmlGenerate => P::XmlGenerate,
            Self::XmlParse => P::XmlParse,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StatementOptionKind {
    AtEnd,
    NotAtEnd,
    InvalidKey,
    NotInvalidKey,
    OnException,
    NotOnException,
    OnOverflow,
    NotOnOverflow,
    OnSizeError,
    NotOnSizeError,
    Rounded,
    Giving,
    Remainder,
    Returning,
    Using,
    Into,
    From,
    ExplicitTerminator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatementOption {
    pub kind: StatementOptionKind,
    pub operands: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirStatement {
    pub kind: StatementKind,
    pub official: Option<ProcedureStatementKind>,
    pub arguments: Vec<String>,
    pub options: Vec<StatementOption>,
    pub line: usize,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlScope {
    If,
    Evaluate,
    Search,
    Perform,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlRole {
    Statement,
    BlockStart,
    Branch,
    BlockEnd,
    Label,
    ExternalTarget,
    Recovered,
    Transfer,
    Terminator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlNode {
    pub id: usize,
    pub line: usize,
    pub role: ControlRole,
    pub scope: Option<ControlScope>,
    pub parent: Option<usize>,
    pub statement: Option<usize>,
    pub text: String,
    pub source: Vec<SourceSpan>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlEdgeKind {
    Fallthrough,
    Branch,
    True,
    False,
    Loop,
    Call,
    Transfer,
    Return,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlEdge {
    pub from: usize,
    pub to: usize,
    pub kind: ControlEdgeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolHir {
    pub program_id: String,
    pub layouts: Vec<CobolLayout>,
    pub files: Vec<CobolFileBinding>,
    pub statements: Vec<HirStatement>,
    pub control_nodes: Vec<ControlNode>,
    pub control_edges: Vec<ControlEdge>,
    pub module: Module,
}

impl CobolHir {
    pub(crate) fn build(
        syntax: &LosslessSyntax,
        semantic: &SemanticModel,
        limits: IrLimits,
    ) -> Result<Self, HirProblem> {
        let (procedure_offset, procedure) =
            procedure_text(syntax.semantic_text()).ok_or(HirProblem::MissingProcedure)?;
        let mut parsed = parse_procedure(procedure, limits.max_operations.saturating_sub(1))?;
        for (statement, range) in parsed.statements.iter_mut().zip(&parsed.statement_ranges) {
            statement.source = source_spans_for_range(syntax, procedure_offset, range);
        }
        for (node, range) in parsed.nodes.iter_mut().zip(&parsed.node_ranges) {
            node.source = source_spans_for_range(syntax, procedure_offset, range);
        }
        let mut statements = parsed.statements;
        if statements.len() > limits.max_operations.saturating_sub(1) {
            return Err(HirProblem::StatementLimitExceeded);
        }
        statements.push(HirStatement {
            kind: StatementKind::ProgramEnd,
            official: None,
            arguments: Vec::new(),
            options: Vec::new(),
            line: syntax.semantic_text().lines().count(),
            source: syntax
                .semantic_text()
                .len()
                .checked_sub(1)
                .map_or_else(Vec::new, |start| {
                    crate::syntax::source_spans(
                        syntax.semantic_origins(),
                        start..syntax.semantic_text().len(),
                    )
                }),
        });
        let module = build_module(&statements, &semantic.layouts, limits)?;
        Ok(Self {
            program_id: semantic.program_id.clone(),
            layouts: semantic.layouts.clone(),
            files: semantic.files.clone(),
            statements,
            control_nodes: parsed.nodes,
            control_edges: parsed.edges,
            module,
        })
    }

    #[must_use]
    pub fn unsupported(&self) -> BTreeSet<StatementKind> {
        self.statements
            .iter()
            .filter(|statement| !statement.kind.supported())
            .map(|statement| statement.kind)
            .collect()
    }

    #[must_use]
    pub fn execution_incomplete_arithmetic_receivers(&self) -> BTreeSet<(StatementKind, usize)> {
        self.statements
            .iter()
            .filter(|statement| {
                matches!(statement.kind, StatementKind::Add | StatementKind::Subtract)
                    && has_multiple_arithmetic_receivers(statement)
            })
            .map(|statement| (statement.kind, statement.line))
            .collect()
    }
}

fn has_multiple_arithmetic_receivers(statement: &HirStatement) -> bool {
    let separator = match statement.kind {
        StatementKind::Add => statement
            .arguments
            .iter()
            .position(|argument| argument == "TO")
            .or_else(|| {
                statement
                    .arguments
                    .iter()
                    .position(|argument| argument == "GIVING")
            }),
        StatementKind::Subtract => statement
            .arguments
            .iter()
            .position(|argument| argument == "FROM"),
        _ => None,
    };
    let Some(separator) = separator else {
        return false;
    };
    let giving = statement
        .arguments
        .iter()
        .enumerate()
        .skip(separator + 1)
        .find_map(|(index, argument)| (argument == "GIVING").then_some(index));
    let primary_start = separator + 1;
    let primary_end = giving.unwrap_or(statement.arguments.len());
    arithmetic_receiver_count(&statement.arguments[primary_start..primary_end]) > 1
        || giving
            .is_some_and(|giving| arithmetic_receiver_count(&statement.arguments[giving + 1..]) > 1)
}

fn arithmetic_receiver_count(tokens: &[String]) -> usize {
    let mut count = 0usize;
    let mut position = 0usize;
    while position < tokens.len() {
        if matches!(tokens[position].as_str(), "," | "ROUNDED") {
            position += 1;
            continue;
        }
        count += 1;
        position += 1;
        while position < tokens.len() {
            if tokens[position] == "(" {
                let mut depth = 1usize;
                position += 1;
                while position < tokens.len() && depth > 0 {
                    match tokens[position].as_str() {
                        "(" => depth += 1,
                        ")" => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    position += 1;
                }
            } else if matches!(tokens[position].as_str(), "OF" | "IN")
                && position + 1 < tokens.len()
            {
                position += 2;
            } else {
                break;
            }
        }
    }
    count
}

fn source_spans_for_range(
    syntax: &LosslessSyntax,
    procedure_offset: usize,
    range: &Range<usize>,
) -> Vec<SourceSpan> {
    if range.start >= range.end {
        return Vec::new();
    }
    crate::syntax::source_spans(
        syntax.semantic_origins(),
        procedure_offset + range.start..procedure_offset + range.end,
    )
}

pub fn cobol_hir_catalog() -> OperationCatalog {
    let mut catalog = OperationCatalog::default();
    for kind in StatementKind::official().into_iter().chain([
        StatementKind::NextSentence,
        StatementKind::ExecCics,
        StatementKind::ExecDli,
        StatementKind::ExecSql,
        StatementKind::DuplicateLabel,
        StatementKind::StructuredControl,
        StatementKind::Label,
        StatementKind::ProgramEnd,
    ]) {
        let identity =
            OperationIdentity::new("cobol.hir", kind.slug(), 1).expect("static HIR identity");
        let mut schema = OperationSchema::pure(identity, 0, 0);
        schema.allowed_effects = effects(kind).into_iter().collect();
        schema.executable = kind.supported();
        schema.terminator = kind == StatementKind::ProgramEnd;
        catalog.register(schema).expect("unique HIR operation");
    }
    catalog
}

fn build_module(
    statements: &[HirStatement],
    layouts: &[CobolLayout],
    limits: IrLimits,
) -> Result<Module, HirProblem> {
    let mut builder = ModuleBuilder::new(limits);
    let mut storage = BTreeMap::new();
    let mut backing_lengths = layouts
        .iter()
        .filter(|layout| layout.allocated)
        .map(|layout| {
            (
                layout.qualified_name.clone(),
                if layout.dynamic {
                    layout.dynamic_limit.unwrap_or_default()
                } else {
                    layout.length
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    for layout in layouts {
        if let Some(target) = &layout.alias_of {
            let required = backing_lengths.entry(target.clone()).or_default();
            *required = (*required).max(layout.length);
        }
    }
    for layout in layouts
        .iter()
        .filter(|layout| layout.allocated && (layout.length > 0 || layout.dynamic))
    {
        let alias = (!layout.dynamic)
            .then_some(layout.alias_of.as_ref())
            .flatten()
            .and_then(|name| storage.get(name))
            .map(|id| StorageReference {
                storage: *id,
                offset: 0,
                length: layout.length as u64,
            });
        let storage_length = if alias.is_some() {
            layout.length
        } else {
            backing_lengths
                .get(&layout.qualified_name)
                .copied()
                .unwrap_or(layout.length)
        };
        let id = builder
            .add_storage(
                layout.qualified_name.to_ascii_lowercase(),
                storage_length as u64,
                alias,
            )
            .map_err(|_| HirProblem::InvalidLayout)?;
        storage.insert(layout.qualified_name.clone(), id);
    }
    let region = builder
        .add_region()
        .map_err(|_| HirProblem::StatementLimitExceeded)?;
    let block = builder
        .add_block(region)
        .map_err(|_| HirProblem::StatementLimitExceeded)?;
    for statement in statements {
        let mut attributes = BTreeMap::new();
        attributes.insert("line".into(), Attribute::Integer(statement.line as i64));
        attributes.insert(
            "arguments".into(),
            Attribute::Bytes(encode_arguments(&statement.arguments)),
        );
        builder
            .add_operation(
                block,
                OperationIdentity::new("cobol.hir", statement.kind.slug(), 1)
                    .map_err(|_| HirProblem::UnknownStatement(statement.line))?,
                Vec::new(),
                0,
                attributes,
                effects(statement.kind),
                Vec::new(),
                None,
            )
            .map_err(|_| HirProblem::StatementLimitExceeded)?;
    }
    builder.finish().map_err(|_| HirProblem::InvalidLayout)
}

fn encode_arguments(arguments: &[String]) -> Vec<u8> {
    let mut encoded = Vec::new();
    for argument in arguments {
        encoded.extend_from_slice(&(argument.len() as u64).to_be_bytes());
        encoded.extend_from_slice(argument.as_bytes());
    }
    encoded
}

pub(crate) fn effects(kind: StatementKind) -> Vec<Effect> {
    use StatementKind as K;
    match kind {
        K::Accept => vec![Effect::TerminalRead, Effect::MemoryWrite],
        K::Display => vec![Effect::MemoryRead, Effect::TerminalWrite],
        K::Call
        | K::Cancel
        | K::GoBack
        | K::GoTo
        | K::Alter
        | K::Perform
        | K::Entry
        | K::Exit
        | K::StopRun => vec![Effect::ProgramControl],
        K::Open | K::Close | K::Read | K::Start => vec![Effect::DatasetRead],
        K::Delete | K::Rewrite | K::Write => vec![Effect::DatasetWrite],
        K::ExecCics | K::ExecDli | K::ExecSql => {
            vec![
                Effect::ProgramControl,
                Effect::MemoryRead,
                Effect::MemoryWrite,
                Effect::Condition,
            ]
        }
        K::Allocate
        | K::Free
        | K::Move
        | K::Initialize
        | K::String
        | K::Unstring
        | K::Inspect
        | K::JsonGenerate
        | K::JsonParse
        | K::XmlGenerate
        | K::XmlParse => vec![Effect::MemoryRead, Effect::MemoryWrite],
        K::Add | K::Subtract | K::Multiply | K::Divide | K::Compute | K::Set => {
            vec![Effect::MemoryRead, Effect::MemoryWrite, Effect::Condition]
        }
        K::If | K::Evaluate | K::Search => vec![Effect::MemoryRead, Effect::Condition],
        _ => Vec::new(),
    }
}

fn procedure_text(source: &str) -> Option<(usize, &str)> {
    let upper = source.to_ascii_uppercase();
    let start = upper.find("PROCEDURE DIVISION")?;
    let rest = &source[start..];
    let dot = rest.find('.')?;
    Some((start + dot + 1, &rest[dot + 1..]))
}

fn statement_options(kind: StatementKind, text: &str) -> Vec<StatementOption> {
    if kind.official_kind().is_none() {
        return Vec::new();
    }
    let upper = text.to_ascii_uppercase();
    let tokens = semantic_tokens(text, 1);
    let mut options = Vec::new();
    for (phrase, kind) in [
        ("NOT AT END", StatementOptionKind::NotAtEnd),
        ("AT END", StatementOptionKind::AtEnd),
        ("NOT INVALID KEY", StatementOptionKind::NotInvalidKey),
        ("INVALID KEY", StatementOptionKind::InvalidKey),
        ("NOT ON EXCEPTION", StatementOptionKind::NotOnException),
        ("ON EXCEPTION", StatementOptionKind::OnException),
        ("NOT ON OVERFLOW", StatementOptionKind::NotOnOverflow),
        ("ON OVERFLOW", StatementOptionKind::OnOverflow),
        ("NOT ON SIZE ERROR", StatementOptionKind::NotOnSizeError),
        ("ON SIZE ERROR", StatementOptionKind::OnSizeError),
    ] {
        if upper.contains(phrase) {
            options.push(StatementOption {
                kind,
                operands: Vec::new(),
            });
        }
    }
    for (word, kind) in [
        ("ROUNDED", StatementOptionKind::Rounded),
        ("GIVING", StatementOptionKind::Giving),
        ("REMAINDER", StatementOptionKind::Remainder),
        ("RETURNING", StatementOptionKind::Returning),
        ("USING", StatementOptionKind::Using),
        ("INTO", StatementOptionKind::Into),
        ("FROM", StatementOptionKind::From),
    ] {
        if let Some(index) = tokens.iter().position(|token| token == word) {
            options.push(StatementOption {
                kind,
                operands: tokens.get(index + 1).into_iter().cloned().collect(),
            });
        }
    }
    if upper
        .split_whitespace()
        .any(|word| word.starts_with("END-"))
    {
        options.push(StatementOption {
            kind: StatementOptionKind::ExplicitTerminator,
            operands: Vec::new(),
        });
    }
    options.sort_by_key(|option| option.kind);
    options.dedup_by_key(|option| option.kind);
    options
}

#[derive(Debug)]
struct ParsedProcedure {
    statements: Vec<HirStatement>,
    statement_ranges: Vec<Range<usize>>,
    nodes: Vec<ControlNode>,
    node_ranges: Vec<Range<usize>>,
    edges: Vec<ControlEdge>,
}

#[derive(Clone, Copy, Debug)]
struct ScopeFrame {
    scope: ControlScope,
    start: usize,
}

#[derive(Debug)]
struct InternalTransfer {
    node: usize,
    target: String,
    through: Option<String>,
    kind: ControlEdgeKind,
}

#[derive(Debug)]
struct NextSentenceTransfer {
    node: usize,
    sentence: usize,
}

#[derive(Debug)]
struct ProcedureParser {
    statements: Vec<HirStatement>,
    statement_ranges: Vec<Range<usize>>,
    nodes: Vec<ControlNode>,
    node_ranges: Vec<Range<usize>>,
    edges: Vec<ControlEdge>,
    scopes: Vec<ScopeFrame>,
    branch_tails: BTreeMap<usize, Vec<usize>>,
    branch_counts: BTreeMap<usize, usize>,
    transfers: Vec<InternalTransfer>,
    next_sentence: Vec<NextSentenceTransfer>,
    sentence_first_nodes: Vec<Option<usize>>,
    last_node: Option<usize>,
    fallthrough_open: bool,
    condition_parent: Option<usize>,
    has_unsupported_include: bool,
    max_statements: usize,
}

impl ProcedureParser {
    fn new(max_statements: usize, sentence_count: usize) -> Self {
        Self {
            statements: Vec::new(),
            statement_ranges: Vec::new(),
            nodes: Vec::new(),
            node_ranges: Vec::new(),
            edges: Vec::new(),
            scopes: Vec::new(),
            branch_tails: BTreeMap::new(),
            branch_counts: BTreeMap::new(),
            transfers: Vec::new(),
            next_sentence: Vec::new(),
            sentence_first_nodes: vec![None; sentence_count],
            last_node: None,
            fallthrough_open: true,
            condition_parent: None,
            has_unsupported_include: false,
            max_statements,
        }
    }

    fn finish(mut self) -> Result<ParsedProcedure, HirProblem> {
        if !self.scopes.is_empty() || self.condition_parent.is_some() {
            return Err(HirProblem::UnmatchedScope);
        }
        let mut labels = BTreeMap::new();
        let mut label_order = Vec::new();
        for node_index in 0..self.nodes.len() {
            if self.nodes[node_index].role == ControlRole::Label {
                let name = self.nodes[node_index].text.to_ascii_uppercase();
                if let Some(previous) = labels.get(&name).copied() {
                    self.nodes[node_index].role = ControlRole::Recovered;
                    if let Some(statement) = self.nodes[node_index].statement {
                        let previous_index = self
                            .nodes
                            .iter()
                            .position(|node| node.id == previous)
                            .expect("recorded label node exists");
                        let between = self.nodes[previous_index + 1..node_index]
                            .iter()
                            .filter_map(|node| node.statement)
                            .collect::<BTreeSet<_>>();
                        let redundant_exit = between.len() == 1
                            && between
                                .iter()
                                .all(|index| self.statements[*index].kind == StatementKind::Exit);
                        if redundant_exit {
                            self.statements[statement].kind = StatementKind::Continue;
                            self.statements[statement].arguments.clear();
                        } else {
                            self.statements[statement].kind = StatementKind::DuplicateLabel;
                        }
                    }
                } else {
                    labels.insert(name.clone(), self.nodes[node_index].id);
                    label_order.push((name, self.nodes[node_index].id));
                }
            }
        }
        if self.has_unsupported_include {
            let external_targets = self
                .transfers
                .iter()
                .flat_map(|transfer| {
                    std::iter::once(&transfer.target).chain(transfer.through.iter())
                })
                .filter(|target| !labels.contains_key(*target))
                .cloned()
                .collect::<BTreeSet<_>>();
            for target in external_targets {
                let id = self.nodes.len();
                self.nodes.push(ControlNode {
                    id,
                    line: 0,
                    role: ControlRole::ExternalTarget,
                    scope: None,
                    parent: None,
                    statement: None,
                    text: target.clone(),
                    source: Vec::new(),
                });
                self.node_ranges.push(0..0);
                labels.insert(target.clone(), id);
                label_order.push((target, id));
            }
        }
        for transfer in &self.transfers {
            let target = labels
                .get(&transfer.target)
                .copied()
                .ok_or_else(|| HirProblem::MissingTarget(transfer.target.clone()))?;
            self.edges.push(ControlEdge {
                from: transfer.node,
                to: target,
                kind: transfer.kind,
            });
            if transfer.kind == ControlEdgeKind::Call {
                let endpoint_start = if let Some(through) = &transfer.through {
                    labels
                        .get(through)
                        .copied()
                        .ok_or_else(|| HirProblem::MissingTarget(through.clone()))?
                } else {
                    target
                };
                let endpoint = paragraph_end(endpoint_start, &label_order, self.nodes.len());
                if let Some(return_to) = transfer
                    .node
                    .checked_add(1)
                    .filter(|id| *id < self.nodes.len())
                {
                    self.edges.push(ControlEdge {
                        from: endpoint,
                        to: return_to,
                        kind: ControlEdgeKind::Return,
                    });
                }
            }
        }
        for transfer in &self.next_sentence {
            let target = self
                .sentence_first_nodes
                .iter()
                .skip(transfer.sentence.saturating_add(1))
                .flatten()
                .next()
                .copied()
                .ok_or_else(|| HirProblem::MissingTarget("<next-sentence>".into()))?;
            self.edges.push(ControlEdge {
                from: transfer.node,
                to: target,
                kind: ControlEdgeKind::Transfer,
            });
        }
        self.edges
            .sort_by_key(|edge| (edge.from, edge.to, edge.kind));
        self.edges.dedup();
        Ok(ParsedProcedure {
            statements: self.statements,
            statement_ranges: self.statement_ranges,
            nodes: self.nodes,
            node_ranges: self.node_ranges,
            edges: self.edges,
        })
    }

    fn push_node(
        &mut self,
        sentence: usize,
        line: usize,
        role: ControlRole,
        scope: Option<ControlScope>,
        statement: Option<usize>,
        text: String,
        range: Range<usize>,
    ) -> usize {
        let id = self.nodes.len();
        let parent = self
            .scopes
            .last()
            .map(|frame| frame.start)
            .or(self.condition_parent);
        if self.sentence_first_nodes[sentence].is_none() {
            self.sentence_first_nodes[sentence] = Some(id);
        }
        if self.fallthrough_open
            && let Some(previous) = self.last_node
        {
            let kind = if self.nodes[previous].role == ControlRole::BlockStart
                && matches!(
                    self.nodes[previous].scope,
                    Some(ControlScope::If | ControlScope::Perform)
                ) {
                ControlEdgeKind::True
            } else {
                ControlEdgeKind::Fallthrough
            };
            self.edges.push(ControlEdge {
                from: previous,
                to: id,
                kind,
            });
        }
        self.nodes.push(ControlNode {
            id,
            line,
            role,
            scope,
            parent,
            statement,
            text,
            source: Vec::new(),
        });
        self.node_ranges.push(range);
        self.last_node = Some(id);
        self.fallthrough_open = true;
        id
    }

    fn push_statement(
        &mut self,
        sentence: usize,
        line: usize,
        header_text: &str,
        range: Range<usize>,
        kind: StatementKind,
        options: Vec<StatementOption>,
        scope: Option<ControlScope>,
    ) -> Result<usize, HirProblem> {
        if self.statements.len() >= self.max_statements {
            return Err(HirProblem::StatementLimitExceeded);
        }
        let keyword_words = match kind {
            StatementKind::GoTo
            | StatementKind::NextSentence
            | StatementKind::JsonGenerate
            | StatementKind::JsonParse
            | StatementKind::XmlGenerate
            | StatementKind::XmlParse
            | StatementKind::StopRun => 2,
            StatementKind::GoBack
                if header_text
                    .trim_start()
                    .to_ascii_uppercase()
                    .starts_with("GO ") =>
            {
                2
            }
            _ => 1,
        };
        let arguments = semantic_tokens(header_text, keyword_words);
        if kind == StatementKind::ExecSql
            && arguments
                .iter()
                .any(|argument| argument.as_str() == "INCLUDE")
        {
            self.has_unsupported_include = true;
        }
        let statement = self.statements.len();
        self.statements.push(HirStatement {
            kind,
            official: kind.official_kind(),
            arguments,
            options,
            line,
            source: Vec::new(),
        });
        self.statement_ranges.push(range.clone());
        let role = statement_role(kind, scope);
        let node = self.push_node(
            sentence,
            line,
            role,
            scope,
            Some(statement),
            header_text.trim().to_string(),
            range,
        );
        if let Some(scope) = scope {
            self.scopes.push(ScopeFrame { scope, start: node });
        }
        match kind {
            StatementKind::GoTo => {
                let (targets, computed) =
                    go_to_targets(header_text).ok_or(HirProblem::UnsupportedForm)?;
                self.transfers
                    .extend(targets.into_iter().map(|target| InternalTransfer {
                        node,
                        target,
                        through: None,
                        kind: ControlEdgeKind::Transfer,
                    }));
                self.fallthrough_open = computed;
            }
            StatementKind::Perform if scope.is_none() => {
                let (target, through) =
                    perform_targets(header_text).ok_or(HirProblem::UnsupportedForm)?;
                self.transfers.push(InternalTransfer {
                    node,
                    target,
                    through,
                    kind: ControlEdgeKind::Call,
                });
            }
            StatementKind::NextSentence => {
                self.next_sentence
                    .push(NextSentenceTransfer { node, sentence });
                self.fallthrough_open = false;
            }
            StatementKind::GoBack | StatementKind::StopRun => {
                self.fallthrough_open = false;
            }
            _ => {}
        }
        Ok(node)
    }

    fn push_branch(
        &mut self,
        sentence: usize,
        line: usize,
        text: &str,
        range: Range<usize>,
        expected: &[ControlScope],
        kind: ControlEdgeKind,
    ) -> Result<usize, HirProblem> {
        let frame = self
            .scopes
            .iter()
            .rev()
            .find(|frame| expected.contains(&frame.scope))
            .copied()
            .ok_or(HirProblem::UnmatchedScope)?;
        self.record_branch_tail(frame.start);
        self.fallthrough_open = false;
        let node = self.push_node(
            sentence,
            line,
            ControlRole::Branch,
            Some(frame.scope),
            None,
            text.trim().to_string(),
            range,
        );
        self.nodes[node].parent = Some(frame.start);
        *self.branch_counts.entry(frame.start).or_default() += 1;
        self.edges.push(ControlEdge {
            from: frame.start,
            to: node,
            kind,
        });
        Ok(node)
    }

    fn close_scope(
        &mut self,
        sentence: usize,
        line: usize,
        scope: ControlScope,
        text: &str,
        range: Range<usize>,
    ) -> Result<(), HirProblem> {
        let frame = self.scopes.pop().ok_or(HirProblem::UnmatchedScope)?;
        if frame.scope != scope {
            return Err(HirProblem::UnmatchedScope);
        }
        self.record_branch_tail(frame.start);
        self.fallthrough_open = false;
        let node = self.push_node(
            sentence,
            line,
            ControlRole::BlockEnd,
            Some(scope),
            None,
            text.trim().to_string(),
            range,
        );
        self.nodes[node].parent = Some(frame.start);
        self.connect_scope_end(frame, node);
        Ok(())
    }

    fn push_terminator(&mut self, sentence: usize, line: usize, text: &str, range: Range<usize>) {
        if let Some(owner) = self.condition_parent.take() {
            self.record_branch_tail(owner);
            self.fallthrough_open = false;
            let node = self.push_node(
                sentence,
                line,
                ControlRole::Terminator,
                None,
                None,
                text.trim().to_string(),
                range,
            );
            self.nodes[node].parent = Some(owner);
            for tail in self.branch_tails.remove(&owner).unwrap_or_default() {
                self.edges.push(ControlEdge {
                    from: tail,
                    to: node,
                    kind: ControlEdgeKind::Fallthrough,
                });
            }
        } else {
            self.push_node(
                sentence,
                line,
                ControlRole::Terminator,
                None,
                None,
                text.trim().to_string(),
                range,
            );
        }
    }

    fn push_label(
        &mut self,
        sentence: usize,
        line: usize,
        name: &str,
        section: bool,
        range: Range<usize>,
    ) {
        let statement = self.statements.len();
        self.statements.push(HirStatement {
            kind: StatementKind::Label,
            official: None,
            arguments: if section {
                vec![name.to_string(), "SECTION".into()]
            } else {
                vec![name.to_string()]
            },
            options: Vec::new(),
            line,
            source: Vec::new(),
        });
        self.statement_ranges.push(range.clone());
        self.push_node(
            sentence,
            line,
            ControlRole::Label,
            None,
            Some(statement),
            name.to_string(),
            range,
        );
    }

    fn push_condition_branch(
        &mut self,
        sentence: usize,
        line: usize,
        text: &str,
        range: Range<usize>,
        owner: usize,
    ) {
        self.condition_parent = None;
        self.record_branch_tail(owner);
        self.fallthrough_open = false;
        let node = self.push_node(
            sentence,
            line,
            ControlRole::Branch,
            None,
            None,
            text.trim().to_string(),
            range,
        );
        self.nodes[node].parent = Some(owner);
        self.edges.push(ControlEdge {
            from: owner,
            to: node,
            kind: ControlEdgeKind::Branch,
        });
        self.condition_parent = Some(owner);
    }

    fn record_branch_tail(&mut self, start: usize) {
        if self.fallthrough_open
            && let Some(tail) = self.last_node.filter(|tail| *tail != start)
        {
            self.branch_tails.entry(start).or_default().push(tail);
        }
    }

    fn connect_scope_end(&mut self, frame: ScopeFrame, end: usize) {
        let tails = self.branch_tails.remove(&frame.start).unwrap_or_default();
        match frame.scope {
            ControlScope::Perform => {
                for tail in tails {
                    self.edges.push(ControlEdge {
                        from: tail,
                        to: frame.start,
                        kind: ControlEdgeKind::Loop,
                    });
                }
                self.edges.push(ControlEdge {
                    from: frame.start,
                    to: end,
                    kind: ControlEdgeKind::False,
                });
            }
            ControlScope::If | ControlScope::Evaluate | ControlScope::Search => {
                for tail in tails {
                    self.edges.push(ControlEdge {
                        from: tail,
                        to: end,
                        kind: ControlEdgeKind::Fallthrough,
                    });
                }
                let branch_count = self.branch_counts.remove(&frame.start).unwrap_or(0);
                if frame.scope != ControlScope::If || branch_count == 0 {
                    self.edges.push(ControlEdge {
                        from: frame.start,
                        to: end,
                        kind: if frame.scope == ControlScope::If {
                            ControlEdgeKind::False
                        } else {
                            ControlEdgeKind::Branch
                        },
                    });
                }
            }
        }
    }
}

fn paragraph_end(start: usize, labels: &[(String, usize)], node_count: usize) -> usize {
    labels
        .iter()
        .map(|(_, node)| *node)
        .find(|node| *node > start)
        .unwrap_or(node_count)
        .saturating_sub(1)
}

fn parse_procedure(source: &str, max_statements: usize) -> Result<ParsedProcedure, HirProblem> {
    use statement_grammar::ProcedureEvent;

    let syntax = statement_grammar::parse(source, max_statements)?;
    let mut parser = ProcedureParser::new(max_statements, syntax.sentence_count);
    let mut event_nodes = BTreeMap::new();
    for (event_index, (event, sentence)) in syntax
        .events
        .into_iter()
        .zip(syntax.event_sentences)
        .enumerate()
    {
        match event {
            ProcedureEvent::Statement {
                kind,
                header,
                range,
                line,
                options,
                scope,
            } => {
                let node = parser.push_statement(
                    sentence,
                    line,
                    &source[header],
                    range,
                    kind,
                    options,
                    scope,
                )?;
                event_nodes.insert(event_index, node);
            }
            ProcedureEvent::Branch {
                range,
                line,
                expected,
                edge,
            } => {
                parser.push_branch(
                    sentence,
                    line,
                    &source[range.clone()],
                    range,
                    &expected,
                    edge,
                )?;
            }
            ProcedureEvent::ConditionBranch { range, line, owner } => {
                let owner = event_nodes
                    .get(&owner)
                    .copied()
                    .ok_or(HirProblem::UnmatchedScope)?;
                parser.push_condition_branch(sentence, line, &source[range.clone()], range, owner);
            }
            ProcedureEvent::ScopeEnd { scope, range, line } => {
                parser.close_scope(sentence, line, scope, &source[range.clone()], range)?;
            }
            ProcedureEvent::Terminator { range, line } => {
                parser.push_terminator(sentence, line, &source[range.clone()], range);
            }
            ProcedureEvent::Label {
                name,
                section,
                range,
                line,
            } => {
                parser.push_label(sentence, line, &name, section, range);
            }
        }
    }
    parser.finish()
}

fn statement_role(kind: StatementKind, scope: Option<ControlScope>) -> ControlRole {
    if scope.is_some() {
        ControlRole::BlockStart
    } else if matches!(
        kind,
        StatementKind::Call
            | StatementKind::Exit
            | StatementKind::GoBack
            | StatementKind::GoTo
            | StatementKind::NextSentence
            | StatementKind::Perform
            | StatementKind::StopRun
    ) {
        ControlRole::Transfer
    } else {
        ControlRole::Statement
    }
}

fn perform_targets(text: &str) -> Option<(String, Option<String>)> {
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

fn go_to_targets(text: &str) -> Option<(Vec<String>, bool)> {
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

fn semantic_tokens(sentence: &str, keyword_words: usize) -> Vec<String> {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HirProblem {
    MissingProcedure,
    UnknownStatement(usize),
    InvalidStatement {
        kind: StatementKind,
        line: usize,
        detail: &'static str,
    },
    MalformedToken(usize),
    StatementLimitExceeded,
    InvalidLayout,
    UnsupportedForm,
    UnmatchedScope,
    UnterminatedExec,
    MissingTarget(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_registry_closes_all_44_official_statement_families() {
        assert_eq!(StatementKind::official().len(), 44);
        assert_eq!(ProcedureStatementKind::ALL.len(), 44);
        assert_eq!(
            StatementKind::official()
                .iter()
                .filter_map(|kind| kind.official_kind())
                .collect::<BTreeSet<_>>(),
            ProcedureStatementKind::ALL.into_iter().collect()
        );
        assert_eq!(
            StatementKind::official()
                .iter()
                .filter(|kind| !kind.supported())
                .count(),
            0
        );
        assert!(crate::PROCEDURE_STATEMENTS.iter().all(|descriptor| {
            !descriptor.grammar_keywords.is_empty()
                && descriptor
                    .grammar_keywords
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && descriptor.grammar_keywords.iter().all(|keyword| {
                    keyword.bytes().any(|byte| byte.is_ascii_alphabetic())
                        && keyword
                            .bytes()
                            .filter(|byte| byte.is_ascii_alphabetic())
                            .all(|byte| byte.is_ascii_uppercase())
                })
        }));
    }
    #[test]
    fn token_parser_does_not_split_decimal() {
        let parsed = parse_procedure("COMPUTE A = 1.5. STOP RUN.", 16).unwrap();
        assert_eq!(parsed.statements[0].kind, StatementKind::Compute);
        assert_eq!(parsed.statements[1].kind, StatementKind::StopRun);
    }

    #[test]
    fn token_parser_ignores_comment_and_embedded_host_periods() {
        let source = "*> sentence. comment\nEXEC SQL SELECT A.COL FROM T END-EXEC. STOP RUN.";
        let parsed = parse_procedure(source, 16).unwrap();
        assert_eq!(parsed.statements[0].kind, StatementKind::ExecSql);
        assert_eq!(parsed.statements[1].kind, StatementKind::StopRun);
        assert!(parsed.nodes[0].text.contains("A.COL"));
    }

    #[test]
    fn optional_xml_parse_and_host_dialect_operands_remain_compatible() {
        let parsed = parse_procedure(
            "ACCEPT A FROM SYSIN. XML PARSE J INTO A. EXEC CICS WRITEQ TD FROM(A) END-EXEC. EXEC DLI GU INTO(A) END-EXEC. STOP RUN.",
            16,
        )
        .unwrap();
        assert!(
            parsed
                .statements
                .iter()
                .any(|statement| statement.kind == StatementKind::XmlParse)
        );
        assert!(parsed.statements.iter().all(|statement| {
            !matches!(
                statement.kind,
                StatementKind::ExecCics | StatementKind::ExecDli
            ) || statement.options.is_empty()
        }));
    }

    #[test]
    fn same_line_and_inline_statements_have_distinct_nodes() {
        let source =
            "MOVE A TO B DISPLAY B. IF A = B DISPLAY 'YES' ELSE DISPLAY 'NO' END-IF. STOP RUN.";
        let parsed = parse_procedure(source, 32).unwrap();
        assert_eq!(
            parsed
                .statements
                .iter()
                .map(|statement| statement.kind)
                .collect::<Vec<_>>(),
            [
                StatementKind::Move,
                StatementKind::Display,
                StatementKind::If,
                StatementKind::Display,
                StatementKind::Display,
                StatementKind::StopRun,
            ]
        );
        assert_eq!(&source[parsed.statement_ranges[0].clone()], "MOVE A TO B");
        assert_eq!(&source[parsed.statement_ranges[1].clone()], "DISPLAY B");
        assert!(parsed.statement_ranges[0].end <= parsed.statement_ranges[1].start);
        assert!(
            parsed
                .node_ranges
                .iter()
                .all(|range| range.start < range.end)
        );
    }

    #[test]
    fn grammar_boundaries_ignore_line_breaks_but_retain_nested_statement_identity() {
        let parsed = parse_procedure(
            "MOVE A\n TO B DISPLAY B. EVALUATE A WHEN 1 DISPLAY 'ONE' WHEN OTHER CONTINUE END-EVALUATE. PERFORM UNTIL A = B\n DISPLAY A\nEND-PERFORM. STOP RUN.",
            64,
        )
        .unwrap();
        assert_eq!(
            parsed
                .statements
                .iter()
                .map(|statement| statement.kind)
                .collect::<Vec<_>>(),
            [
                StatementKind::Move,
                StatementKind::Display,
                StatementKind::Evaluate,
                StatementKind::Display,
                StatementKind::Continue,
                StatementKind::Perform,
                StatementKind::Display,
                StatementKind::StopRun,
            ]
        );
        assert_eq!(
            parsed
                .nodes
                .iter()
                .filter_map(|node| node.scope)
                .collect::<Vec<_>>(),
            [
                ControlScope::Evaluate,
                ControlScope::Evaluate,
                ControlScope::Evaluate,
                ControlScope::Evaluate,
                ControlScope::Perform,
                ControlScope::Perform,
            ]
        );
    }

    #[test]
    fn nested_scopes_and_explicit_terminators_have_control_identity() {
        let parsed = parse_procedure(
            "MAIN.\nIF A = 1\n EVALUATE B\n  WHEN 1\n   SEARCH T\n    WHEN C = 2\n     CONTINUE\n   END-SEARCH\n END-EVALUATE\nELSE\n CONTINUE\nEND-IF.\nSTOP RUN.",
            128,
        )
        .unwrap();
        let starts = parsed
            .nodes
            .iter()
            .filter(|node| node.role == ControlRole::BlockStart)
            .map(|node| node.scope.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            starts,
            vec![
                ControlScope::If,
                ControlScope::Evaluate,
                ControlScope::Search
            ]
        );
        assert_eq!(
            parsed
                .nodes
                .iter()
                .filter(|node| node.role == ControlRole::BlockEnd)
                .count(),
            3
        );
        assert!(
            parsed
                .edges
                .iter()
                .any(|edge| edge.kind == ControlEdgeKind::False)
        );
        assert!(parsed.nodes.iter().all(|node| {
            node.role == ControlRole::Label
                || node.parent.is_some()
                || node.scope != Some(ControlScope::Search)
        }));
    }

    #[test]
    fn perform_go_to_and_next_sentence_resolve_explicit_edges() {
        let parsed = parse_procedure(
            "MAIN.\nPERFORM WORK THRU WORK-EXIT\nPERFORM VARYING I FROM 1 BY 1 UNTIL I > 2\n CONTINUE\nEND-PERFORM\nGO TO DONE.\nWORK.\nCONTINUE.\nWORK-EXIT.\nEXIT.\nDONE.\nNEXT SENTENCE.\nDISPLAY 'SKIPPED'.\nGOBACK.",
            128,
        )
        .unwrap();
        assert!(
            parsed
                .edges
                .iter()
                .any(|edge| edge.kind == ControlEdgeKind::Call)
        );
        assert!(
            parsed
                .edges
                .iter()
                .any(|edge| edge.kind == ControlEdgeKind::Return)
        );
        assert!(
            parsed
                .edges
                .iter()
                .filter(|edge| edge.kind == ControlEdgeKind::Transfer)
                .count()
                >= 2
        );
        assert!(parsed.nodes.iter().any(|node| {
            node.role == ControlRole::BlockStart && node.scope == Some(ControlScope::Perform)
        }));
    }

    #[test]
    fn malformed_or_recovered_control_cannot_build_hir() {
        assert_eq!(
            parse_procedure("END-IF.", 16).unwrap_err(),
            HirProblem::UnmatchedScope
        );
        assert_eq!(
            parse_procedure("FLY TO MARS.", 16).unwrap_err(),
            HirProblem::UnknownStatement(1)
        );
        assert_eq!(
            parse_procedure("GO TO MISSING.", 16).unwrap_err(),
            HirProblem::MissingTarget("MISSING".into())
        );
        assert_eq!(
            parse_procedure("EXEC CICS RETURN.", 16).unwrap_err(),
            HirProblem::UnterminatedExec
        );

        let redundant = parse_procedure("DUP. EXIT. DUP. EXIT.", 16).unwrap();
        assert!(
            redundant
                .statements
                .iter()
                .all(|statement| statement.kind != StatementKind::DuplicateLabel)
        );

        let recovered = parse_procedure("DUP. DISPLAY 'A'. DUP. EXIT.", 16).unwrap();
        assert!(
            recovered
                .nodes
                .iter()
                .any(|node| { node.role == ControlRole::Recovered && node.text == "DUP" })
        );
        assert!(recovered.statements.iter().any(|statement| {
            statement.kind == StatementKind::DuplicateLabel && !statement.kind.supported()
        }));
    }
}
