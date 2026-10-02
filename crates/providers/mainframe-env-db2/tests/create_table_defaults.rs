//! Public partial SQL0050 default intents; no default resolution or execution credit.

use mainframe_env_db2::{
    Db2AstLimits, Db2CreateTableStatement, Db2DefaultSpelling, Db2Literal, Db2SourceLocation,
    Db2SourceSpan, Db2StringKind, Db2SyntaxDiagnostic, Db2SyntaxDiagnosticCode, Db2SyntaxLimits,
    parse_db2_create_table_statement,
};

fn parse(source: &str) -> Result<Db2CreateTableStatement, Db2SyntaxDiagnostic> {
    parse_db2_create_table_statement(source, Default::default(), Default::default())
}

fn location(source: &str, byte: usize) -> Db2SourceLocation {
    let mut location = Db2SourceLocation::START;
    let mut previous_cr = false;
    for character in source[..byte].chars() {
        match character {
            '\r' => {
                location.line += 1;
                location.column = 1;
            }
            '\n' if previous_cr => {}
            '\n' => {
                location.line += 1;
                location.column = 1;
            }
            _ => location.column += 1,
        }
        previous_cr = character == '\r';
    }
    location
}

fn assert_span(source: &str, span: Db2SourceSpan, expected: &str) {
    assert_span_after(source, span, expected, 0);
}

fn assert_span_after(source: &str, span: Db2SourceSpan, expected: &str, search_start: usize) {
    let byte = search_start + source[search_start..].find(expected).unwrap();
    assert_eq!(span.start_byte, byte, "{expected:?}");
    assert_eq!(span.end_byte, byte + expected.len(), "{expected:?}");
    assert_eq!(span.start, location(source, byte));
    assert_eq!(span.end, location(source, byte + expected.len()));
    assert_eq!(&source[span.start_byte..span.end_byte], expected);
}

#[test]
fn omitted_type_default_and_explicit_null_remain_distinct_for_both_spellings() {
    for (spelling, expected) in [
        ("DEFAULT", Db2DefaultSpelling::Default),
        ("WITH DEFAULT", Db2DefaultSpelling::WithDefault),
    ] {
        for data_type in ["INT", "CHAR(4)", "DATE", "TIME", "TIMESTAMP(6)", "s.kind"] {
            let source = format!(
                "CREATE TABLE t (a {data_type}, b {data_type} {spelling}, c {data_type} {spelling} NULL)"
            );
            let statement = parse(&source).unwrap();
            assert!(statement.columns()[0].default().is_none());
            let default = statement.columns()[1].default().unwrap();
            assert_eq!(default.spelling(), expected);
            assert_eq!(default.value(), None);
            assert_span(&source, default.span(), spelling);
            assert_eq!(default.value_span(), None);
            assert_eq!(default.numeric_sign_span(), None);
            assert_eq!(default.numeric_token_span(), None);
            let null = statement.columns()[2].default().unwrap();
            assert_eq!(null.value(), Some(&Db2Literal::Null));
            assert_eq!(null.spelling(), expected);
            assert_span(&source, null.value_span().unwrap(), "NULL");
            assert_span(&source, null.span(), &format!("{spelling} NULL"));
            assert_eq!(null.numeric_sign_span(), None);
            assert_eq!(null.numeric_token_span(), None);
        }
    }
}

#[test]
fn literal_spellings_and_all_original_endpoints_survive_relocation() {
    for prefix in ["", " \t", "/*é*/\r\n", "--名\r\n", "/*名*/\r\n\r\n"] {
        for (clause, literal, operand, sign, number, spelling) in [
            (
                "DEFAULT 001.20",
                Db2Literal::Number("001.20".into()),
                "001.20",
                None,
                Some("001.20"),
                Db2DefaultSpelling::Default,
            ),
            (
                "WITH /*é*/ DEFAULT + \t12.50",
                Db2Literal::Number("+12.50".into()),
                "+ \t12.50",
                Some("+"),
                Some("12.50"),
                Db2DefaultSpelling::WithDefault,
            ),
            (
                "DEFAULT -/*名*/\r\n 12",
                Db2Literal::Number("-12".into()),
                "-/*名*/\r\n 12",
                Some("-"),
                Some("12"),
                Db2DefaultSpelling::Default,
            ),
            (
                "WITH DEFAULT +--名\r\n12",
                Db2Literal::Number("+12".into()),
                "+--名\r\n12",
                Some("+"),
                Some("12"),
                Db2DefaultSpelling::WithDefault,
            ),
            (
                "DEFAULT 'é''名'",
                Db2Literal::String {
                    kind: Db2StringKind::Character,
                    value: "é''名".into(),
                },
                "'é''名'",
                None,
                None,
                Db2DefaultSpelling::Default,
            ),
            (
                "WITH DEFAULT X'Ff00'",
                Db2Literal::String {
                    kind: Db2StringKind::Hex,
                    value: "Ff00".into(),
                },
                "X'Ff00'",
                None,
                None,
                Db2DefaultSpelling::WithDefault,
            ),
            (
                "DEFAULT NULL",
                Db2Literal::Null,
                "NULL",
                None,
                None,
                Db2DefaultSpelling::Default,
            ),
        ] {
            let body = format!("CREATE TABLE \"T\"\"名  \" (\"列\"\"x  \" VARCHAR(20) {clause});");
            let source = format!("{prefix}{body} -- tail");
            let statement = parse(&source).unwrap();
            assert_eq!(statement.table_name().parts()[0].value(), "T\"名");
            let column = &statement.columns()[0];
            assert_eq!(column.name().value(), "列\"x");
            assert_span(&source, statement.span(), &body);
            assert_span(
                &source,
                column.span(),
                &format!("\"列\"\"x  \" VARCHAR(20) {clause}"),
            );
            let default = column.default().unwrap();
            assert_eq!(default.value(), Some(&literal));
            assert_eq!(default.spelling(), spelling);
            assert_span(&source, default.span(), clause);
            assert_span(&source, default.value_span().unwrap(), operand);
            assert_eq!(default.numeric_sign_span().is_some(), sign.is_some());
            assert_eq!(default.numeric_token_span().is_some(), number.is_some());
            if let Some(sign) = sign {
                assert_span_after(
                    &source,
                    default.numeric_sign_span().unwrap(),
                    sign,
                    source.find(clause).unwrap(),
                );
            }
            if let Some(number) = number {
                assert_span(&source, default.numeric_token_span().unwrap(), number);
            }
        }
        let clause = "WITH /*é*/ DEFAULT";
        let source = format!("{prefix}CREATE TABLE t (a INT {clause} /*tail*/ NOT NULL)");
        let statement = parse(&source).unwrap();
        let default = statement.columns()[0].default().unwrap();
        assert_eq!(default.value(), None);
        assert_span(&source, default.span(), clause);
    }
}

#[test]
fn nullable_not_null_and_duplicate_clauses_are_order_independent() {
    for spelling in ["DEFAULT", "WITH DEFAULT"] {
        for options in [
            spelling.to_owned(),
            format!("NOT NULL {spelling}"),
            format!("{spelling} NOT NULL"),
            format!("NOT NULL {spelling} 7"),
            format!("{spelling} 7 NOT NULL"),
        ] {
            let source = format!("CREATE TABLE t (a INT {options})");
            let statement = parse(&source).unwrap();
            assert_eq!(
                statement.columns()[0].is_not_null(),
                options.contains("NOT NULL")
            );
        }
        for options in [
            format!("NOT NULL {spelling} NULL"),
            format!("{spelling} NULL NOT NULL"),
        ] {
            let source = format!("CREATE TABLE t (a INT {options})");
            assert_eq!(
                parse(&source).unwrap_err().code,
                Db2SyntaxDiagnosticCode::InvalidStatementOperand
            );
        }
        for other in ["DEFAULT", "WITH DEFAULT"] {
            for options in [
                format!("{spelling} {other}"),
                format!("{spelling} NULL {other}"),
                format!("{spelling} NOT NULL {other} 1"),
                format!("{spelling} 1 {other} 2"),
            ] {
                let source = format!("CREATE TABLE t (a INT {options})");
                assert_eq!(
                    parse(&source).unwrap_err().code,
                    Db2SyntaxDiagnosticCode::DuplicateClause,
                    "{source}"
                );
            }
        }
    }
    for source in [
        "CREATE TABLE t (a INT DEFAULT NOT NULL NOT NULL)",
        "CREATE TABLE t (a INT DEFAULT, \"A  \" INT)",
        "CREATE TABLE t (\"a\"\"b\" INT DEFAULT, \"a\"\"b  \" INT)",
    ] {
        assert_eq!(
            parse(source).unwrap_err().code,
            Db2SyntaxDiagnosticCode::DuplicateClause
        );
    }
    assert!(parse("CREATE TABLE t (a INT DEFAULT, \"a\" INT WITH DEFAULT)").is_ok());
}

#[test]
fn incomplete_malformed_and_deferred_operands_fail_with_locations() {
    for source in [
        "CREATE TABLE t (a INT DEFAULT",
        "CREATE TABLE t (a INT WITH DEFAULT",
        "CREATE TABLE t (a INT DEFAULT;",
        "CREATE TABLE t (a INT DEFAULT b INT)",
        "CREATE TABLE t (a INT DEFAULT,)",
        "CREATE TABLE t (a INT DEFAULT NOT)",
        "CREATE TABLE t (a INT WITH)",
        "CREATE TABLE t (a INT DEFAULT +)",
        "CREATE TABLE t (a INT DEFAULT -",
        "CREATE TABLE t (a INT DEFAULT + NULL)",
        "CREATE TABLE t (a INT DEFAULT - 'x')",
        "CREATE TABLE t (a INT DEFAULT ++1)",
        "CREATE TABLE t (a INT DEFAULT -+1)",
        "CREATE TABLE t (a INT DEFAULT (1))",
        "CREATE TABLE t (a INT DEFAULT 1 + 2)",
        "CREATE TABLE t (a INT DEFAULT :host)",
        "CREATE TABLE t (a INT DEFAULT ?)",
        "CREATE TABLE t (a INT DEFAULT CURRENT DATE)",
        "CREATE TABLE t (a INT DEFAULT CURRENT SQLID)",
        "CREATE TABLE t (a INT DEFAULT CURRENT_TIMESTAMP)",
        "CREATE TABLE t (a INT DEFAULT SESSION_USER)",
        "CREATE TABLE t (a INT DEFAULT USER)",
        "CREATE TABLE t (a INT DEFAULT ABS(1))",
        "CREATE TABLE t (a INT DEFAULT CAST(1 AS INT))",
        "CREATE TABLE t (a INT DEFAULT \"NULL\")",
        "CREATE TABLE t (a INT DEFAULT TRUE)",
        "CREATE TABLE t (a INT DEFAULT 1E3)",
        "CREATE TABLE t (a INT DEFAULT NAN)",
        "CREATE TABLE t (a INT DEFAULT); SELECT 1",
        "CREATE TABLE t (a INT DEFAULT) EXTRA",
        "CREATE TABLE t (a INT DEFAULT 'x)",
        "CREATE TABLE t (a INT DEFAULT /*unterminated)",
    ] {
        let prefix = "/*名*/\r\n";
        let plain = parse(source).unwrap_err();
        let relocated = parse(&format!("{prefix}{source}")).unwrap_err();
        assert_eq!(relocated.code, plain.code, "{source}");
        assert_eq!(relocated.location.line, plain.location.line + 1, "{source}");
        assert_eq!(relocated.location.column, plain.location.column, "{source}");
    }
    for (source, code) in [
        (
            "CREATE TABLE t (a INT DEFAULT +)",
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
        ),
        (
            "CREATE TABLE t (a INT DEFAULT",
            Db2SyntaxDiagnosticCode::UnbalancedDelimiter,
        ),
        (
            "CREATE TABLE t (a INT DEFAULT; SELECT 1)",
            Db2SyntaxDiagnosticCode::MissingToken,
        ),
        (
            "CREATE TABLE t (a INT DEFAULT 1E3)",
            Db2SyntaxDiagnosticCode::UnsupportedNumericConstant,
        ),
    ] {
        assert_eq!(parse(source).unwrap_err().code, code, "{source}");
    }
}

#[test]
fn exact_and_one_beyond_resource_bounds_keep_statement_wide_authorities() {
    let source = "CREATE TABLE t (a INT DEFAULT +12)";
    let syntax = Db2SyntaxLimits {
        max_statement_bytes: source.len(),
        max_tokens: 10,
        max_token_bytes: 7,
        max_nesting: 1,
    };
    assert!(parse_db2_create_table_statement(source, syntax, Default::default()).is_ok());
    assert_eq!(
        parse_db2_create_table_statement(&format!("{source} "), syntax, Default::default())
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::StatementTooLarge
    );
    for (limits, code) in [
        (
            Db2SyntaxLimits {
                max_tokens: 9,
                ..syntax
            },
            Db2SyntaxDiagnosticCode::TooManyTokens,
        ),
        (
            Db2SyntaxLimits {
                max_token_bytes: 6,
                ..syntax
            },
            Db2SyntaxDiagnosticCode::TokenTooLarge,
        ),
    ] {
        assert_eq!(
            parse_db2_create_table_statement(source, limits, Default::default())
                .unwrap_err()
                .code,
            code
        );
    }
    for (operand, literal_bytes) in [("+12", 3), ("- /*名*/ 12", 3), ("'é''x'", 5)] {
        let source = format!("CREATE TABLE t (a INT DEFAULT {operand})");
        let limits = Db2AstLimits {
            max_literal_bytes: literal_bytes,
            ..Default::default()
        };
        assert!(parse_db2_create_table_statement(&source, Default::default(), limits).is_ok());
        assert_eq!(
            parse_db2_create_table_statement(
                &source,
                Default::default(),
                Db2AstLimits {
                    max_literal_bytes: literal_bytes - 1,
                    ..limits
                }
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
    }
    let limits = Db2AstLimits {
        max_identifier_bytes: 4,
        max_list_items: 3,
        max_expression_nodes: 1,
        max_expression_depth: 1,
        ..Default::default()
    };
    let source =
        "CREATE TABLE t (\"a\"\"\"\"b   \" INT DEFAULT, b INT WITH DEFAULT, c INT DEFAULT NULL)";
    let statement = parse_db2_create_table_statement(source, Default::default(), limits).unwrap();
    assert_eq!(statement.columns()[0].name().value(), "a\"\"b");
    for source in [
        "CREATE TABLE t (\"a\"\"\"\"bx\" INT DEFAULT)",
        "CREATE TABLE t (a INT DEFAULT, b INT DEFAULT, c INT DEFAULT, d INT DEFAULT)",
        "CREATE TABLE t (a INT NOT NULL DEFAULT, b INT NOT NULL DEFAULT, UNIQUE(a), UNIQUE(b))",
    ] {
        assert_eq!(
            parse_db2_create_table_statement(source, Default::default(), limits)
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            "{source}"
        );
    }
    assert!(
        parse_db2_create_table_statement(
            "CREATE TABLE t (a INT NOT NULL DEFAULT, b INT NOT NULL DEFAULT, UNIQUE(a,b))",
            Default::default(),
            limits
        )
        .is_ok()
    );
    let source = "CREATE TABLE t (a INT DEFAULT, b INT DEFAULT)";
    // 12 tokens across both columns; neither DEFAULT resets the lexer budget.
    assert!(
        parse_db2_create_table_statement(
            source,
            Db2SyntaxLimits {
                max_tokens: 12,
                ..Default::default()
            },
            limits
        )
        .is_ok()
    );
    assert_eq!(
        parse_db2_create_table_statement(
            source,
            Db2SyntaxLimits {
                max_tokens: 11,
                ..Default::default()
            },
            limits
        )
        .unwrap_err()
        .code,
        Db2SyntaxDiagnosticCode::TooManyTokens
    );
}

#[test]
fn default_outputs_own_names_literals_and_spans_after_source_drop() {
    let statement = {
        let source = String::from(
            "CREATE TABLE \"T\"\"x\" (a INT, b INT WITH DEFAULT, c INT DEFAULT NULL, d CHAR(4) DEFAULT 'a''b', e INT DEFAULT - /*é*/ 12)",
        );
        parse(&source).unwrap()
    };
    let cloned = statement.clone();
    assert_eq!(cloned, statement);
    assert_eq!(cloned.table_name().parts()[0].value(), "T\"x");
    assert!(cloned.columns()[0].default().is_none());
    assert_eq!(cloned.columns()[1].default().unwrap().value(), None);
    assert_eq!(
        cloned.columns()[2].default().unwrap().value(),
        Some(&Db2Literal::Null)
    );
    assert_eq!(
        cloned.columns()[3].default().unwrap().value(),
        Some(&Db2Literal::String {
            kind: Db2StringKind::Character,
            value: "a''b".into()
        })
    );
    let numeric = cloned.columns()[4].default().unwrap();
    assert_eq!(numeric.value(), Some(&Db2Literal::Number("-12".into())));
    assert!(
        numeric.value_span().unwrap().start_byte < numeric.numeric_token_span().unwrap().start_byte
    );
    assert_eq!(
        numeric.value_span().unwrap().end,
        numeric.numeric_token_span().unwrap().end
    );
}
