//! Owned common-object DROP syntax for the partial SQL0072 surface.
//!
//! Source: ibm-db2-for-zos-13-2026-08-13, db2z_sql_drop.html
//! (320964 bytes, SHA-256 5f75fdde9c96290ba968fb9b2629ca73d9274de9d8b8bd1002e56c341cd446a0).
//! The original DROP and alias-designator diagrams and db2z_namingconventions
//! define this boundary: TABLE/VIEW/ALIAS have at most three name parts; INDEX
//! has at most two. A location qualifier does not establish current-server
//! applicability. Identifier rules come from db2z_sqlidentifiers; implicit
//! qualification comes from db2z_resolutionofobjnames and remains binding-owned.
//! No expression, catalog lookup, authorization, dependency deletion, package
//! invalidation, physical applicability or execution is performed here.

use crate::{
    Db2AstLimits, Db2Identifier, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenCursor,
    Db2TokenKind, lex_db2,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DropObjectKind {
    Table,
    View,
    Index,
    Alias,
}

/// Source spelling only: an omitted designator does not bind a target kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DropAliasDesignator {
    Unspecified,
    ForTable,
}

/// One owned name with original-source spans for both the whole name and its parts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2DropObjectName {
    name: Db2QualifiedName,
    part_spans: Vec<Db2SourceSpan>,
    span: Db2SourceSpan,
}

impl Db2DropObjectName {
    #[must_use]
    pub const fn name(&self) -> &Db2QualifiedName {
        &self.name
    }

    #[must_use]
    pub fn part_spans(&self) -> &[Db2SourceSpan] {
        &self.part_spans
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2DropStatement {
    object_kind: Db2DropObjectKind,
    object_name: Db2DropObjectName,
    alias_designator: Option<Db2DropAliasDesignator>,
    alias_designator_span: Option<Db2SourceSpan>,
    span: Db2SourceSpan,
}

impl Db2DropStatement {
    #[must_use]
    pub const fn object_kind(&self) -> Db2DropObjectKind {
        self.object_kind
    }

    #[must_use]
    pub const fn object_name(&self) -> &Db2DropObjectName {
        &self.object_name
    }

    /// `None` for non-ALIAS objects; `Some(Unspecified)` for omitted alias spelling.
    #[must_use]
    pub const fn alias_designator(&self) -> Option<Db2DropAliasDesignator> {
        self.alias_designator
    }

    /// The complete FOR TABLE spelling, if explicitly present.
    #[must_use]
    pub const fn alias_designator_span(&self) -> Option<Db2SourceSpan> {
        self.alias_designator_span
    }

    /// From DROP through the final operand or optional semicolon, excluding outer trivia.
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse one DROP TABLE/VIEW/INDEX or non-PUBLIC ALIAS name [FOR TABLE].
/// Names use the existing bounded identifier authority after decoding doubled
/// quotation marks exactly once. The configured byte guard applies to the
/// effective identifier; raw spelling remains subject to lexer token bounds.
/// Name parts use `max_name_parts`, additionally bounded by the source object's
/// qualification syntax. No expression arena or operand list is constructed,
/// so list/node/depth budgets supply no expression or backend credit.
pub fn parse_db2_drop_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2DropStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    DropParser {
        cursor: lexed.cursor(),
        previous: None,
        limits: ast_limits,
    }
    .parse()
}

struct DropParser<'a> {
    cursor: Db2TokenCursor<'a>,
    previous: Option<&'a Db2Token>,
    limits: Db2AstLimits,
}

impl DropParser<'_> {
    fn parse(&mut self) -> Result<Db2DropStatement, Db2SyntaxDiagnostic> {
        let first = self.cursor.peek().expect("lexer rejects empty input").span;
        if !self.take_word("DROP") {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "expected the common-object DROP statement family",
            ));
        }
        let object_kind = match self.word() {
            Some("TABLE") => Db2DropObjectKind::Table,
            Some("VIEW") => Db2DropObjectKind::View,
            Some("INDEX") => Db2DropObjectKind::Index,
            Some("ALIAS") => Db2DropObjectKind::Alias,
            Some("PUBLIC") => {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "PUBLIC aliases are outside the common-object DROP subset",
                ));
            }
            _ => {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "DROP supports only TABLE, VIEW, INDEX and non-PUBLIC ALIAS syntax here",
                ));
            }
        };
        self.advance();
        let object_name = self.object_name(object_kind)?;
        let mut alias_designator = None;
        let mut alias_designator_span = None;
        if object_kind == Db2DropObjectKind::Alias {
            alias_designator = Some(Db2DropAliasDesignator::Unspecified);
            let start = self.cursor.peek().map(|token| token.span);
            if self.take_word("FOR") {
                if self.word() == Some("SEQUENCE") {
                    return Err(self.error(
                        Db2SyntaxDiagnosticCode::UnsupportedStatement,
                        "FOR SEQUENCE aliases are outside the common-object DROP subset",
                    ));
                }
                if !self.take_word("TABLE") {
                    return Err(self.error(
                        Db2SyntaxDiagnosticCode::MissingToken,
                        "expected TABLE after the DROP ALIAS FOR keyword",
                    ));
                }
                alias_designator = Some(Db2DropAliasDesignator::ForTable);
                alias_designator_span = Some(join_spans(
                    start.expect("FOR token was present"),
                    self.previous.expect("TABLE was consumed").span,
                ));
            }
        }
        if self.take_symbol(Db2Symbol::Semicolon) {
            if self.cursor.peek().is_some() {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    "only one DROP statement and one terminal semicolon are allowed",
                ));
            }
        } else if self.cursor.peek().is_some() {
            return Err(self.error(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "DROP clauses, function signatures, versions and extensions are unsupported",
            ));
        }
        Ok(Db2DropStatement {
            object_kind,
            object_name,
            alias_designator,
            alias_designator_span,
            span: join_spans(first, self.previous.expect("DROP consumed tokens").span),
        })
    }

    fn object_name(
        &mut self,
        kind: Db2DropObjectKind,
    ) -> Result<Db2DropObjectName, Db2SyntaxDiagnostic> {
        let source_max_parts = if kind == Db2DropObjectKind::Index {
            2
        } else {
            3
        };
        let mut parts = Vec::new();
        let mut part_spans = Vec::new();
        loop {
            if parts.len() >= source_max_parts {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "DROP object name has too many qualifiers for this object kind",
                ));
            }
            if parts.len() >= self.limits.max_name_parts {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "DROP object name exceeds the configured name-part limit",
                ));
            }
            let Some(token) = self.cursor.peek() else {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "expected a DROP object SQL identifier",
                ));
            };
            let Db2TokenKind::Word { value, delimited } = &token.kind else {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    "DROP object names require SQL identifiers, not host or parameter operands",
                ));
            };
            if !delimited
                && matches!(
                    value.as_str(),
                    "IF" | "EXISTS" | "FOR" | "RESTRICT" | "CASCADE" | "VERSION" | "PUBLIC"
                )
            {
                return Err(self.error(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "DROP object name is missing before an unsupported clause keyword",
                ));
            }
            let decoded = if *delimited {
                value.replace("\"\"", "\"")
            } else {
                value.clone()
            };
            let name = Db2Identifier::new(decoded, *delimited, self.limits).map_err(|problem| {
                self.error(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    &problem.message,
                )
            })?;
            parts.push(name);
            part_spans.push(token.span);
            self.advance();
            if !self.take_symbol(Db2Symbol::Period) {
                break;
            }
        }
        let span = join_spans(part_spans[0], *part_spans.last().expect("nonempty name"));
        let name = Db2QualifiedName::new(parts, self.limits).map_err(|problem| {
            self.error(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })?;
        Ok(Db2DropObjectName {
            name,
            part_spans,
            span,
        })
    }

    fn word(&self) -> Option<&str> {
        match self.cursor.peek().map(|token| &token.kind) {
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) => Some(value),
            _ => None,
        }
    }

    fn take_word(&mut self, expected: &str) -> bool {
        if self.word() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn take_symbol(&mut self, expected: Db2Symbol) -> bool {
        if matches!(self.cursor.peek().map(|token| &token.kind),
            Some(Db2TokenKind::Symbol(symbol)) if *symbol == expected)
        {
            self.advance();
            true
        } else {
            false
        }
    }

    fn advance(&mut self) {
        self.previous = self.cursor.next();
    }

    fn error(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
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

fn join_spans(first: Db2SourceSpan, last: Db2SourceSpan) -> Db2SourceSpan {
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

    fn parse(source: &str) -> Result<Db2DropStatement, Db2SyntaxDiagnostic> {
        parse_db2_drop_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
    }

    #[test]
    fn common_objects_accept_their_source_name_part_ranges() {
        for (word, kind, max_parts) in [
            ("TABLE", Db2DropObjectKind::Table, 3),
            ("VIEW", Db2DropObjectKind::View, 3),
            ("INDEX", Db2DropObjectKind::Index, 2),
            ("ALIAS", Db2DropObjectKind::Alias, 3),
        ] {
            for count in 1..=max_parts {
                let name = ["loc", "schema", "object"][3 - count..].join(".");
                for terminal in ["", ";"] {
                    let source = format!("drop {} {name}{terminal}", word.to_ascii_lowercase());
                    let result = parse(&source).unwrap();
                    assert_eq!(result.object_kind(), kind);
                    assert_eq!(result.object_name().name().parts().len(), count);
                    assert_eq!(result.object_name().part_spans().len(), count);
                    assert_eq!(
                        result.object_name().name().parts()[count - 1].value(),
                        "OBJECT"
                    );
                    assert_eq!(result.span().start_byte, 0);
                    assert_eq!(result.span().end_byte, source.len());
                    assert_eq!(
                        result.alias_designator(),
                        (kind == Db2DropObjectKind::Alias)
                            .then_some(Db2DropAliasDesignator::Unspecified)
                    );
                    assert_eq!(result.alias_designator_span(), None);
                }
            }
            let beyond = if max_parts == 2 { "L.S.O" } else { "L.S.O.X" };
            let error = parse(&format!("DROP {word} {beyond}")).unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnsupportedStatement);
        }
    }

    #[test]
    fn alias_designator_preserves_omitted_and_explicit_spelling() {
        for name in ["A", "S.A", "L.S.A"] {
            let omitted = parse(&format!("DROP ALIAS {name}")).unwrap();
            let source = format!("DROP ALIAS {name} FOR /* designator */ TABLE;");
            let explicit = parse(&source).unwrap();
            assert_eq!(
                omitted.alias_designator(),
                Some(Db2DropAliasDesignator::Unspecified)
            );
            assert_eq!(
                explicit.alias_designator(),
                Some(Db2DropAliasDesignator::ForTable)
            );
            let span = explicit.alias_designator_span().unwrap();
            assert_eq!(
                &source[span.start_byte..span.end_byte],
                "FOR /* designator */ TABLE"
            );
            assert_eq!(span.start.column as usize, span.start_byte + 1);
            assert_eq!(span.end.column as usize, span.end_byte + 1);
        }
    }

    #[test]
    fn unsupported_object_portfolios_and_extensions_fail_explicitly() {
        for family in [
            "DATABASE D",
            "FUNCTION F",
            "FUNCTION F()",
            "FUNCTION F(INTEGER)",
            "SPECIFIC FUNCTION S.F",
            "MASK M",
            "PACKAGE C.P VERSION V",
            "PERMISSION P",
            "PROCEDURE P",
            "ROLE R",
            "SEQUENCE S",
            "STOGROUP S",
            "SYNONYM S",
            "TABLESPACE D.T",
            "TRIGGER T",
            "TRUSTED CONTEXT C",
            "TYPE T",
            "VARIABLE V",
            "PUBLIC ALIAS A FOR SEQUENCE",
            "PUBLIC ALIAS A FOR TABLE",
            "PUBLIC ALIAS A",
        ] {
            let error = parse(&format!("DROP {family}")).unwrap_err();
            assert_eq!(
                error.code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "{family}"
            );
        }
        for object in ["TABLE", "VIEW", "INDEX", "ALIAS"] {
            for clause in [
                "RESTRICT",
                "CASCADE",
                "IF EXISTS",
                "VERSION V",
                "(INTEGER)",
                "FOR SEQUENCE",
                "FOR TABLE FOR TABLE",
                "AS X",
                "IN D",
                "A",
                ", X",
            ] {
                let source = format!("DROP {object} O {clause}");
                assert!(parse(&source).is_err(), "{source}");
            }
            let error = parse(&format!("DROP {object} IF EXISTS O")).unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnsupportedStatement);
        }
        let error = parse("DROP ALIAS A FOR SEQUENCE").unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnsupportedStatement);
        assert!(error.message.contains("FOR SEQUENCE"));
        assert!(parse("DROP TABLE T FOR TABLE").is_err());
        assert!(parse("DROP VIEW V FOR TABLE").is_err());
        assert!(parse("DROP INDEX I FOR TABLE").is_err());
    }

    #[test]
    fn malformed_names_clauses_and_extra_statements_never_produce_output() {
        for source in [
            "",
            " ",
            "-- only comment",
            "DROP",
            "DROP TABLE",
            "DROP TABLE ;",
            "DROP TABLE .T",
            "DROP TABLE S.",
            "DROP TABLE S..T",
            "DROP TABLE S.;",
            "DROP TABLE S.:HOST",
            "DROP TABLE S.?",
            "DROP TABLE :HOST",
            "DROP TABLE ?",
            "DROP TABLE 'T'",
            "DROP TABLE 1",
            "DROP TABLE \"\"",
            "DROP TABLE \"   \"",
            "DROP TABLE \"T",
            "DROP TABLE T /*",
            "DROP TABLE T)",
            "DROP TABLE (T)",
            "DROP ALIAS A FOR",
            "DROP ALIAS A FOR ;",
            "DROP ALIAS A FOR VIEW",
            "DROP ALIAS A FOR :HOST",
            "DROP ALIAS A TABLE",
            "DROP ALIAS FOR TABLE",
            "DROP TABLE T;;",
            "DROP TABLE T; DROP TABLE U",
            "DROP TABLE T DROP VIEW V",
            "DROP TABLE T; -- extra\r\nDROP TABLE U",
            "SELECT T",
            "\"DROP\" TABLE T",
            "DROP \"TABLE\" T",
            "DROP TABLE T\0",
        ] {
            assert!(parse(source).is_err(), "{source:?}");
        }
    }

    #[test]
    fn names_decode_once_and_preserve_effective_identifier_identity() {
        let statement = parse("DROP TABLE \" leading \".\"MiX\".\"a\"\"\"\"b   \"").unwrap();
        let parts = statement.object_name().name().parts();
        assert_eq!(parts[0].value(), " leading");
        assert_eq!(parts[1].value(), "MiX");
        assert_eq!(parts[2].value(), "a\"\"b");
        assert!(parts.iter().all(Db2Identifier::is_delimited));
        let ordinary = parse("DROP TABLE mixed").unwrap();
        let delimited = parse("DROP TABLE \"MIXED   \"").unwrap();
        assert_eq!(
            ordinary.object_name().name().parts()[0].value(),
            delimited.object_name().name().parts()[0].value()
        );
        assert_ne!(
            ordinary.object_name().name(),
            delimited.object_name().name()
        );
        assert_ne!(
            parts[1].value(),
            ordinary.object_name().name().parts()[0].value()
        );
        for name in [
            "\"IF\"",
            "\"FOR\"",
            "\"PUBLIC\"",
            "\"RESTRICT\"",
            "\"a.b\"",
            "\"?\"",
            "\":H\"",
        ] {
            assert!(parse(&format!("DROP TABLE {name}")).is_ok(), "{name}");
        }
    }

    fn location(source: &str, byte: usize) -> Db2SourceLocation {
        let mut line = 1;
        let mut column = 1;
        let mut previous = None;
        for ch in source[..byte].chars() {
            if ch == '\r' || (ch == '\n' && previous != Some('\r')) {
                line += 1;
                column = 1;
            } else if ch != '\n' || previous != Some('\r') {
                column += 1;
            }
            previous = Some(ch);
        }
        Db2SourceLocation { line, column }
    }

    fn assert_span(source: &str, span: Db2SourceSpan, spelling: &str) {
        assert_eq!(&source[span.start_byte..span.end_byte], spelling);
        assert_eq!(span.start, location(source, span.start_byte));
        assert_eq!(span.end, location(source, span.end_byte));
    }

    #[test]
    fn original_byte_and_character_locations_survive_trivia_and_relocation() {
        for prefix in ["", "  ", "-- é\r\n", "/* 中文 */\n\r\n  "] {
            for object in ["TABLE", "VIEW", "INDEX", "ALIAS"] {
                let core =
                    format!("DROP/*é/*nested*/ */\r\n{object}\t\"Sché\" /*名*/ .\r\n \"a\"\"b  \"");
                let suffix = if object == "ALIAS" {
                    " FOR\r\n/* é */ TABLE"
                } else {
                    ""
                };
                for terminal in ["", ";"] {
                    let spelling = format!("{core}{suffix}{terminal}");
                    let source = format!("{prefix}{spelling} -- trailing 中文\r\n");
                    let result = parse(&source).unwrap();
                    assert_span(&source, result.span(), &spelling);
                    assert_eq!(result.span().start_byte, prefix.len());
                    let name = result.object_name();
                    assert_span(&source, name.span(), "\"Sché\" /*名*/ .\r\n \"a\"\"b  \"");
                    assert_span(&source, name.part_spans()[0], "\"Sché\"");
                    assert_span(&source, name.part_spans()[1], "\"a\"\"b  \"");
                    if object == "ALIAS" {
                        assert_span(
                            &source,
                            result.alias_designator_span().unwrap(),
                            "FOR\r\n/* é */ TABLE",
                        );
                    }
                    assert_eq!(name.name().parts()[0].value(), "Sché");
                    assert_eq!(name.name().parts()[1].value(), "a\"b");
                }
                let invalid = format!("{prefix}{core} CASCADE");
                let error = parse(&invalid).unwrap_err();
                assert_eq!(
                    error.location,
                    location(&invalid, invalid.find("CASCADE").unwrap())
                );
            }
        }
    }

    #[test]
    fn syntax_output_is_owned_after_source_and_configuration_are_dropped() {
        let result = {
            let source = String::from("-- é\r\nDROP ALIAS L.\"S\".\"a\"\"b \" FOR TABLE;");
            let syntax = Db2SyntaxLimits::default();
            let ast = Db2AstLimits::default();
            parse_db2_drop_statement(&source, syntax, ast).unwrap()
        };
        assert_eq!(result.object_name().name().parts()[2].value(), "a\"b");
        assert_eq!(
            result.span().start,
            Db2SourceLocation { line: 2, column: 1 }
        );
        assert_eq!(
            result.alias_designator(),
            Some(Db2DropAliasDesignator::ForTable)
        );
        assert_eq!(result.clone(), result);
    }

    #[test]
    fn configured_decoded_byte_and_name_part_bounds_are_exact() {
        for count in [128, 129] {
            for name in ["A".repeat(count), format!("\"{}\"", "\"\"".repeat(count))] {
                assert_eq!(parse(&format!("DROP TABLE {name}")).is_ok(), count == 128);
            }
        }
        let limits = Db2AstLimits {
            max_identifier_bytes: 3,
            ..Db2AstLimits::default()
        };
        for name in ["ABC", "\"a\"\"b\"", "\"éa\"", "\"abc     \""] {
            assert!(
                parse_db2_drop_statement(
                    &format!("DROP TABLE {name}"),
                    Db2SyntaxLimits::default(),
                    limits
                )
                .is_ok(),
                "{name}"
            );
        }
        for name in ["ABCD", "\"a\"\"bc\"", "\"éab\"", "\"abcd   \""] {
            let error = parse_db2_drop_statement(
                &format!("DROP TABLE {name}"),
                Db2SyntaxLimits::default(),
                limits,
            )
            .unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
        }
        for max in 1..=3 {
            let limits = Db2AstLimits {
                max_name_parts: max,
                ..Db2AstLimits::default()
            };
            let name = ["L", "S", "T"][3 - max..].join(".");
            assert!(
                parse_db2_drop_statement(
                    &format!("DROP TABLE {name}"),
                    Db2SyntaxLimits::default(),
                    limits
                )
                .is_ok()
            );
            if max < 3 {
                let error = parse_db2_drop_statement(
                    &format!("DROP TABLE X.{name}"),
                    Db2SyntaxLimits::default(),
                    limits,
                )
                .unwrap_err();
                assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
            }
        }
        // There are no expression nodes/lists/depth to consume in this kernel.
        let limits = Db2AstLimits {
            max_list_items: 1,
            max_expression_nodes: 1,
            max_expression_depth: 1,
            ..Db2AstLimits::default()
        };
        assert!(
            parse_db2_drop_statement(
                "DROP ALIAS L.S.A FOR TABLE",
                Db2SyntaxLimits::default(),
                limits
            )
            .is_ok()
        );
    }

    #[test]
    fn configured_input_token_count_and_raw_token_bytes_are_exact() {
        let source = "DROP ALIAS L.S.A FOR TABLE;";
        let tokens = lex_db2(source, Db2SyntaxLimits::default())
            .unwrap()
            .tokens()
            .len();
        let limits = Db2SyntaxLimits {
            max_statement_bytes: source.len(),
            max_tokens: tokens,
            ..Db2SyntaxLimits::default()
        };
        assert!(parse_db2_drop_statement(source, limits, Db2AstLimits::default()).is_ok());
        let error =
            parse_db2_drop_statement(&format!("{source} "), limits, Db2AstLimits::default())
                .unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::StatementTooLarge);
        let error = parse_db2_drop_statement(
            source,
            Db2SyntaxLimits {
                max_tokens: tokens - 1,
                ..limits
            },
            Db2AstLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::TooManyTokens);
        let limits = Db2SyntaxLimits {
            max_token_bytes: 6,
            ..Db2SyntaxLimits::default()
        };
        assert!(
            parse_db2_drop_statement("DROP TABLE \"a\"\"b\"", limits, Db2AstLimits::default())
                .is_ok()
        );
        let error =
            parse_db2_drop_statement("DROP TABLE \"a\"\"bc\"", limits, Db2AstLimits::default())
                .unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::TokenTooLarge);
    }

    #[test]
    fn compiled_identifier_and_input_byte_bounds_are_exact() {
        let ast = Db2AstLimits {
            max_identifier_bytes: 1024,
            ..Db2AstLimits::default()
        };
        // Resource guard, not catalog/physical identifier applicability proof.
        let name = "\"\"".repeat(1024);
        let result = parse_db2_drop_statement(
            &format!("DROP TABLE \"{name}\""),
            Db2SyntaxLimits::default(),
            ast,
        )
        .unwrap();
        assert_eq!(
            result.object_name().name().parts()[0].value(),
            "\"".repeat(1024)
        );
        let error = parse_db2_drop_statement(
            &format!("DROP TABLE \"{name}\"\"\""),
            Db2SyntaxLimits::default(),
            ast,
        )
        .unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
        let syntax = Db2SyntaxLimits {
            max_statement_bytes: 8 * 1024 * 1024,
            max_token_bytes: 1024 * 1024,
            ..Db2SyntaxLimits::default()
        };
        let token = format!("\"T{}\"", " ".repeat(syntax.max_token_bytes - 3));
        assert!(parse_db2_drop_statement(&format!("DROP TABLE {token}"), syntax, ast).is_ok());
        let token = format!("\"T{}\"", " ".repeat(syntax.max_token_bytes - 2));
        let error =
            parse_db2_drop_statement(&format!("DROP TABLE {token}"), syntax, ast).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::TokenTooLarge);
        let source = format!(
            "DROP TABLE T{}",
            " ".repeat(syntax.max_statement_bytes - 12)
        );
        assert_eq!(source.len(), syntax.max_statement_bytes);
        assert!(parse_db2_drop_statement(&source, syntax, ast).is_ok());
        let error = parse_db2_drop_statement(&format!("{source} "), syntax, ast).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::StatementTooLarge);
    }

    #[test]
    fn configured_and_compiled_invalid_limits_fail_closed() {
        let syntax = Db2SyntaxLimits::default();
        let ast = Db2AstLimits::default();
        for invalid in [
            Db2SyntaxLimits {
                max_statement_bytes: 0,
                ..syntax
            },
            Db2SyntaxLimits {
                max_statement_bytes: 8 * 1024 * 1024 + 1,
                ..syntax
            },
            Db2SyntaxLimits {
                max_token_bytes: 0,
                ..syntax
            },
            Db2SyntaxLimits {
                max_token_bytes: 1024 * 1024 + 1,
                ..syntax
            },
            Db2SyntaxLimits {
                max_tokens: 0,
                ..syntax
            },
            Db2SyntaxLimits {
                max_tokens: 262_145,
                ..syntax
            },
            Db2SyntaxLimits {
                max_nesting: 0,
                ..syntax
            },
            Db2SyntaxLimits {
                max_nesting: 1025,
                ..syntax
            },
        ] {
            assert_eq!(
                parse_db2_drop_statement("DROP TABLE T", invalid, ast)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidLimits
            );
        }
        for invalid in [
            Db2AstLimits {
                max_identifier_bytes: 0,
                ..ast
            },
            Db2AstLimits {
                max_identifier_bytes: 1025,
                ..ast
            },
            Db2AstLimits {
                max_name_parts: 0,
                ..ast
            },
            Db2AstLimits {
                max_name_parts: 17,
                ..ast
            },
        ] {
            assert_eq!(
                parse_db2_drop_statement("DROP TABLE T", syntax, invalid)
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::InvalidLimits
            );
        }
        let syntax = Db2SyntaxLimits {
            max_tokens: 262_144,
            max_nesting: 1024,
            ..syntax
        };
        let ast = Db2AstLimits {
            max_name_parts: 16,
            ..ast
        };
        assert!(parse_db2_drop_statement("DROP TABLE L.S.T", syntax, ast).is_ok());
        assert!(parse_db2_drop_statement("DROP TABLE L.S.T.X", syntax, ast).is_err());
    }

    #[test]
    fn inherited_lexer_token_and_comment_nesting_ceilings_remain_enforced() {
        let ast = Db2AstLimits::default();
        for depth in [2, 1024] {
            let syntax = Db2SyntaxLimits {
                max_nesting: depth,
                ..Db2SyntaxLimits::default()
            };
            let source = format!("{}é{} DROP TABLE T", "/*".repeat(depth), "*/".repeat(depth));
            assert!(parse_db2_drop_statement(&source, syntax, ast).is_ok());
            let source = format!("/*{source}*/");
            let error = parse_db2_drop_statement(&source, syntax, ast).unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnbalancedDelimiter);
        }
        // A valid DROP has at most ten tokens. At the compiled lexer ceiling,
        // extra tokens pass lexing but still fail statement syntax; one beyond
        // fails the lexer budget before the parser can inspect trailing input.
        let syntax = Db2SyntaxLimits {
            max_tokens: 262_144,
            ..Db2SyntaxLimits::default()
        };
        let source = format!("DROP TABLE T {}", "X ".repeat(syntax.max_tokens - 3));
        let error = parse_db2_drop_statement(&source, syntax, ast).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnsupportedStatement);
        let error = parse_db2_drop_statement(&format!("{source}X"), syntax, ast).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::TooManyTokens);
        // The existing lexer fences ordinary non-ASCII spellings; delimited
        // UTF-8 names above exercise the accepted route without lifting it.
        let error = parse("DROP TABLE é").unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidCharacter);
    }
}
