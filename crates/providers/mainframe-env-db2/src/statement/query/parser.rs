//! Bounded parsing and located diagnostics for the SELECT core.

use super::*;

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
            &problem.message,
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
                    where_condition = Some(self.parse_search_condition(value_start, value_end)?);
                }
                ClauseKind::GroupBy => {
                    group_by = self.parse_expression_list(value_start, value_end, "GROUP BY")?;
                }
                ClauseKind::Having => {
                    having = Some(self.parse_search_condition(value_start, value_end)?);
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

    fn parse_search_condition(
        &mut self,
        start: usize,
        end: usize,
    ) -> Result<Db2QueryExpression, Db2SyntaxDiagnostic> {
        let expression = self.parse_expression(start, end)?;
        let kind = expression
            .parsed()
            .arena()
            .get(expression.parsed().root())
            .map(|node| node.kind());
        let is_predicate = matches!(
            kind,
            Some(Db2ExpressionKind::IsNull { .. })
                | Some(Db2ExpressionKind::Binary {
                    operator: Db2BinaryOperator::Equal
                        | Db2BinaryOperator::NotEqual
                        | Db2BinaryOperator::Less
                        | Db2BinaryOperator::LessOrEqual
                        | Db2BinaryOperator::Greater
                        | Db2BinaryOperator::GreaterOrEqual
                        | Db2BinaryOperator::And
                        | Db2BinaryOperator::Or,
                    ..
                })
                | Some(Db2ExpressionKind::Unary {
                    operator: Db2UnaryOperator::Not,
                    ..
                })
        );
        if !is_predicate {
            return Err(self.diagnostic_at(
                start,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "WHERE and HAVING require a Db2 search condition",
            ));
        }
        Ok(expression)
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
                            &problem.message,
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
        self.source
            .get(span.start_byte..span.end_byte)
            .ok_or_else(|| {
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
            start_byte: self
                .tokens
                .get(start)
                .map_or_else(|| self.end_byte(), |token| token.span.start_byte),
            end_byte: end
                .checked_sub(1)
                .and_then(|index| self.tokens.get(index))
                .map_or_else(|| self.end_byte(), |token| token.span.end_byte),
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
            start_byte: self.tokens.first().map_or(0, |token| token.span.start_byte),
            end_byte: self.tokens.last().map_or(0, |token| token.span.end_byte),
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

    fn end_byte(&self) -> usize {
        self.tokens
            .get(self.end.saturating_sub(1))
            .map_or(0, |token| token.span.end_byte)
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
        Db2SyntaxDiagnostic::new(code, location, message.as_ref())
    }
}
