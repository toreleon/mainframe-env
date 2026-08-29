use crate::{CobolLayout, LosslessSyntax, SemanticModel};
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
    Display,
    Divide,
    Entry,
    Evaluate,
    ExecCics,
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
    Open,
    Perform,
    Read,
    Release,
    ReturnStatement,
    Search,
    Set,
    Sort,
    StopRun,
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
            Self::Display => "display",
            Self::Divide => "divide",
            Self::Entry => "entry",
            Self::Evaluate => "evaluate",
            Self::ExecCics => "exec_cics",
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
            Self::Open => "open",
            Self::Perform => "perform",
            Self::Read => "read",
            Self::Release => "release",
            Self::ReturnStatement => "return_statement",
            Self::Search => "search",
            Self::Set => "set",
            Self::Sort => "sort",
            Self::StopRun => "stop_run",
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
            Self::ExecSql
                | Self::Invoke
                | Self::Merge
                | Self::Release
                | Self::ReturnStatement
                | Self::Sort
        )
    }

    #[must_use]
    pub const fn frozen() -> [Self; 43] {
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
            Self::Display,
            Self::Divide,
            Self::Entry,
            Self::Evaluate,
            Self::ExecCics,
            Self::ExecSql,
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
            Self::Search,
            Self::Set,
            Self::Sort,
            Self::StopRun,
            Self::String,
            Self::Subtract,
            Self::Unstring,
            Self::Write,
            Self::XmlGenerate,
            Self::XmlParse,
        ]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirStatement {
    pub kind: StatementKind,
    pub arguments: Vec<String>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolHir {
    pub program_id: String,
    pub layouts: Vec<CobolLayout>,
    pub statements: Vec<HirStatement>,
    pub module: Module,
}

impl CobolHir {
    pub(crate) fn build(
        syntax: &LosslessSyntax,
        semantic: &SemanticModel,
        limits: IrLimits,
    ) -> Result<Self, HirProblem> {
        let procedure =
            procedure_text(syntax.semantic_text()).ok_or(HirProblem::MissingProcedure)?;
        let mut statements = Vec::new();
        for (line, sentence) in split_sentences(procedure) {
            if sentence.trim().is_empty() {
                continue;
            }
            let kind = classify(&sentence).ok_or(HirProblem::UnknownStatement)?;
            let arguments = if kind == StatementKind::Label {
                vec![sentence.trim().to_ascii_uppercase()]
            } else {
                semantic_tokens(&sentence)
            };
            validate_form(kind, &arguments)?;
            statements.push(HirStatement {
                kind,
                arguments,
                line,
            });
        }
        if statements.len() > limits.max_operations.saturating_sub(1) {
            return Err(HirProblem::StatementLimitExceeded);
        }
        statements.push(HirStatement {
            kind: StatementKind::ProgramEnd,
            arguments: Vec::new(),
            line: syntax.semantic_text().lines().count(),
        });
        let module = build_module(&statements, &semantic.layouts, limits)?;
        Ok(Self {
            program_id: semantic.program_id.clone(),
            layouts: semantic.layouts.clone(),
            statements,
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
    for kind in StatementKind::frozen()
        .into_iter()
        .chain([StatementKind::Label, StatementKind::ProgramEnd])
    {
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
        let id = builder
            .add_storage(
                layout.name.to_ascii_lowercase(),
                layout.length as u64,
                alias,
            )
            .map_err(|_| HirProblem::InvalidLayout)?;
        storage.insert(layout.name.clone(), id);
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
        for (index, argument) in statement.arguments.iter().enumerate() {
            attributes.insert(format!("arg_{index:03}"), Attribute::Text(argument.clone()));
        }
        builder
            .add_operation(
                block,
                OperationIdentity::new("cobol.hir", statement.kind.slug(), 1)
                    .map_err(|_| HirProblem::UnknownStatement)?,
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
        K::Open | K::Close | K::Read => vec![Effect::DatasetRead],
        K::Write => vec![Effect::DatasetWrite],
        K::ExecCics => vec![Effect::ProgramControl, Effect::Condition],
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

fn procedure_text(source: &str) -> Option<&str> {
    let upper = source.to_ascii_uppercase();
    let start = upper.find("PROCEDURE DIVISION")?;
    let rest = &source[start..];
    let dot = rest.find('.')?;
    Some(&rest[dot + 1..])
}
fn split_sentences(source: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut line = 1usize;
    let mut start_line = 1usize;
    let chars: Vec<char> = source.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
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
        if *ch == '.' && quote.is_none() && !decimal {
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
        ("EXEC SQL", StatementKind::ExecSql),
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
        "RETURN" => StatementKind::ReturnStatement,
        "SEARCH" => StatementKind::Search,
        "SET" => StatementKind::Set,
        "SORT" => StatementKind::Sort,
        "STRING" => StatementKind::String,
        "SUBTRACT" => StatementKind::Subtract,
        "UNSTRING" => StatementKind::Unstring,
        "WRITE" => StatementKind::Write,
        _ if upper.split_whitespace().count() == 1 => StatementKind::Label,
        _ => return None,
    })
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
        K::Perform => {
            arguments.first().is_some_and(|first| first != "VARYING") && !contains("UNTIL")
        }
        K::If => arguments.len() >= 4 && contains("DISPLAY"),
        K::Evaluate => !arguments.is_empty() && contains("WHEN"),
        K::Search => !arguments.is_empty() && contains("WHEN"),
        K::JsonParse | K::XmlParse => contains("INTO"),
        K::Move => contains("TO"),
        K::Add => contains("TO"),
        K::Subtract => contains("FROM"),
        K::Multiply => contains("BY"),
        K::Divide => contains("INTO"),
        K::Compute => contains("="),
        K::String | K::Unstring => contains("INTO"),
        K::GoTo
        | K::Call
        | K::Cancel
        | K::Accept
        | K::Display
        | K::Initialize
        | K::Inspect
        | K::Allocate
        | K::Free
        | K::Set
        | K::Open
        | K::Close
        | K::Read
        | K::Write
        | K::ExecCics => !arguments.is_empty(),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(HirProblem::UnsupportedForm)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HirProblem {
    MissingProcedure,
    UnknownStatement,
    StatementLimitExceeded,
    InvalidLayout,
    UnsupportedForm,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_freezes_43_statement_variants() {
        assert_eq!(StatementKind::frozen().len(), 43);
        assert_eq!(
            StatementKind::frozen()
                .iter()
                .filter(|kind| !kind.supported())
                .count(),
            6
        );
    }
    #[test]
    fn sentence_split_does_not_split_decimal() {
        assert_eq!(split_sentences("COMPUTE A = 1.5. STOP RUN.").len(), 2);
    }
}
