//! Owned, bounded searched UPDATE syntax, SQL 0155.
//!
//! Source: Db2 13 baseline `ibm-db2-for-zos-13-2026-08-13`,
//! `SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_update.html`, 252756 bytes,
//! SHA-256 `0ddb9b81001292f17d7b2fe9d20db41274bb921a85ca11a8fab3ec2c0931c4ab`.
//! Lines 60–91 declare searched UPDATE and single-column assignments; lines
//! 305, 311–312 require unique columns and forbid aggregate assignment functions.
//! Aggregate/function-invocation/function-resolution context is pinned in the
//! same manifest. Known aggregate-family spellings are fenced pending function
//! binding: this does not declare every scalar/UDF with that name invalid SQL.
//! Binding, column/type/default/nullability validity and execution remain pending.
//! No whole-row recognition, conformance, backend or licensed credit is claimed.
//!
//! Expression transfer follows CREATE VIEW: decode SQL names once, relocate all
//! spans, preserve inherited literal escape text. Intermediate raw names must
//! fit the shared 1024-byte ceiling; CAST still cannot start its type with a
//! delimited component. No shared-parser fence is lifted here.

use crate::{
    Db2AstLimits, Db2BinaryOperator, Db2DataType, Db2ExpressionArena, Db2ExpressionId,
    Db2ExpressionKind, Db2Identifier, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan,
    Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token,
    Db2TokenKind, Db2UnaryOperator, lex_db2, parse_db2_expression,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2UpdateStatement {
    target: Db2QualifiedName,
    target_span: Db2SourceSpan,
    assignments: Vec<Db2UpdateAssignment>,
    where_condition: Option<Db2UpdateExpression>,
    span: Db2SourceSpan,
}

impl Db2UpdateStatement {
    #[must_use]
    pub const fn target(&self) -> &Db2QualifiedName {
        &self.target
    }
    #[must_use]
    pub const fn target_span(&self) -> Db2SourceSpan {
        self.target_span
    }
    #[must_use]
    pub fn assignments(&self) -> &[Db2UpdateAssignment] {
        &self.assignments
    }
    #[must_use]
    pub const fn where_condition(&self) -> Option<&Db2UpdateExpression> {
        self.where_condition.as_ref()
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2UpdateAssignment {
    column: Db2Identifier,
    column_span: Db2SourceSpan,
    value: Db2UpdateValue,
    span: Db2SourceSpan,
}

impl Db2UpdateAssignment {
    #[must_use]
    pub const fn column(&self) -> &Db2Identifier {
        &self.column
    }
    #[must_use]
    pub const fn column_span(&self) -> Db2SourceSpan {
        self.column_span
    }
    #[must_use]
    pub const fn value(&self) -> &Db2UpdateValue {
        &self.value
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2UpdateValue {
    Expression(Db2UpdateExpression),
    Default(Db2SourceSpan),
    Null(Db2SourceSpan),
}

impl Db2UpdateValue {
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        match self {
            Self::Expression(value) => value.span(),
            Self::Default(span) | Self::Null(span) => *span,
        }
    }
}

/// The wrapper and every arena node refer to the original complete source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2UpdateExpression {
    arena: Db2ExpressionArena,
    root: Db2ExpressionId,
    span: Db2SourceSpan,
}

impl Db2UpdateExpression {
    #[must_use]
    pub const fn arena(&self) -> &Db2ExpressionArena {
        &self.arena
    }
    #[must_use]
    pub const fn root(&self) -> Db2ExpressionId {
        self.root
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse one searched UPDATE with an optional terminal semicolon.
/// `max_list_items` counts assignments plus all function arguments, CASE
/// branches and built-in CAST type arguments throughout SET and WHERE.
/// `max_expression_nodes` counts their combined nodes, including each standalone
/// DEFAULT/NULL. Depth bounds apply to every expression tree.
pub fn parse_db2_searched_update(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2UpdateStatement, Db2SyntaxDiagnostic> {
    ast_limits.validate().map_err(|error| {
        problem(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &error.message,
        )
    })?;
    let lexed = lex_db2(source, syntax_limits)?;
    let tokens = lexed.tokens();
    let end = tokens.len()
        - usize::from(
            tokens
                .last()
                .is_some_and(|token| token.kind == Db2TokenKind::Symbol(Db2Symbol::Semicolon)),
        );
    for token in &tokens[..end] {
        if token.kind == Db2TokenKind::Symbol(Db2Symbol::Semicolon) {
            return Err(problem(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                token.span.start,
                "UPDATE permits only one terminal semicolon",
            ));
        }
    }
    let mut parser = UpdateParser {
        source,
        tokens: &tokens[..end],
        position: 0,
        syntax_limits,
        ast_limits,
        list_items: 0,
        nodes: 0,
    };
    if !parser.take_word("UPDATE") {
        return Err(parser.error(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "expected the declared searched UPDATE family",
        ));
    }
    let target_start = parser.position;
    let mut parts = vec![parser.identifier()?];
    while parser.take_symbol(Db2Symbol::Period) {
        if parts.len() >= ast_limits.max_name_parts {
            return Err(parser.invalid("UPDATE target exceeds the configured name-part limit"));
        }
        parts.push(parser.identifier()?);
    }
    let target =
        Db2QualifiedName::new(parts, ast_limits).map_err(|error| parser.invalid(&error.message))?;
    let target_span = parser.range_span(target_start, parser.position);
    if !parser.take_word("SET") {
        return Err(parser.error(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "UPDATE requires SET; correlation, period and INCLUDE clauses are deferred",
        ));
    }
    let mut assignments = Vec::new();
    let mut names = BTreeSet::new();
    loop {
        let start = parser.position;
        parser.add_list(1, parser.location())?;
        let column = parser.identifier()?;
        let column_span = parser.range_span(start, parser.position);
        if !names.insert(column.value().to_owned()) {
            return Err(problem(
                Db2SyntaxDiagnosticCode::DuplicateClause,
                column_span.start,
                "UPDATE repeats an effective assignment column name",
            ));
        }
        if !parser.take_symbol(Db2Symbol::Equal) {
            return Err(parser.error(Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "UPDATE requires one unqualified column = value; tuple and qualified targets are deferred"));
        }
        let value = parser.value()?;
        assignments.push(Db2UpdateAssignment {
            column,
            column_span,
            value,
            span: parser.range_span(start, parser.position),
        });
        if !parser.take_symbol(Db2Symbol::Comma) {
            break;
        }
    }
    let where_condition = if parser.take_word("WHERE") {
        if parser.word() == Some("CURRENT") {
            return Err(parser.error(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "positioned UPDATE WHERE CURRENT OF is deferred",
            ));
        }
        let start = parser.position;
        parser.position = end;
        Some(parser.expression(start, end, true)?)
    } else {
        None
    };
    if parser.position != end {
        return Err(parser.error(Db2SyntaxDiagnosticCode::UnexpectedToken,
            "undeclared UPDATE suffix; isolation, SKIP LOCKED, QUERYNO and extra statements are deferred"));
    }
    Ok(Db2UpdateStatement {
        target,
        target_span,
        assignments,
        where_condition,
        span: Db2SourceSpan {
            end_byte: tokens.last().expect("UPDATE has tokens").span.end_byte,
            end: tokens.last().expect("UPDATE has tokens").span.end,
            ..parser.range_span(0, end)
        },
    })
}

struct UpdateParser<'a> {
    source: &'a str,
    tokens: &'a [Db2Token],
    position: usize,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
    list_items: usize,
    nodes: usize,
}

impl UpdateParser<'_> {
    fn value(&mut self) -> Result<Db2UpdateValue, Db2SyntaxDiagnostic> {
        let start = self.position;
        let mut depth = 0_usize;
        while let Some(token) = self.tokens.get(self.position) {
            match token.kind {
                Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => depth += 1,
                Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) if depth > 0 => depth -= 1,
                Db2TokenKind::Symbol(Db2Symbol::Comma) if depth == 0 => break,
                _ if depth == 0 && self.word() == Some("WHERE") => break,
                _ => {}
            }
            self.position += 1;
        }
        if start == self.position {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                "missing UPDATE value",
            ));
        }
        let span = self.range_span(start, self.position);
        if self.position == start + 1 {
            let value = match self.word_at(start) {
                Some("DEFAULT") => Some(Db2UpdateValue::Default(span)),
                Some("NULL") => Some(Db2UpdateValue::Null(span)),
                _ => None,
            };
            if let Some(value) = value {
                self.add_nodes(1, span.start)?;
                return Ok(value);
            }
        }
        self.expression(start, self.position, false)
            .map(Db2UpdateValue::Expression)
    }

    fn expression(
        &mut self,
        start: usize,
        end: usize,
        predicate: bool,
    ) -> Result<Db2UpdateExpression, Db2SyntaxDiagnostic> {
        if start == end {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                "missing UPDATE search condition",
            ));
        }
        let span = self.range_span(start, end);
        let mut raw_limits = self.ast_limits;
        // Permit bounded raw quoted names only in the intermediate expression
        // graph. Enforce effective name bounds on every transferred name below.
        for token in &self.tokens[start..end] {
            if matches!(&token.kind, Db2TokenKind::Word { value, delimited: false }
                if matches!(value.as_str(), "SELECT" | "VALUES" | "DEFAULT"))
            {
                return Err(problem(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    token.span.start,
                    "UPDATE fullselect forms and DEFAULT inside expressions are deferred",
                ));
            }
            if let Db2TokenKind::Word {
                value,
                delimited: true,
            } = &token.kind
            {
                raw_limits.max_identifier_bytes = raw_limits
                    .max_identifier_bytes
                    .max(value.trim_end_matches(' ').len());
                raw_limits.validate().map_err(|_| problem(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement, token.span.start,
                    "UPDATE expression exceeds the inherited AST raw-name ceiling before decoding"))?;
            }
        }
        let parsed = parse_db2_expression(
            &self.source[span.start_byte..span.end_byte],
            self.syntax_limits,
            raw_limits,
        )
        .map_err(|mut error| {
            error.location = relocate_location(error.location, span.start);
            error
        })?;
        self.add_nodes(parsed.arena().nodes().len(), span.start)?;
        let mut arena = Db2ExpressionArena::new(self.ast_limits)
            .map_err(|error| self.invalid(&error.message))?;
        let mut shapes = Vec::new();
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
                    *name = decoded_name(name, self.ast_limits, node_span.start)?;
                }
                // Intermediate name budget expansion must never expand hosts.
                Db2ExpressionKind::HostVariable(host)
                    if host.variable().value().len() > self.ast_limits.max_identifier_bytes
                        || host.indicator().is_some_and(|name| {
                            name.value().len() > self.ast_limits.max_identifier_bytes
                        }) =>
                {
                    return Err(problem(
                        Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                        node_span.start,
                        "UPDATE host identifier exceeds its configured byte limit",
                    ));
                }
                _ => {}
            }
            let entries = match &kind {
                Db2ExpressionKind::Function { name, arguments } => {
                    let last = name
                        .parts()
                        .last()
                        .expect("owned names are nonempty")
                        .value();
                    if aggregate_family(last) {
                        return Err(problem(
                            Db2SyntaxDiagnosticCode::UnsupportedStatement,
                            node_span.start,
                            "UPDATE aggregate-family call is deferred pending scalar/UDF function binding; aggregate assignment functions are forbidden (source line 311)",
                        ));
                    }
                    if last == "UNPACK" {
                        return Err(problem(
                            Db2SyntaxDiagnosticCode::UnsupportedStatement,
                            node_span.start,
                            "UPDATE UNPACK function form is deferred pending function binding",
                        ));
                    }
                    arguments.len()
                }
                Db2ExpressionKind::Case { branches, .. } => branches.len(),
                Db2ExpressionKind::Cast {
                    data_type: Db2DataType::BuiltIn(data_type),
                    ..
                } => data_type.arguments().len(),
                _ => 0,
            };
            self.add_list(entries, node_span.start)?;
            // Classify bottom-up over the bounded, backward-reference-only arena.
            // AND/OR/NOT need predicate children at every depth, including CASE;
            // comparison and NULL operands must be scalar.
            let shape = expression_shape(&kind, &shapes);
            if shape == Shape::Invalid {
                return Err(problem(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    node_span.start,
                    "UPDATE expression has invalid scalar/predicate composition",
                ));
            }
            shapes.push(shape);
            arena.push(kind, node_span).map_err(|error| {
                problem(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    node_span.start,
                    &error.message,
                )
            })?;
        }
        let expected = if predicate {
            Shape::Predicate
        } else {
            Shape::Scalar
        };
        if shapes[parsed.root().index() as usize] != expected {
            return Err(problem(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                span.start,
                if predicate {
                    "UPDATE WHERE requires a structural search condition"
                } else {
                    "UPDATE assignment requires a scalar expression"
                },
            ));
        }
        Ok(Db2UpdateExpression {
            arena,
            root: parsed.root(),
            span,
        })
    }

    fn add_list(
        &mut self,
        count: usize,
        location: Db2SourceLocation,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        self.list_items = self
            .list_items
            .checked_add(count)
            .filter(|total| *total <= self.ast_limits.max_list_items)
            .ok_or_else(|| {
                problem(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    location,
                    "UPDATE exceeds the aggregate list-entry limit",
                )
            })?;
        Ok(())
    }

    fn add_nodes(
        &mut self,
        count: usize,
        location: Db2SourceLocation,
    ) -> Result<(), Db2SyntaxDiagnostic> {
        self.nodes = self
            .nodes
            .checked_add(count)
            .filter(|total| *total <= self.ast_limits.max_expression_nodes)
            .ok_or_else(|| {
                problem(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    location,
                    "UPDATE exceeds the aggregate expression-node limit",
                )
            })?;
        Ok(())
    }

    fn identifier(&mut self) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(Db2Token {
            kind: Db2TokenKind::Word { value, delimited },
            span,
        }) = self.tokens.get(self.position)
        else {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                "expected one SQL identifier",
            ));
        };
        let identifier = decoded_identifier(value, *delimited, self.ast_limits, span.start)?;
        self.position += 1;
        Ok(identifier)
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
    fn word(&self) -> Option<&str> {
        self.word_at(self.position)
    }
    fn take_word(&mut self, word: &str) -> bool {
        if self.word() == Some(word) {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn take_symbol(&mut self, symbol: Db2Symbol) -> bool {
        if self
            .tokens
            .get(self.position)
            .is_some_and(|token| token.kind == Db2TokenKind::Symbol(symbol))
        {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn range_span(&self, start: usize, end: usize) -> Db2SourceSpan {
        Db2SourceSpan {
            end_byte: self.tokens[end - 1].span.end_byte,
            end: self.tokens[end - 1].span.end,
            ..self.tokens[start].span
        }
    }
    fn location(&self) -> Db2SourceLocation {
        self.tokens.get(self.position).map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        )
    }
    fn error(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        problem(code, self.location(), message)
    }
    fn invalid(&self, message: &str) -> Db2SyntaxDiagnostic {
        self.error(Db2SyntaxDiagnosticCode::InvalidStatementOperand, message)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Shape {
    Scalar,
    Predicate,
    Invalid,
}

fn expression_shape(kind: &Db2ExpressionKind, shapes: &[Shape]) -> Shape {
    let is = |id: Db2ExpressionId, expected| shapes.get(id.index() as usize) == Some(&expected);
    let (valid, result) = match kind {
        Db2ExpressionKind::Unary {
            operator: Db2UnaryOperator::Not,
            operand,
        } => (is(*operand, Shape::Predicate), Shape::Predicate),
        Db2ExpressionKind::Unary { operand, .. } => (is(*operand, Shape::Scalar), Shape::Scalar),
        Db2ExpressionKind::Binary {
            left,
            operator,
            right,
        } => {
            let boolean = matches!(operator, Db2BinaryOperator::And | Db2BinaryOperator::Or);
            let comparison = matches!(
                operator,
                Db2BinaryOperator::Equal
                    | Db2BinaryOperator::NotEqual
                    | Db2BinaryOperator::Less
                    | Db2BinaryOperator::LessOrEqual
                    | Db2BinaryOperator::Greater
                    | Db2BinaryOperator::GreaterOrEqual
            );
            let operand = if boolean {
                Shape::Predicate
            } else {
                Shape::Scalar
            };
            (
                is(*left, operand) && is(*right, operand),
                if boolean || comparison {
                    Shape::Predicate
                } else {
                    Shape::Scalar
                },
            )
        }
        Db2ExpressionKind::IsNull { expression, .. } => {
            (is(*expression, Shape::Scalar), Shape::Predicate)
        }
        Db2ExpressionKind::Cast { expression, .. } => {
            (is(*expression, Shape::Scalar), Shape::Scalar)
        }
        Db2ExpressionKind::Function { arguments, .. } => (
            arguments.iter().all(|id| is(*id, Shape::Scalar)),
            Shape::Scalar,
        ),
        Db2ExpressionKind::Case {
            operand,
            branches,
            otherwise,
        } => {
            let condition = if operand.is_some() {
                Shape::Scalar
            } else {
                Shape::Predicate
            };
            (
                operand.is_none_or(|id| is(id, Shape::Scalar))
                    && branches
                        .iter()
                        .all(|(when, then)| is(*when, condition) && is(*then, Shape::Scalar))
                    && otherwise.is_none_or(|id| is(id, Shape::Scalar)),
                Shape::Scalar,
            )
        }
        _ => (true, Shape::Scalar),
    };
    if valid { result } else { Shape::Invalid }
}

// Exact effective spellings from the pinned aggregate introduction and
// db2z_bif_regr.html line 5. Qualification or delimiting
// cannot establish scalar-vs-aggregate resolution; do not infer it here.
fn aggregate_family(name: &str) -> bool {
    matches!(
        name,
        "ARRAY_AGG"
            | "AVG"
            | "CORR"
            | "CORRELATION"
            | "COUNT"
            | "COUNT_BIG"
            | "COVAR_POP"
            | "COVARIANCE"
            | "COVAR"
            | "COVAR_SAMP"
            | "COVARIANCE_SAMP"
            | "CUME_DIST"
            | "GROUPING"
            | "LISTAGG"
            | "MAX"
            | "MEDIAN"
            | "MIN"
            | "PERCENTILE_CONT"
            | "PERCENTILE_DISC"
            | "PERCENT_RANK"
            | "REGR_AVGX"
            | "REGR_AVGY"
            | "REGR_COUNT"
            | "REGR_INTERCEPT"
            | "REGR_ICPT"
            | "REGR_R2"
            | "REGR_SLOPE"
            | "REGR_SXX"
            | "REGR_SXY"
            | "REGR_SYY"
            | "STDDEV_POP"
            | "STDDEV"
            | "STDDEV_SAMP"
            | "SUM"
            | "VAR_POP"
            | "VARIANCE"
            | "VAR"
            | "VAR_SAMP"
            | "VARIANCE_SAMP"
            | "XMLAGG"
    )
}

fn decoded_identifier(
    raw: &str,
    delimited: bool,
    limits: Db2AstLimits,
    location: Db2SourceLocation,
) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
    let effective = if delimited {
        raw.replace("\"\"", "\"")
    } else {
        raw.to_owned()
    };
    Db2Identifier::new(effective, delimited, limits).map_err(|error| {
        problem(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            location,
            &error.message,
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
    Db2QualifiedName::new(parts, limits).map_err(|error| {
        problem(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            location,
            &error.message,
        )
    })
}
fn relocate_location(local: Db2SourceLocation, base: Db2SourceLocation) -> Db2SourceLocation {
    Db2SourceLocation {
        line: base.line + local.line - 1,
        column: if local.line == 1 {
            base.column + local.column - 1
        } else {
            local.column
        },
    }
}
fn relocate_span(local: Db2SourceSpan, base: Db2SourceSpan) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: base.start_byte + local.start_byte,
        end_byte: base.start_byte + local.end_byte,
        start: relocate_location(local.start, base.start),
        end: relocate_location(local.end, base.start),
    }
}
fn problem(
    code: Db2SyntaxDiagnosticCode,
    location: Db2SourceLocation,
    message: &str,
) -> Db2SyntaxDiagnostic {
    Db2SyntaxDiagnostic::new(code, location, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Db2Literal, Db2StringKind};

    fn parse(sql: &str) -> Result<Db2UpdateStatement, Db2SyntaxDiagnostic> {
        parse_db2_searched_update(sql, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    fn expression(value: &Db2UpdateValue) -> &Db2UpdateExpression {
        let Db2UpdateValue::Expression(value) = value else {
            panic!("expected expression")
        };
        value
    }

    fn location(sql: &str, offset: usize) -> Db2SourceLocation {
        let before = &sql[..offset];
        Db2SourceLocation {
            line: before.bytes().filter(|b| *b == b'\n').count() as u32 + 1,
            column: before.rsplit('\n').next().unwrap().chars().count() as u32 + 1,
        }
    }

    fn assert_span(sql: &str, span: Db2SourceSpan, text: &str) {
        assert_eq!(&sql[span.start_byte..span.end_byte], text);
        assert_eq!(span.start, location(sql, span.start_byte));
        assert_eq!(span.end, location(sql, span.end_byte));
    }

    #[test]
    fn preserves_owned_target_assignments_defaults_null_and_columns() {
        let owned = {
            let sql = String::from(
                "UPDATE loc.sch.tbl SET a=other+1,b=DEFAULT,c=NULL,d=:Mixed-Host INDICATOR :Ind,e=? WHERE tbl.a>=:Low AND NOT (b IS NULL OR c<5);",
            );
            parse(&sql).unwrap()
        };
        assert_eq!(
            owned
                .target()
                .parts()
                .iter()
                .map(Db2Identifier::value)
                .collect::<Vec<_>>(),
            ["LOC", "SCH", "TBL"]
        );
        assert_eq!(owned.assignments().len(), 5);
        assert_eq!(owned.assignments()[0].column().value(), "A");
        assert!(matches!(
            owned.assignments()[1].value(),
            Db2UpdateValue::Default(_)
        ));
        assert!(matches!(
            owned.assignments()[2].value(),
            Db2UpdateValue::Null(_)
        ));
        let first = expression(owned.assignments()[0].value());
        assert!(matches!(
            first.arena().get(first.root()).unwrap().kind(),
            Db2ExpressionKind::Binary {
                operator: Db2BinaryOperator::Add,
                ..
            }
        ));
        assert!(matches!(
            first.arena().nodes()[0].kind(),
            Db2ExpressionKind::Column(_)
        ));
        let host = expression(owned.assignments()[3].value());
        let Db2ExpressionKind::HostVariable(host) = host.arena().nodes()[0].kind() else {
            panic!("host")
        };
        assert_eq!(host.variable().value(), "Mixed-Host");
        assert_eq!(host.indicator().unwrap().value(), "Ind");
        assert!(owned.where_condition().is_some());
        assert!(
            parse("update t set a=1")
                .unwrap()
                .where_condition()
                .is_none()
        );
    }

    #[test]
    fn effective_identifiers_decode_once_preserve_case_and_ignore_trailing_spaces() {
        let sql = r#"UPDATE "s""x"."t""""q " SET "a""""b "="c""""d"+"f""x"(CAST(1 AS s."typ""q")),a=2,"a"=3"#;
        let owned = parse(sql).unwrap();
        assert_eq!(owned.target().parts()[0].value(), "s\"x");
        assert_eq!(owned.target().parts()[1].value(), "t\"\"q");
        assert_eq!(owned.assignments()[0].column().value(), "a\"\"b");
        assert_eq!(owned.assignments()[1].column().value(), "A");
        assert_eq!(owned.assignments()[2].column().value(), "a");
        let arena = expression(owned.assignments()[0].value()).arena();
        let names = arena
            .nodes()
            .iter()
            .filter_map(|node| match node.kind() {
                Db2ExpressionKind::Column(n)
                | Db2ExpressionKind::Function { name: n, .. }
                | Db2ExpressionKind::Cast {
                    data_type: Db2DataType::Distinct(n),
                    ..
                } => Some(n.parts().last().unwrap().value()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(names, ["c\"\"d", "typ\"q", "f\"x"]);
        for sql in [
            r#"UPDATE t SET a=1,"A"=2"#,
            r#"UPDATE t SET "a "=1,"a"=2"#,
            r#"UPDATE t SET "a""b"=1,"a""b "=2"#,
        ] {
            assert_eq!(
                parse(sql).unwrap_err().code,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "{sql}"
            );
        }
    }

    #[test]
    fn comments_whitespace_and_nested_expression_boundaries() {
        for sep in [
            " ",
            "\n\t",
            "/* outer /* nested */ comment */",
            "-- comment\n",
        ] {
            let sql = format!(
                "{sep}update{sep}s{sep}.{sep}t{sep}set{sep}a{sep}={sep}f(1,g(2,3)),{sep}b=CASE WHEN c=1 THEN CAST(NULL AS INTEGER) ELSE 2 END{sep}where{sep}a=1;{sep}"
            );
            let owned = parse(&sql).unwrap();
            assert_eq!(owned.assignments().len(), 2);
            assert_eq!(owned.target().parts().len(), 2);
            assert!(owned.where_condition().is_some());
        }
    }

    #[test]
    fn rejects_malformed_trailing_and_undeclared_forms() {
        for sql in [
            "",
            "-- only comment",
            "UPDATE",
            "UPDATE t",
            "UPDATE t SET",
            "UPDATE t SET a",
            "UPDATE t SET a=",
            "UPDATE t SET =1",
            "UPDATE t SET a=1,",
            "UPDATE t SET a=,b=1",
            "UPDATE t SET a=1,,b=2",
            "UPDATE t SET a=1 WHERE",
            "UPDATE t SET a=(1",
            "UPDATE t SET a=1)",
            "UPDATE t SET a=f(,1)",
            "UPDATE t SET a=CASE WHEN c=1 THEN 2",
            "UPDATE t SET a=DEFAULT+1",
            "UPDATE t SET a=(DEFAULT)",
            "UPDATE t SET a=NULL+1",
            "UPDATE t SET a=(NULL)",
            "UPDATE t SET (a,b)=(1,2)",
            "UPDATE t SET t.a=1",
            "UPDATE t SET a=(1,2)",
            "UPDATE t SET a=(SELECT a FROM x)",
            "UPDATE t SET a=SELECT",
            "UPDATE t SET a=UNPACK(x)",
            "UPDATE t SET a=s.UNPACK(x)",
            "UPDATE t x SET a=1",
            "UPDATE t AS x SET a=1",
            "UPDATE t INCLUDE (b INT) SET a=1",
            "UPDATE t FOR PORTION OF BUSINESS_TIME FROM 1 TO 2 SET a=1",
            "UPDATE t SET a=1 WHERE CURRENT OF c",
            "UPDATE t SET a=1 WHERE CURRENT OF c FOR ROW 1",
            "UPDATE t SET a=1 WITH CS",
            "UPDATE t SET a=1 SKIP LOCKED DATA",
            "UPDATE t SET a=1 QUERYNO 2",
            "UPDATE t SET a=1 WHERE a=1 WITH RR",
            "UPDATE t SET a=1 WHERE a=1 SKIP LOCKED DATA",
            "UPDATE t SET a=1 WHERE a=1 QUERYNO 2",
            "UPDATE t SET a=1 WHERE a=1 WHERE b=2",
            "UPDATE t SET a=1 RETURNING a",
            "UPDATE t SET a=1; UPDATE t SET b=2",
            "UPDATE t SET a=1;;",
            "UPDATE t SET a=1 junk",
            "DELETE FROM t",
            "UPDATE t SET a=1 WHERE EXISTS (SELECT a FROM t)",
            "UPDATE t SET a=1 WHERE a IN (1,2)",
            "UPDATE t SET a=1 WHERE a BETWEEN 1 AND 2",
            "UPDATE t SET a=1 WHERE ? IS NULL",
            "UPDATE t SET a=1e2",
            "UPDATE t SET a=TRUE",
        ] {
            let error = parse(sql).expect_err(sql);
            assert!(
                error.location.line > 0 && error.location.column > 0,
                "{sql}"
            );
            assert!(error.message.len() <= 256, "{sql}");
        }
    }

    #[test]
    fn recursively_validates_predicate_and_scalar_compositions() {
        for condition in [
            "a=1",
            "a<>b",
            "a<=b",
            "a>b",
            "a IS NOT NULL",
            "NOT (a=1 OR b IS NULL) AND c<2",
            "CASE WHEN a=1 THEN b ELSE c END=2",
        ] {
            parse(&format!("UPDATE t SET a=1 WHERE {condition}")).unwrap();
        }
        for condition in [
            "a",
            "1",
            "f(a)",
            "a=1 AND b",
            "a OR b=1",
            "NOT (a=1 AND 2)",
            "(a=1 OR b) AND c=2",
            "a=(b=1)",
            "(a=1)+2=3",
            "f(a=1)=2",
            "CAST(a=1 AS INTEGER)=2",
            "(a=1 AND b=2) IS NULL",
            "CASE WHEN a=1 THEN b=2 ELSE c END=2",
        ] {
            parse(&format!("UPDATE t SET a=1 WHERE {condition}")).expect_err(condition);
        }
        for value in [
            "a=1",
            "a=1 AND b=2",
            "f(a=1)",
            "CASE WHEN a=1 OR b THEN 2 ELSE 3 END",
            "CASE a WHEN b=1 THEN 2 ELSE 3 END",
        ] {
            parse(&format!("UPDATE t SET a={value}")).expect_err(value);
        }
    }

    #[test]
    fn aggregate_and_ambiguous_families_are_explicitly_binding_pending() {
        let families = [
            "ARRAY_AGG",
            "AVG",
            "CORR",
            "CORRELATION",
            "COUNT",
            "COUNT_BIG",
            "COVAR_POP",
            "COVARIANCE",
            "COVAR",
            "COVAR_SAMP",
            "COVARIANCE_SAMP",
            "CUME_DIST",
            "GROUPING",
            "LISTAGG",
            "MAX",
            "MEDIAN",
            "MIN",
            "PERCENTILE_CONT",
            "PERCENTILE_DISC",
            "PERCENT_RANK",
            "REGR_AVGX",
            "REGR_AVGY",
            "REGR_COUNT",
            "REGR_INTERCEPT",
            "REGR_ICPT",
            "REGR_R2",
            "REGR_SLOPE",
            "REGR_SXX",
            "REGR_SXY",
            "REGR_SYY",
            "STDDEV_POP",
            "STDDEV",
            "STDDEV_SAMP",
            "SUM",
            "VAR_POP",
            "VARIANCE",
            "VAR",
            "VAR_SAMP",
            "VARIANCE_SAMP",
            "XMLAGG",
        ];
        for family in families {
            for call in [
                format!("{family}(c)"),
                format!("s.\"{family} \"(c)"),
                format!("CASE WHEN c=1 THEN f({family}(c)) ELSE 0 END"),
            ] {
                let error = parse(&format!("UPDATE t SET a={call}")).unwrap_err();
                assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnsupportedStatement);
                assert!(
                    error
                        .message
                        .contains("pending scalar/UDF function binding"),
                    "{call}"
                );
                let error = parse(&format!("UPDATE t SET a=1 WHERE {call}=1")).unwrap_err();
                assert!(error.message.contains("function binding"));
            }
        }
        for call in ["COUNT(*)", "COUNT_BIG(*)", "MAX(a,b)", "s.SUM(a)"] {
            assert!(
                parse(&format!("UPDATE t SET a={call}"))
                    .unwrap_err()
                    .message
                    .contains("function binding")
            );
        }
        for call in [
            "f(c)",
            "s.unknown(c)",
            "REGR_CUSTOM(c)",
            "\"sum\"(c)",
            "\"max\"(a,b)",
        ] {
            parse(&format!("UPDATE t SET a={call}")).unwrap();
        }
    }

    #[test]
    fn relocates_all_wrappers_nodes_and_diagnostics_on_original_source() {
        for prefix in ["", "  ", "-- lead\n\t", "/*é*/\n\n   "] {
            let sql = format!(
                "{prefix}UPDATE s.\"t\" SET a=f(1,\n :h)+2,\n b=DEFAULT,c=NULL WHERE a=1\n AND b IS NULL; /* tail */"
            );
            let owned = parse(&sql).unwrap();
            assert_span(
                &sql,
                owned.span(),
                "UPDATE s.\"t\" SET a=f(1,\n :h)+2,\n b=DEFAULT,c=NULL WHERE a=1\n AND b IS NULL;",
            );
            assert_span(&sql, owned.target_span(), "s.\"t\"");
            for (assignment, text) in
                owned
                    .assignments()
                    .iter()
                    .zip(["a=f(1,\n :h)+2", "b=DEFAULT", "c=NULL"])
            {
                assert_span(&sql, assignment.span(), text);
                assert_span(
                    &sql,
                    assignment.column_span(),
                    assignment.column().value().to_ascii_lowercase().as_str(),
                );
            }
            assert_span(&sql, owned.assignments()[1].value().span(), "DEFAULT");
            assert_span(&sql, owned.assignments()[2].value().span(), "NULL");
            let value = expression(owned.assignments()[0].value());
            assert_span(&sql, value.span(), "f(1,\n :h)+2");
            for (node, text) in
                value
                    .arena()
                    .nodes()
                    .iter()
                    .zip(["1", ":h", "f(1,\n :h)", "2", "f(1,\n :h)+2"])
            {
                assert_span(&sql, node.span(), text);
            }
            let condition = owned.where_condition().unwrap();
            assert_span(&sql, condition.span(), "a=1\n AND b IS NULL");
            for (node, text) in condition.arena().nodes().iter().zip([
                "a",
                "1",
                "a=1",
                "b",
                "b IS NULL",
                "a=1\n AND b IS NULL",
            ]) {
                assert_span(&sql, node.span(), text);
            }
            for tail in ["f(1,\n *)", "1 WHERE f(1,\n *)=2", "SUM(c)"] {
                let bad = format!("{prefix}UPDATE t SET a={tail}");
                let error = parse(&bad).unwrap_err();
                let at = if tail.contains('*') {
                    bad.rfind('*').unwrap()
                } else {
                    bad.rfind("SUM").unwrap()
                };
                assert_eq!(error.location, location(&bad, at), "{bad}: {error}");
            }
        }
    }

    #[test]
    fn assignment_where_and_nested_lists_share_exact_aggregate_budgets() {
        // 3 assignments + f's 2 arguments + g's 1 + h's 1 = 7;
        // f/g: 4 nodes, NULL/DEFAULT: 2, h(...)=2: 4 = 10.
        let sql = "UPDATE t SET a=f(1,g(2)),b=NULL,c=DEFAULT WHERE h(3)=2";
        let limits = Db2AstLimits {
            max_list_items: 7,
            max_expression_nodes: 10,
            ..Db2AstLimits::default()
        };
        parse_db2_searched_update(sql, Db2SyntaxLimits::default(), limits).unwrap();
        for limits in [
            Db2AstLimits {
                max_list_items: 6,
                ..limits
            },
            Db2AstLimits {
                max_expression_nodes: 9,
                ..limits
            },
        ] {
            assert!(
                parse_db2_searched_update(sql, Db2SyntaxLimits::default(), limits)
                    .unwrap_err()
                    .message
                    .contains("aggregate")
            );
        }
        let sql = "UPDATE t SET a=CASE WHEN c=1 THEN CAST(2 AS DECIMAL(3,1)) ELSE 0 END,b=DEFAULT WHERE f(c)=1";
        // 2 assignments + CASE branch + 2 CAST arguments + WHERE f argument.
        for (count, pass) in [(6, true), (5, false)] {
            let result = parse_db2_searched_update(
                sql,
                Db2SyntaxLimits::default(),
                Db2AstLimits {
                    max_list_items: count,
                    ..Db2AstLimits::default()
                },
            );
            assert_eq!(result.is_ok(), pass, "{result:?}");
        }
        for (count, pass) in [(2, true), (1, false)] {
            let result = parse_db2_searched_update(
                "UPDATE t SET a=NULL,b=DEFAULT",
                Db2SyntaxLimits::default(),
                Db2AstLimits {
                    max_expression_nodes: count,
                    max_list_items: count,
                    ..Db2AstLimits::default()
                },
            );
            assert_eq!(result.is_ok(), pass);
        }
    }

    #[test]
    fn exact_and_one_beyond_depth_names_literals_and_lexical_limits() {
        let sql = "UPDATE s.t SET a=-(-1) WHERE NOT (a=1)";
        for (depth, pass) in [(3, true), (2, false)] {
            assert_eq!(
                parse_db2_searched_update(
                    sql,
                    Db2SyntaxLimits::default(),
                    Db2AstLimits {
                        max_expression_depth: depth,
                        ..Db2AstLimits::default()
                    }
                )
                .is_ok(),
                pass
            );
        }
        for (parts, pass) in [(2, true), (1, false)] {
            assert_eq!(
                parse_db2_searched_update(
                    sql,
                    Db2SyntaxLimits::default(),
                    Db2AstLimits {
                        max_name_parts: parts,
                        ..Db2AstLimits::default()
                    }
                )
                .is_ok(),
                pass
            );
        }
        let limits = Db2AstLimits {
            max_identifier_bytes: 3,
            ..Db2AstLimits::default()
        };
        for sql in [
            r#"UPDATE "t""x" SET "a""b"="c""d""#,
            r#"UPDATE t SET a="f""x"(1)"#,
            r#"UPDATE t SET a=CAST(1 AS s."d""t")"#,
        ] {
            parse_db2_searched_update(sql, Db2SyntaxLimits::default(), limits).unwrap();
        }
        for sql in [
            "UPDATE long SET a=1",
            "UPDATE t SET long=1",
            "UPDATE t SET a=long",
            "UPDATE t SET a=long(1)",
            "UPDATE t SET a=CAST(1 AS s.long)",
            r#"UPDATE t SET a=f("c""d",:long)"#,
            r#"UPDATE t SET a=f("c""d",:h :long)"#,
        ] {
            parse_db2_searched_update(sql, Db2SyntaxLimits::default(), limits).expect_err(sql);
        }
        let sql = "UPDATE t SET a='abc'";
        for (bytes, pass) in [(3, true), (2, false)] {
            assert_eq!(
                parse_db2_searched_update(
                    sql,
                    Db2SyntaxLimits::default(),
                    Db2AstLimits {
                        max_literal_bytes: bytes,
                        ..Db2AstLimits::default()
                    }
                )
                .is_ok(),
                pass
            );
        }
        let sql = "UPDATE t SET a=f((1))";
        let exact = Db2SyntaxLimits {
            max_statement_bytes: sql.len(),
            max_tokens: 11,
            max_token_bytes: 6,
            max_nesting: 2,
        };
        parse_db2_searched_update(sql, exact, Db2AstLimits::default()).unwrap();
        for limits in [
            Db2SyntaxLimits {
                max_statement_bytes: sql.len() - 1,
                ..exact
            },
            Db2SyntaxLimits {
                max_tokens: 10,
                ..exact
            },
            Db2SyntaxLimits {
                max_token_bytes: 5,
                ..exact
            },
            Db2SyntaxLimits {
                max_nesting: 1,
                ..exact
            },
        ] {
            parse_db2_searched_update(sql, limits, Db2AstLimits::default())
                .expect_err("one beyond");
        }
        assert_eq!(
            parse_db2_searched_update(
                sql,
                exact,
                Db2AstLimits {
                    max_list_items: 0,
                    ..Db2AstLimits::default()
                }
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
        assert_eq!(
            parse_db2_searched_update(
                sql,
                Db2SyntaxLimits {
                    max_tokens: 0,
                    ..exact
                },
                Db2AstLimits::default()
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
    }

    #[test]
    fn preserves_inherited_literal_and_raw_name_cast_fences() {
        let owned = parse("UPDATE t SET a='it''s'").unwrap();
        assert_eq!(
            expression(owned.assignments()[0].value()).arena().nodes()[0].kind(),
            &Db2ExpressionKind::Literal(Db2Literal::String {
                kind: Db2StringKind::Character,
                value: "it''s".into()
            })
        );
        assert!(parse(r#"UPDATE t SET a=CAST(1 AS "s".typ)"#).is_err());
        let raw = format!("\"{}\"", "\"\"".repeat(513));
        let limits = Db2AstLimits {
            max_identifier_bytes: 1024,
            ..Db2AstLimits::default()
        };
        parse_db2_searched_update(
            &format!("UPDATE {raw} SET a=1"),
            Db2SyntaxLimits::default(),
            limits,
        )
        .unwrap();
        let error = parse_db2_searched_update(
            &format!("UPDATE t SET a={raw}"),
            Db2SyntaxLimits::default(),
            limits,
        )
        .unwrap_err();
        assert!(error.message.contains("inherited AST raw-name ceiling"));
    }
}
