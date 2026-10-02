//! Owned bounded syntax for the common CREATE INDEX slice (SQL 0039).
//!
//! This pure parser preserves syntax only. Catalog-dependent validity, index
//! constraints, authorization, execution, and physical options remain pending.

use crate::{
    Db2AstLimits, Db2Identifier, Db2QualifiedName, Db2SourceLocation, Db2SourceSpan, Db2Symbol,
    Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2Token, Db2TokenCursor,
    Db2TokenKind, lex_db2,
};

const MAX_INDEX_KEYS: usize = 64;
const MAX_LOCAL_NAME_PARTS: usize = 2;

/// Source spelling of the optional uniqueness group; no constraint is executed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2IndexUniqueness {
    Nonunique,
    Unique,
    UniqueWhereNotNull,
}

/// Omitted ordering remains distinguishable from an explicit ASC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2IndexKeyOrder {
    Unspecified,
    Asc,
    Desc,
    Random,
}

/// One unqualified column key, including its optional ordering keyword span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CreateIndexKey {
    column_name: Db2Identifier,
    order: Db2IndexKeyOrder,
    span: Db2SourceSpan,
}

impl Db2CreateIndexKey {
    #[must_use]
    pub const fn column_name(&self) -> &Db2Identifier {
        &self.column_name
    }

    #[must_use]
    pub const fn order(&self) -> Db2IndexKeyOrder {
        self.order
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CreateIndexStatement {
    uniqueness: Db2IndexUniqueness,
    index_name: Db2QualifiedName,
    table_name: Db2QualifiedName,
    keys: Vec<Db2CreateIndexKey>,
    span: Db2SourceSpan,
}

impl Db2CreateIndexStatement {
    #[must_use]
    pub const fn uniqueness(&self) -> Db2IndexUniqueness {
        self.uniqueness
    }

    #[must_use]
    pub const fn index_name(&self) -> &Db2QualifiedName {
        &self.index_name
    }

    #[must_use]
    pub const fn table_name(&self) -> &Db2QualifiedName {
        &self.table_name
    }

    #[must_use]
    pub fn keys(&self) -> &[Db2CreateIndexKey] {
        &self.keys
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Parse exactly one CREATE [UNIQUE [WHERE NOT NULL]] INDEX statement with
/// local (optionally schema-qualified) names and 1..=64 distinct column keys.
/// The sole key list is bounded by `max_list_items`; name components use
/// `max_name_parts`. No expression arena is constructed. Expressions, XML,
/// auxiliary-index syntax, BUSINESS_TIME, INCLUDE, and physical clauses fail
/// explicitly. A table's actual kind and column types require later binding.
pub fn parse_db2_create_index_statement(
    source: &str,
    syntax_limits: Db2SyntaxLimits,
    ast_limits: Db2AstLimits,
) -> Result<Db2CreateIndexStatement, Db2SyntaxDiagnostic> {
    let lexed = lex_db2(source, syntax_limits)?;
    ast_limits.validate().map_err(|problem| {
        Db2SyntaxDiagnostic::new(
            Db2SyntaxDiagnosticCode::InvalidLimits,
            Db2SourceLocation::START,
            &problem.message,
        )
    })?;
    CreateIndexParser {
        cursor: lexed.cursor(),
        previous: None,
        limits: ast_limits,
    }
    .parse()
}

struct CreateIndexParser<'a> {
    cursor: Db2TokenCursor<'a>,
    previous: Option<&'a Db2Token>,
    limits: Db2AstLimits,
}

impl<'a> CreateIndexParser<'a> {
    fn parse(&mut self) -> Result<Db2CreateIndexStatement, Db2SyntaxDiagnostic> {
        let first = self
            .cursor
            .peek()
            .expect("lexer rejects an empty statement")
            .span;
        if !self.take_word("CREATE") {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "statement is outside the common CREATE INDEX family",
            ));
        }
        let uniqueness = if self.take_word("UNIQUE") {
            if self.take_word("WHERE") {
                self.expect_word("NOT")?;
                self.expect_word("NULL")?;
                Db2IndexUniqueness::UniqueWhereNotNull
            } else {
                Db2IndexUniqueness::Unique
            }
        } else {
            Db2IndexUniqueness::Nonunique
        };
        self.expect_word("INDEX")?;
        let index_name = self.qualified_name("index name")?;
        self.expect_word("ON")?;
        let table_name = self.qualified_name("table name")?;
        if !self.take_symbol(Db2Symbol::LeftParenthesis) {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "common CREATE INDEX requires column keys; auxiliary and XML forms are unsupported",
            ));
        }

        let mut keys: Vec<Db2CreateIndexKey> = Vec::new();
        loop {
            if keys.len() >= MAX_INDEX_KEYS || keys.len() >= self.limits.max_list_items {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "CREATE INDEX keys exceed 64 columns or the configured list limit",
                ));
            }
            let key = self.key()?;
            // Identifier equality also includes quote spelling; SQL identity
            // uses only the effective value after normalization/trimming.
            if keys
                .iter()
                .any(|other| other.column_name.value() == key.column_name.value())
            {
                return Err(Db2SyntaxDiagnostic::new(
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    key.span.start,
                    "CREATE INDEX column key is specified more than once",
                ));
            }
            keys.push(key);
            if self.take_symbol(Db2Symbol::RightParenthesis) {
                break;
            }
            if !self.take_symbol(Db2Symbol::Comma) {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::MissingToken,
                    "expected comma or closing parenthesis after CREATE INDEX key",
                ));
            }
        }
        if self.take_symbol(Db2Symbol::Semicolon) {
            if self.cursor.peek().is_some() {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::UnexpectedToken,
                    "only one Db2 statement and one terminal semicolon are allowed",
                ));
            }
        } else if self.cursor.peek().is_some() {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "trailing CREATE INDEX clause is outside the common subset",
            ));
        }
        let last = self.previous.expect("CREATE INDEX consumed tokens").span;
        Ok(Db2CreateIndexStatement {
            uniqueness,
            index_name,
            table_name,
            keys,
            span: Db2SourceSpan {
                start_byte: first.start_byte,
                end_byte: last.end_byte,
                start: first.start,
                end: last.end,
            },
        })
    }

    fn key(&mut self) -> Result<Db2CreateIndexKey, Db2SyntaxDiagnostic> {
        if matches!(self.word(), Some("TRUE" | "FALSE")) {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "Db2 Boolean constant syntax is source-pending on #350",
            ));
        }
        if self.word() == Some("BUSINESS_TIME") {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "BUSINESS_TIME index keys are outside the common subset",
            ));
        }
        if self.peek_symbol(Db2Symbol::LeftParenthesis)
            || matches!(self.word(), Some("NULL" | "DEFAULT"))
        {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "expressions and constants are outside common CREATE INDEX column-key syntax",
            ));
        }
        let start = self.cursor.peek().map(|token| token.span);
        let column_name = self.identifier("unqualified column key")?;
        let order = if self.take_word("ASC") {
            Db2IndexKeyOrder::Asc
        } else if self.take_word("DESC") {
            Db2IndexKeyOrder::Desc
        } else if self.take_word("RANDOM") {
            Db2IndexKeyOrder::Random
        } else {
            Db2IndexKeyOrder::Unspecified
        };
        if self.cursor.peek().is_some()
            && !self.peek_symbol(Db2Symbol::Comma)
            && !self.peek_symbol(Db2Symbol::RightParenthesis)
            && !self.peek_symbol(Db2Symbol::Semicolon)
        {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "only unqualified column keys with one optional ASC, DESC, or RANDOM are supported",
            ));
        }
        let first = start.expect("identifier consumed a token");
        let last = self.previous.expect("key consumed tokens").span;
        Ok(Db2CreateIndexKey {
            column_name,
            order,
            span: Db2SourceSpan {
                start_byte: first.start_byte,
                end_byte: last.end_byte,
                start: first.start,
                end: last.end,
            },
        })
    }

    fn qualified_name(&mut self, label: &str) -> Result<Db2QualifiedName, Db2SyntaxDiagnostic> {
        let mut parts = Vec::new();
        loop {
            if parts.len() >= self.limits.max_name_parts {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                    "CREATE INDEX object name exceeds the configured part limit",
                ));
            }
            if parts.len() >= MAX_LOCAL_NAME_PARTS {
                return Err(self.diagnostic_here(
                    Db2SyntaxDiagnosticCode::UnsupportedStatement,
                    "CREATE INDEX supports only local optionally schema-qualified object names",
                ));
            }
            parts.push(self.identifier(label)?);
            if !self.take_symbol(Db2Symbol::Period) {
                break;
            }
        }
        Db2QualifiedName::new(parts, self.limits).map_err(|problem| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })
    }

    fn identifier(&mut self, label: &str) -> Result<Db2Identifier, Db2SyntaxDiagnostic> {
        let Some(token) = self.cursor.peek() else {
            return Err(self.diagnostic_here(Db2SyntaxDiagnosticCode::MissingToken, label));
        };
        let Db2TokenKind::Word { value, delimited } = &token.kind else {
            return Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::UnexpectedToken,
                "CREATE INDEX requires an SQL identifier",
            ));
        };
        // The lexer retains the quoted body spelling. Decode its doubled
        // escapes before AST identifier length/identity validation (pinned
        // db2z_sqlidentifiers, delimited-identifier rules).
        let value = if *delimited {
            value.replace("\"\"", "\"")
        } else {
            value.clone()
        };
        let name = Db2Identifier::new(value, *delimited, self.limits).map_err(|problem| {
            self.diagnostic_here(
                Db2SyntaxDiagnosticCode::InvalidStatementOperand,
                &problem.message,
            )
        })?;
        self.advance();
        Ok(name)
    }

    fn expect_word(&mut self, expected: &str) -> Result<(), Db2SyntaxDiagnostic> {
        if self.take_word(expected) {
            Ok(())
        } else {
            Err(self.diagnostic_here(
                Db2SyntaxDiagnosticCode::MissingToken,
                &format!("expected Db2 keyword {expected}"),
            ))
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

    fn word(&self) -> Option<&str> {
        match self.cursor.peek().map(|token| &token.kind) {
            Some(Db2TokenKind::Word {
                value,
                delimited: false,
            }) => Some(value),
            _ => None,
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

    fn diagnostic_here(&self, code: Db2SyntaxDiagnosticCode, message: &str) -> Db2SyntaxDiagnostic {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Db2CreateIndexStatement, Db2SyntaxDiagnostic> {
        parse_db2_create_index_statement(
            source,
            Db2SyntaxLimits::default(),
            Db2AstLimits::default(),
        )
    }

    fn with_ast(
        source: &str,
        limits: Db2AstLimits,
    ) -> Result<Db2CreateIndexStatement, Db2SyntaxDiagnostic> {
        parse_db2_create_index_statement(source, Db2SyntaxLimits::default(), limits)
    }

    fn key_list(count: usize) -> String {
        (0..count)
            .map(|i| format!("C{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn common_index_preserves_names_mode_and_order() {
        let index = parse_db2_create_index_statement(
            "CREATE UNIQUE WHERE NOT NULL INDEX S.I ON S.T (A, B ASC, C DESC, D RANDOM);",
            Db2SyntaxLimits::default(),
            Db2AstLimits::default(),
        )
        .unwrap();
        assert_eq!(index.uniqueness(), Db2IndexUniqueness::UniqueWhereNotNull);
        assert_eq!(index.index_name().parts()[1].value(), "I");
        assert_eq!(index.table_name().parts()[1].value(), "T");
        assert_eq!(
            index
                .keys()
                .iter()
                .map(|key| key.order())
                .collect::<Vec<_>>(),
            vec![
                Db2IndexKeyOrder::Unspecified,
                Db2IndexKeyOrder::Asc,
                Db2IndexKeyOrder::Desc,
                Db2IndexKeyOrder::Random,
            ]
        );
    }

    #[test]
    fn each_uniqueness_mode_accepts_each_order_and_preserves_omission() {
        for (spelling, mode) in [
            ("", Db2IndexUniqueness::Nonunique),
            ("UNIQUE ", Db2IndexUniqueness::Unique),
            (
                "UNIQUE WHERE NOT NULL ",
                Db2IndexUniqueness::UniqueWhereNotNull,
            ),
        ] {
            for (ordering, order) in [
                ("", Db2IndexKeyOrder::Unspecified),
                (" ASC", Db2IndexKeyOrder::Asc),
                (" DESC", Db2IndexKeyOrder::Desc),
                (" RANDOM", Db2IndexKeyOrder::Random),
            ] {
                let sql = format!("CREATE {spelling}INDEX i ON t (a{ordering})");
                let index = parse(&sql).unwrap();
                assert_eq!(index.uniqueness(), mode);
                assert_eq!(index.keys().len(), 1);
                assert_eq!(index.keys()[0].column_name().value(), "A");
                assert_eq!(index.keys()[0].order(), order);
                assert_eq!(index.span().end_byte, sql.len());
            }
        }
    }

    #[test]
    fn delimited_names_preserve_case_quotes_and_significant_content() {
        let index = parse("CREATE INDEX \"Mixed schema\".\"i\"\"x\" ON \"s\".\"t\" (\"asc\" DESC, \"BUSINESS_TIME\", \"a \" RANDOM)").unwrap();
        assert_eq!(index.index_name().parts()[0].value(), "Mixed schema");
        assert_eq!(index.index_name().parts()[1].value(), "i\"x");
        assert!(index.index_name().parts()[1].is_delimited());
        assert_eq!(index.table_name().parts()[0].value(), "s");
        assert_eq!(index.keys()[0].column_name().value(), "asc");
        assert_eq!(index.keys()[1].column_name().value(), "BUSINESS_TIME");
        assert_eq!(index.keys()[2].column_name().value(), "a");
    }

    #[test]
    fn duplicates_use_effective_identifier_values() {
        for keys in [
            "a, A",
            "A, \"A\"",
            "\"A  \", A",
            "\"a\", \"a \"",
            "A ASC, A DESC",
            "\"a\"\"b\", \"a\"\"b \"",
        ] {
            let sql = format!("CREATE INDEX I ON T ({keys})");
            let error = parse(&sql).unwrap_err();
            assert_eq!(
                error.code,
                Db2SyntaxDiagnosticCode::DuplicateClause,
                "{sql}"
            );
            assert!(error.message.len() <= 256);
        }
        let index = parse("CREATE INDEX I ON T (A, \"a\")").unwrap();
        assert_eq!(index.keys().len(), 2);
    }

    #[test]
    fn sixty_four_keys_pass_and_sixty_five_fail_in_all_modes() {
        for mode in ["", "UNIQUE ", "UNIQUE WHERE NOT NULL "] {
            let index = parse(&format!("CREATE {mode}INDEX I ON T ({})", key_list(64))).unwrap();
            assert_eq!(index.keys().len(), 64);
            let error =
                parse(&format!("CREATE {mode}INDEX I ON T ({})", key_list(65))).unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
        }
    }

    #[test]
    fn key_list_limit_is_enforced_before_retaining_an_extra_key() {
        let limits = Db2AstLimits {
            max_list_items: 2,
            ..Db2AstLimits::default()
        };
        assert_eq!(
            with_ast("CREATE INDEX S.I ON S.T (A, B)", limits)
                .unwrap()
                .keys()
                .len(),
            2
        );
        let error = with_ast("CREATE INDEX I ON T (A, B, C)", limits).unwrap_err();
        assert_eq!(error.code, Db2SyntaxDiagnosticCode::InvalidStatementOperand);
        assert_eq!(error.location.column, 28);
        // This grammar has no expression nodes; their limit cannot be charged
        // independently per key or used to admit expression-based indexes.
        let limits = Db2AstLimits {
            max_expression_nodes: 1,
            ..Db2AstLimits::default()
        };
        assert_eq!(
            with_ast(&format!("CREATE INDEX I ON T ({})", key_list(64)), limits)
                .unwrap()
                .keys()
                .len(),
            64
        );
        assert!(with_ast("CREATE INDEX I ON T (A + 1)", limits).is_err());
    }

    #[test]
    fn configured_names_and_identifier_bytes_are_bounded() {
        let limits = Db2AstLimits {
            max_name_parts: 1,
            ..Db2AstLimits::default()
        };
        assert!(with_ast("CREATE INDEX I ON T (A)", limits).is_ok());
        for sql in ["CREATE INDEX S.I ON T (A)", "CREATE INDEX I ON S.T (A)"] {
            assert_eq!(
                with_ast(sql, limits).unwrap_err().code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }
        for sql in ["CREATE INDEX R.S.I ON T (A)", "CREATE INDEX I ON R.S.T (A)"] {
            assert_eq!(
                parse(sql).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement
            );
        }
        let limits = Db2AstLimits {
            max_identifier_bytes: 4,
            ..Db2AstLimits::default()
        };
        assert!(with_ast("CREATE INDEX ABCD ON T (\"éé\")", limits).is_ok());
        assert!(with_ast("CREATE INDEX I ON T (\"a\"\"bc\")", limits).is_ok());
        for sql in [
            "CREATE INDEX ABCDE ON T (A)",
            "CREATE INDEX I ON ABCDE (A)",
            "CREATE INDEX I ON T (\"ééé\")",
        ] {
            assert_eq!(
                with_ast(sql, limits).unwrap_err().code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }
        assert!(parse(&format!("CREATE INDEX {} ON T (A)", "I".repeat(128))).is_ok());
        assert!(parse(&format!("CREATE INDEX {} ON T (A)", "I".repeat(129))).is_err());
    }

    #[test]
    fn whitespace_and_comments_are_valid_at_every_token_boundary() {
        let tokens = [
            "create", "unique", "where", "not", "null", "index", "s", ".", "i", "on", "s", ".",
            "t", "(", "a", "asc", ",", "b", "random", ")", ";",
        ];
        let expected = parse(&tokens.join(" ")).unwrap();
        for separator in [
            " ",
            "\t",
            "\r\n",
            "/* outer /* inner */ end */",
            "-- line\n",
            "\n\t/* 文 */\r\n",
        ] {
            let actual = parse(&tokens.join(separator)).unwrap();
            assert_eq!(actual.uniqueness(), expected.uniqueness());
            assert_eq!(actual.index_name(), expected.index_name());
            assert_eq!(actual.table_name(), expected.table_name());
            let keys = |index: &Db2CreateIndexStatement| {
                index
                    .keys()
                    .iter()
                    .map(|key| (key.column_name().value().to_owned(), key.order()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(keys(&actual), keys(&expected));
        }
    }

    #[test]
    fn byte_line_and_column_spans_relocate_with_unicode_prefix() {
        let sql = "CREATE INDEX I ON T (\"é\" DESC, B);";
        let prefix = "/* 文 */\r\n\t";
        let original = parse(sql).unwrap();
        let relocated_source = format!("{prefix}{sql} -- trailing comment\n");
        let relocated = parse(&relocated_source).unwrap();
        let relocate = |before: Db2SourceSpan, after: Db2SourceSpan| {
            assert_eq!(after.start_byte, before.start_byte + prefix.len());
            assert_eq!(after.end_byte, before.end_byte + prefix.len());
            assert_eq!(after.start.line, before.start.line + 1);
            assert_eq!(after.end.line, before.end.line + 1);
            assert_eq!(after.start.column, before.start.column + 1);
            assert_eq!(after.end.column, before.end.column + 1);
        };
        relocate(original.span(), relocated.span());
        for (before, after) in original.keys().iter().zip(relocated.keys()) {
            relocate(before.span(), after.span());
        }
        let key = relocated.keys()[0].span();
        assert_eq!(
            &relocated_source[key.start_byte..key.end_byte],
            "\"é\" DESC"
        );
        assert_eq!(
            relocated.keys()[1].span().end.column - relocated.keys()[1].span().start.column,
            1
        );
    }

    #[test]
    fn duplicate_and_unsupported_diagnostics_relocate() {
        for sql in [
            "CREATE INDEX I ON T (A, \"A\")",
            "CREATE INDEX I ON T (A + 1)",
            "CREATE INDEX I ON T (A) INCLUDE (B)",
        ] {
            let error = parse(sql).unwrap_err();
            let relocated = parse(&format!("-- prefix\n  {sql}")).unwrap_err();
            assert_eq!(error.code, relocated.code);
            assert_eq!(error.location.line + 1, relocated.location.line);
            assert_eq!(error.location.column + 2, relocated.location.column);
        }
    }

    #[test]
    fn unsupported_key_families_fail_explicitly() {
        for key in [
            "T.A",
            "A + 1",
            "LOWER(A)",
            "CAST(A AS INTEGER)",
            "(A)",
            "BUSINESS_TIME WITHOUT OVERLAPS",
            "A, BUSINESS_TIME WITH OVERLAPS",
            "A ASC DESC",
            "A DESC RANDOM",
            "A NULLS FIRST",
        ] {
            let sql = format!("CREATE INDEX I ON T ({key})");
            let error = parse(&sql).unwrap_err();
            assert_eq!(
                error.code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "{sql}: {error}"
            );
        }
        for key in ["1", "'A'", ":HV", "?", "*"] {
            let error = parse(&format!("CREATE INDEX I ON T ({key})")).unwrap_err();
            assert_eq!(error.code, Db2SyntaxDiagnosticCode::UnexpectedToken);
        }
    }

    #[test]
    fn xml_auxiliary_include_and_physical_clauses_are_rejected() {
        for suffix in [
            "GENERATE KEY USING XMLPATTERN '/a' AS SQL VARCHAR(10)",
            "GENERATE KEYS USING XMLPATTERN '/a' AS SQL VARCHAR(10)",
            "INCLUDE (B)",
            "NOT CLUSTER",
            "CLUSTER",
            "PARTITIONED",
            "NOT PADDED",
            "PADDED",
            "USING VCAT CAT",
            "USING STOGROUP SG",
            "FREEPAGE 0",
            "PCTFREE 10",
            "GBPCACHE CHANGED",
            "DEFINE YES",
            "COMPRESS YES",
            "INCLUDE NULL KEYS",
            "EXCLUDE NULL KEYS",
            "PARTITION BY RANGE (PARTITION 1)",
            "BUFFERPOOL BP0",
            "CLOSE YES",
            "DEFER NO",
            "DSSIZE 1 G",
            "PIECESIZE 1 G",
            "COPY YES",
        ] {
            let sql = format!("CREATE INDEX I ON T (A) {suffix}");
            assert_eq!(
                parse(&sql).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement,
                "{sql}"
            );
        }
        for sql in [
            "CREATE UNIQUE INDEX I ON AUX",
            "CREATE INDEX I ON T GENERATE KEY USING XMLPATTERN '/a' AS SQL VARCHAR(10)",
        ] {
            assert_eq!(
                parse(sql).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedStatement
            );
        }
    }

    #[test]
    fn malformed_modes_names_lists_and_multiple_statements_fail() {
        for sql in [
            "CREATE",
            "CREATE INDEX",
            "CREATE UNIQUE INDEX",
            "CREATE UNIQUE WHERE INDEX I ON T (A)",
            "CREATE UNIQUE WHERE NULL INDEX I ON T (A)",
            "CREATE UNIQUE WHERE NOT INDEX I ON T (A)",
            "CREATE WHERE NOT NULL INDEX I ON T (A)",
            "CREATE UNIQUE UNIQUE INDEX I ON T (A)",
            "CREATE INDEX I UNIQUE ON T (A)",
            "CREATE INDEX I T (A)",
            "CREATE INDEX S. ON T (A)",
            "CREATE INDEX I ON T. (A)",
            "CREATE INDEX I ON T ()",
            "CREATE INDEX I ON T (A,)",
            "CREATE INDEX I ON T (,A)",
            "CREATE INDEX I ON T (A,,B)",
            "CREATE INDEX I ON T (A B)",
            "CREATE INDEX I ON T (A",
            "CREATE INDEX I ON T (A;)",
            "CREATE INDEX I ON T (A);;",
            "CREATE INDEX I ON T (A); COMMIT",
            "CREATE INDEX I ON T (A) CREATE INDEX J ON T (B)",
            "CREATE VIEW V AS SELECT A FROM T",
            "COMMIT",
            "CREATE INDEX :I ON T (A)",
        ] {
            assert!(parse(sql).is_err(), "accepted {sql}");
        }
        assert!(parse("CREATE INDEX I ON T (A); /* end */ -- end\n").is_ok());
        assert!(parse("CREATE INDEX I ON T (A) /* end */").is_ok());
    }

    #[test]
    fn syntax_input_token_and_nesting_limits_remain_owned_by_lexer() {
        let sql = "CREATE INDEX I ON T (A)";
        let syntax = Db2SyntaxLimits {
            max_statement_bytes: sql.len(),
            max_tokens: 8,
            ..Db2SyntaxLimits::default()
        };
        assert!(parse_db2_create_index_statement(sql, syntax, Db2AstLimits::default()).is_ok());
        for (limits, code) in [
            (
                Db2SyntaxLimits {
                    max_statement_bytes: sql.len() - 1,
                    ..syntax
                },
                Db2SyntaxDiagnosticCode::StatementTooLarge,
            ),
            (
                Db2SyntaxLimits {
                    max_tokens: 7,
                    ..syntax
                },
                Db2SyntaxDiagnosticCode::TooManyTokens,
            ),
            (
                Db2SyntaxLimits {
                    max_token_bytes: 5,
                    ..Db2SyntaxLimits::default()
                },
                Db2SyntaxDiagnosticCode::TokenTooLarge,
            ),
            (
                Db2SyntaxLimits {
                    max_nesting: 1,
                    ..Db2SyntaxLimits::default()
                },
                Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
            ),
        ] {
            let source = if code == Db2SyntaxDiagnosticCode::UnbalancedDelimiter {
                "CREATE INDEX I ON T ((A))"
            } else {
                sql
            };
            assert_eq!(
                parse_db2_create_index_statement(source, limits, Db2AstLimits::default())
                    .unwrap_err()
                    .code,
                code
            );
        }
    }

    #[test]
    fn invalid_limits_and_source_fences_remain_errors() {
        let sql = "CREATE INDEX I ON T (A)";
        assert_eq!(
            with_ast(
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
        assert_eq!(
            parse_db2_create_index_statement(
                sql,
                Db2SyntaxLimits {
                    max_tokens: 0,
                    ..Db2SyntaxLimits::default()
                },
                Db2AstLimits::default()
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidLimits
        );
        for key in ["1E2", "1.0E+2", "DECFLOAT'1.2'"] {
            assert_eq!(
                parse(&format!("CREATE INDEX I ON T ({key})"))
                    .unwrap_err()
                    .code,
                Db2SyntaxDiagnosticCode::UnsupportedNumericConstant
            );
        }
        // Boolean/decfloat calls cannot escape the expression/index fence.
        for key in [
            "TRUE",
            "FALSE",
            "NULL",
            "DEFAULT",
            "TRUE + A",
            "DECFLOAT(A)",
        ] {
            assert!(parse(&format!("CREATE INDEX I ON T ({key})")).is_err());
        }
    }
}
