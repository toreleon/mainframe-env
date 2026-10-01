//! Recovered SELECT core regressions.

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
             WHERE E.ACTIVE = 1 GROUP BY E.DEPT HAVING AVG(E.SALARY) > 10\n\
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

    let host = parse("SELECT :HV FROM T WHERE A = :LOOKUP").unwrap();
    let selected = expression(&host.items()[0]);
    assert!(matches!(
        selected
            .parsed()
            .arena()
            .get(selected.parsed().root())
            .unwrap()
            .kind(),
        Db2ExpressionKind::HostVariable(_)
    ));
}

#[test]
fn optional_clauses_are_independently_present_or_absent() {
    let bare = parse("SELECT A FROM T").unwrap();
    assert!(bare.where_condition().is_none());
    assert!(bare.group_by().is_empty());
    assert!(bare.having().is_none());
    assert!(bare.order_by().is_empty());
    assert!(bare.offset().is_none());
    assert!(bare.fetch().is_none());

    let clauses = [
        "SELECT A FROM T WHERE A = 1",
        "SELECT A FROM T GROUP BY A",
        "SELECT A FROM T HAVING COUNT(A) > 0",
        "SELECT A FROM T ORDER BY A",
        "SELECT A FROM T OFFSET 0 ROWS",
        "SELECT A FROM T FETCH FIRST ROW ONLY",
    ];
    for (index, source) in clauses.iter().enumerate() {
        let query = parse(source).unwrap();
        let present = [
            query.where_condition().is_some(),
            !query.group_by().is_empty(),
            query.having().is_some(),
            !query.order_by().is_empty(),
            query.offset().is_some(),
            query.fetch().is_some(),
        ];
        assert_eq!(
            present.iter().filter(|value| **value).count(),
            1,
            "{source}"
        );
        assert!(present[index], "{source}");
    }
}

#[test]
fn where_and_having_require_search_conditions() {
    for source in ["SELECT A FROM T WHERE A", "SELECT A FROM T HAVING A + 1"] {
        let problem = parse(source).unwrap_err();
        assert_eq!(
            problem.code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
        assert!(problem.location.column > 1);
    }
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
        "SELECT A FROM T FOR READ ONLY",
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
    assert_eq!(
        parse("SELECT A INTO :OUT FROM T")
            .unwrap_err()
            .location
            .column,
        10
    );
    assert_eq!(
        parse("VALUES 1 INTO :OUT").unwrap_err().code,
        Db2SyntaxDiagnosticCode::UnsupportedStatement
    );
    assert_eq!(
        parse("SELECT TRUE FROM T").unwrap_err().code,
        Db2SyntaxDiagnosticCode::UnsupportedStatement
    );
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
    let source =
        "-- lambda λ\nSELECT \"Écart\" + 1, \"表\".* FROM \"模式\".\"表\"\nWHERE \"表\".\"列\" > 0";
    let query = parse(source).unwrap();
    assert_eq!(query.span().start, Db2SourceLocation { line: 2, column: 1 });
    assert_eq!(
        query.span().end,
        Db2SourceLocation {
            line: 3,
            column: 18
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

    let problem = parse("SELECT A FROM \"表\"\nWHERE \"列\" +").unwrap_err();
    assert_eq!(problem.code, Db2SyntaxDiagnosticCode::MissingToken);
    assert_eq!(problem.location.line, 2);
    assert_eq!(problem.location.column, 12);
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
        "SELECT A FROM T FETCH FIRST 18446744073709551616 ROWS ONLY",
    ] {
        assert_eq!(
            parse(source).unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand
        );
    }
    assert_eq!(
        parse("SELECT A FROM T ORDER BY 4294967295")
            .unwrap()
            .order_by()[0]
            .key(),
        &Db2OrderKey::Ordinal(u32::MAX)
    );
    assert_eq!(
        parse("SELECT A FROM T OFFSET 18446744073709551615 ROWS")
            .unwrap()
            .offset()
            .unwrap()
            .row_count(),
        u64::MAX
    );
    assert_eq!(
        parse("SELECT A FROM T FETCH FIRST 18446744073709551615 ROWS ONLY")
            .unwrap()
            .fetch()
            .unwrap()
            .explicit_row_count(),
        Some(u64::MAX)
    );
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
    for (source, limits) in [
        (
            "SELECT A FROM T",
            Db2SyntaxLimits {
                max_token_bytes: 2,
                ..Db2SyntaxLimits::default()
            },
        ),
        (
            "SELECT ((A)) FROM T",
            Db2SyntaxLimits {
                max_nesting: 1,
                ..Db2SyntaxLimits::default()
            },
        ),
    ] {
        assert!(
            parse_db2_select_core(source, limits, Db2AstLimits::default()).is_err(),
            "bound did not reject: {source}"
        );
    }
    for (source, limits) in [
        (
            "SELECT 'AB' FROM T",
            Db2AstLimits {
                max_literal_bytes: 1,
                ..Db2AstLimits::default()
            },
        ),
        (
            "SELECT A + 1 FROM T",
            Db2AstLimits {
                max_expression_depth: 1,
                ..Db2AstLimits::default()
            },
        ),
    ] {
        assert!(
            parse_db2_select_core(source, Db2SyntaxLimits::default(), limits).is_err(),
            "AST bound did not reject: {source}"
        );
    }
}
