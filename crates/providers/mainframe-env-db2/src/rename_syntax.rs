//! Owned bounded RENAME TABLE / INDEX syntax (partial SQL 0105).
//!
//! Source: Db2 13 baseline `ibm-db2-for-zos-13-2026-08-13`,
//! `db2z_sql_rename.html`, 25184 bytes, SHA-256
//! `ea6063fba847a91a891db54d4b1741f6dc379a7dbf61c8f15069867365f63984`.
//! Qualification uses `db2z_namingconventions.html`; identifier escape and
//! insignificant trailing-space rules use `db2z_sqlidentifiers.html`.
//! Current-server applicability, aliases, implicit qualifiers, object existence,
//! destination conflicts, privileges, dependencies and mutation require binding
//! and execution. No qualifier or catalog conflict is inferred by this parser.

use crate::{
    Db2AstLimits, Db2Identifier, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenCursor,
    Db2TokenKind, lex_db2,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2RenameObjectKind {
    Table,
    Index,
}

/// Explicit source qualification and an unqualified destination, with original
/// complete-source byte and line/column spans. No source text is retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2RenameStatement {
    object_kind: Db2RenameObjectKind,
    source_name: Db2QualifiedName,
    source_span: Db2SourceSpan,
    source_part_spans: Vec<Db2SourceSpan>,
    destination_identifier: Db2Identifier,
    destination_span: Db2SourceSpan,
    span: Db2SourceSpan,
}

impl Db2RenameStatement {
    #[must_use]
    pub const fn object_kind(&self) -> Db2RenameObjectKind {
        self.object_kind
    }

    #[must_use]
    pub const fn source_name(&self) -> &Db2QualifiedName {
        &self.source_name
    }

    #[must_use]
    pub const fn source_span(&self) -> Db2SourceSpan {
        self.source_span
    }

    #[must_use]
    pub fn source_part_spans(&self) -> &[Db2SourceSpan] {
        &self.source_part_spans
    }

    #[must_use]
    pub const fn destination_identifier(&self) -> &Db2Identifier {
        &self.destination_identifier
    }

    #[must_use]
    pub const fn destination_span(&self) -> Db2SourceSpan {
        self.destination_span
    }

    /// Includes the optional terminal semicolon, excluding surrounding trivia.
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse exactly one RENAME TABLE source TO identifier or RENAME INDEX source
/// TO identifier, with at most one terminal semicolon. TABLE sources admit up
/// to three explicit components; INDEX sources admit up to two. The configured
/// name-part limit may narrow those bounds. Effective decoded identifier values
/// use the existing AST byte limit; raw spelling uses the lexer token limit.
/// No expressions, expression arena or operand lists are constructed.
pub fn parse_db2_rename_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2RenameStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    RenameParser {
        cursor: lexed.cursor(),
        previous: None,
        limits: ast_limits,
    }
    .parse()
}

struct RenameParser<'a> {
    cursor: Db2TokenCursor<'a>,
    previous: Option<&'a Db2Token>,
    limits: Db2AstLimits,
}

impl RenameParser<'_> {
    fn parse(&mut self) -> Result<Db2RenameStatement, Db2SyntaxDiagnostic> {
        let first = self.cursor.peek().expect("lexer rejects empty SQL").span;
        if !self.take_word("RENAME") {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "statement is outside the common RENAME family",
            ));
        }
        let (object_kind, max_parts) = if self.take_word("TABLE") {
            (Db2RenameObjectKind::Table, 3)
        } else if self.take_word("INDEX") {
            (Db2RenameObjectKind::Index, 2)
        } else {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "common RENAME supports only TABLE and INDEX",
            ));
        };
        let source_start = self.cursor.peek().map(|token| token.span);
        let mut parts = Vec::new();
        let mut source_part_spans = Vec::new();
        loop {
            if parts.len() >= self.limits.max_name_parts {
                return Err(self.here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "RENAME source exceeds the configured name-part limit",
                ));
            }
            if parts.len() >= max_parts {
                return Err(self.here(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "RENAME TABLE allows at most three source parts; INDEX allows at most two",
                ));
            }
            let (identifier, span) = self.identifier()?;
            parts.push(identifier);
            source_part_spans.push(span);
            if !self.take_symbol(Db2Symbol::Period) {
                break;
            }
        }
        let source_span = joined(
            source_start.expect("source identifier consumed a token"),
            self.previous
                .expect("source identifier consumed a token")
                .span,
        );
        let source_name = Db2QualifiedName::new(parts, self.limits).map_err(|problem| {
            self.here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })?;
        if !self.take_word("TO") {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "expected TO after the RENAME source name",
            ));
        }
        let (destination_identifier, destination_span) = self.identifier()?;
        if self.peek_symbol(Db2Symbol::Period) {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                "RENAME destination must be an unqualified identifier",
            ));
        }
        if self.take_symbol(Db2Symbol::Semicolon) {
            if self.cursor.peek().is_some() {
                return Err(self.here(
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    "only one Db2 statement and one terminal semicolon are allowed",
                ));
            }
        } else if self.cursor.peek().is_some() {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "trailing RENAME operands or extension clauses are unsupported",
            ));
        }
        Ok(Db2RenameStatement {
            object_kind,
            source_name,
            source_span,
            source_part_spans,
            destination_identifier,
            destination_span,
            span: joined(first, self.previous.expect("RENAME consumed tokens").span),
        })
    }

    fn identifier(&mut self) -> Result<(Db2Identifier, Db2SourceSpan), Db2SyntaxDiagnostic> {
        let Some(token) = self.cursor.peek() else {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::MissingToken,
                "RENAME requires an SQL identifier",
            ));
        };
        let Db2TokenKind::Word { value, delimited } = &token.kind else {
            return Err(self.here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "RENAME requires an SQL identifier",
            ));
        };
        // Lexer words retain doubled escape spelling. Decode exactly once,
        // before the AST trims insignificant spaces and bounds effective bytes.
        let value = if *delimited {
            value.replace("\"\"", "\"")
        } else {
            value.clone()
        };
        let identifier = Db2Identifier::new(value, *delimited, self.limits).map_err(|problem| {
            self.here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })?;
        let span = token.span;
        self.advance();
        Ok((identifier, span))
    }

    fn take_word(&mut self, expected: &str) -> bool {
        if matches!(self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::Word { value, delimited: false }) if value == expected)
        {
            self.advance();
            true
        } else {
            false
        }
    }

    fn peek_symbol(&self, expected: Db2Symbol) -> bool {
        matches!(self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::Symbol(symbol)) if *symbol == expected)
    }

    fn take_symbol(&mut self, expected: Db2Symbol) -> bool {
        if self.peek_symbol(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        self.previous = self.cursor.next();
    }

    fn here(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
        let location = self.cursor.peek().map_or_else(
            || {
                self.previous
                    .map_or(Db2SourceLocation::START, |token| token.span.end)
            },
            |token| token.span.start,
        );
        Db2SyntaxDiagnostic::new(code, location, message)
    }
}

fn joined(first: Db2SourceSpan, last: Db2SourceSpan) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: first.start_byte,
        end_byte: last.end_byte,
        start: first.start,
        end: last.end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2RenameStatement, Db2SyntaxDiagnostic> {
        parse_db2_rename_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    fn with_ast(
        source: &str,
        limits: Db2AstLimits,
    ) -> Result<Db2RenameStatement, Db2SyntaxDiagnostic> {
        parse_db2_rename_statement(source, Db2SyntaxLimits::default(), limits)
    }

    fn with_syntax(
        source: &str,
        limits: Db2SyntaxLimits,
    ) -> Result<Db2RenameStatement, Db2SyntaxDiagnostic> {
        parse_db2_rename_statement(source, limits, Db2AstLimits::default())
    }

    // Independent expectation for original-source locations: UTF-8 byte offsets
    // differ from character columns and a CRLF advances exactly one line.
    fn location(source: &str, byte: usize) -> Db2SourceLocation {
        let mut line = 1;
        let mut column = 1;
        let mut was_cr = false;
        for ch in source[..byte].chars() {
            if ch == '\r' || (ch == '\n' && !was_cr) {
                line += 1;
                column = 1;
            } else if ch != '\n' || !was_cr {
                column += 1;
            }
            was_cr = ch == '\r';
        }
        Db2SourceLocation { line, column }
    }

    fn assert_span(source: &str, span: Db2SourceSpan, spelling: &str) {
        assert_eq!(&source[span.start_byte..span.end_byte], spelling);
        assert_eq!(span.start, location(source, span.start_byte));
        assert_eq!(span.end, location(source, span.end_byte));
    }

    #[test]
    fn both_forms_preserve_explicit_source_parts_and_unqualified_destination() {
        for (kind, keyword, sources) in [
            (
                Db2RenameObjectKind::Table,
                "TABLE",
                vec!["t", "s.t", "loc.s.t"],
            ),
            (Db2RenameObjectKind::Index, "INDEX", vec!["i", "s.i"]),
        ] {
            for source_name in sources {
                for terminator in ["", ";"] {
                    let sql = format!("rename {keyword} {source_name} to new_name{terminator}");
                    let result = parse(&sql).unwrap();
                    assert_eq!(result.object_kind(), kind);
                    assert_eq!(
                        result
                            .source_name()
                            .parts()
                            .iter()
                            .map(Db2Identifier::value)
                            .collect::<Vec<_>>(),
                        source_name
                            .to_ascii_uppercase()
                            .split('.')
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(result.destination_identifier().value(), "NEW_NAME");
                    assert_span(&sql, result.source_span(), source_name);
                    assert_span(&sql, result.destination_span(), "new_name");
                    assert_span(&sql, result.span(), &sql);
                }
            }
        }
    }

    #[test]
    fn quotes_decode_once_and_preserve_effective_case_and_spaces() {
        let result =
            parse("RENAME TABLE \"s\"\"\"\"x \".\" leading name  \" TO \"To\"\"Name  \"").unwrap();
        assert_eq!(result.source_name().parts()[0].value(), "s\"\"x");
        assert_eq!(result.source_name().parts()[1].value(), " leading name");
        assert_eq!(result.destination_identifier().value(), "To\"Name");
        assert!(result.destination_identifier().is_delimited());
        assert!(
            result
                .source_name()
                .parts()
                .iter()
                .all(Db2Identifier::is_delimited)
        );
        assert_eq!(
            parse("RENAME INDEX \"INDEX\" TO \"TO\"")
                .unwrap()
                .destination_identifier()
                .value(),
            "TO"
        );
    }

    #[test]
    fn lexical_same_names_do_not_establish_catalog_conflicts() {
        for keyword in ["TABLE", "INDEX"] {
            for source in ["same", "s.same", "\"SAME \"", "\"Same\""] {
                let result = parse(&format!("RENAME {keyword} {source} TO \"SAME\"")).unwrap();
                assert_eq!(result.destination_identifier().value(), "SAME");
                if source != "\"Same\"" {
                    assert_eq!(result.source_name().parts().last().unwrap().value(), "SAME");
                }
            }
        }
        // Alias and current-server applicability cannot be known from spelling.
        assert_eq!(
            parse("RENAME TABLE other_server.s.alias_candidate TO n")
                .unwrap()
                .source_name()
                .parts()
                .len(),
            3
        );
    }

    #[test]
    fn original_spans_survive_trivia_utf8_crlf_and_relocation() {
        for prefix in ["", "  ", "-- π\r\n", "/* 漢\n字 */\r\n\t"] {
            for keyword in ["TABLE", "INDEX"] {
                let source_name = "\"Sché\" /* between */ .\r\n \"名\"\"x \"";
                let sql = format!(
                    "{prefix}RENAME /* kind */ {keyword}\r\n{source_name}\r\nTO\t\"新\"\"名  \"; -- tail\r\n"
                );
                let result = parse(&sql).unwrap();
                assert_span(&sql, result.source_span(), source_name);
                assert_span(&sql, result.source_part_spans()[0], "\"Sché\"");
                assert_span(&sql, result.source_part_spans()[1], "\"名\"\"x \"");
                assert_span(&sql, result.destination_span(), "\"新\"\"名  \"");
                let statement = &sql[prefix.len()..sql.find("; -- tail").unwrap() + 1];
                assert_span(&sql, result.span(), statement);
                assert_eq!(result.source_name().parts()[1].value(), "名\"x");
                assert_eq!(result.destination_identifier().value(), "新\"名");
            }
        }
    }

    #[test]
    fn whitespace_and_nested_comments_do_not_change_structure() {
        for sql in [
            "RENAME\tTABLE\nS /* a /* b */ c */ . T -- source\r\n TO N /* trailing */",
            "rename\rindex\rs.i\rto\rn;\r",
            "RENAME/**/TABLE/**/T/**/TO/**/N;/**/",
        ] {
            assert_eq!(parse(sql).unwrap().destination_identifier().value(), "N");
        }
    }

    #[test]
    fn unsupported_kinds_extensions_and_extra_statements_have_locations() {
        for (sql, marker, code) in [
            (
                "DROP TABLE t",
                "DROP",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME VIEW t TO n",
                "VIEW",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME ALIAS t TO n",
                "ALIAS",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME COLUMN t TO n",
                "COLUMN",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME \"TABLE\" t TO n",
                "\"TABLE\"",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME TABLE t TO s.n",
                ".",
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            ),
            (
                "RENAME INDEX i TO s.n",
                ".",
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            ),
            (
                "RENAME INDEX l.s.i TO n",
                "i TO",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME TABLE l.s.t.extra TO n",
                "extra",
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            ),
            (
                "RENAME TABLE t TO n CASCADE",
                "CASCADE",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME INDEX i TO n RESTRICT",
                "RESTRICT",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME TABLE t TO n AS x",
                "AS",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME TABLE t TO n , x",
                ",",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME TABLE t TO n TO x",
                "TO x",
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
            ),
            (
                "RENAME TABLE t TO n; RENAME INDEX i TO j",
                "RENAME INDEX",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME INDEX i TO n;;",
                ";;",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
        ] {
            for prefix in ["", "/* π */\r\n"] {
                let relocated = format!("{prefix}{sql}");
                let error = parse(&relocated).unwrap_err();
                let mut byte = prefix.len() + sql.find(marker).unwrap();
                if marker == ";;" {
                    byte += 1;
                }
                assert_eq!(error.code, code, "{relocated}: {error}");
                assert_eq!(error.location, location(&relocated, byte), "{relocated}");
                assert!(error.message.len() <= 256);
            }
        }
    }

    #[test]
    fn malformed_operands_and_missing_clauses_fail_at_original_locations() {
        for (sql, marker, code) in [
            ("RENAME", "", Db2SyntaxDiagnosticCode::UnsupportedStatement),
            ("RENAME TABLE", "", Db2SyntaxDiagnosticCode::MissingToken),
            ("RENAME INDEX i", "", Db2SyntaxDiagnosticCode::MissingToken),
            (
                "RENAME TABLE t TO",
                "",
                Db2SyntaxDiagnosticCode::MissingToken,
            ),
            (
                "RENAME TABLE .t TO n",
                ".",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME TABLE s..t TO n",
                "..",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME TABLE s. TO n",
                "n",
                Db2SyntaxDiagnosticCode::MissingToken,
            ),
            (
                "RENAME TABLE t n",
                "n",
                Db2SyntaxDiagnosticCode::MissingToken,
            ),
            (
                "RENAME TABLE t \"TO\" n",
                "\"TO\"",
                Db2SyntaxDiagnosticCode::MissingToken,
            ),
            (
                "RENAME TABLE t TO :host",
                ":host",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME INDEX i TO ?",
                "?",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME TABLE t TO 'n'",
                "'n'",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME TABLE 123 TO n",
                "123",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME TABLE t TO ;",
                ";",
                Db2SyntaxDiagnosticCode::UnexpectedToken,
            ),
            (
                "RENAME TABLE \"  \" TO n",
                "\"  \"",
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            ),
        ] {
            for prefix in ["", "-- é\r\n\t"] {
                let relocated = format!("{prefix}{sql}");
                let error = parse(&relocated).unwrap_err();
                let byte = if marker.is_empty() {
                    relocated.len()
                } else {
                    prefix.len() + sql.find(marker).unwrap() + usize::from(marker == "..")
                };
                assert_eq!(error.code, code, "{relocated}: {error}");
                assert_eq!(error.location, location(&relocated, byte), "{relocated}");
            }
        }
        for sql in [
            "",
            "/* only */",
            "RENAME TABLE \"t TO n",
            "RENAME TABLE t TO n /*",
            "RENAME TABLE t TO n\0",
            "RENAME TABLE t TO (n)",
        ] {
            assert!(parse(sql).is_err(), "{sql:?}");
        }
    }

    #[test]
    fn effective_identifier_limits_include_decoded_quotes_and_utf8() {
        let limits = Db2AstLimits {
            max_identifier_bytes: 4,
            ..Db2AstLimits::default()
        };
        for keyword in ["TABLE", "INDEX"] {
            for name in ["abcd", "\"ab\"\"c   \"", "\"éé\""] {
                assert!(with_ast(&format!("RENAME {keyword} {name} TO {name}"), limits).is_ok());
            }
            for name in ["abcde", "\"ab\"\"cd\"", "\"ééa\""] {
                for sql in [
                    format!("RENAME {keyword} {name} TO n"),
                    format!("RENAME {keyword} s TO {name}"),
                ] {
                    let error = with_ast(&sql, limits).unwrap_err();
                    assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
                    assert_eq!(error.location, location(&sql, sql.find(name).unwrap()));
                }
            }
        }
        let limits = Db2AstLimits {
            max_identifier_bytes: 1024,
            ..Db2AstLimits::default()
        };
        for bytes in [1024, 1025] {
            let name = format!("\"{}\"", "\"\"".repeat(bytes));
            let sql = format!("RENAME TABLE {name} TO {name}");
            assert_eq!(with_ast(&sql, limits).is_ok(), bytes == 1024);
        }
    }

    #[test]
    fn configured_name_part_bounds_and_source_kind_fences_are_distinct() {
        for count in 1..=3 {
            let limits = Db2AstLimits {
                max_name_parts: count,
                ..Db2AstLimits::default()
            };
            let name = ["l", "s", "t"][..count].join(".");
            assert!(with_ast(&format!("RENAME TABLE {name} TO n"), limits).is_ok());
            let error = with_ast(&format!("RENAME TABLE {name}.extra TO n"), limits).unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
        }
        let limits = Db2AstLimits {
            max_name_parts: 16,
            ..Db2AstLimits::default()
        };
        assert!(with_ast("RENAME TABLE l.s.t TO n", limits).is_ok());
        for sql in ["RENAME TABLE l.s.t.extra TO n", "RENAME INDEX l.s.i TO n"] {
            assert_eq!(
                with_ast(sql, limits).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement
            );
        }
        // No expression/list budget is consumed or backend capability claimed.
        let limits = Db2AstLimits {
            max_list_items: 1,
            max_expression_nodes: 1,
            max_expression_depth: 1,
            ..Db2AstLimits::default()
        };
        assert!(with_ast("RENAME TABLE l.s.t TO n", limits).is_ok());
    }

    #[test]
    fn configured_statement_token_and_nesting_limits_are_exact() {
        let sql = "RENAME TABLE s.t TO n;";
        for bytes in [sql.len(), sql.len() - 1] {
            let limits = Db2SyntaxLimits {
                max_statement_bytes: bytes,
                ..Db2SyntaxLimits::default()
            };
            assert_eq!(with_syntax(sql, limits).is_ok(), bytes == sql.len());
        }
        for count in [8, 7] {
            let limits = Db2SyntaxLimits {
                max_tokens: count,
                ..Db2SyntaxLimits::default()
            };
            let result = with_syntax(sql, limits);
            if count == 8 {
                assert!(result.is_ok());
            } else {
                let error = result.unwrap_err();
                assert_eq!(error.code, Db2SyntaxDiagnosticCode::TooManyTokens);
                assert_eq!(error.location, location(sql, sql.len() - 1));
            }
        }
        // Raw quoted bytes include delimiters/escapes, unlike effective names.
        let sql = "RENAME TABLE \"a\"\"b\" TO n";
        for bytes in [6, 5] {
            let limits = Db2SyntaxLimits {
                max_token_bytes: bytes,
                ..Db2SyntaxLimits::default()
            };
            let result = with_syntax(sql, limits);
            if bytes == 6 {
                assert!(result.is_ok());
            } else {
                assert_eq!(
                    result.unwrap_err().code,
                    Db2SyntaxDiagnosticCode::TokenTooLarge
                );
            }
        }
        let limits = Db2SyntaxLimits {
            max_nesting: 2,
            ..Db2SyntaxLimits::default()
        };
        assert!(with_syntax("/* /* nested */ */ RENAME TABLE t TO n", limits).is_ok());
        assert_eq!(
            with_syntax("/* /* /* deep */ */ */ RENAME TABLE t TO n", limits)
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter
        );
    }

    #[test]
    fn compiled_lexer_input_token_and_nesting_boundaries_remain_enforced() {
        let limits = Db2SyntaxLimits {
            max_statement_bytes: 8 * 1024 * 1024,
            max_token_bytes: 1024 * 1024,
            max_tokens: 262_144,
            max_nesting: 1024,
        };
        let base = "RENAME TABLE t TO n";
        let exact = format!(
            "{base}{}",
            " ".repeat(limits.max_statement_bytes - base.len())
        );
        assert!(with_syntax(&exact, limits).is_ok());
        assert_eq!(
            with_syntax(&(exact + " "), limits).unwrap_err().code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );
        for bytes in [limits.max_token_bytes, limits.max_token_bytes + 1] {
            let name = format!("\"a{}\"", " ".repeat(bytes - 3));
            let result = with_syntax(&format!("RENAME TABLE {name} TO n"), limits);
            if bytes == limits.max_token_bytes {
                assert!(result.is_ok());
            } else {
                assert_eq!(
                    result.unwrap_err().code,
                    Db2SyntaxDiagnosticCode::TokenTooLarge
                );
            }
        }
        for count in [limits.max_tokens, limits.max_tokens + 1] {
            let sql = format!("{base} {}", "x ".repeat(count - 5));
            let code = with_syntax(&sql, limits).unwrap_err().code;
            assert_eq!(
                code,
                if count == limits.max_tokens {
                    Db2SyntaxDiagnosticCode::UnsupportedStatement
                } else {
                    Db2SyntaxDiagnosticCode::TooManyTokens
                }
            );
        }
        for depth in [limits.max_nesting, limits.max_nesting + 1] {
            let sql = format!("{} {} {base}", "/* ".repeat(depth), "*/ ".repeat(depth));
            let result = with_syntax(&sql, limits);
            if depth == limits.max_nesting {
                assert!(result.is_ok());
            } else {
                assert_eq!(
                    result.unwrap_err().code,
                    Db2SyntaxDiagnosticCode::UnbalancedDelimiter
                );
            }
        }
    }

    #[test]
    fn zero_and_one_beyond_compiled_limits_fail_closed() {
        let sql = "RENAME TABLE t TO n";
        for value in [0, 8 * 1024 * 1024 + 1] {
            assert_eq!(
                with_syntax(
                    sql,
                    Db2SyntaxLimits {
                        max_statement_bytes: value,
                        ..Db2SyntaxLimits::default()
                    }
                )
                .unwrap_err()
                .code,
                Db2SyntaxDiagnosticCode::InvalidLimits
            );
        }
        for (field, ceiling) in [(0, 262_144), (1, 1024 * 1024), (2, 1024)] {
            for value in [0, ceiling, ceiling + 1] {
                let mut limits = Db2SyntaxLimits::default();
                match field {
                    0 => limits.max_tokens = value,
                    1 => limits.max_token_bytes = value,
                    _ => limits.max_nesting = value,
                }
                assert_eq!(with_syntax(sql, limits).is_ok(), value == ceiling);
            }
        }
        for (field, ceiling) in [
            (0, 1024),
            (1, 16),
            (2, 8 * 1024 * 1024),
            (3, 262_144),
            (4, 65_536),
            (5, 1024),
        ] {
            for value in [0, ceiling, ceiling + 1] {
                let mut limits = Db2AstLimits::default();
                match field {
                    0 => limits.max_identifier_bytes = value,
                    1 => limits.max_name_parts = value,
                    2 => limits.max_literal_bytes = value,
                    3 => limits.max_expression_nodes = value,
                    4 => limits.max_list_items = value,
                    _ => limits.max_expression_depth = value,
                }
                let result = with_ast(sql, limits);
                if value == ceiling {
                    assert!(result.is_ok());
                } else {
                    assert_eq!(
                        result.unwrap_err().code,
                        Db2SyntaxDiagnosticCode::InvalidLimits
                    );
                }
            }
        }
    }

    #[test]
    fn results_own_names_and_locations_after_source_is_dropped() {
        let result = {
            let source = String::from("-- π\r\nRENAME TABLE loc.\"Sch\".\"a\"\"b\" TO \"New\";");
            let limits = Db2AstLimits::default();
            with_ast(&source, limits).unwrap()
        };
        let clone = result.clone();
        drop(result);
        assert_eq!(clone.object_kind(), Db2RenameObjectKind::Table);
        assert_eq!(
            clone
                .source_name()
                .parts()
                .iter()
                .map(Db2Identifier::value)
                .collect::<Vec<_>>(),
            ["LOC", "Sch", "a\"b"]
        );
        assert_eq!(clone.destination_identifier().value(), "New");
        assert_eq!(clone.span().start, Db2SourceLocation { line: 2, column: 1 });
        assert_eq!(clone.source_part_spans().len(), 3);
    }
}
