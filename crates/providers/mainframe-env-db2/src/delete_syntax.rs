//! Owned bounded searched DELETE syntax for SQL 0065, Db2 13 baseline
//! `ibm-db2-for-zos-13-2026-08-13`, `db2z_sql_delete.html`.
//!
//! Only DELETE FROM name [WHERE common-search-condition] is accepted. Target,
//! column, function and type binding, privileges and deletion remain pending.
//! This private kernel has no execution route or official coverage credit.
//! Raw doubled identifier quotes are decoded once on transfer. Literal escape
//! text is inherited unchanged. The shared expression parser's 1024-byte raw
//! name ceiling, delimited first CAST-type-component restriction, numeric and
//! special-register fences remain in force.

use crate::{
    Db2AstLimits, Db2BinaryOperator, Db2DataType, Db2ExpressionArena, Db2ExpressionId,
    Db2ExpressionKind, Db2HostIdentifier, Db2HostReference, Db2Identifier, Db2QualifiedName,
    Db2SourceLocation, Db2SourceSpan, Db2Symbol, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2Token, Db2TokenKind, Db2UnaryOperator, lex_db2, parse_db2_expression,
};

/// A value located in the original complete DELETE source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2DeleteLocated<T> {
    value: T,
    span: Db2SourceSpan,
}

impl<T> Db2DeleteLocated<T> {
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// An owned common search condition with original-source arena spans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2DeleteSearchCondition {
    arena: Db2ExpressionArena,
    root: Db2ExpressionId,
    span: Db2SourceSpan,
}

impl Db2DeleteSearchCondition {
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

/// Omitted WHERE is represented by None, never by a synthetic true predicate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2SearchedDeleteStatement {
    target: Db2DeleteLocated<Db2QualifiedName>,
    where_condition: Option<Db2DeleteSearchCondition>,
    span: Db2SourceSpan,
}

impl Db2SearchedDeleteStatement {
    #[must_use]
    pub const fn target(&self) -> &Db2DeleteLocated<Db2QualifiedName> {
        &self.target
    }

    #[must_use]
    pub const fn where_condition(&self) -> Option<&Db2DeleteSearchCondition> {
        self.where_condition.as_ref()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse exactly one searched DELETE with one optional terminal semicolon.
/// The node budget covers the complete condition; the list budget aggregates
/// all function arguments, CASE branches and built-in CAST type arguments.
/// Name components use the separate configured name-part bound (at most three).
pub fn parse_db2_searched_delete(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2SearchedDeleteStatement, Db2SyntaxDiagnostic> {
    ast_limits.validate().map_err(|problem| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    let lexed = lex_db2(source, syntax_limits)?;
    let tokens = lexed.tokens();
    let mut end = tokens.len();
    if matches!(
        tokens.last().map(|t| &t.kind),
        Some(Db2TokenKind::Symbol(Db2Symbol::Semicolon))
    ) {
        end -= 1;
    }
    if let Some(token) = tokens[..end]
        .iter()
        .find(|t| matches!(t.kind, Db2TokenKind::Symbol(Db2Symbol::Semicolon)))
    {
        return Err(diagnostic(
            Db2SyntaxDiagnosticCode::UnexpectedToken,
            token.span.start,
            "DELETE accepts one statement and at most one terminal semicolon",
        ));
    }
    let mut parser = DeleteParser {
        tokens: &tokens[..end],
        position: 0,
        limits: ast_limits,
    };
    if parser.word() != Some("DELETE") {
        return Err(parser.error(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "expected searched DELETE",
        ));
    }
    parser.expect_word("DELETE")?;
    parser.expect_word("FROM")?;
    let name_start = parser.position;
    let mut parts = vec![parser.identifier()?];
    while parser.take_symbol(Db2Symbol::Period) {
        if parts.len() >= ast_limits.max_name_parts.min(3) {
            return Err(parser.error(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "DELETE target exceeds the configured or Db2 three-part name limit",
            ));
        }
        parts.push(parser.identifier()?);
    }
    let target_span = parser.range_span(name_start, parser.position);
    let target = Db2DeleteLocated {
        value: Db2QualifiedName::new(parts, ast_limits).map_err(|e| {
            diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                target_span.start,
                &e.message,
            )
        })?,
        span: target_span,
    };
    let where_condition = if parser.take_word("WHERE") {
        let start = parser.position;
        if start == end {
            return Err(parser.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                "WHERE requires a search condition",
            ));
        }
        let span = parser.range_span(start, end);
        // Widen only the bounded intermediate raw-name representation. Transfer
        // reapplies the caller's decoded-name and host-identifier limits.
        let mut expression_limits = ast_limits;
        for token in &parser.tokens[start..end] {
            if let Db2TokenKind::Word { value, delimited } = &token.kind {
                if !delimited && matches!(value.as_str(), "SELECT" | "EXISTS" | "VALUES") {
                    return Err(diagnostic(
                        Db2SyntaxDiagnosticCode::UnsupportedStatement,
                        token.span.start,
                        "CTE, fullselect and subquery forms are outside searched DELETE syntax",
                    ));
                }
                if *delimited {
                    expression_limits.max_identifier_bytes = expression_limits
                        .max_identifier_bytes
                        .max(value.trim_end_matches(' ').len());
                    expression_limits.validate().map_err(|_| {
                        diagnostic(Db2SyntaxDiagnosticCode::UnsupportedStatement, token.span.start,
                            "expression identifier exceeds the inherited AST raw-name ceiling before decoding")
                    })?;
                }
            }
        }
        let parsed = parse_db2_expression(
            &source[span.start_byte..span.end_byte],
            syntax_limits,
            expression_limits,
        )
        .map_err(|mut e| {
            e.location = relocate_location(e.location, span.start);
            e
        })?;
        let condition = transfer_condition(parsed.arena(), parsed.root(), span, ast_limits)?;
        parser.position = end;
        Some(condition)
    } else {
        None
    };
    if parser.position != end {
        return Err(parser.error(Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "aliases, periods, INCLUDE/SET, FETCH, isolation, SKIP LOCKED and QUERYNO are outside searched DELETE syntax"));
    }
    Ok(Db2SearchedDeleteStatement {
        target,
        where_condition,
        span: join_spans(tokens[0].span, tokens[tokens.len() - 1].span),
    })
}

fn decode_identifier(
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
    Db2Identifier::new(value, delimited, limits).map_err(|e| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            location,
            &e.message,
        )
    })
}

fn decode_name(
    name: &Db2QualifiedName,
    limits: Db2AstLimits,
    location: Db2SourceLocation,
) -> Result<Db2QualifiedName, Db2SyntaxDiagnostic> {
    let parts = name
        .parts()
        .iter()
        .map(|p| decode_identifier(p.value(), p.is_delimited(), limits, location))
        .collect::<Result<Vec<_>, _>>()?;
    Db2QualifiedName::new(parts, limits).map_err(|e| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            location,
            &e.message,
        )
    })
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Shape {
    Value,
    Predicate,
    Wildcard,
}

fn transfer_condition(
    raw: &Db2ExpressionArena,
    root: Db2ExpressionId,
    span: Db2SourceSpan,
    limits: Db2AstLimits,
) -> Result<Db2DeleteSearchCondition, Db2SyntaxDiagnostic> {
    let mut arena = Db2ExpressionArena::new(limits).map_err(|e| {
        diagnostic(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            span.start,
            &e.message,
        )
    })?;
    // Append-only IDs reference earlier nodes. Classify in one bounded linear
    // pass, avoiding a second recursive traversal or merely testing the root.
    let mut shapes = Vec::with_capacity(raw.nodes().len());
    let mut lists = 0_usize;
    for node in raw.nodes() {
        let node_span = relocate_span(node.span(), span);
        let invalid = || {
            diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                node_span.start,
                "search condition requires predicates for AND/OR/NOT and value operands for expressions",
            )
        };
        let shape = |id: Db2ExpressionId| shapes.get(id.index() as usize).copied();
        let value = |id| shape(id) == Some(Shape::Value);
        let predicate = |id| shape(id) == Some(Shape::Predicate);
        let mut kind = node.kind().clone();
        let (classification, count) = match &kind {
            Db2ExpressionKind::Wildcard => (Shape::Wildcard, 0),
            Db2ExpressionKind::Literal(_)
            | Db2ExpressionKind::Column(_)
            | Db2ExpressionKind::HostVariable(_)
            | Db2ExpressionKind::ParameterMarker => (Shape::Value, 0),
            Db2ExpressionKind::Unary {
                operator: Db2UnaryOperator::Not,
                operand,
            } => {
                if !predicate(*operand) {
                    return Err(invalid());
                }
                (Shape::Predicate, 0)
            }
            Db2ExpressionKind::Unary { operand, .. } => {
                if !value(*operand) {
                    return Err(invalid());
                }
                (Shape::Value, 0)
            }
            Db2ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                if matches!(operator, Db2BinaryOperator::And | Db2BinaryOperator::Or) {
                    if !predicate(*left) || !predicate(*right) {
                        return Err(invalid());
                    }
                    (Shape::Predicate, 0)
                } else {
                    if !value(*left) || !value(*right) {
                        return Err(invalid());
                    }
                    let comparison = matches!(
                        operator,
                        Db2BinaryOperator::Equal
                            | Db2BinaryOperator::NotEqual
                            | Db2BinaryOperator::Less
                            | Db2BinaryOperator::LessOrEqual
                            | Db2BinaryOperator::Greater
                            | Db2BinaryOperator::GreaterOrEqual
                    );
                    (
                        if comparison {
                            Shape::Predicate
                        } else {
                            Shape::Value
                        },
                        0,
                    )
                }
            }
            Db2ExpressionKind::IsNull { expression, .. } => {
                if !value(*expression) {
                    return Err(invalid());
                }
                (Shape::Predicate, 0)
            }
            Db2ExpressionKind::Function { arguments, .. } => {
                // Wildcards can only come from the inherited COUNT substrate;
                // function/category/catalog validity is a later binder obligation.
                if arguments
                    .iter()
                    .any(|id| !value(*id) && shape(*id) != Some(Shape::Wildcard))
                {
                    return Err(invalid());
                }
                (Shape::Value, arguments.len())
            }
            Db2ExpressionKind::Cast {
                expression,
                data_type,
            } => {
                if !value(*expression) {
                    return Err(invalid());
                }
                (
                    Shape::Value,
                    match data_type {
                        Db2DataType::BuiltIn(t) => t.arguments().len(),
                        _ => 0,
                    },
                )
            }
            Db2ExpressionKind::Case {
                operand,
                branches,
                otherwise,
            } => {
                if operand.is_some_and(|id| !value(id))
                    || branches.iter().any(|(when, then)| {
                        !(if operand.is_some() {
                            value(*when)
                        } else {
                            predicate(*when)
                        }) || !value(*then)
                    })
                    || otherwise.is_some_and(|id| !value(id))
                {
                    return Err(invalid());
                }
                (Shape::Value, branches.len())
            }
        };
        lists = lists.saturating_add(count);
        if lists > limits.max_list_items {
            return Err(diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                node_span.start,
                "DELETE exceeds the aggregate list-item limit",
            ));
        }
        match &mut kind {
            Db2ExpressionKind::Column(name)
            | Db2ExpressionKind::Function { name, .. }
            | Db2ExpressionKind::Cast {
                data_type: Db2DataType::Distinct(name),
                ..
            } => {
                *name = decode_name(name, limits, node_span.start)?;
            }
            Db2ExpressionKind::HostVariable(host) => {
                let make = |name: &Db2HostIdentifier| {
                    Db2HostIdentifier::new(name.value(), limits).map_err(|e| {
                        diagnostic(
                            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                            node_span.start,
                            &e.message,
                        )
                    })
                };
                *host = Db2HostReference::new(
                    make(host.variable())?,
                    host.indicator().map(make).transpose()?,
                );
            }
            _ => {}
        }
        arena.push(kind, node_span).map_err(|e| {
            diagnostic(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                node_span.start,
                &e.message,
            )
        })?;
        shapes.push(classification);
    }
    if shapes.get(root.index() as usize) != Some(&Shape::Predicate) {
        return Err(diagnostic(
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            span.start,
            "WHERE requires a common comparison or IS NULL search condition",
        ));
    }
    Ok(Db2DeleteSearchCondition { arena, root, span })
}

fn diagnostic(
    code: Db2SyntaxDiagnosticCode,
    location: Db2SourceLocation,
    message: &str,
) -> Db2SyntaxDiagnostic {
    Db2SyntaxDiagnostic::new(code, location, message)
}

fn join_spans(first: Db2SourceSpan, last: Db2SourceSpan) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: first.start_byte,
        start: first.start,
        end_byte: last.end_byte,
        end: last.end,
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

struct DeleteParser<'a> {
    tokens: &'a [Db2Token],
    position: usize,
    limits: Db2AstLimits,
}

impl DeleteParser<'_> {
    fn identifier(&mut self) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(Db2Token {
            kind: Db2TokenKind::Word { value, delimited },
            span,
        }) = self.tokens.get(self.position)
        else {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::MissingToken,
                "DELETE requires a target identifier",
            ));
        };
        if !delimited
            && matches!(
                value.as_str(),
                "DELETE"
                    | "FROM"
                    | "WHERE"
                    | "FOR"
                    | "INCLUDE"
                    | "SET"
                    | "FETCH"
                    | "WITH"
                    | "SKIP"
                    | "QUERYNO"
                    | "SELECT"
                    | "CURRENT"
            )
        {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "target identifier is missing before a clause keyword",
            ));
        }
        let name = decode_identifier(value, *delimited, self.limits, span.start)?;
        self.position += 1;
        Ok(name)
    }

    fn word(&self) -> Option<&str> {
        match self.tokens.get(self.position).map(|t| &t.kind) {
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) => Some(value),
            _ => None,
        }
    }

    fn take_word(&mut self, word: &str) -> bool {
        if self.word() == Some(word) {
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
                &format!("expected DELETE keyword {word}"),
            ))
        }
    }

    fn take_symbol(&mut self, symbol: Db2Symbol) -> bool {
        if matches!(self.tokens.get(self.position).map(|t| &t.kind), Some(Db2TokenKind::Symbol(s)) if *s == symbol)
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn range_span(&self, start: usize, end: usize) -> Db2SourceSpan {
        join_spans(self.tokens[start].span, self.tokens[end - 1].span)
    }

    fn error(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        let location = self.tokens.get(self.position).map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(Db2SourceLocation::START, |t| t.span.end)
            },
            |t| t.span.start,
        );
        diagnostic(code, location, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db2Literal;

    fn parse(sql: &str) -> Result<Db2SearchedDeleteStatement, Db2SyntaxDiagnostic> {
        with_limits(sql, Db2AstLimits::default())
    }

    fn with_limits(
        sql: &str,
        limits: Db2AstLimits,
    ) -> Result<Db2SearchedDeleteStatement, Db2SyntaxDiagnostic> {
        parse_db2_searched_delete(sql, Db2SyntaxLimits::default(), limits)
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

    fn check_span(sql: &str, span: Db2SourceSpan) {
        assert!(span.start_byte < span.end_byte);
        assert!(sql.get(span.start_byte..span.end_byte).is_some());
        assert_eq!(span.start, location(sql, span.start_byte));
        assert_eq!(span.end, location(sql, span.end_byte));
    }

    #[test]
    fn rejects_bare_values_and_structurally_invalid_predicates() {
        for condition in [
            "1",
            "A",
            "?",
            ":host",
            "F(A)",
            "A+1",
            "CAST(A AS INT)",
            "1 AND 2",
            "A=1 AND 2",
            "1 OR A=2",
            "A=1 OR (B=2 AND 3)",
            "NOT (A=1 AND 2)",
            "NOT 1",
            "(A=1 OR B) AND C=3",
            "(A=1)=(B=2)",
            "1=(A=2)",
            "(A=1)+2=3",
            "-(A=1)=2",
            "(A=1) IS NULL",
            "F(A=1)=2",
            "CAST(A=1 AS INT)=2",
            "CASE WHEN A=1 AND 2 THEN 3 ELSE 4 END=3",
            "CASE A WHEN B=2 THEN 3 END=3",
            "CASE WHEN A=1 THEN B=2 END=3",
        ] {
            let sql = format!("DELETE FROM T WHERE {condition}");
            let e = parse(&sql).unwrap_err();
            assert_eq!(
                e.code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{sql}: {e}"
            );
            assert!(e.location.column >= 21, "{sql}: {e}");
        }
    }

    #[test]
    fn optional_where_and_terminal_semicolon_are_preserved() {
        for sql in [
            "DELETE FROM T",
            "delete from s.t;",
            "DELETE FROM L.S.T ; -- end",
        ] {
            let statement = parse(sql).unwrap();
            assert!(statement.where_condition().is_none());
            check_span(sql, statement.span());
            check_span(sql, statement.target().span());
        }
        assert!(
            parse("DELETE FROM T WHERE 1=1")
                .unwrap()
                .where_condition()
                .is_some()
        );
        assert!(parse("DELETE FROM T WHERE").is_err());
    }

    #[test]
    fn accepts_current_common_predicates_and_value_expressions() {
        for condition in [
            "A=1",
            "A<>1",
            "A<1",
            "A<=1",
            "A>1",
            "A>=1",
            "A IS NULL",
            "A IS NOT NULL",
            "NOT A=1",
            "NOT (A=1 OR B IS NULL)",
            "A=1 AND B=2 OR C=3",
            "A=1 AND (B=2 OR C=3)",
            "F(A,1)+2*3=G(B)",
            "A||'x'='ax'",
            "A=:my-host INDICATOR :my-ind",
            "A=?",
            "CAST(NULL AS INT) IS NULL",
            "CAST(A AS DECIMAL(5,2))=1.25",
            "CAST(A AS TIMESTAMP(6) WITH TIME ZONE)=B",
            "CAST(A AS TIMESTAMP WITHOUT TIME ZONE) IS NULL",
            "CASE WHEN A=1 AND B=2 THEN F(C) ELSE NULL END IS NULL",
            "CASE A WHEN 1 THEN 2 ELSE 3 END=2",
            "COUNT(*)=1",
        ] {
            let sql = format!("DELETE FROM S.T WHERE {condition};");
            let statement = parse(&sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
            let condition = statement.where_condition().unwrap();
            assert!(condition.arena().get(condition.root()).is_some());
            for node in condition.arena().nodes() {
                check_span(&sql, node.span());
            }
        }
    }

    #[test]
    fn preserves_operator_precedence_in_owned_arena() {
        let s = parse("DELETE FROM T WHERE NOT A=1 AND B=2 OR C=3").unwrap();
        let c = s.where_condition().unwrap();
        let Db2ExpressionKind::Binary {
            left,
            operator: Db2BinaryOperator::Or,
            ..
        } = c.arena().get(c.root()).unwrap().kind()
        else {
            panic!()
        };
        let Db2ExpressionKind::Binary {
            left,
            operator: Db2BinaryOperator::And,
            ..
        } = c.arena().get(*left).unwrap().kind()
        else {
            panic!()
        };
        assert!(matches!(
            c.arena().get(*left).unwrap().kind(),
            Db2ExpressionKind::Unary {
                operator: Db2UnaryOperator::Not,
                ..
            }
        ));
    }

    #[test]
    fn decodes_target_and_every_transferred_name_once() {
        let sql = "DELETE FROM loc.\"s\".\"a\"\"\"\"b  \" WHERE \"s\".\"a\"\"\"\"b\"=\"f\"\"x\"(CAST(1 AS ty.\"d\"\"\"\"t\"))";
        let s = parse(sql).unwrap();
        let parts = s.target().value().parts();
        assert_eq!(
            parts.iter().map(|p| p.value()).collect::<Vec<_>>(),
            ["LOC", "s", "a\"\"b"]
        );
        assert!(parts[1].is_delimited());
        let c = s.where_condition().unwrap();
        let mut names = Vec::new();
        for n in c.arena().nodes() {
            match n.kind() {
                Db2ExpressionKind::Column(name)
                | Db2ExpressionKind::Function { name, .. }
                | Db2ExpressionKind::Cast {
                    data_type: Db2DataType::Distinct(name),
                    ..
                } => {
                    names.push(name.parts().iter().map(|p| p.value()).collect::<Vec<_>>());
                }
                _ => {}
            }
        }
        assert_eq!(
            names,
            [vec!["s", "a\"\"b"], vec!["TY", "d\"\"t"], vec!["f\"x"]]
        );
        // Effective equality ignores quote provenance and insignificant trailing spaces.
        let ordinary = parse("DELETE FROM a").unwrap();
        let delimited = parse("DELETE FROM \"A  \"").unwrap();
        assert_eq!(
            ordinary.target().value().parts()[0].value(),
            delimited.target().value().parts()[0].value()
        );
        assert_ne!(
            ordinary.target().value().parts()[0].is_delimited(),
            delimited.target().value().parts()[0].is_delimited()
        );
        assert_ne!(
            parse("DELETE FROM \"a\"").unwrap().target().value().parts()[0].value(),
            ordinary.target().value().parts()[0].value()
        );
    }

    #[test]
    fn preserves_inherited_literals_and_host_case_after_source_drop() {
        let s = {
            let sql = String::from("DELETE FROM T WHERE \"C\"='a''b' AND A=:MiXeD-host :InD");
            parse(&sql).unwrap()
        };
        let c = s.where_condition().unwrap();
        assert!(c.arena().nodes().iter().any(|n| matches!(n.kind(), Db2ExpressionKind::Literal(Db2Literal::String { value, .. }) if value == "a''b")));
        let host = c
            .arena()
            .nodes()
            .iter()
            .find_map(|n| match n.kind() {
                Db2ExpressionKind::HostVariable(h) => Some(h),
                _ => None,
            })
            .unwrap();
        assert_eq!(host.variable().value(), "MiXeD-host");
        assert_eq!(host.indicator().unwrap().value(), "InD");
        assert_eq!(s.target().value().parts()[0].value(), "T");
        assert!(c.arena().get(c.root()).is_some());
    }

    #[test]
    fn relocates_all_spans_in_multibyte_multiline_comments_and_expressions() {
        for sql in [
            "/*é*/ DELETE /*x*/ FROM \"表\".\"t\" WHERE (\"列\"=F(1,CAST(2 AS DECIMAL(3,1)))) ; --tail",
            "-- é\n DELETE\nFROM S /*é*/ . T\nWHERE /*x*/ NOT (A=1\nAND CASE WHEN B IS NULL THEN 1 ELSE 2 END=2);",
            "/* é\n nested /* comment */ */\n DELETE FROM T WHERE\n :h :i=1 OR A IS NOT NULL",
        ] {
            let s = parse(sql).unwrap();
            check_span(sql, s.span());
            check_span(sql, s.target().span());
            let c = s.where_condition().unwrap();
            check_span(sql, c.span());
            for node in c.arena().nodes() {
                check_span(sql, node.span());
                assert!(node.span().start_byte >= c.span().start_byte);
                assert!(node.span().end_byte <= c.span().end_byte);
            }
            assert_eq!(
                &sql[s.target().span().start_byte..s.target().span().end_byte],
                if sql.contains("表") {
                    "\"表\".\"t\""
                } else if sql.contains("FROM S") {
                    "S /*é*/ . T"
                } else {
                    "T"
                }
            );
        }
    }

    #[test]
    fn relocates_expression_errors_on_first_and_later_lines() {
        for (prefix, body) in [
            ("/*é*/ DELETE FROM T WHERE ", "A +"),
            ("-- é\n DELETE FROM T\nWHERE ", "A=1 AND\nB +"),
            ("DELETE FROM T WHERE ", "? IS NULL"),
        ] {
            let original =
                parse_db2_expression(body, Db2SyntaxLimits::default(), Db2AstLimits::default())
                    .unwrap_err();
            let sql = format!("{prefix}{body}");
            let error = parse(&sql).unwrap_err();
            assert_eq!(error.code, original.code);
            assert_eq!(
                error.location,
                relocate_location(original.location, location(&sql, prefix.len()))
            );
        }
        let sql = "-- é\nDELETE FROM T WHERE A=1 AND\n2";
        assert_eq!(
            parse(sql).unwrap_err().location,
            location(sql, sql.find("A=1").unwrap())
        );
    }

    #[test]
    fn malformed_trailing_and_undeclared_forms_fail_closed() {
        for sql in [
            "",
            " ",
            ";",
            "DELETE",
            "DELETE T",
            "DELETE FROM",
            "DELETE FROM .T",
            "DELETE FROM T.",
            "DELETE FROM T..U",
            "DELETE FROM WHERE",
            "DELETE FROM T WHERE",
            "DELETE FROM T WHERE ()",
            "DELETE FROM T WHERE A=",
            "DELETE FROM T WHERE A=1 AND",
            "DELETE FROM T WHERE (A=1",
            "DELETE FROM T WHERE A=1)",
            "DELETE FROM T WHERE A=1 WHERE B=2",
            "DELETE FROM T WHERE A=1 garbage",
            "DELETE FROM T;;",
            "DELETE FROM T; DELETE FROM U",
            "DELETE FROM T WHERE A=1; SELECT A FROM T",
            "DELETE FROM T; ; --tail",
            "DELETE FROM T C",
            "DELETE FROM T AS C",
            "DELETE FROM T WHERE CURRENT OF C",
            "DELETE FROM T WHERE CURRENT OF C FOR ROW 1 OF ROWSET",
            "DELETE FROM T FOR PORTION OF BUSINESS_TIME FROM 1 TO 2",
            "DELETE FROM T INCLUDE (X INT) SET X=1",
            "DELETE FROM T SET X=1",
            "DELETE FROM (SELECT A FROM T)",
            "WITH X AS (SELECT A FROM T) DELETE FROM X",
            "DELETE FROM T WHERE A=(SELECT A FROM U)",
            "DELETE FROM T WHERE EXISTS (SELECT A FROM U)",
            "DELETE FROM T WHERE A IN (1,2)",
            "DELETE FROM T WHERE A BETWEEN 1 AND 2",
            "DELETE FROM T WHERE A LIKE 'x'",
            "DELETE FROM T WHERE A=1 FETCH FIRST 1 ROW ONLY",
            "DELETE FROM T FETCH FIRST 1 ROW ONLY",
            "DELETE FROM T WITH CS",
            "DELETE FROM T WHERE A=1 WITH RR",
            "DELETE FROM T SKIP LOCKED DATA",
            "DELETE FROM T WHERE A=1 SKIP LOCKED DATA",
            "DELETE FROM T QUERYNO 1",
            "DELETE FROM T WHERE A=1 QUERYNO 1",
            "DELETE FROM T WHERE A!=1",
            "DELETE FROM T WHERE A=TRUE",
            "DELETE FROM T WHERE A=1E2",
            "DELETE FROM T WHERE A=NULL",
            "DELETE FROM T WHERE A=CURRENT DATE",
            "DELETE FROM T WHERE A=1 /*",
            "UPDATE T SET A=1",
        ] {
            let outcome = std::panic::catch_unwind(|| parse(sql));
            let error = outcome
                .unwrap_or_else(|_| panic!("panic: {sql}"))
                .unwrap_err();
            assert!(error.message.len() <= 256, "{sql}");
            assert!(
                error.location.line >= 1 && error.location.column >= 1,
                "{sql}"
            );
        }
        assert!(parse("DELETE FROM \"WHERE\" WHERE \"SELECT\"=1").is_ok());
    }

    #[test]
    fn aggregate_list_budget_counts_all_nested_families() {
        for (body, count) in [
            ("F(1)=G(2)", 2),
            ("F(G(1),2)=H(3)", 4),
            (
                "CASE WHEN A=1 THEN F(2) ELSE G(3) END=CAST(1 AS DECIMAL(4,2))",
                5,
            ),
            ("CASE A WHEN 1 THEN 2 WHEN 2 THEN 3 END=F(1)", 3),
        ] {
            let sql = format!("DELETE FROM T WHERE {body}");
            assert!(
                with_limits(
                    &sql,
                    Db2AstLimits {
                        max_list_items: count,
                        ..Db2AstLimits::default()
                    }
                )
                .is_ok(),
                "{sql}"
            );
            let error = with_limits(
                &sql,
                Db2AstLimits {
                    max_list_items: count - 1,
                    ..Db2AstLimits::default()
                },
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{sql}"
            );
        }
    }

    #[test]
    fn aggregate_node_and_depth_budgets_have_exact_boundaries() {
        // Both predicates share one arena: 3 + 3 + AND = 7 nodes, depth 3.
        let sql = "DELETE FROM T WHERE A=1 AND B=2";
        let c = parse(sql).unwrap();
        assert_eq!(c.where_condition().unwrap().arena().nodes().len(), 7);
        for (nodes, depth, accepted) in [(7, 3, true), (6, 3, false), (7, 2, false)] {
            let outcome = with_limits(
                sql,
                Db2AstLimits {
                    max_expression_nodes: nodes,
                    max_expression_depth: depth,
                    ..Db2AstLimits::default()
                },
            );
            assert_eq!(outcome.is_ok(), accepted);
        }
        let sql = "DELETE FROM T WHERE NOT NOT A=1";
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_expression_depth: 4,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_expression_depth: 3,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        let sql = format!(
            "DELETE FROM T WHERE {}A=1{}",
            "(".repeat(128),
            ")".repeat(128)
        );
        assert!(parse(&sql).is_err()); // inherited parser recursion fence
    }

    #[test]
    fn decoded_name_bytes_and_name_parts_have_exact_boundaries() {
        let limits = Db2AstLimits {
            max_identifier_bytes: 3,
            ..Db2AstLimits::default()
        };
        for sql in [
            "DELETE FROM ABC",
            "DELETE FROM \"a\"\"b \"",
            "DELETE FROM T WHERE \"a\"\"b\"=1",
            "DELETE FROM T WHERE \"a\"\"b\"(1)=1",
            "DELETE FROM T WHERE CAST(1 AS s.\"a\"\"b\")=1",
            "DELETE FROM T WHERE :abc=1",
            "DELETE FROM T WHERE A=:a :abc",
        ] {
            assert!(with_limits(sql, limits).is_ok(), "{sql}");
        }
        for sql in [
            "DELETE FROM ABCD",
            "DELETE FROM \"a\"\"bc\"",
            "DELETE FROM T WHERE \"a\"\"bc\"=1",
            "DELETE FROM T WHERE \"a\"\"bc\"(1)=1",
            "DELETE FROM T WHERE CAST(1 AS s.\"a\"\"bc\")=1",
            "DELETE FROM T WHERE :abcd=1",
            "DELETE FROM T WHERE A=:a :abcd",
            "DELETE FROM T WHERE \"a\"\"b\"=abcd",
            "DELETE FROM T WHERE \"a\"\"b\"=:abcd",
        ] {
            let error = with_limits(sql, limits).unwrap_err();
            assert!(error.message.contains("byte limit"), "{sql}: {error}");
        }
        assert!(
            with_limits(
                "DELETE FROM S.T",
                Db2AstLimits {
                    max_name_parts: 2,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                "DELETE FROM L.S.T",
                Db2AstLimits {
                    max_name_parts: 2,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        assert!(parse("DELETE FROM L.S.T").is_ok());
        assert!(
            with_limits(
                "DELETE FROM X.L.S.T",
                Db2AstLimits {
                    max_name_parts: 4,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn raw_name_ceiling_and_cast_first_component_remain_explicit() {
        let limits = Db2AstLimits {
            max_identifier_bytes: 1024,
            ..Db2AstLimits::default()
        };
        let name = "\"\"".repeat(512);
        assert!(with_limits(&format!("DELETE FROM T WHERE \"{name}\"=1"), limits).is_ok());
        let name = "\"\"".repeat(513);
        let error = with_limits(&format!("DELETE FROM T WHERE \"{name}\"=1"), limits).unwrap_err();
        assert!(error.message.contains("raw-name ceiling"));
        // Target does not use the shared raw-name constructor.
        assert!(with_limits(&format!("DELETE FROM \"{name}\""), limits).is_ok());
        assert!(parse("DELETE FROM T WHERE CAST(1 AS \"d\")=1").is_err());
        assert!(parse("DELETE FROM T WHERE CAST(1 AS S.\"d\")=1").is_ok());
    }

    #[test]
    fn literal_and_lexer_limits_have_exact_boundaries() {
        let sql = "DELETE FROM T WHERE A='abc'";
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_literal_bytes: 3,
                    ..Db2AstLimits::default()
                }
            )
            .is_ok()
        );
        assert!(
            with_limits(
                sql,
                Db2AstLimits {
                    max_literal_bytes: 2,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        let sql = "DELETE FROM T WHERE A=1";
        let defaults = Db2SyntaxLimits::default();
        for limits in [
            Db2SyntaxLimits {
                max_statement_bytes: sql.len(),
                ..defaults
            },
            Db2SyntaxLimits {
                max_tokens: 7,
                ..defaults
            },
            Db2SyntaxLimits {
                max_token_bytes: 6,
                ..defaults
            },
        ] {
            assert!(parse_db2_searched_delete(sql, limits, Db2AstLimits::default()).is_ok());
        }
        for limits in [
            Db2SyntaxLimits {
                max_statement_bytes: sql.len() - 1,
                ..defaults
            },
            Db2SyntaxLimits {
                max_tokens: 6,
                ..defaults
            },
            Db2SyntaxLimits {
                max_token_bytes: 5,
                ..defaults
            },
        ] {
            assert!(parse_db2_searched_delete(sql, limits, Db2AstLimits::default()).is_err());
        }
        assert!(
            parse_db2_searched_delete(
                "DELETE FROM T WHERE (A=1)",
                Db2SyntaxLimits {
                    max_nesting: 1,
                    ..defaults
                },
                Db2AstLimits::default()
            )
            .is_ok()
        );
        assert!(
            parse_db2_searched_delete(
                "DELETE FROM T WHERE ((A=1))",
                Db2SyntaxLimits {
                    max_nesting: 1,
                    ..defaults
                },
                Db2AstLimits::default()
            )
            .is_err()
        );
        assert!(
            with_limits(
                "DELETE FROM T",
                Db2AstLimits {
                    max_expression_nodes: 0,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        assert!(
            parse_db2_searched_delete(
                sql,
                Db2SyntaxLimits {
                    max_tokens: 0,
                    ..defaults
                },
                Db2AstLimits::default()
            )
            .is_err()
        );
    }
}
