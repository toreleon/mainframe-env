//! Bounded common CREATE VIEW syntax; no binding, execution or row credit.
//!
//! Source: Db2 13 baseline ibm-db2-for-zos-13-2026-08-13, SQL 0059,
//! db2z_sql_createview.html (2e5f8b1f122d42ee5fceb45bff571596b834a287ae5dba17fbed4b40f9230a26).
//! The existing SELECT core is the grammar authority. Its immutable, fragment-local
//! spans are converted to owned view syntax referencing the original CREATE SQL.
//! SQL identifiers use the pinned db2z_sqlidentifiers.html rules, lines 24..25
//! (d5c99a5640234e19a310d2e44f1abd9bbe3bf9c0fea1cf87c06b24ee9f4a1f8b).
//! Identifier transfer decodes raw doubled quotes once; literal values retain
//! the existing lexer/expression representation without normalization.

use crate::{
    Db2AstLimits, Db2DataType, Db2ExpressionArena, Db2ExpressionId, Db2ExpressionKind,
    Db2FetchPosition, Db2Identifier, Db2OrderDirection, Db2OrderKey, Db2QualifiedName,
    Db2QueryExpression, Db2SelectCore, Db2SelectItem, Db2SelectQuantifier, Db2SourceLocation,
    Db2SourceSpan, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits,
    Db2Token, Db2TokenKind, lex_db2, parse_db2_select_core,
};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ViewCheckMode {
    ImplicitCascaded,
    Cascaded,
    Local,
}

/// A typed value whose byte and line/column span refers to the CREATE source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ViewLocated<T> {
    value: T,
    span: Db2SourceSpan,
}

impl<T> Db2ViewLocated<T> {
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ViewExpression {
    arena: Db2ExpressionArena,
    root: Db2ExpressionId,
}

impl Db2ViewExpression {
    #[must_use]
    pub const fn arena(&self) -> &Db2ExpressionArena {
        &self.arena
    }

    #[must_use]
    pub const fn root(&self) -> Db2ExpressionId {
        self.root
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2ViewOrderKey {
    Expression(Db2ViewLocated<Db2ViewExpression>),
    Ordinal(u32),
}

/// Owned conversion of the public SELECT core, with all nested spans relocated.
/// No wildcard projection is representable without catalog-bound width.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ViewSelectCore {
    quantifier: Db2SelectQuantifier,
    items: Vec<Db2ViewLocated<Db2ViewExpression>>,
    sources: Vec<Db2ViewLocated<Db2QualifiedName>>,
    where_condition: Option<Db2ViewLocated<Db2ViewExpression>>,
    group_by: Vec<Db2ViewLocated<Db2ViewExpression>>,
    having: Option<Db2ViewLocated<Db2ViewExpression>>,
    order_by: Vec<Db2ViewLocated<(Db2ViewOrderKey, Db2OrderDirection)>>,
    offset: Option<Db2ViewLocated<u64>>,
    fetch: Option<Db2ViewLocated<(Db2FetchPosition, Option<u64>)>>,
    span: Db2SourceSpan,
}

impl Db2ViewSelectCore {
    #[must_use]
    pub const fn quantifier(&self) -> Db2SelectQuantifier {
        self.quantifier
    }

    #[must_use]
    pub fn items(&self) -> &[Db2ViewLocated<Db2ViewExpression>] {
        &self.items
    }

    #[must_use]
    pub fn sources(&self) -> &[Db2ViewLocated<Db2QualifiedName>] {
        &self.sources
    }

    #[must_use]
    pub const fn where_condition(&self) -> Option<&Db2ViewLocated<Db2ViewExpression>> {
        self.where_condition.as_ref()
    }

    #[must_use]
    pub fn group_by(&self) -> &[Db2ViewLocated<Db2ViewExpression>] {
        &self.group_by
    }

    #[must_use]
    pub const fn having(&self) -> Option<&Db2ViewLocated<Db2ViewExpression>> {
        self.having.as_ref()
    }

    #[must_use]
    pub fn order_by(&self) -> &[Db2ViewLocated<(Db2ViewOrderKey, Db2OrderDirection)>] {
        &self.order_by
    }

    #[must_use]
    pub const fn offset(&self) -> Option<&Db2ViewLocated<u64>> {
        self.offset.as_ref()
    }

    #[must_use]
    pub const fn fetch(&self) -> Option<&Db2ViewLocated<(Db2FetchPosition, Option<u64>)>> {
        self.fetch.as_ref()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CreateViewStatement {
    view_name: Db2ViewLocated<Db2QualifiedName>,
    result_columns: Option<Vec<Db2ViewLocated<Db2Identifier>>>,
    definition: Db2ViewSelectCore,
    check_option: Option<Db2ViewLocated<Db2ViewCheckMode>>,
    span: Db2SourceSpan,
}

impl Db2CreateViewStatement {
    #[must_use]
    pub const fn view_name(&self) -> &Db2ViewLocated<Db2QualifiedName> {
        &self.view_name
    }

    #[must_use]
    pub fn result_columns(&self) -> Option<&[Db2ViewLocated<Db2Identifier>]> {
        self.result_columns.as_deref()
    }

    #[must_use]
    pub const fn definition(&self) -> &Db2ViewSelectCore {
        &self.definition
    }

    #[must_use]
    pub const fn check_option(&self) -> Option<&Db2ViewLocated<Db2ViewCheckMode>> {
        self.check_option.as_ref()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse one common CREATE VIEW. Catalog lookup, mutability, CHECK applicability,
/// general function resolution, and fullselect extensions remain binder-pending.
pub fn parse_db2_create_view_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2CreateViewStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    let tokens = lexed.tokens();
    let end = tokens.len()
        - usize::from(matches!(
            tokens.last().map(|t| &t.kind),
            Some(Db2TokenKind::Symbol(Db2Symbol::Semicolon))
        ));
    for token in &tokens[..end] {
        if matches!(token.kind, Db2TokenKind::Symbol(Db2Symbol::Semicolon)) {
            return Err(diagnostic(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                token.span.start,
                "CREATE VIEW permits only one terminal semicolon",
            ));
        }
        if matches!(
            token.kind,
            Db2TokenKind::HostVariable(_) | Db2TokenKind::ParameterMarker
        ) {
            return Err(diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                token.span.start,
                "CREATE VIEW cannot refer to host variables or parameter markers",
            ));
        }
    }
    let mut parser = ViewParser {
        tokens: &tokens[..end],
        position: 0,
        limits: ast_limits,
    };
    parser.expect_word("CREATE")?;
    parser.expect_word("VIEW")?;
    let name_start = parser.position;
    let mut parts = vec![parser.identifier()?];
    while parser.take_symbol(Db2Symbol::Period) {
        if parts.len() >= ast_limits.max_name_parts.min(3) {
            return Err(parser.error(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "view name exceeds the configured or Db2 three-part name limit",
            ));
        }
        parts.push(parser.identifier()?);
    }
    let name_span = parser.range_span(name_start, parser.position);
    let view_name = Db2ViewLocated {
        value: Db2QualifiedName::new(parts, ast_limits).map_err(|problem| {
            diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                name_span.start,
                &problem.message,
            )
        })?,
        span: name_span,
    };
    let result_columns = if parser.take_symbol(Db2Symbol::LeftParenthesis) {
        let mut columns = Vec::new();
        let mut names = BTreeSet::new();
        loop {
            if columns.len() >= ast_limits.max_list_items {
                return Err(parser.error(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "view result columns exceed the configured list limit",
                ));
            }
            let index = parser.position;
            let name = parser.identifier()?;
            if !names.insert(name.value().to_owned()) {
                return Err(diagnostic(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    parser.tokens[index].span.start,
                    "view result column names must be unique",
                ));
            }
            columns.push(Db2ViewLocated {
                value: name,
                span: parser.tokens[index].span,
            });
            if parser.take_symbol(Db2Symbol::RightParenthesis) {
                break;
            }
            parser.expect_symbol(Db2Symbol::Comma)?;
        }
        Some(columns)
    } else {
        None
    };
    parser.expect_word("AS")?;
    let select_start = parser.position;
    if parser.word_at(select_start) != Some("SELECT") {
        return Err(parser.error(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "view definition requires the existing SELECT core; CTE/fullselect forms are pending",
        ));
    }
    let mut depth = 0_usize;
    let mut select_end = end;
    for index in select_start..end {
        match tokens[index].kind {
            Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => depth += 1,
            Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) => depth -= 1,
            _ if depth == 0 && parser.word_at(index) == Some("WITH") => {
                select_end = index;
                break;
            }
            _ => {}
        }
    }
    parser.position = select_end;
    let check_option = if parser.take_word("WITH") {
        let start = select_end;
        let mode = if parser.take_word("CASCADED") {
            Db2ViewCheckMode::Cascaded
        } else if parser.take_word("LOCAL") {
            Db2ViewCheckMode::Local
        } else {
            Db2ViewCheckMode::ImplicitCascaded
        };
        parser.expect_word("CHECK")?;
        parser.expect_word("OPTION")?;
        Some(Db2ViewLocated {
            value: mode,
            span: parser.range_span(start, parser.position),
        })
    } else {
        None
    };
    if parser.position != end {
        return Err(parser.error(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "trailing CREATE VIEW clauses are outside the declared common subset",
        ));
    }
    let select_span = parser.range_span(select_start, select_end);
    // SELECT's constructors currently measure raw delimited token values. Permit
    // their bounded intermediate representation, then apply the caller's byte
    // limit to decoded names during transfer. No other limit is relaxed.
    let mut select_limits = ast_limits;
    for token in &tokens[select_start..select_end] {
        if let Db2TokenKind::Word {
            value,
            delimited: true,
        } = &token.kind
        {
            select_limits.max_identifier_bytes = select_limits
                .max_identifier_bytes
                .max(value.trim_end_matches(' ').len());
            select_limits.validate().map_err(|_| {
                diagnostic(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    token.span.start,
                    "SELECT identifier exceeds the inherited AST raw-name ceiling before decoding",
                )
            })?;
        }
    }
    let core = parse_db2_select_core(
        &source[select_span.start_byte..select_span.end_byte],
        syntax_limits,
        select_limits,
    )
    .map_err(|mut problem| {
        problem.location = relocate_location(problem.location, select_span.start);
        problem
    })?;
    let mut conversion = ViewConversion {
        base: select_span,
        limits: ast_limits,
        nodes: 0,
        list_items: result_columns.as_ref().map_or(0, Vec::len),
    };
    let definition = conversion.select(&core)?;
    validate_projection_names(&definition, result_columns.as_deref())?;
    Ok(Db2CreateViewStatement {
        view_name,
        result_columns,
        definition,
        check_option,
        span: Db2SourceSpan {
            start_byte: tokens[0].span.start_byte,
            start: tokens[0].span.start,
            end_byte: tokens[tokens.len() - 1].span.end_byte,
            end: tokens[tokens.len() - 1].span.end,
        },
    })
}

fn validate_projection_names(
    core: &Db2ViewSelectCore,
    columns: Option<&[Db2ViewLocated<Db2Identifier>]>,
) -> Result<(), Db2SyntaxDiagnostic> {
    let mut names = BTreeSet::new();
    for expression in core.items() {
        if columns.is_none() {
            let kind = expression
                .value()
                .arena()
                .get(expression.value().root())
                .map(|n| n.kind());
            let name = match kind {
                Some(Db2ExpressionKind::Column(name)) => name.parts().last(),
                _ => None,
            };
            if name.is_none_or(|name| !names.insert(name.value().to_owned())) {
                return Err(diagnostic(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    expression.span().start,
                    "unnamed or duplicate SELECT projections require explicit view result columns",
                ));
            }
        }
    }
    if columns.is_some_and(|columns| columns.len() != core.items().len()) {
        return Err(diagnostic(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            core.span().start,
            "view result column count must equal the known SELECT width",
        ));
    }
    Ok(())
}

/// Only raw token/SELECT-owned names enter this helper. A single replacement
/// pass preserves adjacent effective quotes (four raw quotes become two).
fn decoded_identifier(
    raw: &str,
    delimited: bool,
    limits: Db2AstLimits,
    location: Db2SourceLocation,
) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
    let value = if delimited {
        raw.replace("\"\"", "\"")
    } else {
        raw.to_owned()
    };
    Db2Identifier::new(value, delimited, limits).map_err(|problem| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            location,
            &problem.message,
        )
    })
}

fn decoded_name(
    raw: &Db2QualifiedName,
    limits: Db2AstLimits,
    location: Db2SourceLocation,
) -> Result<Db2QualifiedName, Db2SyntaxDiagnostic> {
    let parts = raw
        .parts()
        .iter()
        .map(|part| decoded_identifier(part.value(), part.is_delimited(), limits, location))
        .collect::<Result<Vec<_>, _>>()?;
    Db2QualifiedName::new(parts, limits).map_err(|problem| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            location,
            &problem.message,
        )
    })
}

struct ViewConversion {
    base: Db2SourceSpan,
    limits: Db2AstLimits,
    nodes: usize,
    list_items: usize,
}

impl ViewConversion {
    fn list(
        &mut self,
        count: usize,
        location: Db2SourceLocation,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        self.list_items = self.list_items.saturating_add(count);
        if self.list_items > self.limits.max_list_items {
            return Err(diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                location,
                "CREATE VIEW exceeds the aggregate list-item limit",
            ));
        }
        Ok(())
    }

    fn located<T>(&self, value: T, span: Db2SourceSpan) -> Db2ViewLocated<T> {
        Db2ViewLocated {
            value,
            span: relocate_span(span, self.base),
        }
    }

    fn expression(
        &mut self,
        expression: &Db2QueryExpression,
    ) -> Result<Db2ViewLocated<Db2ViewExpression>, Db2SyntaxDiagnostic> {
        // Query expression wrapper spans are SELECT-local, while arena nodes are
        // local to this expression fragment. Relocate each level exactly once.
        let span = relocate_span(expression.span(), self.base);
        let parsed = expression.parsed();
        self.nodes = self.nodes.saturating_add(parsed.arena().nodes().len());
        if self.nodes > self.limits.max_expression_nodes {
            return Err(diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                span.start,
                "CREATE VIEW exceeds the aggregate expression-node limit",
            ));
        }
        let mut arena = Db2ExpressionArena::new(self.limits).map_err(|problem| {
            diagnostic(
                Db2SyntaxDiagnosticCode::InvalidLimits,
                span.start,
                &problem.message,
            )
        })?;
        for node in parsed.arena().nodes() {
            let node_span = relocate_span(node.span(), span);
            let mut kind = node.kind().clone();
            match &mut kind {
                Db2ExpressionKind::Column(name)
                | Db2ExpressionKind::Function { name, .. }
                | Db2ExpressionKind::Cast {
                    data_type: Db2DataType::Distinct(name),
                    ..
                } => {
                    *name = decoded_name(name, self.limits, node_span.start)?;
                }
                _ => {}
            }
            match &kind {
                Db2ExpressionKind::Function { name, arguments } => {
                    if name.parts().last().is_some_and(|part| {
                        matches!(
                            part.value(),
                            "UNPACK"
                                | "AI_ANALOGY"
                                | "AI_COMMONALITY"
                                | "AI_SEMANTIC_CLUSTER"
                                | "AI_SIMILARITY"
                        )
                    }) {
                        return Err(diagnostic(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            node_span.start,
                            "function is forbidden in a CREATE VIEW definition",
                        ));
                    }
                    self.list(arguments.len(), node_span.start)?;
                }
                Db2ExpressionKind::Case { branches, .. } => {
                    self.list(branches.len(), node_span.start)?
                }
                Db2ExpressionKind::Cast {
                    data_type: Db2DataType::BuiltIn(data_type),
                    ..
                } => {
                    self.list(data_type.arguments().len(), node_span.start)?;
                }
                Db2ExpressionKind::HostVariable(_) | Db2ExpressionKind::ParameterMarker => {
                    return Err(diagnostic(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        node_span.start,
                        "view expression cannot contain host variables or parameters",
                    ));
                }
                _ => {}
            }
            arena.push(kind, node_span).map_err(|problem| {
                diagnostic(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    node_span.start,
                    &problem.message,
                )
            })?;
        }
        Ok(Db2ViewLocated {
            value: Db2ViewExpression {
                arena,
                root: parsed.root(),
            },
            span,
        })
    }

    fn select(&mut self, core: &Db2SelectCore) -> Result<Db2ViewSelectCore, Db2SyntaxDiagnostic> {
        self.list(
            core.items().len()
                + core.sources().len()
                + core.group_by().len()
                + core.order_by().len(),
            self.base.start,
        )?;
        let items = core
            .items()
            .iter()
            .map(|item| match item {
                Db2SelectItem::Expression(expression) => self.expression(expression),
                Db2SelectItem::Wildcard { span, .. } => Err(diagnostic(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    relocate_location(span.start, self.base.start),
                    "view wildcard width requires catalog binding",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sources = core
            .sources()
            .iter()
            .map(|source| {
                let name = decoded_name(
                    source.name(),
                    self.limits,
                    relocate_location(source.span().start, self.base.start),
                )?;
                Ok(self.located(name, source.span()))
            })
            .collect::<Result<Vec<_>, Db2SyntaxDiagnostic>>()?;
        let where_condition = core
            .where_condition()
            .map(|expression| self.expression(expression))
            .transpose()?;
        let group_by = core
            .group_by()
            .iter()
            .map(|expression| self.expression(expression))
            .collect::<Result<Vec<_>, _>>()?;
        let having = core
            .having()
            .map(|expression| self.expression(expression))
            .transpose()?;
        let order_by = core
            .order_by()
            .iter()
            .map(|item| {
                let key = match item.key() {
                    Db2OrderKey::Expression(expression) => {
                        Db2ViewOrderKey::Expression(self.expression(expression)?)
                    }
                    Db2OrderKey::Ordinal(value) => Db2ViewOrderKey::Ordinal(*value),
                };
                Ok(self.located((key, item.direction()), item.span()))
            })
            .collect::<Result<Vec<_>, Db2SyntaxDiagnostic>>()?;
        Ok(Db2ViewSelectCore {
            quantifier: core.quantifier(),
            items,
            sources,
            where_condition,
            group_by,
            having,
            order_by,
            offset: core
                .offset()
                .map(|clause| self.located(clause.row_count(), clause.span())),
            fetch: core.fetch().map(|clause| {
                self.located(
                    (clause.position(), clause.explicit_row_count()),
                    clause.span(),
                )
            }),
            span: relocate_span(core.span(), self.base),
        })
    }
}

fn relocate_location(location: Db2SourceLocation, base: Db2SourceLocation) -> Db2SourceLocation {
    Db2SourceLocation {
        line: base.line + location.line - 1,
        column: if location.line == 1 {
            base.column + location.column - 1
        } else {
            location.column
        },
    }
}

fn relocate_span(span: Db2SourceSpan, base: Db2SourceSpan) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: base.start_byte + span.start_byte,
        end_byte: base.start_byte + span.end_byte,
        start: relocate_location(span.start, base.start),
        end: relocate_location(span.end, base.start),
    }
}

fn diagnostic(
    code: Db2SyntaxDiagnosticCode,
    location: Db2SourceLocation,
    message: &str,
) -> Db2SyntaxDiagnostic {
    Db2SyntaxDiagnostic::new(code, location, message)
}

struct ViewParser<'a> {
    tokens: &'a [Db2Token],
    position: usize,
    limits: Db2AstLimits,
}

impl ViewParser<'_> {
    fn identifier(&mut self) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(Db2Token {
            kind: Db2TokenKind::Word { value, delimited },
            span,
        }) = self.tokens.get(self.position)
        else {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                "expected an unqualified SQL identifier",
            ));
        };
        if !delimited
            && matches!(
                value.as_str(),
                "AS" | "SELECT" | "WITH" | "CHECK" | "OPTION"
            )
        {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "view identifier is missing before a clause keyword",
            ));
        }
        let name = decoded_identifier(value, *delimited, self.limits, span.start)?;
        self.position += 1;
        Ok(name)
    }

    fn word_at(&self, index: usize) -> Option<&str> {
        match self.tokens.get(index).map(|token| &token.kind) {
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) => Some(value),
            _ => None,
        }
    }

    fn take_word(&mut self, word: &str) -> bool {
        if self.word_at(self.position) == Some(word) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_word(&mut self, word: &str) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_word(word) {
            Ok(())
        } else {
            Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("expected CREATE VIEW keyword {word}"),
            ))
        }
    }

    fn take_symbol(&mut self, symbol: Db2Symbol) -> bool {
        if matches!(self.tokens.get(self.position).map(|token| &token.kind), Some(Db2TokenKind::Symbol(actual)) if *actual == symbol)
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_symbol(&mut self, symbol: Db2Symbol) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_symbol(symbol) {
            Ok(())
        } else {
            Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("expected CREATE VIEW symbol {symbol:?}"),
            ))
        }
    }

    fn range_span(&self, start: usize, end: usize) -> Db2SourceSpan {
        let first = self.tokens[start].span;
        let last = self.tokens[end - 1].span;
        Db2SourceSpan {
            start_byte: first.start_byte,
            start: first.start,
            end_byte: last.end_byte,
            end: last.end,
        }
    }

    fn error(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        diagnostic(
            code,
            self.tokens.get(self.position).map_or_else(
                || {
                    self.tokens
                        .last()
                        .map_or(Db2SourceLocation::START, |token| token.span.end)
                },
                |token| token.span.start,
            ),
            message,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2CreateViewStatement, Db2SyntaxDiagnostic> {
        parse_db2_create_view_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    fn location(source: &str, byte: usize) -> Db2SourceLocation {
        source[..byte]
            .chars()
            .fold(Db2SourceLocation::START, |mut loc, ch| {
                if ch == '\n' {
                    loc.line += 1;
                    loc.column = 1;
                } else {
                    loc.column += 1;
                }
                loc
            })
    }

    fn check_span(source: &str, span: Db2SourceSpan) {
        assert!(span.start_byte < span.end_byte);
        assert!(source.get(span.start_byte..span.end_byte).is_some());
        assert_eq!(span.start, location(source, span.start_byte));
        assert_eq!(span.end, location(source, span.end_byte));
    }

    fn check_expression(source: &str, expression: &Db2ViewLocated<Db2ViewExpression>) {
        check_span(source, expression.span());
        assert!(
            expression
                .value()
                .arena()
                .get(expression.value().root())
                .is_some()
        );
        for node in expression.value().arena().nodes() {
            check_span(source, node.span());
            assert!(node.span().start_byte >= expression.span().start_byte);
            assert!(node.span().end_byte <= expression.span().end_byte);
        }
    }

    #[test]
    fn preserves_names_and_check_option_spelling() {
        for (suffix, expected) in [
            ("", None),
            (
                " WITH CHECK OPTION",
                Some(Db2ViewCheckMode::ImplicitCascaded),
            ),
            (
                " WITH CASCADED CHECK OPTION",
                Some(Db2ViewCheckMode::Cascaded),
            ),
            (" WITH LOCAL CHECK OPTION", Some(Db2ViewCheckMode::Local)),
        ] {
            let sql = format!(
                "CREATE VIEW LOC.S.\"view\" (A, \"a\") AS SELECT T.X, U.X FROM T,U{suffix}; -- done"
            );
            let view = parse(&sql).unwrap();
            assert_eq!(
                view.view_name()
                    .value()
                    .parts()
                    .iter()
                    .map(Db2Identifier::value)
                    .collect::<Vec<_>>(),
                ["LOC", "S", "view"]
            );
            assert_eq!(
                view.result_columns()
                    .unwrap()
                    .iter()
                    .map(|c| c.value().value())
                    .collect::<Vec<_>>(),
                ["A", "a"]
            );
            assert_eq!(view.check_option().map(|c| *c.value()), expected);
            assert_eq!(view.definition().items.len(), 2);
            assert_eq!(view.definition().sources.len(), 2);
            check_span(&sql, view.span());
            if let Some(clause) = view.check_option() {
                check_span(&sql, clause.span());
            }
        }
        let view = parse("create view v as select t.a, \"a\" from t").unwrap();
        assert!(view.result_columns().is_none());
        assert_eq!(
            view.definition().quantifier,
            Db2SelectQuantifier::ImplicitAll
        );
    }

    #[test]
    fn effective_identifier_values_determine_duplicates() {
        for sql in [
            "CREATE VIEW V(A,\"A\") AS SELECT X,Y FROM T",
            "CREATE VIEW V(\"A   \",A) AS SELECT X,Y FROM T",
            "CREATE VIEW V AS SELECT T.A,U.A FROM T,U",
            "CREATE VIEW V AS SELECT A,\"A\" FROM T",
            "CREATE VIEW V AS SELECT \"A   \",A FROM T",
        ] {
            assert!(parse(sql).is_err(), "{sql}");
        }
        for sql in [
            "CREATE VIEW V(A,\"a\") AS SELECT X,Y FROM T",
            "CREATE VIEW V AS SELECT A,\"a\" FROM T",
            "CREATE VIEW V(A,B) AS SELECT T.A,U.A FROM T,U",
        ] {
            assert!(parse(sql).is_ok(), "{sql}");
        }
    }

    #[test]
    fn escaped_names_decode_once_throughout_owned_view_syntax() {
        let sql = r#"/* é */
CREATE VIEW "S""c"."v""""Q   " ("r""""Q   ", R2, R3, R4) AS
SELECT "c""""Q"."a""b", "f""""Q"("a""b"),
CAST(A AS S."ty""pe"), 'a""b it''s'
FROM "S""c"."t""""Q" WHERE "w""x"=1
GROUP BY "g""x" HAVING "h""x"=1 ORDER BY "o""x" WITH CHECK OPTION;"#;
        let view = parse(sql).unwrap();
        let signature = |name: &Db2QualifiedName| {
            name.parts()
                .iter()
                .map(|part| (part.value().to_owned(), part.is_delimited()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            signature(view.view_name().value()),
            [("S\"c".into(), true), ("v\"\"Q".into(), true)]
        );
        assert_eq!(view.result_columns().unwrap()[0].value().value(), "r\"\"Q");
        assert!(view.result_columns().unwrap()[0].value().is_delimited());
        assert_eq!(
            signature(view.definition().sources()[0].value()),
            [("S\"c".into(), true), ("t\"\"Q".into(), true)]
        );
        let core = view.definition();
        let root = |index: usize| {
            core.items()[index]
                .value()
                .arena()
                .get(core.items()[index].value().root())
                .unwrap()
                .kind()
        };
        let Db2ExpressionKind::Column(name) = root(0) else {
            panic!("column")
        };
        assert_eq!(
            signature(name),
            [("c\"\"Q".into(), true), ("a\"b".into(), true)]
        );
        let Db2ExpressionKind::Function { name, .. } = root(1) else {
            panic!("function")
        };
        assert_eq!(name.parts()[0].value(), "f\"\"Q");
        let Db2ExpressionKind::Cast {
            data_type: Db2DataType::Distinct(name),
            ..
        } = root(2)
        else {
            panic!("distinct type")
        };
        assert_eq!(name.parts()[0].value(), "S");
        assert_eq!(name.parts()[1].value(), "ty\"pe");
        let Db2ExpressionKind::Literal(crate::Db2Literal::String { value, .. }) = root(3) else {
            panic!("string")
        };
        // Identifier decoding does not repair inherited raw literal escapes.
        assert_eq!(value, "a\"\"b it''s");
        for expression in core
            .items()
            .iter()
            .chain(core.where_condition())
            .chain(core.group_by())
            .chain(core.having())
        {
            check_expression(sql, expression);
        }
        for node in core.where_condition().unwrap().value().arena().nodes() {
            if let Db2ExpressionKind::Column(name) = node.kind() {
                assert_eq!(name.parts()[0].value(), "w\"x");
            }
        }
        for source in core.sources() {
            check_span(sql, source.span());
        }
        check_span(sql, view.view_name().span());
        for column in view.result_columns().unwrap() {
            check_span(sql, column.span());
        }
        let Db2ViewOrderKey::Expression(order) = &core.order_by()[0].value().0 else {
            panic!("order expression")
        };
        check_expression(sql, order);
        assert!(
            matches!(order.value().arena().get(order.value().root()).unwrap().kind(),
            Db2ExpressionKind::Column(name) if name.parts()[0].value() == "o\"x")
        );
        assert_eq!(
            &sql[core.items()[0].span().start_byte..core.items()[0].span().end_byte],
            r#""c""""Q"."a""b""#
        );
    }

    #[test]
    fn duplicate_projection_names_use_decoded_effective_values() {
        for sql in [
            r#"CREATE VIEW V AS SELECT T."a""b", U."a""b   " FROM T,U"#,
            r#"CREATE VIEW V("a""b", "a""b   ") AS SELECT A,B FROM T"#,
            r#"CREATE VIEW V AS SELECT "a""""b", "a""""b" FROM T"#,
        ] {
            assert!(parse(sql).is_err(), "{sql}");
        }
        for sql in [
            r#"CREATE VIEW V AS SELECT "a""b", "A""b" FROM T"#,
            r#"CREATE VIEW V AS SELECT "a""b", "a""""b" FROM T"#,
            r#"CREATE VIEW V("a""b", "A""b") AS SELECT A,B FROM T"#,
        ] {
            assert!(parse(sql).is_ok(), "{sql}");
        }
    }

    #[test]
    fn decoded_identifier_byte_limits_cover_every_transfer_boundary() {
        let limits = Db2AstLimits {
            max_identifier_bytes: 3,
            ..Db2AstLimits::default()
        };
        for identifier in [r#""x""y""#, r#""x""""""#, r#""é""   ""#] {
            // Each name is exactly three decoded UTF-8 bytes. Escaped raw bytes
            // can exceed three, and trailing spaces remain insignificant.
            for sql in [
                format!("CREATE VIEW {identifier} AS SELECT A FROM T"),
                format!("CREATE VIEW V({identifier}) AS SELECT A FROM T"),
                format!("CREATE VIEW V AS SELECT {identifier} FROM T"),
                format!("CREATE VIEW V(X) AS SELECT {identifier}(A) FROM T"),
                format!("CREATE VIEW V AS SELECT A FROM {identifier}"),
                format!("CREATE VIEW V(X) AS SELECT CAST(A AS S.{identifier}) FROM T"),
            ] {
                assert!(with_limits(&sql, limits).is_ok(), "{sql}");
            }
        }
        for sql in [
            r#"CREATE VIEW "x""yz" AS SELECT A FROM T"#,
            r#"CREATE VIEW V("x""yz") AS SELECT A FROM T"#,
            r#"CREATE VIEW V AS SELECT "x""yz" FROM T"#,
            r#"CREATE VIEW V(X) AS SELECT "x""yz"(A) FROM T"#,
            r#"CREATE VIEW V AS SELECT A FROM "x""yz""#,
            r#"CREATE VIEW V(X) AS SELECT CAST(A AS S."x""yz") FROM T"#,
            // Widening the intermediate limit never widens ordinary names.
            r#"CREATE VIEW V(X) AS SELECT "x""y"(ABCD) FROM T"#,
        ] {
            let error = with_limits(sql, limits).unwrap_err();
            assert_eq!(
                error.code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{sql}: {error}"
            );
            assert!(error.message.contains("byte limit"), "{sql}: {error}");
        }
    }

    #[test]
    fn inherited_raw_select_ceiling_is_explicit() {
        let identifier = format!("\"{}\"", "\"\"".repeat(513));
        let limits = Db2AstLimits {
            max_identifier_bytes: 1024,
            ..Db2AstLimits::default()
        };
        // View/result names decode before any shared AST constructor.
        assert!(
            with_limits(
                &format!("CREATE VIEW {identifier}({identifier}) AS SELECT A FROM T"),
                limits
            )
            .is_ok()
        );
        // SELECT still needs its raw name to fit the shared compiled ceiling.
        let sql = format!("CREATE VIEW V AS SELECT {identifier} FROM T");
        let error = with_limits(&sql, limits).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnsupportedStatement);
        assert!(error.message.contains("inherited AST raw-name ceiling"));
        assert_eq!(
            error.location,
            location(&sql, sql.find(&identifier).unwrap())
        );
        // CAST's existing parser requires an ordinary first type component.
        let error = parse(r#"CREATE VIEW V(X) AS SELECT CAST(A AS "t""x") FROM T"#).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::MissingToken);
        assert_eq!(error.message, "CAST requires a Db2 data type");
    }

    #[test]
    fn known_width_and_required_result_names() {
        for projection in [
            "1",
            "A+1",
            "F(A)",
            "COUNT(*)",
            "CAST(A AS INTEGER)",
            "CASE WHEN A=1 THEN A ELSE B END",
        ] {
            assert!(parse(&format!("CREATE VIEW V AS SELECT {projection} FROM T")).is_err());
            assert!(
                parse(&format!("CREATE VIEW V(C) AS SELECT {projection} FROM T")).is_ok(),
                "{projection}"
            );
        }
        for sql in [
            "CREATE VIEW V(A) AS SELECT X,Y FROM T",
            "CREATE VIEW V(A,B) AS SELECT X FROM T",
            "CREATE VIEW V AS SELECT * FROM T",
            "CREATE VIEW V(A) AS SELECT T.* FROM T",
            "CREATE VIEW V(A,B) AS SELECT X,* FROM T",
        ] {
            assert!(parse(sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn relocates_every_nested_span_to_original_multibyte_multiline_source() {
        let sql = "/* 🙂 prefix */\n  CREATE /* gap */ VIEW \"é\"(\"résultat\") AS SELECT\n F(\"é\", CASE WHEN A = 1 THEN CAST(B AS INTEGER) ELSE C END)\n FROM \"schéma\".T WHERE G(A) > 1\n GROUP BY H(A) HAVING COUNT(*) > 1\n ORDER BY I(A) DESC, 1 OFFSET 0 ROWS FETCH NEXT 2 ROWS ONLY\n WITH /* mode */ LOCAL CHECK OPTION;";
        let view = parse(sql).unwrap();
        check_span(sql, view.span());
        check_span(sql, view.view_name().span());
        check_span(sql, view.result_columns().unwrap()[0].span());
        check_span(sql, view.check_option().unwrap().span());
        let core = view.definition();
        check_span(sql, core.span);
        assert_eq!(
            &sql[core.span.start_byte..core.span.start_byte + 6],
            "SELECT"
        );
        for expression in core
            .items
            .iter()
            .chain(core.where_condition.iter())
            .chain(core.group_by.iter())
            .chain(core.having.iter())
        {
            check_expression(sql, expression);
        }
        for source in &core.sources {
            check_span(sql, source.span());
        }
        for order in &core.order_by {
            check_span(sql, order.span());
            if let Db2ViewOrderKey::Expression(expression) = &order.value().0 {
                check_expression(sql, expression);
            }
        }
        check_span(sql, core.offset.as_ref().unwrap().span());
        check_span(sql, core.fetch.as_ref().unwrap().span());
        assert_eq!(*core.offset.as_ref().unwrap().value(), 0);
        assert_eq!(
            *core.fetch.as_ref().unwrap().value(),
            (Db2FetchPosition::Next, Some(2))
        );
        assert_eq!(core.order_by[0].value().1, Db2OrderDirection::Descending);
        assert_eq!(core.order_by[1].value().0, Db2ViewOrderKey::Ordinal(1));
    }

    #[test]
    fn whitespace_and_comments_preserve_select_semantics() {
        for gap in [
            " ",
            "\t",
            "\n",
            " /* x /* nested */ y */ ",
            " -- x\n",
            "\r\n",
        ] {
            let sql = [
                "CREATE", "VIEW", "V", "(", "X", ")", "AS", "SELECT", "DISTINCT", "F", "(", "A",
                ")", "FROM", "T", "WITH", "CHECK", "OPTION", ";",
            ]
            .join(gap);
            let view = parse(&sql).unwrap();
            assert_eq!(view.definition().quantifier, Db2SelectQuantifier::Distinct);
            assert_eq!(
                view.check_option().unwrap().value(),
                &Db2ViewCheckMode::ImplicitCascaded
            );
            let expression = &view.definition().items[0];
            check_expression(&sql, expression);
            let root = expression
                .value()
                .arena()
                .get(expression.value().root())
                .unwrap();
            let Db2ExpressionKind::Function { name, arguments } = root.kind() else {
                panic!("expected function")
            };
            assert_eq!(name.parts()[0].value(), "F");
            assert_eq!(arguments.len(), 1);
        }
        let sql = "CREATE VIEW V(X) AS SELECT F('?:UNPACK') FROM T /* :host ? */";
        assert!(parse(sql).is_ok());
    }

    #[test]
    fn rejects_hosts_and_parameters_anywhere_in_definition() {
        for definition in [
            "SELECT :H FROM T",
            "SELECT A FROM :T",
            "SELECT A FROM T WHERE A=?",
            "SELECT A FROM T GROUP BY :G",
            "SELECT A FROM T HAVING F(?)=1",
            "SELECT A FROM T ORDER BY :O",
            "SELECT A FROM T OFFSET ? ROWS",
            "SELECT A FROM T FETCH FIRST :N ROWS ONLY",
            "SELECT F(CASE WHEN A=1 THEN ? ELSE B END) FROM T",
        ] {
            let sql = format!("/* 🙂 */\n CREATE VIEW V(X) AS {definition}");
            let error = parse(&sql).unwrap_err();
            let tokens = lex_db2(&sql, Db2SyntaxLimits::default()).unwrap();
            let expected = tokens
                .tokens()
                .iter()
                .find(|token| {
                    matches!(
                        token.kind,
                        Db2TokenKind::HostVariable(_) | Db2TokenKind::ParameterMarker
                    )
                })
                .unwrap();
            assert_eq!(error.location, expected.span.start, "{sql}");
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
        }
    }

    #[test]
    fn forbidden_functions_are_found_in_all_expression_trees() {
        for function in [
            "UNPACK",
            "AI_ANALOGY",
            "AI_COMMONALITY",
            "AI_SEMANTIC_CLUSTER",
            "AI_SIMILARITY",
            "SYSIBM.\"UNPACK\"",
        ] {
            for definition in [
                format!("SELECT F({function}(A)) FROM T"),
                format!("SELECT A FROM T WHERE CAST({function}(A) AS INTEGER)>1"),
                format!("SELECT A FROM T GROUP BY {function}(A)"),
                format!("SELECT A FROM T HAVING {function}(A)>1"),
                format!("SELECT A FROM T ORDER BY CASE WHEN A=1 THEN {function}(A) ELSE A END"),
            ] {
                let sql = format!("\n CREATE VIEW V(X) AS {definition}");
                let error = parse(&sql).unwrap_err();
                assert_eq!(
                    error.code,
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "{sql}"
                );
                assert!(error.message.contains("forbidden"), "{sql}: {error}");
                assert_eq!(
                    error.location,
                    location(&sql, sql.find(function).unwrap()),
                    "{sql}"
                );
            }
        }
        for projection in ["UNPACK", "\"unpack\"(A)", "F('AI_SIMILARITY')"] {
            assert!(parse(&format!("CREATE VIEW V(X) AS SELECT {projection} FROM T")).is_ok());
        }
    }

    #[test]
    fn malformed_and_unsupported_forms_fail_closed_without_panics() {
        for sql in [
            "",
            ";",
            "CREATE",
            "CREATE VIEW",
            "CREATE VIEW AS SELECT A FROM T",
            "CREATE OR REPLACE VIEW V AS SELECT A FROM T",
            "CREATE TABLE V(A INTEGER)",
            "CREATE VIEW A.B.C.D AS SELECT A FROM T",
            "CREATE VIEW V() AS SELECT A FROM T",
            "CREATE VIEW V(A,) AS SELECT A FROM T",
            "CREATE VIEW V(A,,B) AS SELECT A,B FROM T",
            "CREATE VIEW V(T.A) AS SELECT A FROM T",
            "CREATE VIEW V AS",
            "CREATE VIEW V AS SELECT",
            "CREATE VIEW V AS WITH C AS (SELECT A FROM T) SELECT A FROM C",
            "CREATE VIEW V AS (SELECT A FROM T)",
            "CREATE VIEW V AS VALUES (1)",
            "CREATE VIEW V AS SELECT A AS X FROM T",
            "CREATE VIEW V AS SELECT A FROM T U",
            "CREATE VIEW V AS SELECT A FROM T JOIN U ON T.A=U.A",
            "CREATE VIEW V AS SELECT A FROM T UNION SELECT A FROM U",
            "CREATE VIEW V AS SELECT (SELECT A FROM T) FROM T",
            "CREATE VIEW V AS SELECT A INTO :H FROM T",
            "CREATE VIEW V AS SELECT A FROM T FOR UPDATE",
            "CREATE VIEW V AS SELECT A FROM T WITH UR",
            "CREATE VIEW V AS SELECT A FROM T WITH CHECK",
            "CREATE VIEW V AS SELECT A FROM T WITH LOCAL CASCADED CHECK OPTION",
            "CREATE VIEW V AS SELECT A FROM T WITH CHECK OPTION WITH CHECK OPTION",
            "CREATE VIEW V AS SELECT A FROM T WITH CHECK OPTION EXTRA",
            "CREATE VIEW V AS SELECT A FROM T; WITH CHECK OPTION",
            "CREATE VIEW V AS SELECT A FROM T;;",
            "CREATE VIEW V AS SELECT A FROM T; SELECT A FROM T",
            "CREATE VIEW V AS SELECT A FROM T WITH CHECK OPTION; SELECT A FROM T",
            "CREATE VIEW V(X) AS SELECT 1E3 FROM T",
            "CREATE VIEW V(X) AS SELECT DECFLOAT'NaN' FROM T",
            "CREATE VIEW V(X) AS SELECT TRUE FROM T",
        ] {
            let error = parse(sql).expect_err(sql);
            assert!(
                error.location.line > 0 && error.location.column > 0,
                "{sql}"
            );
            assert!(error.message.len() <= 256);
        }
    }

    #[test]
    fn select_errors_relocate_first_and_later_lines() {
        for sql in [
            "/* é */ CREATE VIEW V(X) AS SELECT A + FROM T",
            "\nCREATE VIEW V(X) AS SELECT A +\n FROM T",
            "CREATE VIEW V(X) AS SELECT F(A,) FROM T",
        ] {
            let start = sql.find("SELECT").unwrap();
            let core_error = parse_db2_select_core(
                &sql[start..],
                Db2SyntaxLimits::default(),
                Db2AstLimits::default(),
            )
            .unwrap_err();
            let error = parse(sql).unwrap_err();
            assert_eq!(error.code, core_error.code);
            assert_eq!(
                error.location,
                relocate_location(core_error.location, location(sql, start))
            );
        }
    }

    #[test]
    fn conversion_preserves_existing_select_core_nodes_and_clause_values() {
        for quantifier in ["", "ALL ", "DISTINCT "] {
            let select = format!(
                "SELECT {quantifier}F(A),CAST(B AS INTEGER) FROM S.T WHERE A=1 GROUP BY B HAVING COUNT(*)>1 ORDER BY G(B) ASC OFFSET 1 ROW FETCH FIRST ROW ONLY"
            );
            let sql = format!("CREATE VIEW S.V(X,Y) AS {select} WITH CHECK OPTION");
            let original =
                parse_db2_select_core(&select, Db2SyntaxLimits::default(), Db2AstLimits::default())
                    .unwrap();
            let view = parse(&sql).unwrap();
            let converted = view.definition();
            assert_eq!(converted.quantifier(), original.quantifier());
            for (before, after) in original.items().iter().zip(converted.items()) {
                let Db2SelectItem::Expression(before) = before else {
                    panic!("expected expression")
                };
                assert_eq!(before.parsed().root(), after.value().root());
                assert_eq!(
                    before
                        .parsed()
                        .arena()
                        .nodes()
                        .iter()
                        .map(|n| n.kind())
                        .collect::<Vec<_>>(),
                    after
                        .value()
                        .arena()
                        .nodes()
                        .iter()
                        .map(|n| n.kind())
                        .collect::<Vec<_>>()
                );
            }
            assert_eq!(converted.sources()[0].value(), original.sources()[0].name());
            assert_eq!(
                *converted.offset().unwrap().value(),
                original.offset().unwrap().row_count()
            );
            assert_eq!(
                *converted.fetch().unwrap().value(),
                (
                    original.fetch().unwrap().position(),
                    original.fetch().unwrap().explicit_row_count()
                )
            );
            for expression in converted
                .where_condition()
                .into_iter()
                .chain(converted.group_by())
                .chain(converted.having())
            {
                check_expression(&sql, expression);
            }
            assert_eq!(converted.order_by().len(), original.order_by().len());
            check_span(&sql, converted.span());
        }
    }

    fn with_limits(
        sql: &str,
        limits: Db2AstLimits,
    ) -> Result<Db2CreateViewStatement, Db2SyntaxDiagnostic> {
        parse_db2_create_view_statement(sql, Db2SyntaxLimits::default(), limits)
    }

    #[test]
    fn aggregate_nodes_and_lists_have_exact_boundaries() {
        let sql =
            "CREATE VIEW V(X) AS SELECT F(A) FROM T WHERE B=1 GROUP BY C HAVING D=2 ORDER BY E";
        // 2 projection nodes + 3 WHERE + 1 GROUP + 3 HAVING + 1 ORDER.
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_expression_nodes: 10,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_expression_nodes: 9,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        // Result columns + projection + source + group + order + F argument.
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_list_items: 6,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_list_items: 5,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        assert!(
            with_limits(
                "CREATE VIEW V AS SELECT A FROM T",
                Db2AstLimits {
                    max_list_items: 2,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                "CREATE VIEW V AS SELECT A FROM T",
                Db2AstLimits {
                    max_list_items: 1,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        // Nested argument lists, CASE branches and CAST type arguments are included.
        let sql = "CREATE VIEW V(X) AS SELECT F(G(A,B),CASE WHEN A=1 THEN CAST(B AS DECIMAL(5,2)) ELSE C END) FROM T";
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_list_items: 10,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_list_items: 9,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn identifier_expression_and_lexer_bounds_remain_enforced() {
        let sql = "CREATE VIEW ABC AS SELECT A FROM T";
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_identifier_bytes: 3,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_identifier_bytes: 2,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        assert!(
            with_limits(
                "CREATE VIEW S.V AS SELECT A FROM T",
                Db2AstLimits {
                    max_name_parts: 1,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        let sql = "CREATE VIEW V(X) AS SELECT F(G(A)) FROM T";
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_expression_depth: 3,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_expression_depth: 2,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        assert_eq!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_list_items: 0,
                    ..Db2AstLimits::default()
                }
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
        let sql = "CREATE VIEW V AS SELECT A FROM T";
        let tokens = lex_db2(sql, Db2SyntaxLimits::default())
            .unwrap()
            .tokens()
            .len();
        for limits in [
            Db2SyntaxLimits {
                max_statement_bytes: sql.len() - 1,
                ..Db2SyntaxLimits::default()
            },
            Db2SyntaxLimits {
                max_tokens: tokens - 1,
                ..Db2SyntaxLimits::default()
            },
            Db2SyntaxLimits {
                max_token_bytes: 5,
                ..Db2SyntaxLimits::default()
            },
        ] {
            assert!(parse_db2_create_view_statement(sql, limits, Db2AstLimits::default()).is_err());
        }
        assert!(
            parse_db2_create_view_statement(
                sql,
                Db2SyntaxLimits {
                    max_statement_bytes: sql.len(),
                    max_tokens: tokens,
                    max_token_bytes: 6,
                    ..Db2SyntaxLimits::default()
                },
                Db2AstLimits::default()
            )
            .is_ok()
        );
    }
}
