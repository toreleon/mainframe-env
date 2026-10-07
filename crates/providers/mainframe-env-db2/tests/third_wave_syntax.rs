//! Public syntax contracts only: binding and execution remain separate.

use mainframe_env_db2::{
    Db2AstLimits, Db2BinaryOperator, Db2CursorHostOperands, Db2CursorOperationKind,
    Db2ExpressionKind, Db2FetchOrientation, Db2SourceSpan, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2UpdateValue, parse_db2_cursor_operation_statement,
    parse_db2_searched_delete, parse_db2_searched_update,
};

fn spelling(source: &str, span: Db2SourceSpan) -> &str {
    &source[span.start_byte..span.end_byte]
}

#[test]
fn delete_owns_decoded_names_and_original_predicate_spans() {
    let source =
        "/*é*/\r\nDELETE FROM \"S\".\"T\"\"1\" WHERE NOT (\"A\"\"B\" = :Ws-Val) OR C IS NULL;";
    let statement =
        parse_db2_searched_delete(source, Default::default(), Default::default()).unwrap();
    assert_eq!(statement.target().value().parts()[1].value(), "T\"1");
    assert_eq!(
        spelling(source, statement.target().span()),
        "\"S\".\"T\"\"1\""
    );
    let condition = statement.where_condition().unwrap();
    assert!(matches!(
        condition.arena().get(condition.root()).unwrap().kind(),
        Db2ExpressionKind::Binary {
            operator: Db2BinaryOperator::Or,
            ..
        }
    ));
    assert_eq!(condition.span().start.line, 2);
    assert_eq!(
        spelling(source, condition.span()),
        "NOT (\"A\"\"B\" = :Ws-Val) OR C IS NULL"
    );
    for node in condition.arena().nodes() {
        assert!(!spelling(source, node.span()).is_empty());
        assert_eq!(node.span().start.line, 2);
    }
    let owned = {
        let local = String::from("DELETE FROM S.T");
        parse_db2_searched_delete(&local, Default::default(), Default::default()).unwrap()
    };
    assert!(owned.where_condition().is_none());
    assert_eq!(owned.target().value().parts()[1].value(), "T");
}

#[test]
fn update_preserves_values_names_and_complete_source_locations() {
    let source = "/*é*/\r\nUPDATE S.T SET \"A\"\"B\" = CASE WHEN X = 1 THEN :Ws-Val ELSE 0 END, C = DEFAULT, D = NULL WHERE X IS NOT NULL;";
    let statement =
        parse_db2_searched_update(source, Default::default(), Default::default()).unwrap();
    assert_eq!(statement.target().parts()[1].value(), "T");
    assert_eq!(spelling(source, statement.target_span()), "S.T");
    assert_eq!(statement.assignments()[0].column().value(), "A\"B");
    let Db2UpdateValue::Expression(expression) = statement.assignments()[0].value() else {
        panic!("owned CASE expression")
    };
    assert!(matches!(
        expression.arena().get(expression.root()).unwrap().kind(),
        Db2ExpressionKind::Case { .. }
    ));
    assert!(spelling(source, expression.span()).starts_with("CASE WHEN"));
    for node in expression.arena().nodes() {
        assert_eq!(node.span().start.line, 2);
        assert!(!spelling(source, node.span()).is_empty());
    }
    assert!(matches!(
        statement.assignments()[1].value(),
        Db2UpdateValue::Default(_)
    ));
    assert!(matches!(
        statement.assignments()[2].value(),
        Db2UpdateValue::Null(_)
    ));
    assert_eq!(
        spelling(source, statement.where_condition().unwrap().span()),
        "X IS NOT NULL"
    );
    assert!(
        parse_db2_searched_update("UPDATE T SET A = 1", Default::default(), Default::default())
            .unwrap()
            .where_condition()
            .is_none()
    );
}

#[test]
fn open_and_fetch_own_host_leaves_descriptors_and_omitted_orientation() {
    let source = "OPEN \"C\"\"1\" USING :Ws-Val INDICATOR :Ws-Ind, :Ws-Val";
    let statement =
        parse_db2_cursor_operation_statement(source, Default::default(), Default::default())
            .unwrap();
    assert_eq!(statement.cursor_name().value(), "C\"1");
    let Db2CursorOperationKind::Open {
        using:
            Db2CursorHostOperands::Variables {
                references,
                clause_span,
            },
    } = statement.kind()
    else {
        panic!("USING list")
    };
    assert_eq!(references.len(), 2); // Repeated OPEN inputs are legal.
    assert_eq!(references[0].reference().variable().value(), "Ws-Val");
    assert_eq!(
        references[0].reference().indicator().unwrap().value(),
        "Ws-Ind"
    );
    assert!(spelling(source, *clause_span).starts_with("USING"));
    assert_eq!(
        spelling(source, references[0].indicator_keyword_span().unwrap()),
        "INDICATOR"
    );
    assert_eq!(spelling(source, references[0].variable_span()), ":Ws-Val");
    let owned = {
        let local = String::from("FETCH FROM C INTO DESCRIPTOR :SqlDa");
        parse_db2_cursor_operation_statement(&local, Default::default(), Default::default())
            .unwrap()
    };
    let Db2CursorOperationKind::Fetch {
        orientation,
        orientation_span,
        from_span,
        into: Db2CursorHostOperands::Descriptor { name, .. },
    } = owned.kind()
    else {
        panic!("FETCH descriptor")
    };
    assert_eq!(*orientation, Db2FetchOrientation::Unspecified);
    assert_eq!(orientation.effective(), Db2FetchOrientation::Next);
    assert!(orientation_span.is_none());
    assert!(from_span.is_some());
    assert_eq!(name.value(), "SqlDa");
    let positioned = parse_db2_cursor_operation_statement(
        "FETCH CURRENT C",
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(matches!(
        positioned.kind(),
        Db2CursorOperationKind::Fetch {
            orientation: Db2FetchOrientation::Current,
            into: Db2CursorHostOperands::None,
            ..
        }
    ));
}

#[test]
fn duplicate_effective_columns_and_exact_fetch_targets_fail() {
    for source in [
        "UPDATE T SET A = 1, a = 2",
        "UPDATE T SET A = 1, \"A \" = 2",
    ] {
        assert_eq!(
            parse_db2_searched_update(source, Default::default(), Default::default())
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::DuplicateClause
        );
    }
    assert_eq!(
        parse_db2_cursor_operation_statement(
            "FETCH C INTO :Ws-Val, :Ws-Val",
            Default::default(),
            Default::default()
        )
        .unwrap_err()
        .code,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );
}

#[test]
fn public_syntax_rejects_deferred_and_invalid_nested_forms() {
    for source in [
        "DELETE FROM T WHERE CURRENT OF C",
        "DELETE FROM T WHERE (1 AND 2) = 3",
        "DELETE FROM T FETCH FIRST 1 ROW ONLY",
        "DELETE FROM T; DELETE FROM U",
    ] {
        assert!(
            parse_db2_searched_delete(source, Default::default(), Default::default()).is_err(),
            "{source}"
        );
    }
    for source in [
        "UPDATE T SET A = SUM(B)",
        "UPDATE T SET A = (1 AND 2) + 3",
        "UPDATE T SET A = 1 WHERE 1 OR 2",
        "UPDATE T SET (A, B) = (1, 2)",
        "UPDATE T SET A = 1 WHERE CURRENT OF C",
    ] {
        assert!(
            parse_db2_searched_update(source, Default::default(), Default::default()).is_err(),
            "{source}"
        );
    }
    for source in [
        "FETCH ABSOLUTE 1 C INTO :A",
        "FETCH C FOR 2 ROWS INTO :A",
        "OPEN C USING DESCRIPTOR :D :I",
        "CLOSE C",
        "FETCH C INTO :A; OPEN C",
    ] {
        assert!(
            parse_db2_cursor_operation_statement(source, Default::default(), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn statement_wide_budgets_remain_public_and_bounded() {
    let limits = Db2AstLimits {
        max_list_items: 2,
        ..Default::default()
    };
    assert!(
        parse_db2_searched_delete(
            "DELETE FROM T WHERE F(A, B) = G(C)",
            Default::default(),
            limits
        )
        .is_err()
    );
    assert!(
        parse_db2_searched_update("UPDATE T SET A = F(1), B = 2", Default::default(), limits)
            .is_err()
    );
    assert!(
        parse_db2_cursor_operation_statement("OPEN C USING :A, :B, :C", Default::default(), limits)
            .is_err()
    );
    let nodes = Db2AstLimits {
        max_expression_nodes: 2,
        ..Default::default()
    };
    assert!(
        parse_db2_searched_update(
            "UPDATE T SET A = 1, B = NULL, C = DEFAULT",
            Default::default(),
            nodes
        )
        .is_err()
    );
    let source = Db2SyntaxLimits {
        max_statement_bytes: 5,
        ..Default::default()
    };
    assert_eq!(
        parse_db2_searched_delete("DELETE FROM T", source, Default::default())
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::StatementTooLarge
    );
    let decoded = Db2AstLimits {
        max_identifier_bytes: 3,
        ..Default::default()
    };
    assert!(
        parse_db2_searched_update("UPDATE T SET \"A\"\"B\" = 1", Default::default(), decoded)
            .is_ok()
    );
    assert!(
        parse_db2_cursor_operation_statement("OPEN \"A\"\"B\"", Default::default(), decoded)
            .is_ok()
    );
}
