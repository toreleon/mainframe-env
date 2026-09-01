use crate::{
    CobolFileBinding, CobolLayout, LosslessSyntax, ProcedureStatementKind, SemanticModel,
    SourceSpan,
};
use mainframe_env_ir::{
    Attribute, Effect, IrLimits, Module, ModuleBuilder, OperationCatalog, OperationIdentity,
    OperationSchema, StorageReference,
};
use std::collections::{BTreeMap, BTreeSet};

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
        !matches!(
            self,
            Self::DuplicateLabel
                | Self::Invoke
                | Self::Merge
                | Self::Release
                | Self::ReturnStatement
                | Self::Sort
                | Self::Delete
                | Self::Start
                | Self::StructuredControl
        )
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
        for statement in &mut parsed.statements {
            if let Some(range) = line_range(procedure, statement.line) {
                statement.source = crate::syntax::source_spans(
                    syntax.semantic_origins(),
                    procedure_offset + range.start..procedure_offset + range.end,
                );
            }
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
        .map(|layout| (layout.qualified_name.clone(), layout.length))
        .collect::<BTreeMap<_, _>>();
    for layout in layouts {
        if let Some(target) = &layout.alias_of {
            let required = backing_lengths.entry(target.clone()).or_default();
            *required = (*required).max(layout.length);
        }
    }
    for layout in layouts.iter().filter(|layout| layout.length > 0) {
        let alias = layout
            .alias_of
            .as_ref()
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

fn line_range(source: &str, line: usize) -> Option<std::ops::Range<usize>> {
    if line == 0 {
        return None;
    }
    let mut current = 1usize;
    let mut start = 0usize;
    for (index, byte) in source.bytes().enumerate() {
        if current == line && byte == b'\n' {
            return Some(start..index);
        }
        if byte == b'\n' {
            current += 1;
            start = index + 1;
        }
    }
    (current == line).then_some(start..source.len())
}

fn statement_options(text: &str) -> Vec<StatementOption> {
    let upper = text.to_ascii_uppercase();
    let tokens = semantic_tokens(text);
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
    nodes: Vec<ControlNode>,
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
    nodes: Vec<ControlNode>,
    edges: Vec<ControlEdge>,
    scopes: Vec<ScopeFrame>,
    branch_tails: BTreeMap<usize, Vec<usize>>,
    branch_counts: BTreeMap<usize, usize>,
    transfers: Vec<InternalTransfer>,
    next_sentence: Vec<NextSentenceTransfer>,
    sentence_first_nodes: Vec<Option<usize>>,
    last_node: Option<usize>,
    fallthrough_open: bool,
    has_unsupported_include: bool,
    max_statements: usize,
}

impl ProcedureParser {
    fn new(max_statements: usize, sentence_count: usize) -> Self {
        Self {
            statements: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            scopes: Vec::new(),
            branch_tails: BTreeMap::new(),
            branch_counts: BTreeMap::new(),
            transfers: Vec::new(),
            next_sentence: Vec::new(),
            sentence_first_nodes: vec![None; sentence_count],
            last_node: None,
            fallthrough_open: true,
            has_unsupported_include: false,
            max_statements,
        }
    }

    fn finish(mut self) -> Result<ParsedProcedure, HirProblem> {
        if !self.scopes.is_empty() {
            return Err(HirProblem::UnmatchedScope);
        }
        for statement in &self.statements {
            validate_form(statement.kind, &statement.arguments)?;
            validate_statement_options(statement.kind, &statement.options)?;
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
                });
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
            nodes: self.nodes,
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
    ) -> usize {
        let id = self.nodes.len();
        let parent = self.scopes.last().map(|frame| frame.start);
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
        });
        self.last_node = Some(id);
        self.fallthrough_open = true;
        id
    }

    fn push_statement(
        &mut self,
        sentence: usize,
        line: usize,
        text: &str,
        kind: StatementKind,
    ) -> Result<usize, HirProblem> {
        if self.statements.len() >= self.max_statements {
            return Err(HirProblem::StatementLimitExceeded);
        }
        let arguments = semantic_tokens(text);
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
            options: statement_options(text),
            line,
            source: Vec::new(),
        });
        let scope = scope_for(kind, text);
        let role = statement_role(kind, scope);
        let node = self.push_node(
            sentence,
            line,
            role,
            scope,
            Some(statement),
            text.trim().to_string(),
        );
        if let Some(scope) = scope {
            self.scopes.push(ScopeFrame { scope, start: node });
        }
        match kind {
            StatementKind::GoTo => {
                let target = go_to_target(text).ok_or(HirProblem::UnsupportedForm)?;
                self.transfers.push(InternalTransfer {
                    node,
                    target,
                    through: None,
                    kind: ControlEdgeKind::Transfer,
                });
                self.fallthrough_open = false;
            }
            StatementKind::Perform if scope.is_none() => {
                let (target, through) = perform_targets(text).ok_or(HirProblem::UnsupportedForm)?;
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
        );
        self.nodes[node].parent = Some(frame.start);
        self.connect_scope_end(frame, node);
        Ok(())
    }

    fn close_sentence_scopes(
        &mut self,
        sentence: usize,
        line: usize,
        depth: usize,
    ) -> Result<(), HirProblem> {
        if self.scopes.len() < depth {
            return Err(HirProblem::UnmatchedScope);
        }
        while self.scopes.len() > depth {
            let frame = self.scopes.pop().ok_or(HirProblem::UnmatchedScope)?;
            self.record_branch_tail(frame.start);
            self.fallthrough_open = false;
            let node = self.push_node(
                sentence,
                line,
                ControlRole::BlockEnd,
                Some(frame.scope),
                None,
                ".".to_string(),
            );
            self.nodes[node].parent = Some(frame.start);
            self.connect_scope_end(frame, node);
        }
        Ok(())
    }

    fn push_terminator(&mut self, sentence: usize, line: usize, text: &str) {
        self.push_node(
            sentence,
            line,
            ControlRole::Terminator,
            None,
            None,
            text.trim().to_string(),
        );
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
    let sentences = split_sentences(source);
    let mut parser = ProcedureParser::new(max_statements, sentences.len());
    for (sentence_index, (start_line, sentence)) in sentences.iter().enumerate() {
        let depth = parser.scopes.len();
        let mut exec: Option<(usize, String)> = None;
        let mut extendable_statement = None;
        let mut extendable_control = None;
        for (offset, raw) in sentence.lines().enumerate() {
            let line_number = start_line.saturating_add(offset);
            let line = raw.trim();
            if line.is_empty() || line.starts_with("*>") {
                continue;
            }
            let upper = line.to_ascii_uppercase();
            if let Some((exec_line, exec_text)) = &mut exec {
                exec_text.push(' ');
                exec_text.push_str(line);
                if contains_word(&upper, "END-EXEC") {
                    let text = std::mem::take(exec_text);
                    let kind = classify(&text).ok_or(HirProblem::UnknownStatement(*exec_line))?;
                    parser.push_statement(sentence_index, *exec_line, &text, kind)?;
                    exec = None;
                }
                continue;
            }
            if is_exec_start(&upper) {
                if contains_word(&upper, "END-EXEC") {
                    let kind = classify(line).ok_or(HirProblem::UnknownStatement(line_number))?;
                    parser.push_statement(sentence_index, line_number, line, kind)?;
                } else {
                    exec = Some((line_number, line.to_string()));
                }
                extendable_statement = None;
                extendable_control = None;
                continue;
            }
            if let Some(scope) = explicit_scope_end(&upper) {
                parser.close_scope(sentence_index, line_number, scope, line)?;
                extendable_statement = None;
                extendable_control = None;
            } else if is_terminator(&upper) {
                parser.push_terminator(sentence_index, line_number, line);
                extendable_statement = None;
                extendable_control = None;
            } else if upper == "ELSE" || upper.starts_with("ELSE ") {
                let node = parser.push_branch(
                    sentence_index,
                    line_number,
                    line,
                    &[ControlScope::If],
                    ControlEdgeKind::False,
                )?;
                extendable_statement = None;
                extendable_control = Some(node);
            } else if upper == "WHEN" || upper.starts_with("WHEN ") {
                let node = parser.push_branch(
                    sentence_index,
                    line_number,
                    line,
                    &[ControlScope::Evaluate, ControlScope::Search],
                    ControlEdgeKind::Branch,
                )?;
                extendable_statement = None;
                extendable_control = Some(node);
            } else if let Some((condition, action)) = conditional_branch_parts(line) {
                let node = parser.push_node(
                    sentence_index,
                    line_number,
                    ControlRole::Branch,
                    None,
                    None,
                    condition,
                );
                if action.is_empty() {
                    extendable_statement = None;
                    extendable_control = Some(node);
                } else {
                    let kind =
                        classify(&action).ok_or(HirProblem::UnknownStatement(line_number))?;
                    let action_node =
                        parser.push_statement(sentence_index, line_number, &action, kind)?;
                    extendable_statement = parser.nodes[action_node].statement;
                    extendable_control = None;
                    add_inline_markers(
                        &mut parser,
                        sentence_index,
                        line_number,
                        &action.to_ascii_uppercase(),
                        kind,
                    )?;
                }
            } else if extendable_statement.is_none()
                && extendable_control.is_none()
                && classify(line).is_none()
                && is_paragraph_header(&upper)
            {
                let statement = parser.statements.len();
                if statement >= max_statements {
                    return Err(HirProblem::StatementLimitExceeded);
                }
                let label = upper
                    .split_whitespace()
                    .next()
                    .ok_or(HirProblem::UnknownStatement(line_number))?;
                parser.statements.push(HirStatement {
                    kind: StatementKind::Label,
                    official: None,
                    arguments: vec![label.to_string()],
                    options: Vec::new(),
                    line: line_number,
                    source: Vec::new(),
                });
                parser.push_node(
                    sentence_index,
                    line_number,
                    ControlRole::Label,
                    None,
                    Some(statement),
                    label.to_string(),
                );
                extendable_statement = None;
                extendable_control = None;
            } else if let Some(kind) = classify(line) {
                let node = parser.push_statement(sentence_index, line_number, line, kind)?;
                extendable_statement = parser.nodes[node].statement;
                extendable_control = None;
                add_inline_markers(&mut parser, sentence_index, line_number, &upper, kind)?;
            } else if let Some(statement) = extendable_statement {
                let extra = semantic_tokens_continuation(line);
                if extra.is_empty() {
                    return Err(HirProblem::UnknownStatement(line_number));
                }
                parser.statements[statement].arguments.extend(extra);
                if let Some(node) = parser
                    .nodes
                    .iter_mut()
                    .rev()
                    .find(|node| node.statement == Some(statement))
                {
                    node.text.push(' ');
                    node.text.push_str(line);
                }
            } else if let Some(node) = extendable_control {
                parser.nodes[node].text.push(' ');
                parser.nodes[node].text.push_str(line);
            } else {
                return Err(HirProblem::UnknownStatement(line_number));
            }
        }
        if exec.is_some() {
            return Err(HirProblem::UnterminatedExec);
        }
        let end_line = start_line.saturating_add(sentence.lines().count().saturating_sub(1));
        parser.close_sentence_scopes(sentence_index, end_line, depth)?;
    }
    parser.finish()
}

fn add_inline_markers(
    parser: &mut ProcedureParser,
    sentence: usize,
    line: usize,
    upper: &str,
    kind: StatementKind,
) -> Result<(), HirProblem> {
    if kind == StatementKind::If && contains_word_after_first(upper, "ELSE") {
        parser.push_branch(
            sentence,
            line,
            "ELSE",
            &[ControlScope::If],
            ControlEdgeKind::False,
        )?;
    }
    if matches!(kind, StatementKind::Evaluate | StatementKind::Search)
        && contains_word_after_first(upper, "WHEN")
    {
        parser.push_branch(
            sentence,
            line,
            "WHEN",
            &[ControlScope::Evaluate, ControlScope::Search],
            ControlEdgeKind::Branch,
        )?;
    }
    if let Some(scope) = inline_scope_end(upper) {
        parser.close_scope(sentence, line, scope, scope_end_text(scope))?;
    }
    Ok(())
}

fn scope_end_text(scope: ControlScope) -> &'static str {
    match scope {
        ControlScope::If => "END-IF",
        ControlScope::Evaluate => "END-EVALUATE",
        ControlScope::Search => "END-SEARCH",
        ControlScope::Perform => "END-PERFORM",
    }
}

fn split_sentences(source: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut comment = false;
    let mut in_exec = false;
    let mut word = String::new();
    let mut previous_word = String::new();
    let mut line = 1usize;
    let mut start_line = 1usize;
    let chars: Vec<char> = source.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if comment {
            if *ch == '\n' {
                comment = false;
                current.push(*ch);
                line += 1;
                if current.trim().is_empty() {
                    start_line = line;
                }
            }
            continue;
        }
        if quote.is_none() && *ch == '*' && chars.get(index.saturating_add(1)) == Some(&'>') {
            comment = true;
            word.clear();
            previous_word.clear();
            continue;
        }
        if quote.is_none() {
            if ch.is_ascii_alphanumeric() || *ch == '-' {
                word.push(ch.to_ascii_uppercase());
            } else if !word.is_empty() {
                let completed = std::mem::take(&mut word);
                if previous_word == "EXEC" && matches!(completed.as_str(), "CICS" | "SQL" | "DLI") {
                    in_exec = true;
                }
                if completed == "END-EXEC" {
                    in_exec = false;
                }
                previous_word = completed;
            }
        }
        if matches!(*ch, '\'' | '"') {
            if quote == Some(*ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(*ch);
            }
        }
        let decimal = *ch == '.'
            && index > 0
            && index + 1 < chars.len()
            && chars[index - 1].is_ascii_digit()
            && chars[index + 1].is_ascii_digit();
        if *ch == '.' && quote.is_none() && !decimal && !in_exec {
            result.push((start_line, current.trim().to_string()));
            current.clear();
            start_line = line;
        } else {
            current.push(*ch);
        }
        if *ch == '\n' {
            line += 1;
            if current.trim().is_empty() {
                start_line = line;
            }
        }
    }
    if !current.trim().is_empty() {
        result.push((start_line, current.trim().to_string()));
    }
    result
}
fn classify(sentence: &str) -> Option<StatementKind> {
    let upper = sentence.trim().to_ascii_uppercase();
    let pairs = [
        ("STOP RUN", StatementKind::StopRun),
        ("GO BACK", StatementKind::GoBack),
        ("GOBACK", StatementKind::GoBack),
        ("GO TO", StatementKind::GoTo),
        ("EXEC CICS", StatementKind::ExecCics),
        ("EXEC DLI", StatementKind::ExecDli),
        ("EXEC SQL", StatementKind::ExecSql),
        ("NEXT SENTENCE", StatementKind::NextSentence),
        ("JSON GENERATE", StatementKind::JsonGenerate),
        ("JSON PARSE", StatementKind::JsonParse),
        ("XML GENERATE", StatementKind::XmlGenerate),
        ("XML PARSE", StatementKind::XmlParse),
    ];
    for (prefix, kind) in pairs {
        if upper.starts_with(prefix) {
            return Some(kind);
        }
    }
    let first = upper.split_whitespace().next()?;
    Some(match first {
        "ACCEPT" => StatementKind::Accept,
        "ADD" => StatementKind::Add,
        "ALLOCATE" => StatementKind::Allocate,
        "ALTER" => StatementKind::Alter,
        "CALL" => StatementKind::Call,
        "CANCEL" => StatementKind::Cancel,
        "CLOSE" => StatementKind::Close,
        "COMPUTE" => StatementKind::Compute,
        "CONTINUE" => StatementKind::Continue,
        "DELETE" => StatementKind::Delete,
        "DISPLAY" => StatementKind::Display,
        "DIVIDE" => StatementKind::Divide,
        "ENTRY" => StatementKind::Entry,
        "EVALUATE" => StatementKind::Evaluate,
        "EXIT" => StatementKind::Exit,
        "FREE" => StatementKind::Free,
        "IF" => StatementKind::If,
        "INITIALIZE" => StatementKind::Initialize,
        "INSPECT" => StatementKind::Inspect,
        "INVOKE" => StatementKind::Invoke,
        "MERGE" => StatementKind::Merge,
        "MOVE" => StatementKind::Move,
        "MULTIPLY" => StatementKind::Multiply,
        "OPEN" => StatementKind::Open,
        "PERFORM" => StatementKind::Perform,
        "READ" => StatementKind::Read,
        "RELEASE" => StatementKind::Release,
        "REWRITE" => StatementKind::Rewrite,
        "RETURN" => StatementKind::ReturnStatement,
        "SEARCH" => StatementKind::Search,
        "SET" => StatementKind::Set,
        "SORT" => StatementKind::Sort,
        "START" => StatementKind::Start,
        "STRING" => StatementKind::String,
        "SUBTRACT" => StatementKind::Subtract,
        "UNSTRING" => StatementKind::Unstring,
        "WRITE" => StatementKind::Write,
        _ => return None,
    })
}

fn semantic_tokens_continuation(text: &str) -> Vec<String> {
    semantic_tokens(&format!("_ {text}"))
}

fn scope_for(kind: StatementKind, text: &str) -> Option<ControlScope> {
    match kind {
        StatementKind::If => Some(ControlScope::If),
        StatementKind::Evaluate => Some(ControlScope::Evaluate),
        StatementKind::Search => Some(ControlScope::Search),
        StatementKind::Perform if is_inline_perform(text) => Some(ControlScope::Perform),
        _ => None,
    }
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

fn is_inline_perform(text: &str) -> bool {
    let upper = text.trim().to_ascii_uppercase();
    let mut words = upper.split_whitespace();
    if words.next() != Some("PERFORM") {
        return false;
    }
    match words.next() {
        None | Some("UNTIL" | "VARYING" | "WITH") => true,
        Some(first) if first.chars().all(|ch| ch.is_ascii_digit()) => true,
        Some(_) => false,
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

fn go_to_target(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.trim_matches([',', '.']).to_ascii_uppercase())
        .collect();
    words
        .windows(2)
        .find(|pair| pair[0] == "TO")
        .map(|pair| pair[1].clone())
}

fn is_exec_start(upper: &str) -> bool {
    ["EXEC CICS", "EXEC SQL", "EXEC DLI"]
        .iter()
        .any(|prefix| upper.starts_with(prefix))
}

fn is_paragraph_header(upper: &str) -> bool {
    let words: Vec<&str> = upper.split_whitespace().collect();
    let candidate = match words.as_slice() {
        [name] => *name,
        [name, "SECTION"] => *name,
        _ => return false,
    };
    !candidate.is_empty()
        && candidate.chars().any(|ch| ch.is_ascii_alphabetic())
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn explicit_scope_end(upper: &str) -> Option<ControlScope> {
    if upper == "END-IF" {
        Some(ControlScope::If)
    } else if upper == "END-EVALUATE" {
        Some(ControlScope::Evaluate)
    } else if upper == "END-SEARCH" {
        Some(ControlScope::Search)
    } else if upper == "END-PERFORM" {
        Some(ControlScope::Perform)
    } else {
        None
    }
}

fn inline_scope_end(upper: &str) -> Option<ControlScope> {
    [
        ("END-IF", ControlScope::If),
        ("END-EVALUATE", ControlScope::Evaluate),
        ("END-SEARCH", ControlScope::Search),
        ("END-PERFORM", ControlScope::Perform),
    ]
    .into_iter()
    .find(|(marker, _)| contains_word_after_first(upper, marker))
    .map(|(_, scope)| scope)
}

fn is_terminator(upper: &str) -> bool {
    upper.split_whitespace().next().is_some_and(|word| {
        matches!(
            word,
            "END-ACCEPT"
                | "END-ADD"
                | "END-CALL"
                | "END-COMPUTE"
                | "END-DELETE"
                | "END-DISPLAY"
                | "END-DIVIDE"
                | "END-INVOKE"
                | "END-MULTIPLY"
                | "END-READ"
                | "END-RECEIVE"
                | "END-RETURN"
                | "END-REWRITE"
                | "END-START"
                | "END-STRING"
                | "END-SUBTRACT"
                | "END-UNSTRING"
                | "END-WRITE"
                | "END-XML"
        )
    })
}

fn conditional_branch_parts(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let upper = trimmed.to_ascii_uppercase();
    [
        "NOT AT END",
        "AT END",
        "NOT INVALID KEY",
        "INVALID KEY",
        "NOT ON EXCEPTION",
        "ON EXCEPTION",
        "NOT ON SIZE ERROR",
        "ON SIZE ERROR",
        "NOT ON OVERFLOW",
        "ON OVERFLOW",
        "OVERFLOW",
    ]
    .iter()
    .find_map(|prefix| {
        (upper == *prefix || upper.starts_with(&format!("{prefix} "))).then(|| {
            (
                (*prefix).to_string(),
                trimmed[prefix.len()..].trim_start().to_string(),
            )
        })
    })
}

fn contains_word(text: &str, needle: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-'))
        .any(|word| word == needle)
}

fn contains_word_after_first(text: &str, needle: &str) -> bool {
    text.split_whitespace()
        .skip(1)
        .map(|word| word.trim_matches(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-')))
        .any(|word| word == needle)
}
fn semantic_tokens(sentence: &str) -> Vec<String> {
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
        .skip(1)
        .map(|token| {
            if token.starts_with(['\'', '"']) {
                token
            } else {
                token.to_ascii_uppercase()
            }
        })
        .collect()
}

fn validate_form(kind: StatementKind, arguments: &[String]) -> Result<(), HirProblem> {
    use StatementKind as K;
    let contains = |value: &str| arguments.iter().any(|argument| argument == value);
    let valid = match kind {
        K::Accept
        | K::Allocate
        | K::Call
        | K::Cancel
        | K::Close
        | K::Display
        | K::Entry
        | K::Free
        | K::Initialize
        | K::Read
        | K::Release
        | K::ReturnStatement
        | K::Rewrite
        | K::Search
        | K::Write => !arguments.is_empty(),
        K::Add => contains("TO") || contains("GIVING"),
        K::Alter => contains("TO"),
        K::Compute => arguments.first().is_some_and(|argument| argument != "=") && contains("="),
        K::Continue | K::GoBack => arguments.is_empty(),
        K::Delete => contains("RECORD") && !arguments.is_empty(),
        K::Subtract => contains("FROM"),
        K::Multiply => contains("BY"),
        K::Divide => contains("INTO") || contains("BY"),
        K::Evaluate | K::If | K::Perform => !arguments.is_empty(),
        K::Exit => arguments.first().is_none_or(|argument| {
            matches!(
                argument.as_str(),
                "PROGRAM" | "METHOD" | "FUNCTION" | "PERFORM" | "PARAGRAPH" | "SECTION"
            )
        }),
        K::GoTo => contains("TO") && arguments.len() >= 2,
        K::Inspect => {
            arguments.len() >= 2
                && arguments.iter().any(|argument| {
                    matches!(argument.as_str(), "TALLYING" | "REPLACING" | "CONVERTING")
                })
        }
        K::Invoke => arguments.len() >= 2,
        K::JsonGenerate | K::XmlGenerate => arguments.len() >= 3 && contains("FROM"),
        K::JsonParse => arguments.len() >= 3 && contains("INTO"),
        K::XmlParse => arguments.len() >= 4 && contains("PROCESSING") && contains("PROCEDURE"),
        K::Merge => contains("USING") && contains("GIVING"),
        K::Move => contains("TO"),
        K::Open => {
            arguments.first().is_some_and(|argument| {
                matches!(argument.as_str(), "INPUT" | "OUTPUT" | "I-O" | "EXTEND")
            }) && arguments.len() >= 2
        }
        K::Set => contains("TO") || contains("BY"),
        K::Sort => {
            !arguments.is_empty()
                && arguments.iter().any(|argument| {
                    matches!(argument.as_str(), "USING" | "INPUT" | "GIVING" | "OUTPUT")
                })
        }
        K::Start => !arguments.is_empty(),
        K::StopRun => {
            arguments == ["RUN"]
                || (arguments.first().is_some_and(|argument| argument == "RUN")
                    && arguments
                        .get(1)
                        .is_some_and(|argument| argument == "RETURNING")
                    && arguments.len() == 3)
        }
        K::String | K::Unstring => contains("INTO"),
        K::ExecCics | K::ExecDli | K::ExecSql => !arguments.is_empty(),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(HirProblem::UnsupportedForm)
    }
}

fn validate_statement_options(
    kind: StatementKind,
    options: &[StatementOption],
) -> Result<(), HirProblem> {
    use StatementKind as K;
    use StatementOptionKind as O;
    let allowed = |option: O| match option {
        O::AtEnd | O::NotAtEnd => matches!(kind, K::Read | K::ReturnStatement),
        O::InvalidKey | O::NotInvalidKey => {
            matches!(kind, K::Delete | K::Read | K::Rewrite | K::Start | K::Write)
        }
        O::OnException | O::NotOnException => matches!(
            kind,
            K::Accept
                | K::Call
                | K::Invoke
                | K::JsonGenerate
                | K::JsonParse
                | K::XmlGenerate
                | K::XmlParse
        ),
        O::OnOverflow | O::NotOnOverflow => matches!(kind, K::String | K::Unstring),
        O::OnSizeError | O::NotOnSizeError | O::Rounded => {
            matches!(
                kind,
                K::Add | K::Compute | K::Divide | K::Multiply | K::Subtract
            )
        }
        O::Giving => matches!(
            kind,
            K::Add | K::Divide | K::Merge | K::Multiply | K::Sort | K::Subtract
        ),
        O::Remainder => kind == K::Divide,
        O::Returning => matches!(kind, K::Allocate | K::Call | K::Invoke | K::StopRun),
        O::Using => matches!(kind, K::Call | K::Entry | K::Invoke | K::Merge | K::Sort),
        O::Into => matches!(
            kind,
            K::Divide
                | K::ExecSql
                | K::JsonParse
                | K::Read
                | K::ReturnStatement
                | K::String
                | K::Unstring
        ),
        O::From => matches!(
            kind,
            K::ExecSql
                | K::JsonGenerate
                | K::Perform
                | K::Release
                | K::Rewrite
                | K::Subtract
                | K::Write
                | K::XmlGenerate
        ),
        O::ExplicitTerminator => true,
    };
    if options.iter().all(|option| allowed(option.kind)) {
        Ok(())
    } else {
        Err(HirProblem::UnsupportedForm)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HirProblem {
    MissingProcedure,
    UnknownStatement(usize),
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
            7
        );
    }
    #[test]
    fn sentence_split_does_not_split_decimal() {
        assert_eq!(split_sentences("COMPUTE A = 1.5. STOP RUN.").len(), 2);
    }

    #[test]
    fn sentence_split_ignores_comment_and_embedded_host_periods() {
        let source = "*> sentence. comment\nEXEC SQL SELECT A.COL FROM T END-EXEC. STOP RUN.";
        let sentences = split_sentences(source);
        assert_eq!(sentences.len(), 2);
        assert!(sentences[0].1.contains("A.COL"));
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
