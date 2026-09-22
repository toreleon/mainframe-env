//! Owned bounded syntax for one common Db2 SELECT subselect.

use crate::{
    Db2AstLimits, Db2ExpressionArena, Db2Identifier, Db2ParsedExpression, Db2QualifiedName,
    Db2SourceLocation, Db2SourceSpan, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2Token, Db2TokenKind, lex_db2, parse_db2_expression,
};

/// Whether the SELECT clause used its implicit default or an explicit set
/// quantifier. Keeping implicit and explicit ALL distinct preserves syntax
/// without assigning execution semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2SelectQuantifier {
    ImplicitAll,
    All,
    Distinct,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2QueryExpression {
    parsed: Db2ParsedExpression,
    span: Db2SourceSpan,
}

impl Db2QueryExpression {
    #[must_use]
    pub const fn parsed(&self) -> &Db2ParsedExpression {
        &self.parsed
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2SelectItem {
    Expression(Db2QueryExpression),
    Wildcard {
        qualifier: Option<Db2QualifiedName>,
        span: Db2SourceSpan,
    },
}

impl Db2SelectItem {
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        match self {
            Self::Expression(expression) => expression.span(),
            Self::Wildcard { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NamedTableSource {
    name: Db2QualifiedName,
    span: Db2SourceSpan,
}

impl Db2NamedTableSource {
    #[must_use]
    pub const fn name(&self) -> &Db2QualifiedName {
        &self.name
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2OrderDirection {
    Unspecified,
    Ascending,
    Descending,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2OrderKey {
    Expression(Db2QueryExpression),
    Ordinal(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2OrderByItem {
    key: Db2OrderKey,
    direction: Db2OrderDirection,
    span: Db2SourceSpan,
}

impl Db2OrderByItem {
    #[must_use]
    pub const fn key(&self) -> &Db2OrderKey {
        &self.key
    }

    #[must_use]
    pub const fn direction(&self) -> Db2OrderDirection {
        self.direction
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2OffsetClause {
    row_count: u64,
    span: Db2SourceSpan,
}

impl Db2OffsetClause {
    #[must_use]
    pub const fn row_count(&self) -> u64 {
        self.row_count
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2FetchPosition {
    First,
    Next,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2FetchClause {
    position: Db2FetchPosition,
    row_count: Option<u64>,
    span: Db2SourceSpan,
}

impl Db2FetchClause {
    #[must_use]
    pub const fn position(&self) -> Db2FetchPosition {
        self.position
    }

    /// `None` preserves the source form whose Db2 default is one row.
    #[must_use]
    pub const fn explicit_row_count(&self) -> Option<u64> {
        self.row_count
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// One bounded SELECT subselect. This type is intentionally disconnected from
/// binding, execution, authorization, SQLCA mapping, and whole-row recognition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SelectCore {
    quantifier: Db2SelectQuantifier,
    items: Vec<Db2SelectItem>,
    sources: Vec<Db2NamedTableSource>,
    where_condition: Option<Db2QueryExpression>,
    group_by: Vec<Db2QueryExpression>,
    having: Option<Db2QueryExpression>,
    order_by: Vec<Db2OrderByItem>,
    offset: Option<Db2OffsetClause>,
    fetch: Option<Db2FetchClause>,
    span: Db2SourceSpan,
}

impl Db2SelectCore {
    #[must_use]
    pub const fn quantifier(&self) -> Db2SelectQuantifier {
        self.quantifier
    }

    #[must_use]
    pub fn items(&self) -> &[Db2SelectItem] {
        &self.items
    }

    #[must_use]
    pub fn sources(&self) -> &[Db2NamedTableSource] {
        &self.sources
    }

    #[must_use]
    pub const fn where_condition(&self) -> Option<&Db2QueryExpression> {
        self.where_condition.as_ref()
    }

    #[must_use]
    pub fn group_by(&self) -> &[Db2QueryExpression] {
        &self.group_by
    }

    #[must_use]
    pub const fn having(&self) -> Option<&Db2QueryExpression> {
        self.having.as_ref()
    }

    #[must_use]
    pub fn order_by(&self) -> &[Db2OrderByItem] {
        &self.order_by
    }

    #[must_use]
    pub const fn offset(&self) -> Option<&Db2OffsetClause> {
        self.offset.as_ref()
    }

    #[must_use]
    pub const fn fetch(&self) -> Option<&Db2FetchClause> {
        self.fetch.as_ref()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse exactly one source-reviewed common SELECT subselect. CTEs, set
/// operations, joins, subqueries, SELECT INTO, aliases, and outer SELECT
/// clauses are rejected rather than represented as raw tokens or generic AST.
pub fn parse_db2_select_core(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2SelectCore, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    Db2ExpressionArena::new(ast_limits).map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            Db2SourceLocation::START,
            problem.message,
        )
    })?;
    QueryParser::new(source, lexed.tokens(), syntax_limits, ast_limits).parse()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClauseKind {
    Where,
    GroupBy,
    Having,
    OrderBy,
    Offset,
    Fetch,
}

impl ClauseKind {
    const fn order(self) -> usize {
        match self {
            Self::Where => 0,
            Self::GroupBy => 1,
            Self::Having => 2,
            Self::OrderBy => 3,
            Self::Offset => 4,
            Self::Fetch => 5,
        }
    }

    const fn keyword_count(self) -> usize {
        match self {
            Self::GroupBy | Self::OrderBy => 2,
            _ => 1,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Where => "WHERE",
            Self::GroupBy => "GROUP BY",
            Self::Having => "HAVING",
            Self::OrderBy => "ORDER BY",
            Self::Offset => "OFFSET",
            Self::Fetch => "FETCH",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ClauseMarker {
    kind: ClauseKind,
    index: usize,
}

struct QueryParser<'a> {
    source: &'a str,
    tokens: &'a [Db2Token],
    end: usize,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
    expression_nodes: usize,
}

impl<'a> QueryParser<'a> {
    fn new(
        source: &'a str,
        tokens: &'a [Db2Token],
        syntax_limits: Db2SyntaxLimits,
        ast_limits: Db2AstLimits,
    ) -> Self {
        Self {
            source,
            tokens,
            end: tokens.len(),
            syntax_limits,
            ast_limits,
            expression_nodes: 0,
        }
    }

    fn parse(mut self) -> Result<Db2SelectCore, Db2SyntaxDiagnostic> {
        self.trim_statement_terminator()?;
        if self.word_at(0) != Some("SELECT") {
            let message = if self.word_at(0) == Some("WITH") {
                "common table expressions are outside the Db2 SELECT core slice"
            } else {
                "Db2 SELECT core must begin with SELECT"
            };
            return Err(self.diagnostic_at(
                0,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                message,
            ));
        }

        self.reject_unsupported_top_level(1, self.end)?;
        let from = self
            .find_top_level_word(1, self.end, "FROM")
            .ok_or_else(|| {
                self.diagnostic_at(
                    self.end,
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "Db2 SELECT core requires a FROM clause",
                )
            })?;
        if let Some(second) = self.find_top_level_word(from + 1, self.end, "FROM") {
            return Err(self.diagnostic_at(
                second,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "FROM is specified more than once",
            ));
        }

        let mut item_start = 1;
        let quantifier = if self.word_at(item_start) == Some("ALL") {
            item_start += 1;
            Db2SelectQuantifier::All
        } else if self.word_at(item_start) == Some("DISTINCT") {
            item_start += 1;
            Db2SelectQuantifier::Distinct
        } else {
            Db2SelectQuantifier::ImplicitAll
        };
        if matches!(self.word_at(item_start), Some("ALL" | "DISTINCT")) {
            return Err(self.diagnostic_at(
                item_start,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "SELECT set quantifier is specified more than once",
            ));
        }
        let items = self.parse_select_items(item_start, from)?;

        let markers = self.clause_markers(from + 1)?;
        let source_end = markers.first().map_or(self.end, |marker| marker.index);
        let sources = self.parse_sources(from + 1, source_end)?;

        let mut where_condition = None;
        let mut group_by = Vec::new();
        let mut having = None;
        let mut order_by = Vec::new();
        let mut offset = None;
        let mut fetch = None;
        for (position, marker) in markers.iter().enumerate() {
            let value_start = marker.index + marker.kind.keyword_count();
            let value_end = markers
                .get(position + 1)
                .map_or(self.end, |next| next.index);
            match marker.kind {
                ClauseKind::Where => {
                    where_condition = Some(self.parse_expression(value_start, value_end)?);
                }
                ClauseKind::GroupBy => {
                    group_by = self.parse_expression_list(value_start, value_end, "GROUP BY")?;
                }
                ClauseKind::Having => {
                    having = Some(self.parse_expression(value_start, value_end)?);
                }
                ClauseKind::OrderBy => {
                    order_by = self.parse_order_by(value_start, value_end)?;
                }
                ClauseKind::Offset => {
                    offset = Some(self.parse_offset(marker.index, value_start, value_end)?);
                }
                ClauseKind::Fetch => {
                    fetch = Some(self.parse_fetch(marker.index, value_start, value_end)?);
                }
            }
        }

        Ok(Db2SelectCore {
            quantifier,
            items,
            sources,
            where_condition,
            group_by,
            having,
            order_by,
            offset,
            fetch,
            span: self.statement_span(),
        })
    }

    fn trim_statement_terminator(&mut self) -> Result<(), Db2SyntaxDiagnostic> {
        let semicolons = self
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| matches!(token.kind, Db2TokenKind::Symbol(Db2Symbol::Semicolon)))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match semicolons.as_slice() {
            [] => {}
            [only] if *only + 1 == self.tokens.len() => self.end -= 1,
            [first, ..] => {
                return Err(self.diagnostic_at(
                    first + 1,
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    "only one Db2 SELECT statement is allowed",
                ));
            }
        }
        Ok(())
    }

    fn reject_unsupported_top_level(
        &self,
        start: usize,
        end: usize,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        let mut depth = 0_usize;
        for index in start..end {
            match self.tokens[index].kind {
                Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => depth += 1,
                Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) => depth -= 1,
                _ if depth != 0 => continue,
                _ => {}
            }
            let Some(word) = self.word_at(index) else {
                continue;
            };
            let unsupported = match word {
                "UNION" | "EXCEPT" | "INTERSECT" => Some("set operations"),
                "JOIN" | "INNER" | "LEFT" | "RIGHT" | "FULL" | "CROSS" => Some("joins"),
                "INTO" => Some("SELECT INTO"),
                "FOR" | "WITH" | "OPTIMIZE" | "ISOLATION" | "QUERYNO" | "SKIP" => {
                    Some("outer SELECT clauses")
                }
                _ => None,
            };
            if let Some(family) = unsupported {
                return Err(self.diagnostic_at(
                    index,
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    format!("{family} are outside the Db2 SELECT core slice"),
                ));
            }
        }
        Ok(())
    }

    fn clause_markers(&self, start: usize) -> Result<Vec<ClauseMarker>, Db2SyntaxDiagnostic> {
        let mut markers = Vec::new();
        let mut seen = [false; 6];
        let mut previous_order = None;
        let mut depth = 0_usize;
        let mut index = start;
        while index < self.end {
            match self.tokens[index].kind {
                Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => {
                    depth += 1;
                    index += 1;
                    continue;
                }
                Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) => {
                    depth -= 1;
                    index += 1;
                    continue;
                }
                _ if depth != 0 => {
                    index += 1;
                    continue;
                }
                _ => {}
            }
            let kind = match self.word_at(index) {
                Some("WHERE") => Some(ClauseKind::Where),
                Some("GROUP") if self.word_at(index + 1) == Some("BY") => Some(ClauseKind::GroupBy),
                Some("HAVING") => Some(ClauseKind::Having),
                Some("ORDER") if self.word_at(index + 1) == Some("BY") => Some(ClauseKind::OrderBy),
                Some("OFFSET") => Some(ClauseKind::Offset),
                Some("FETCH") => Some(ClauseKind::Fetch),
                _ => None,
            };
            let Some(kind) = kind else {
                index += 1;
                continue;
            };
            if seen[kind.order()] {
                return Err(self.diagnostic_at(
                    index,
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    format!("{} is specified more than once", kind.name()),
                ));
            }
            if previous_order.is_some_and(|previous| kind.order() < previous) {
                return Err(self.diagnostic_at(
                    index,
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    format!("{} is out of Db2 subselect clause order", kind.name()),
                ));
            }
            seen[kind.order()] = true;
            previous_order = Some(kind.order());
            markers.push(ClauseMarker { kind, index });
            index += kind.keyword_count();
        }
        Ok(markers)
    }

    fn parse_select_items(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<Vec<Db2SelectItem>, Db2SyntaxDiagnostic> {
        let ranges = self.split_list(start, end, "SELECT list")?;
        let mut items = Vec::with_capacity(ranges.len());
        for (item_start, item_end) in ranges {
            if item_end == item_start + 1 && self.symbol_at(item_start) == Some(Db2Symbol::Multiply)
            {
                items.push(Db2SelectItem::Wildcard {
                    qualifier: None,
                    span: self.range_span(item_start, item_end),
                });
                continue;
            }
            if item_end >= item_start + 3
                && self.symbol_at(item_end - 1) == Some(Db2Symbol::Multiply)
                && self.symbol_at(item_end - 2) == Some(Db2Symbol::Period)
            {
                let qualifier = self.parse_qualified_name(item_start, item_end - 2, "wildcard")?;
                items.push(Db2SelectItem::Wildcard {
                    qualifier: Some(qualifier),
                    span: self.range_span(item_start, item_end),
                });
                continue;
            }
            items.push(Db2SelectItem::Expression(
                self.parse_expression(item_start, item_end)?,
            ));
        }
        Ok(items)
    }

    fn parse_sources(
        &self,
        start: usize,
        end: usize,
    ) -> Result<Vec<Db2NamedTableSource>, Db2SyntaxDiagnostic> {
        let ranges = self.split_list(start, end, "FROM list")?;
        ranges
            .into_iter()
            .map(|(source_start, source_end)| {
                Ok(Db2NamedTableSource {
                    name: self.parse_qualified_name(source_start, source_end, "table source")?,
                    span: self.range_span(source_start, source_end),
                })
            })
            .collect()
    }

    fn parse_expression_list(
        &mut self,
        start: usize,
        end: usize,
        label: &str,
    ) -> Result<Vec<Db2QueryExpression>, Db2SyntaxDiagnostic> {
        self.split_list(start, end, label)?
            .into_iter()
            .map(|(expression_start, expression_end)| {
                self.parse_expression(expression_start, expression_end)
            })
            .collect()
    }

    fn parse_order_by(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<Vec<Db2OrderByItem>, Db2SyntaxDiagnostic> {
        let ranges = self.split_list(start, end, "ORDER BY list")?;
        let mut items = Vec::with_capacity(ranges.len());
        for (item_start, item_end) in ranges {
            let span = self.range_span(item_start, item_end);
            let (key_end, direction) = match self.word_at(item_end - 1) {
                Some("ASC") => (item_end - 1, Db2OrderDirection::Ascending),
                Some("DESC") => (item_end - 1, Db2OrderDirection::Descending),
                _ => (item_end, Db2OrderDirection::Unspecified),
            };
            if key_end == item_start {
                return Err(self.diagnostic_at(
                    item_start,
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "ORDER BY direction requires a sort key",
                ));
            }
            if matches!(self.word_at(key_end - 1), Some("ASC" | "DESC")) {
                return Err(self.diagnostic_at(
                    key_end - 1,
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "ORDER BY direction is specified more than once",
                ));
            }
            let key = if key_end == item_start + 1 {
                if let Some(value) = self.unsigned_integer_at(item_start) {
                    let ordinal = value
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|value| *value > 0);
                    Db2OrderKey::Ordinal(ordinal.ok_or_else(|| {
                        self.diagnostic_at(
                            item_start,
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            "ORDER BY ordinal must be between 1 and u32::MAX",
                        )
                    })?)
                } else {
                    Db2OrderKey::Expression(self.parse_expression(item_start, key_end)?)
                }
            } else {
                Db2OrderKey::Expression(self.parse_expression(item_start, key_end)?)
            };
            items.push(Db2OrderByItem {
                key,
                direction,
                span,
            });
        }
        Ok(items)
    }

    fn parse_offset(
        &self,
        clause_start: usize,
        start: usize,
        end: usize,
    ) -> Result<Db2OffsetClause, Db2SyntaxDiagnostic> {
        let row_count = self.required_row_count(start, "OFFSET")?;
        if !matches!(self.word_at(start + 1), Some("ROW" | "ROWS")) {
            return Err(self.diagnostic_at(
                start + 1,
                Db2SyntaxDiagnosticCode::MissingToken,
                "OFFSET row count requires ROW or ROWS",
            ));
        }
        if start + 2 != end {
            return Err(self.diagnostic_at(
                start + 2,
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "unexpected token after OFFSET clause",
            ));
        }
        Ok(Db2OffsetClause {
            row_count,
            span: self.range_span(clause_start, end),
        })
    }

    fn parse_fetch(
        &self,
        clause_start: usize,
        start: usize,
        end: usize,
    ) -> Result<Db2FetchClause, Db2SyntaxDiagnostic> {
        let position = match self.word_at(start) {
            Some("FIRST") => Db2FetchPosition::First,
            Some("NEXT") => Db2FetchPosition::Next,
            _ => {
                return Err(self.diagnostic_at(
                    start,
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "FETCH requires FIRST or NEXT",
                ));
            }
        };
        let mut cursor = start + 1;
        let row_count = if matches!(self.word_at(cursor), Some("ROW" | "ROWS")) {
            None
        } else {
            let count = self.required_row_count(cursor, "FETCH")?;
            if count == 0 {
                return Err(self.diagnostic_at(
                    cursor,
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "FETCH row count must be greater than zero",
                ));
            }
            cursor += 1;
            Some(count)
        };
        if !matches!(self.word_at(cursor), Some("ROW" | "ROWS")) {
            return Err(self.diagnostic_at(
                cursor,
                Db2SyntaxDiagnosticCode::MissingToken,
                "FETCH requires ROW or ROWS",
            ));
        }
        cursor += 1;
        if self.word_at(cursor) != Some("ONLY") {
            return Err(self.diagnostic_at(
                cursor,
                Db2SyntaxDiagnosticCode::MissingToken,
                "FETCH requires ONLY",
            ));
        }
        cursor += 1;
        if cursor != end {
            return Err(self.diagnostic_at(
                cursor,
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "unexpected token after FETCH clause",
            ));
        }
        Ok(Db2FetchClause {
            position,
            row_count,
            span: self.range_span(clause_start, end),
        })
    }

    fn required_row_count(&self, index: usize, clause: &str) -> Result<u64, Db2SyntaxDiagnostic> {
        self.unsigned_integer_at(index).flatten().ok_or_else(|| {
            self.diagnostic_at(
                index,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                format!("{clause} row count must be an unsigned 64-bit integer"),
            )
        })
    }

    fn unsigned_integer_at(&self, index: usize) -> Option<Option<u64>> {
        let Db2TokenKind::Number(value) = &self.tokens.get(index)?.kind else {
            return None;
        };
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        Some(value.parse::<u64>().ok())
    }

    fn parse_expression(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<Db2QueryExpression, Db2SyntaxDiagnostic> {
        if start >= end {
            return Err(self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::MissingToken,
                "missing Db2 expression operand",
            ));
        }
        let span = self.range_span(start, end);
        let fragment = self.source_fragment(span)?;
        let parsed = parse_db2_expression(fragment, self.syntax_limits, self.ast_limits)
            .map_err(|problem| self.relocate_diagnostic(problem, span.start))?;
        self.expression_nodes = self
            .expression_nodes
            .checked_add(parsed.arena().nodes().len())
            .ok_or_else(|| {
                self.diagnostic_at(
                    start,
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "Db2 SELECT expression-node count overflowed",
                )
            })?;
        if self.expression_nodes > self.ast_limits.max_expression_nodes {
            return Err(self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "Db2 SELECT exceeds the configured total expression-node limit",
            ));
        }
        Ok(Db2QueryExpression { parsed, span })
    }

    fn parse_qualified_name(
        &self,
        start: usize,
        end: usize,
        label: &str,
    ) -> Result<Db2QualifiedName, Db2SyntaxDiagnostic> {
        if start >= end {
            return Err(self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("missing Db2 {label}"),
            ));
        }
        let mut parts = Vec::new();
        let mut cursor = start;
        loop {
            if parts.len() >= self.ast_limits.max_name_parts {
                return Err(self.diagnostic_at(
                    cursor,
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "Db2 qualified name exceeds the configured part limit",
                ));
            }
            let Some(token) = self.tokens.get(cursor) else {
                return Err(self.diagnostic_at(
                    cursor,
                    Db2SyntaxDiagnosticCode::MissingToken,
                    format!("missing Db2 {label} identifier"),
                ));
            };
            let Db2TokenKind::Word { value, delimited } = &token.kind else {
                return Err(self.diagnostic_at(
                    cursor,
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    format!("Db2 {label} must be a qualified name"),
                ));
            };
            parts.push(
                Db2Identifier::new(value.clone(), *delimited, self.ast_limits).map_err(
                    |problem| {
                        Db2SyntaxDiagnostic::new(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            token.span.start,
                            problem.message,
                        )
                    },
                )?,
            );
            cursor += 1;
            if cursor == end {
                break;
            }
            if self.symbol_at(cursor) != Some(Db2Symbol::Period) {
                return Err(self.diagnostic_at(
                    cursor,
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    format!("Db2 {label} accepts no alias or table expression"),
                ));
            }
            cursor += 1;
            if cursor == end {
                return Err(self.diagnostic_at(
                    cursor,
                    Db2SyntaxDiagnosticCode::MissingToken,
                    format!("Db2 {label} has a trailing qualifier separator"),
                ));
            }
        }
        Db2QualifiedName::new(parts, self.ast_limits).map_err(|problem| {
            self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                problem.message,
            )
        })
    }

    fn split_list(
        &self,
        start: usize,
        end: usize,
        label: &str,
    ) -> Result<Vec<(usize, usize)>, Db2SyntaxDiagnostic> {
        if start >= end {
            return Err(self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("Db2 {label} must not be empty"),
            ));
        }
        let mut ranges = Vec::new();
        let mut item_start = start;
        let mut depth = 0_usize;
        for index in start..end {
            match self.tokens[index].kind {
                Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => depth += 1,
                Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) => depth -= 1,
                Db2TokenKind::Symbol(Db2Symbol::Comma) if depth == 0 => {
                    if item_start == index {
                        return Err(self.diagnostic_at(
                            index,
                            Db2SyntaxDiagnosticCode::MissingToken,
                            format!("Db2 {label} contains an empty item"),
                        ));
                    }
                    ranges.push((item_start, index));
                    item_start = index + 1;
                }
                _ => {}
            }
        }
        if item_start == end {
            return Err(self.diagnostic_at(
                end,
                Db2SyntaxDiagnosticCode::MissingToken,
                format!("Db2 {label} has a trailing comma"),
            ));
        }
        ranges.push((item_start, end));
        if ranges.len() > self.ast_limits.max_list_items {
            return Err(self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                format!("Db2 {label} exceeds the configured item limit"),
            ));
        }
        Ok(ranges)
    }

    fn find_top_level_word(&self, start: usize, end: usize, expected: &str) -> Option<usize> {
        let mut depth = 0_usize;
        for index in start..end {
            match self.tokens[index].kind {
                Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => depth += 1,
                Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) => depth -= 1,
                _ if depth == 0 && self.word_at(index) == Some(expected) => return Some(index),
                _ => {}
            }
        }
        None
    }

    fn source_fragment(&self, span: Db2SourceSpan) -> Result<&'a str, Db2SyntaxDiagnostic> {
        let start = source_offset(self.source, span.start).ok_or_else(|| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                span.start,
                "Db2 token start is outside the source",
            )
        })?;
        let end = source_offset(self.source, span.end).ok_or_else(|| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                span.end,
                "Db2 token end is outside the source",
            )
        })?;
        self.source.get(start..end).ok_or_else(|| {
            Db2SyntaxDiagnostic::new(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                span.start,
                "Db2 token span is not a UTF-8 source boundary",
            )
        })
    }

    fn relocate_diagnostic(
        &self,
        mut problem: Db2SyntaxDiagnostic,
        fragment_start: Db2SourceLocation,
    ) -> Db2SyntaxDiagnostic {
        problem.location = if problem.location.line == 1 {
            Db2SourceLocation {
                line: fragment_start.line,
                column: fragment_start
                    .column
                    .saturating_add(problem.location.column.saturating_sub(1)),
            }
        } else {
            Db2SourceLocation {
                line: fragment_start
                    .line
                    .saturating_add(problem.location.line.saturating_sub(1)),
                column: problem.location.column,
            }
        };
        problem
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

    fn symbol_at(&self, index: usize) -> Option<Db2Symbol> {
        match self.tokens.get(index).map(|token| &token.kind) {
            Some(Db2TokenKind::Symbol(symbol)) => Some(*symbol),
            _ => None,
        }
    }

    fn range_span(&self, start: usize, end: usize) -> Db2SourceSpan {
        Db2SourceSpan {
            start: self
                .tokens
                .get(start)
                .map_or_else(|| self.end_location(), |token| token.span.start),
            end: end
                .checked_sub(1)
                .and_then(|index| self.tokens.get(index))
                .map_or_else(|| self.end_location(), |token| token.span.end),
        }
    }

    fn statement_span(&self) -> Db2SourceSpan {
        Db2SourceSpan {
            start: self
                .tokens
                .first()
                .map_or(Db2SourceLocation::START, |token| token.span.start),
            end: self
                .tokens
                .last()
                .map_or(Db2SourceLocation::START, |token| token.span.end),
        }
    }

    fn end_location(&self) -> Db2SourceLocation {
        self.tokens
            .get(self.end.saturating_sub(1))
            .map_or(Db2SourceLocation::START, |token| token.span.end)
    }

    fn diagnostic_at(
        &self,
        index: usize,
        code: Db2SyntaxDiagnosticCode,
        message: impl AsRef<str>,
    ) -> Db2SyntaxDiagnostic {
        let location = self
            .tokens
            .get(index)
            .map_or_else(|| self.end_location(), |token| token.span.start);
        Db2SyntaxDiagnostic::new(code, location, message)
    }
}

fn source_offset(source: &str, target: Db2SourceLocation) -> Option<usize> {
    let mut line = 1_u32;
    let mut column = 1_u32;
    for (offset, character) in source.char_indices() {
        if line == target.line && column == target.column {
            return Some(offset);
        }
        if character == '\n' {
            line = line.checked_add(1)?;
            column = 1;
        } else {
            column = column.checked_add(1)?;
        }
    }
    (line == target.line && column == target.column).then_some(source.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Db2BinaryOperator, Db2ExpressionKind};

    fn parse(source: &str) -> Result<Db2SelectCore, Db2SyntaxDiagnostic> {
        parse_db2_select_core(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    fn expression(item: &Db2SelectItem) -> &Db2QueryExpression {
        let Db2SelectItem::Expression(expression) = item else {
            panic!("expected expression select item")
        };
        expression
    }

    #[test]
    fn complete_core_is_typed_and_preserves_clause_order() {
        let query = parse(
            "SELECT DISTINCT E.DEPT, AVG(E.SALARY) FROM APP.EMP, DEPT\n\
             WHERE E.ACTIVE = TRUE GROUP BY E.DEPT HAVING AVG(E.SALARY) > 10\n\
             ORDER BY 2 DESC, E.DEPT ASC OFFSET 5 ROWS FETCH NEXT 10 ROWS ONLY;",
        )
        .unwrap();
        assert_eq!(query.quantifier(), Db2SelectQuantifier::Distinct);
        assert_eq!(query.items().len(), 2);
        assert_eq!(query.sources().len(), 2);
        assert_eq!(query.sources()[0].name().parts()[0].value(), "APP");
        assert_eq!(query.sources()[0].span().start.line, 1);
        assert!(query.where_condition().is_some());
        assert_eq!(query.group_by().len(), 1);
        assert!(query.having().is_some());
        assert_eq!(query.order_by().len(), 2);
        assert_eq!(query.order_by()[0].key(), &Db2OrderKey::Ordinal(2));
        assert_eq!(
            query.order_by()[0].direction(),
            Db2OrderDirection::Descending
        );
        assert_eq!(query.order_by()[0].span().start.line, 3);
        assert_eq!(query.offset().unwrap().row_count(), 5);
        assert_eq!(query.offset().unwrap().span().start.line, 3);
        assert_eq!(query.fetch().unwrap().position(), Db2FetchPosition::Next);
        assert_eq!(query.fetch().unwrap().explicit_row_count(), Some(10));
        assert_eq!(query.fetch().unwrap().span().start.line, 3);
        assert_eq!(query.span().start, Db2SourceLocation::START);
        assert_eq!(query.span().end.line, 3);
    }

    #[test]
    fn quantifiers_wildcards_and_default_fetch_count_are_preserved() {
        let implicit = parse("SELECT *, T.*, T.A + 1 FROM S.T FETCH FIRST ROW ONLY").unwrap();
        assert_eq!(implicit.quantifier(), Db2SelectQuantifier::ImplicitAll);
        assert!(matches!(
            &implicit.items()[0],
            Db2SelectItem::Wildcard {
                qualifier: None,
                ..
            }
        ));
        let Db2SelectItem::Wildcard {
            qualifier: Some(name),
            ..
        } = &implicit.items()[1]
        else {
            panic!("expected qualified wildcard")
        };
        assert_eq!(name.parts()[0].value(), "T");
        assert_eq!(implicit.fetch().unwrap().explicit_row_count(), None);

        assert_eq!(
            parse("SELECT ALL A FROM T").unwrap().quantifier(),
            Db2SelectQuantifier::All
        );
        assert_eq!(
            parse("SELECT DISTINCT A FROM T").unwrap().quantifier(),
            Db2SelectQuantifier::Distinct
        );
    }

    #[test]
    fn expressions_use_the_owned_parser_and_arena() {
        let query = parse("SELECT A + 2 * 3 FROM T WHERE NOT A = 1 OR B IS NULL").unwrap();
        let selected = expression(&query.items()[0]);
        assert!(matches!(
            selected
                .parsed()
                .arena()
                .get(selected.parsed().root())
                .unwrap()
                .kind(),
            Db2ExpressionKind::Binary {
                operator: Db2BinaryOperator::Add,
                ..
            }
        ));
        let predicate = query.where_condition().unwrap();
        assert!(matches!(
            predicate
                .parsed()
                .arena()
                .get(predicate.parsed().root())
                .unwrap()
                .kind(),
            Db2ExpressionKind::Binary {
                operator: Db2BinaryOperator::Or,
                ..
            }
        ));
    }

    #[test]
    fn malformed_lists_and_missing_operands_fail_closed() {
        for source in [
            "SELECT FROM T",
            "SELECT A, FROM T",
            "SELECT A FROM",
            "SELECT A FROM T,",
            "SELECT A FROM T WHERE",
            "SELECT A FROM T GROUP BY",
            "SELECT A FROM T HAVING",
            "SELECT A FROM T ORDER BY",
            "SELECT A FROM T ORDER BY DESC",
            "SELECT A FROM T OFFSET ROWS",
            "SELECT A FROM T FETCH ROW ONLY",
            "SELECT A FROM T FETCH FIRST 2",
            "SELECT A FROM T FETCH NEXT 2 ROWS",
        ] {
            assert!(parse(source).is_err(), "unexpected parse success: {source}");
        }
    }

    #[test]
    fn duplicates_and_clause_order_are_rejected_deterministically() {
        for source in [
            "SELECT ALL ALL A FROM T",
            "SELECT DISTINCT ALL A FROM T",
            "SELECT A FROM T FROM U",
            "SELECT A FROM T WHERE A=1 WHERE B=2",
            "SELECT A FROM T GROUP BY A GROUP BY B",
            "SELECT A FROM T HAVING A=1 HAVING B=2",
            "SELECT A FROM T ORDER BY A ORDER BY B",
            "SELECT A FROM T OFFSET 1 ROW OFFSET 2 ROWS",
            "SELECT A FROM T FETCH FIRST ROW ONLY FETCH NEXT ROW ONLY",
            "SELECT A FROM T ORDER BY A ASC DESC",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "wrong duplicate diagnostic: {source}"
            );
        }
        for source in [
            "SELECT A FROM T HAVING A=1 WHERE B=2",
            "SELECT A FROM T ORDER BY A GROUP BY A",
            "SELECT A FROM T FETCH FIRST ROW ONLY OFFSET 1 ROW",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "wrong order diagnostic: {source}"
            );
        }
    }

    #[test]
    fn unsupported_select_families_never_receive_generic_nodes() {
        for source in [
            "WITH X AS (SELECT A FROM T) SELECT A FROM X",
            "SELECT A FROM T UNION SELECT A FROM U",
            "SELECT A FROM T EXCEPT SELECT A FROM U",
            "SELECT A FROM T INTERSECT SELECT A FROM U",
            "SELECT A FROM T JOIN U ON T.ID=U.ID",
            "SELECT A INTO :OUT FROM T",
            "SELECT A FROM T FOR UPDATE",
            "SELECT A FROM T WITH UR",
            "SELECT A FROM T OPTIMIZE FOR 1 ROW",
            "SELECT A FROM T QUERYNO 7",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "unsupported family was not explicit: {source}"
            );
        }
        for source in [
            "SELECT (SELECT A FROM T) FROM U",
            "SELECT A FROM (SELECT A FROM T)",
            "SELECT A FROM T WHERE A = (SELECT A FROM U)",
            "SELECT A AS B FROM T",
            "SELECT A FROM T X",
        ] {
            assert!(parse(source).is_err(), "unexpected parse success: {source}");
        }
    }

    #[test]
    fn extra_and_multiple_statements_are_rejected() {
        assert!(parse("SELECT A FROM T;").is_ok());
        for source in [
            "SELECT A FROM T; SELECT B FROM U",
            "SELECT A FROM T;;",
            "SELECT A FROM T GARBAGE",
            "VALUES 1",
            "",
        ] {
            assert!(parse(source).is_err(), "unexpected parse success: {source}");
        }
    }

    #[test]
    fn unicode_identifiers_and_original_spans_are_preserved() {
        let source = "-- lambda λ\nSELECT \"Écart\" + 1, 表.* FROM 模式.表\nWHERE 表.列 > 0";
        let query = parse(source).unwrap();
        assert_eq!(query.span().start, Db2SourceLocation { line: 2, column: 1 });
        assert_eq!(
            query.span().end,
            Db2SourceLocation {
                line: 3,
                column: 14
            }
        );
        assert_eq!(query.items()[0].span().start.column, 8);
        assert_eq!(query.items()[0].span().end.column, 19);
        let Db2SelectItem::Wildcard {
            qualifier: Some(name),
            span,
        } = &query.items()[1]
        else {
            panic!("expected qualified Unicode wildcard")
        };
        assert_eq!(name.parts()[0].value(), "表");
        assert_eq!(span.start.column, 21);
        assert_eq!(query.sources()[0].name().parts()[0].value(), "模式");
        assert_eq!(query.where_condition().unwrap().span().start.line, 3);
        assert_eq!(query.where_condition().unwrap().span().start.column, 7);

        let problem = parse("SELECT A FROM 表\nWHERE 列 +").unwrap_err();
        assert_eq!(problem.code, Db2SyntaxDiagnosticCode::MissingToken);
        assert_eq!(problem.location.line, 2);
        assert_eq!(problem.location.column, 10);
    }

    #[test]
    fn list_expression_name_and_count_bounds_are_enforced() {
        let list_limits = Db2AstLimits {
            max_list_items: 1,
            ..Db2AstLimits::default()
        };
        for source in [
            "SELECT A, B FROM T",
            "SELECT A FROM T, U",
            "SELECT A FROM T GROUP BY A, B",
            "SELECT A FROM T ORDER BY A, B",
        ] {
            assert_eq!(
                parse_db2_select_core(source, Db2SyntaxLimits::default(), list_limits)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }

        let node_limits = Db2AstLimits {
            max_expression_nodes: 3,
            ..Db2AstLimits::default()
        };
        assert!(
            parse_db2_select_core(
                "SELECT A + 1, B + 2 FROM T",
                Db2SyntaxLimits::default(),
                node_limits,
            )
            .is_err()
        );
        let name_limits = Db2AstLimits {
            max_identifier_bytes: 2,
            max_name_parts: 1,
            ..Db2AstLimits::default()
        };
        assert!(
            parse_db2_select_core(
                "SELECT A FROM LONG",
                Db2SyntaxLimits::default(),
                name_limits,
            )
            .is_err()
        );
        assert!(
            parse_db2_select_core("SELECT A FROM S.T", Db2SyntaxLimits::default(), name_limits,)
                .is_err()
        );
        for source in [
            "SELECT A FROM T ORDER BY 0",
            "SELECT A FROM T ORDER BY 4294967296",
            "SELECT A FROM T OFFSET 18446744073709551616 ROWS",
            "SELECT A FROM T FETCH FIRST 0 ROWS ONLY",
        ] {
            assert_eq!(
                parse(source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }
    }

    #[test]
    fn lexer_resource_limits_are_retained() {
        let limits = Db2SyntaxLimits {
            max_statement_bytes: 8,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            parse_db2_select_core("SELECT A FROM T", limits, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );
        let limits = Db2SyntaxLimits {
            max_tokens: 3,
            ..Db2SyntaxLimits::default()
        };
        assert_eq!(
            parse_db2_select_core("SELECT A FROM T", limits, Db2AstLimits::default())
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::TooManyTokens
        );
    }
}
