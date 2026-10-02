//! Owned syntax for the common INSERT VALUES slice.
//!
//! Source: Db2 13 SQL row 0096, `db2z_sql_insert.html`, baseline
//! `ibm-db2-for-zos-13-2026-08-13`. This partial syntax has no execution route
//! or official conformance credit. Target binding, types and nullability are
//! later authorities.

use crate::{
    Db2AstLimits, Db2DataType, Db2ExpressionArena, Db2ExpressionId, Db2ExpressionKind,
    Db2Identifier, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenKind, lex_db2,
    parse_db2_expression,
};
use std::collections::BTreeSet;

/// One bounded INSERT with an optional column list and parenthesized VALUES rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2InsertValuesStatement {
    target: Db2QualifiedName,
    columns: Option<Vec<Db2Identifier>>,
    rows: Vec<Db2InsertValuesRow>,
    span: Db2SourceSpan,
}

impl Db2InsertValuesStatement {
    #[must_use]
    pub const fn target(&self) -> &Db2QualifiedName {
        &self.target
    }

    #[must_use]
    pub fn columns(&self) -> Option<&[Db2Identifier]> {
        self.columns.as_deref()
    }

    #[must_use]
    pub fn rows(&self) -> &[Db2InsertValuesRow] {
        &self.rows
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2InsertValuesRow {
    values: Vec<Db2InsertValue>,
    span: Db2SourceSpan,
}

impl Db2InsertValuesRow {
    #[must_use]
    pub fn values(&self) -> &[Db2InsertValue] {
        &self.values
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2InsertValue {
    Expression(Db2InsertExpression),
    Default(Db2SourceSpan),
    Null(Db2SourceSpan),
}

impl Db2InsertValue {
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        match self {
            Self::Expression(expression) => expression.span(),
            Self::Default(span) | Self::Null(span) => *span,
        }
    }
}

/// An expression whose arena spans are relative to the complete INSERT source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2InsertExpression {
    arena: Db2ExpressionArena,
    root: Db2ExpressionId,
    span: Db2SourceSpan,
}

impl Db2InsertExpression {
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

/// Parse exactly one declared INSERT VALUES statement, with one optional terminal
/// semicolon. `max_list_items` bounds the aggregate column, row, value, function
/// argument, CASE branch and built-in type argument entries. The expression-node
/// budget includes standalone DEFAULT and NULL values.
pub fn parse_db2_insert_values(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2InsertValuesStatement, Db2SyntaxDiagnostic> {
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    let lexed = lex_db2(source, syntax_limits)?;
    let tokens = lexed.tokens();
    let mut parser = InsertParser {
        source,
        tokens,
        position: 0,
        syntax_limits,
        ast_limits,
        list_items: 0,
        expression_nodes: 0,
    };
    if parser.word() != Some("INSERT") {
        return Err(parser.problem(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "expected the declared INSERT VALUES family",
        ));
    }
    parser.expect_word("INSERT")?;
    parser.expect_word("INTO")?;
    let mut parts = vec![parser.identifier()?];
    while parser.take_symbol(Db2Symbol::Period) {
        if parts.len() >= ast_limits.max_name_parts {
            return Err(parser.invalid("INSERT target exceeds the configured name-part limit"));
        }
        parts.push(parser.identifier()?);
    }
    let target = Db2QualifiedName::new(parts, ast_limits)
        .map_err(|problem| parser.invalid(&problem.message))?;
    let columns = if parser.take_symbol(Db2Symbol::LeftParenthesis) {
        let mut columns = Vec::new();
        let mut names = BTreeSet::new();
        loop {
            parser.add_list_items(1)?;
            let column_start = parser.position;
            let column = parser.identifier()?;
            if !names.insert(column.value().to_owned()) {
                return Err(parser.problem_at(
                    column_start,
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "INSERT column list contains a duplicate effective SQL identifier",
                ));
            }
            columns.push(column);
            if parser.take_symbol(Db2Symbol::RightParenthesis) {
                break;
            }
            parser.expect_symbol(Db2Symbol::Comma)?;
        }
        Some(columns)
    } else {
        None
    };
    if parser.word() != Some("VALUES") {
        return Err(parser.problem(
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "INSERT requires VALUES; fullselect, CTE, INCLUDE and OVERRIDING are unsupported",
        ));
    }
    parser.expect_word("VALUES")?;
    let mut rows = Vec::new();
    let mut width = columns.as_ref().map(Vec::len);
    loop {
        parser.add_list_items(1)?;
        let start = parser.position;
        if !parser.take_symbol(Db2Symbol::LeftParenthesis) {
            return Err(parser.problem(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "INSERT VALUES requires parenthesized rows; scalar rows and host arrays are unsupported",
            ));
        }
        let mut values = Vec::new();
        loop {
            parser.add_list_items(1)?;
            values.push(parser.value()?);
            if parser.take_symbol(Db2Symbol::RightParenthesis) {
                break;
            }
            parser.expect_symbol(Db2Symbol::Comma)?;
        }
        if width.is_some_and(|width| width != values.len()) {
            return Err(parser.problem_at(
                start,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "INSERT rows must have uniform width matching the explicit column list",
            ));
        }
        width = Some(values.len());
        rows.push(Db2InsertValuesRow {
            values,
            span: parser.range_span(start, parser.position),
        });
        if !parser.take_symbol(Db2Symbol::Comma) {
            break;
        }
    }
    parser.take_symbol(Db2Symbol::Semicolon);
    if parser.position != tokens.len() {
        return Err(parser.problem(
            Db2SyntaxDiagnosticCode::UnexpectedToken,
            "unsupported INSERT suffix or extra statement; isolation, QUERYNO and FOR n ROWS are unsupported",
        ));
    }
    Ok(Db2InsertValuesStatement {
        target,
        columns,
        rows,
        span: parser.range_span(0, tokens.len()),
    })
}

struct InsertParser<'a> {
    source: &'a str,
    tokens: &'a [Db2Token],
    position: usize,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
    list_items: usize,
    expression_nodes: usize,
}

impl InsertParser<'_> {
    fn value(&mut self) -> Result<Db2InsertValue, Db2SyntaxDiagnostic> {
        let start = self.position;
        let mut depth = 0_usize;
        while let Some(token) = self.tokens.get(self.position) {
            match token.kind {
                Db2TokenKind::Symbol(Db2Symbol::LeftParenthesis) => depth += 1,
                Db2TokenKind::Symbol(Db2Symbol::RightParenthesis) if depth > 0 => depth -= 1,
                Db2TokenKind::Symbol(Db2Symbol::Comma | Db2Symbol::RightParenthesis)
                    if depth == 0 =>
                {
                    break;
                }
                Db2TokenKind::Symbol(Db2Symbol::Semicolon) => {
                    return Err(self.problem(
                        Db2SyntaxDiagnosticCode::UnexpectedToken,
                        "semicolon is only permitted after the complete INSERT statement",
                    ));
                }
                _ => {}
            }
            self.position += 1;
        }
        if start == self.position {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::MissingToken,
                "missing INSERT value",
            ));
        }
        let span = self.range_span(start, self.position);
        if self.position == start + 1 {
            match self.word_at(start) {
                Some("DEFAULT") => {
                    self.add_nodes(1)?;
                    return Ok(Db2InsertValue::Default(span));
                }
                Some("NULL") => {
                    self.add_nodes(1)?;
                    return Ok(Db2InsertValue::Null(span));
                }
                _ => {}
            }
        }
        let fragment = &self.source[span.start_byte..span.end_byte];
        let parsed = parse_db2_expression(fragment, self.syntax_limits, self.ast_limits).map_err(
            |mut problem| {
                problem.location = relocate_location(problem.location, span.start);
                problem
            },
        )?;
        self.add_nodes(parsed.arena().nodes().len())?;
        let mut arena = Db2ExpressionArena::new(self.ast_limits)
            .map_err(|problem| self.invalid(&problem.message))?;
        for node in parsed.arena().nodes() {
            let local = node.span();
            let relocated = Db2SourceSpan {
                start_byte: span.start_byte + local.start_byte,
                end_byte: span.start_byte + local.end_byte,
                start: relocate_location(local.start, span.start),
                end: relocate_location(local.end, span.start),
            };
            if matches!(node.kind(), Db2ExpressionKind::Column(_)) {
                return Err(Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    relocated.start,
                    "INSERT VALUES expressions must not contain column names (Db2 INSERT source line 151)",
                ));
            }
            let entries = match node.kind() {
                Db2ExpressionKind::Function { arguments, .. } => arguments.len(),
                Db2ExpressionKind::Case { branches, .. } => branches.len(),
                Db2ExpressionKind::Cast {
                    data_type: Db2DataType::BuiltIn(data_type),
                    ..
                } => data_type.arguments().len(),
                _ => 0,
            };
            self.add_list_items(entries)?;
            // Insertion order and references are unchanged, so expression IDs
            // remain valid in this relocated arena.
            arena
                .push(node.kind().clone(), relocated)
                .map_err(|problem| self.invalid(&problem.message))?;
        }
        Ok(Db2InsertValue::Expression(Db2InsertExpression {
            arena,
            root: parsed.root(),
            span,
        }))
    }

    fn add_list_items(&mut self, count: usize) -> Result<(), Db2SyntaxDiagnostic> {
        self.list_items = self
            .list_items
            .checked_add(count)
            .filter(|total| *total <= self.ast_limits.max_list_items)
            .ok_or_else(|| self.invalid("INSERT exceeds the aggregate list-entry limit"))?;
        Ok(())
    }

    fn add_nodes(&mut self, count: usize) -> Result<(), Db2SyntaxDiagnostic> {
        self.expression_nodes = self
            .expression_nodes
            .checked_add(count)
            .filter(|total| *total <= self.ast_limits.max_expression_nodes)
            .ok_or_else(|| self.invalid("INSERT exceeds the aggregate expression-node limit"))?;
        Ok(())
    }

    fn identifier(&mut self) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(Db2Token {
            kind: Db2TokenKind::Word { value, delimited },
            ..
        }) = self.tokens.get(self.position)
        else {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::MissingToken,
                "expected an SQL identifier",
            ));
        };
        // The owned lexer retains doubled delimiter escapes. Decode once at
        // this target/column boundary before the AST constructor trims trailing
        // spaces and enforces the effective identifier's UTF-8 byte budget.
        // Token/source spans still refer to the original SQL bytes.
        let effective = if *delimited {
            value.replace("\"\"", "\"")
        } else {
            value.clone()
        };
        let name = Db2Identifier::new(effective, *delimited, self.ast_limits)
            .map_err(|problem| self.invalid(&problem.message))?;
        self.position += 1;
        Ok(name)
    }

    fn word(&self) -> Option<&str> {
        self.word_at(self.position)
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

    fn expect_word(&mut self, word: &str) -> Result<(), Db2SyntaxDiagnostic> {
        if self.word() != Some(word) {
            return Err(self.problem(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("expected {word}"),
            ));
        }
        self.position += 1;
        Ok(())
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

    fn expect_symbol(&mut self, symbol: Db2Symbol) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_symbol(symbol) {
            Ok(())
        } else {
            Err(self.problem(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                &format!("expected {symbol:?}; unsupported INSERT form"),
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

    fn invalid(&self, message: &str) -> Db2SyntaxDiagnostic {
        self.problem(Db2SyntaxDiagnosticCode::InvalidStatementOperand, message)
    }

    fn problem(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        self.problem_at(self.position, code, message)
    }

    fn problem_at(
        &self,
        index: usize,
        code: Db2SyntaxDiagnosticCode,
        message: &str,
    ) -> Db2SyntaxDiagnostic {
        let location = self.tokens.get(index).map_or_else(
            || {
                self.tokens
                    .last()
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        );
        Db2SyntaxDiagnostic::new(code, location, message)
    }
}

fn relocate_location(local: Db2SourceLocation, start: Db2SourceLocation) -> Db2SourceLocation {
    Db2SourceLocation {
        line: start.line + local.line - 1,
        column: if local.line == 1 {
            start.column + local.column - 1
        } else {
            local.column
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2InsertValuesStatement, Db2SyntaxDiagnostic> {
        parse_db2_insert_values(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    fn expression(value: &Db2InsertValue) -> &Db2InsertExpression {
        let Db2InsertValue::Expression(expression) = value else {
            panic!("expected expression");
        };
        expression
    }

    #[test]
    fn preserves_target_columns_rows_and_owned_values() {
        let source = "INSERT INTO loc.schema.\"Mixed\" (a, \"b\", c) VALUES (1 + 2, DEFAULT, NULL), (:Host-Value :ind, CAST(NULL AS INTEGER), CASE WHEN 1=1 THEN 'x' ELSE NULL END);";
        let statement = parse(source).unwrap();
        assert_eq!(
            statement
                .target()
                .parts()
                .iter()
                .map(Db2Identifier::value)
                .collect::<Vec<_>>(),
            ["LOC", "SCHEMA", "Mixed"]
        );
        assert_eq!(
            statement
                .columns()
                .unwrap()
                .iter()
                .map(Db2Identifier::value)
                .collect::<Vec<_>>(),
            ["A", "b", "C"]
        );
        assert_eq!(statement.rows().len(), 2);
        assert!(statement.rows().iter().all(|row| row.values().len() == 3));
        assert!(matches!(
            statement.rows()[0].values()[1],
            Db2InsertValue::Default(_)
        ));
        assert!(matches!(
            statement.rows()[0].values()[2],
            Db2InsertValue::Null(_)
        ));
        let host = expression(&statement.rows()[1].values()[0]);
        let Db2ExpressionKind::HostVariable(reference) =
            host.arena().get(host.root()).unwrap().kind()
        else {
            panic!("expected host reference");
        };
        assert_eq!(reference.variable().value(), "Host-Value");
        assert_eq!(reference.indicator().unwrap().value(), "ind");
        for row in statement.rows() {
            assert!(source[row.span().start_byte..row.span().end_byte].starts_with('('));
            assert!(source[row.span().start_byte..row.span().end_byte].ends_with(')'));
        }
        assert_eq!(statement.span().end_byte, source.len());
    }

    #[test]
    fn accepts_common_expressions_and_implicit_columns_without_binding() {
        for operand in [
            "?",
            ":v INDICATOR :i",
            "-1.25",
            "F(1, G('x'))",
            "CAST(? AS DECIMAL(9,2))",
            "CASE 1 WHEN 1 THEN 2 ELSE 3 END",
            "CASE WHEN :v IS NULL THEN NULL ELSE 1 END",
            "('a' || 'b')",
            "X'CAFE'",
            "NULL",
            "DEFAULT",
        ] {
            let statement = parse(&format!("INSERT INTO t VALUES ({operand})")).unwrap();
            assert!(statement.columns().is_none(), "{operand}");
            assert_eq!(statement.rows().len(), 1);
        }
    }

    #[test]
    fn duplicate_columns_use_effective_identifier_value() {
        for columns in ["a,A", "A,\"A\"", "\"A  \",a", "\"a\",\"a \""] {
            let problem = parse(&format!("INSERT INTO t ({columns}) VALUES (1,2)")).unwrap_err();
            assert_eq!(
                problem.code,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "{columns}"
            );
        }
        let statement = parse("INSERT INTO t (A,\"a\") VALUES (1,2)").unwrap();
        assert_eq!(statement.columns().unwrap().len(), 2);
    }

    #[test]
    fn escaped_target_and_column_identifiers_decode_once_with_raw_spans() {
        let source = "/* lead */\nINSERT INTO \"sch\"\"ema\".\"ta\"\"\"\"ble\" (\"co\"\"l\", \"Co\"\"l  \") VALUES (1,2);";
        let statement = parse(source).unwrap();
        let target = statement.target().parts();
        assert_eq!(target[0].value(), "sch\"ema");
        assert_eq!(target[1].value(), "ta\"\"ble");
        assert!(target.iter().all(Db2Identifier::is_delimited));
        let columns = statement.columns().unwrap();
        assert_eq!(columns[0].value(), "co\"l");
        assert_eq!(columns[1].value(), "Co\"l");
        assert!(columns.iter().all(Db2Identifier::is_delimited));
        let span = statement.span();
        assert_eq!(span.start_byte, source.find("INSERT").unwrap());
        assert_eq!(span.start, Db2SourceLocation { line: 2, column: 1 });
        assert_eq!(span.end_byte, source.len());
        assert_eq!(span.end, location(source, source.len()));
        let row = &statement.rows()[0];
        assert_eq!(&source[row.span().start_byte..row.span().end_byte], "(1,2)");
        for value in row.values() {
            let expression = expression(value);
            let span = expression.arena().get(expression.root()).unwrap().span();
            assert_eq!(span.start, location(source, span.start_byte));
            assert_eq!(span.end, location(source, span.end_byte));
            assert!(matches!(&source[span.start_byte..span.end_byte], "1" | "2"));
        }
    }

    #[test]
    fn escaped_identifier_limits_count_decoded_utf8_bytes() {
        // é (two UTF-8 bytes), one decoded quote and Q require four bytes.
        // The raw token payload has five bytes plus insignificant spaces.
        for source in [
            "INSERT INTO \"é\"\"Q  \" VALUES (1)",
            "INSERT INTO t (\"é\"\"Q  \") VALUES (1)",
        ] {
            let statement = with_ast(
                source,
                Db2AstLimits {
                    max_identifier_bytes: 4,
                    ..Db2AstLimits::default()
                },
            )
            .unwrap();
            let name = statement
                .columns()
                .map_or_else(|| &statement.target().parts()[0], |columns| &columns[0]);
            assert_eq!(name.value(), "é\"Q");
            assert_eq!(name.value().len(), 4);
            let problem = with_ast(
                source,
                Db2AstLimits {
                    max_identifier_bytes: 3,
                    ..Db2AstLimits::default()
                },
            )
            .unwrap_err();
            assert_eq!(
                problem.code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
            assert!(problem.message.contains("byte limit"));
            assert_eq!(
                problem.location,
                location(source, source.find('"').unwrap())
            );
        }
        let statement = with_ast(
            "INSERT INTO \"\"\"\"\"\"\"\"\"\" VALUES (1)",
            Db2AstLimits {
                max_identifier_bytes: 4,
                ..Db2AstLimits::default()
            },
        )
        .unwrap();
        assert_eq!(statement.target().parts()[0].value(), "\"\"\"\"");
    }

    #[test]
    fn escaped_column_duplicates_and_case_use_decoded_identity() {
        for columns in ["\"A\"\"B\",\"A\"\"B  \"", "\"A\"\"\"\"B\",\"A\"\"\"\"B \""] {
            let problem = parse(&format!("INSERT INTO t ({columns}) VALUES (1,2)")).unwrap_err();
            assert_eq!(problem.code, Db2SyntaxDiagnosticCode::DuplicateClause);
        }
        let statement =
            parse("INSERT INTO t (\"A\"\"B\",\"A\"\"\"\"B\",\"a\"\"b\",ordinary) VALUES (1,2,3,4)")
                .unwrap();
        let columns = statement.columns().unwrap();
        assert_eq!(
            columns.iter().map(Db2Identifier::value).collect::<Vec<_>>(),
            ["A\"B", "A\"\"B", "a\"b", "ORDINARY"]
        );
        assert!(!columns[3].is_delimited());
    }

    #[test]
    fn rejects_columns_at_every_expression_depth() {
        for operand in [
            "c",
            "t.c",
            "c+1",
            "F(c)",
            "F(1,G(c))",
            "CAST(c AS INTEGER)",
            "CASE c WHEN 1 THEN 2 END",
            "CASE 1 WHEN c THEN 2 END",
            "CASE WHEN c=1 THEN 2 END",
            "CASE WHEN 1=1 THEN c ELSE 2 END",
            "CASE WHEN 1=1 THEN 2 ELSE c END",
            "F(CAST(CASE WHEN 1=1 THEN c END AS INTEGER))",
            "c IS NULL",
            "\"c\"",
        ] {
            let source = format!("INSERT INTO t VALUES ({operand})");
            let problem = parse(&source).unwrap_err();
            assert_eq!(
                problem.code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{operand}"
            );
            assert!(
                problem.message.contains("column names"),
                "{operand}: {problem}"
            );
        }
    }

    #[test]
    fn rejects_unequal_and_explicit_column_widths() {
        for source in [
            "INSERT INTO t VALUES (1),(2,3)",
            "INSERT INTO t VALUES (1,2),(3)",
            "INSERT INTO t (a,b) VALUES (1)",
            "INSERT INTO t (a) VALUES (1,2)",
            "INSERT INTO t (a,b) VALUES (1,2),(3)",
        ] {
            let problem = parse(source).unwrap_err();
            assert_eq!(
                problem.code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "{source}"
            );
            assert!(problem.message.contains("width"));
        }
    }

    #[test]
    fn rejects_all_undeclared_statement_forms_and_malformed_lists() {
        for source in [
            "SELECT 1",
            "WITH x AS (SELECT 1) INSERT INTO t VALUES (1)",
            "INSERT t VALUES (1)",
            "INSERT INTO t SELECT 1",
            "INSERT INTO t WITH x AS (SELECT 1) SELECT 1",
            "INSERT INTO t INCLUDE (a INTEGER) VALUES (1)",
            "INSERT INTO t OVERRIDING USER VALUE VALUES (1)",
            "INSERT INTO t (t.a) VALUES (1)",
            "INSERT INTO t VALUES 1",
            "INSERT INTO t VALUES 1,2",
            "INSERT INTO t VALUES (1),2",
            "INSERT INTO t VALUES (:a[1])",
            "INSERT INTO t VALUES (:a(1))",
            "INSERT INTO t VALUES (1) FOR 2 ROWS",
            "INSERT INTO t VALUES (1) FOR :n ROWS ATOMIC",
            "INSERT INTO t VALUES (1) WITH RR",
            "INSERT INTO t VALUES (1) WITH RS",
            "INSERT INTO t VALUES (1) WITH CS",
            "INSERT INTO t VALUES (1) QUERYNO 7",
            "INSERT INTO t VALUES (1) NOT ATOMIC CONTINUE ON SQLEXCEPTION",
            "INSERT INTO t VALUES (SELECT 1)",
            "INSERT INTO t VALUES (DEFAULT+1)",
            "INSERT INTO t VALUES (NULL+1)",
            "INSERT INTO t VALUES ()",
            "INSERT INTO t () VALUES (1)",
            "INSERT INTO t (a,) VALUES (1)",
            "INSERT INTO t VALUES (1,)",
            "INSERT INTO t VALUES (,1)",
            "INSERT INTO t VALUES (1),",
            "INSERT INTO t VALUES",
            "INSERT INTO t VALUES (1",
            "INSERT INTO t VALUES (1))",
            "INSERT INTO t VALUES ((1;))",
            "INSERT INTO t VALUES (1);;",
            "INSERT INTO t VALUES (1); SELECT 2",
            "INSERT INTO t VALUES (1) VALUES (2)",
            "INSERT INTO t alias VALUES (1)",
        ] {
            let problem = parse(source).expect_err(source);
            assert!(problem.message.len() <= 256);
            assert!(problem.location.line >= 1 && problem.location.column >= 1);
        }
    }

    #[test]
    fn whitespace_and_comment_variants_preserve_structure() {
        let plain = parse("INSERT INTO s.t(a,b) VALUES (F(1,2),NULL),(DEFAULT,?)").unwrap();
        let variants = [
            "insert\tinto\ns /* x */ . t ( a , b ) values ( F ( 1 , 2 ) , null ) , ( default , ? ) ; -- done",
            "/* lead */ INSERT-- comment\nINTO s./* nested /* x */ comment */t(a,\nb)VALUES(F(1,2),NULL),\n(DEFAULT,?)",
        ];
        for source in variants {
            let statement = parse(source).unwrap();
            assert_eq!(statement.target(), plain.target());
            assert_eq!(statement.columns(), plain.columns());
            for (actual, expected) in statement.rows().iter().zip(plain.rows()) {
                assert_eq!(actual.values().len(), expected.values().len());
                for (actual, expected) in actual.values().iter().zip(expected.values()) {
                    match (actual, expected) {
                        (Db2InsertValue::Expression(a), Db2InsertValue::Expression(b)) => {
                            assert_eq!(a.root(), b.root());
                            assert_eq!(
                                a.arena()
                                    .nodes()
                                    .iter()
                                    .map(|node| node.kind())
                                    .collect::<Vec<_>>(),
                                b.arena()
                                    .nodes()
                                    .iter()
                                    .map(|node| node.kind())
                                    .collect::<Vec<_>>()
                            );
                        }
                        (Db2InsertValue::Null(_), Db2InsertValue::Null(_))
                        | (Db2InsertValue::Default(_), Db2InsertValue::Default(_)) => {}
                        _ => panic!("value structure changed"),
                    }
                }
            }
        }
    }

    fn location(source: &str, byte: usize) -> Db2SourceLocation {
        let before = &source[..byte];
        Db2SourceLocation {
            line: before.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1,
            column: before.rsplit('\n').next().unwrap().chars().count() as u32 + 1,
        }
    }

    #[test]
    fn relocates_every_arena_span_and_diagnostic_including_multiline_unicode() {
        let source = "-- lead\nINSERT INTO \"é\" VALUES (F('é',\n /* x */ CAST(2 AS INTEGER))),\n(CASE WHEN 1=1 THEN 3 ELSE NULL END);";
        let statement = parse(source).unwrap();
        for row in statement.rows() {
            for value in row.values() {
                let expression = expression(value);
                for node in expression.arena().nodes() {
                    let span = node.span();
                    assert_eq!(span.start, location(source, span.start_byte));
                    assert_eq!(span.end, location(source, span.end_byte));
                    assert!(span.start_byte >= value.span().start_byte);
                    assert!(span.end_byte <= value.span().end_byte);
                    assert!(!source[span.start_byte..span.end_byte].is_empty());
                }
            }
        }
        for source in [
            "-- lead\nINSERT INTO t VALUES (F('é',bad))",
            "INSERT INTO t VALUES (F('é',\n CAST(bad AS INTEGER)))",
            "INSERT INTO t VALUES (F(1,\n 2 + ))",
        ] {
            let problem = parse(source).unwrap_err();
            let byte = source
                .find("bad")
                .unwrap_or_else(|| source.find(" + ").unwrap() + 3);
            assert_eq!(
                problem.location,
                location(source, byte),
                "{source}: {problem}"
            );
        }
    }

    fn with_ast(
        source: &str,
        limits: Db2AstLimits,
    ) -> Result<Db2InsertValuesStatement, Db2SyntaxDiagnostic> {
        parse_db2_insert_values(source, Db2SyntaxLimits::default(), limits)
    }

    #[test]
    fn aggregate_nodes_and_lists_have_exact_boundaries_across_rows() {
        let source = "INSERT INTO t VALUES (1+2),(3+4)";
        for maximum in [5, 6] {
            assert_eq!(
                with_ast(
                    source,
                    Db2AstLimits {
                        max_expression_nodes: maximum,
                        ..Db2AstLimits::default()
                    }
                )
                .is_ok(),
                maximum == 6
            );
        }
        assert!(
            with_ast(
                "INSERT INTO t VALUES (DEFAULT),(NULL)",
                Db2AstLimits {
                    max_expression_nodes: 1,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
        // Two columns + two rows + four values + two arguments + one CASE
        // branch + two CAST type arguments = thirteen aggregate list entries.
        let source = "INSERT INTO t(a,b) VALUES (F(1,2),CASE 1 WHEN 1 THEN 2 END),(CAST(3 AS DECIMAL(9,2)),DEFAULT)";
        for maximum in [12, 13] {
            assert_eq!(
                with_ast(
                    source,
                    Db2AstLimits {
                        max_list_items: maximum,
                        ..Db2AstLimits::default()
                    }
                )
                .is_ok(),
                maximum == 13
            );
        }
        assert!(
            with_ast(
                "INSERT INTO t VALUES (1),(2),(3)",
                Db2AstLimits {
                    max_list_items: 5,
                    ..Db2AstLimits::default()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn enforces_configured_lexical_name_literal_depth_and_limit_validation() {
        let source = "INSERT INTO t VALUES (1)";
        for maximum in [source.len() - 1, source.len()] {
            assert_eq!(
                parse_db2_insert_values(
                    source,
                    Db2SyntaxLimits {
                        max_statement_bytes: maximum,
                        ..Db2SyntaxLimits::default()
                    },
                    Db2AstLimits::default()
                )
                .is_ok(),
                maximum == source.len()
            );
        }
        for limits in [
            Db2AstLimits {
                max_identifier_bytes: 1,
                ..Db2AstLimits::default()
            },
            Db2AstLimits {
                max_name_parts: 1,
                ..Db2AstLimits::default()
            },
            Db2AstLimits {
                max_literal_bytes: 1,
                ..Db2AstLimits::default()
            },
            Db2AstLimits {
                max_expression_depth: 1,
                ..Db2AstLimits::default()
            },
        ] {
            assert!(with_ast("INSERT INTO schema.tab VALUES ('xx'+1)", limits).is_err());
        }
        assert_eq!(
            with_ast(
                source,
                Db2AstLimits {
                    max_list_items: 0,
                    ..Db2AstLimits::default()
                }
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
        for limits in [
            Db2SyntaxLimits {
                max_tokens: 3,
                ..Db2SyntaxLimits::default()
            },
            Db2SyntaxLimits {
                max_nesting: 1,
                ..Db2SyntaxLimits::default()
            },
            Db2SyntaxLimits {
                max_token_bytes: 3,
                ..Db2SyntaxLimits::default()
            },
        ] {
            assert!(
                parse_db2_insert_values(
                    "INSERT INTO t VALUES ((1234))",
                    limits,
                    Db2AstLimits::default()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn preserves_source_pending_numeric_and_boolean_fences() {
        for operand in ["1e2", "1.0e-3", "TRUE", "FALSE", "F(TRUE)"] {
            let problem = parse(&format!("INSERT INTO t VALUES ({operand})")).unwrap_err();
            assert!(problem.message.contains("#350"), "{operand}: {problem}");
        }
    }
}
