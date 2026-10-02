//! Public pure syntax contracts, without official row or execution credit.

use mainframe_env_db2::{
    Db2AstLimits, Db2ExpressionKind, Db2IndexKeyOrder, Db2IndexUniqueness, Db2InsertValue,
    Db2SourceLocation, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, Db2ViewCheckMode,
    parse_db2_create_index_statement, parse_db2_create_view_statement, parse_db2_insert_values,
};

#[test]
fn public_insert_owns_rows_and_relocated_expression_nodes() {
    let source = "INSERT INTO schema.t (a,b,c) VALUES (1 + 2,DEFAULT,NULL), (3,4,5);";
    let statement =
        parse_db2_insert_values(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
            .unwrap();
    assert_eq!(statement.target().parts()[0].value(), "SCHEMA");
    assert_eq!(statement.columns().unwrap().len(), 3);
    assert_eq!(statement.rows().len(), 2);
    let Db2InsertValue::Expression(expression) = &statement.rows()[0].values()[0] else {
        panic!("expected an owned expression");
    };
    let root = expression.arena().get(expression.root()).unwrap();
    assert_eq!(
        &source[root.span().start_byte..root.span().end_byte],
        "1 + 2"
    );
    assert!(matches!(
        statement.rows()[0].values()[1],
        Db2InsertValue::Default(_)
    ));
    assert!(matches!(
        statement.rows()[0].values()[2],
        Db2InsertValue::Null(_)
    ));
    assert_eq!(statement.span().end_byte, source.len());

    let owned = {
        let source = String::from(r#"INSERT INTO "t""q" ("a""b") VALUES (1)"#);
        parse_db2_insert_values(&source, Db2SyntaxLimits::default(), Db2AstLimits::default())
            .unwrap()
    };
    assert_eq!(owned.target().parts()[0].value(), "t\"q");
    assert_eq!(owned.columns().unwrap()[0].value(), "a\"b");
}

#[test]
fn public_index_preserves_uniqueness_order_and_key_spans() {
    let source = "CREATE UNIQUE WHERE NOT NULL INDEX s.i ON s.t (a ASC,b DESC,c RANDOM);";
    let statement = parse_db2_create_index_statement(
        source,
        Db2SyntaxLimits::default(),
        Db2AstLimits::default(),
    )
    .unwrap();
    assert_eq!(
        statement.uniqueness(),
        Db2IndexUniqueness::UniqueWhereNotNull
    );
    assert_eq!(statement.index_name().parts()[1].value(), "I");
    assert_eq!(statement.table_name().parts()[1].value(), "T");
    assert_eq!(
        statement
            .keys()
            .iter()
            .map(|key| key.order())
            .collect::<Vec<_>>(),
        [
            Db2IndexKeyOrder::Asc,
            Db2IndexKeyOrder::Desc,
            Db2IndexKeyOrder::Random
        ]
    );
    for (key, spelling) in statement.keys().iter().zip(["a ASC", "b DESC", "c RANDOM"]) {
        assert_eq!(
            &source[key.span().start_byte..key.span().end_byte],
            spelling
        );
    }
}

#[test]
fn public_view_owns_decoded_names_and_original_create_locations() {
    let source =
        "CREATE VIEW schema.v (x) AS\nSELECT \"a\"\"b\" FROM \"t\"\"q\" WITH LOCAL CHECK OPTION;";
    let statement = parse_db2_create_view_statement(
        source,
        Db2SyntaxLimits::default(),
        Db2AstLimits::default(),
    )
    .unwrap();
    assert_eq!(statement.view_name().value().parts()[1].value(), "V");
    assert_eq!(statement.result_columns().unwrap()[0].value().value(), "X");
    assert_eq!(
        statement.check_option().unwrap().value(),
        &Db2ViewCheckMode::Local
    );
    assert_eq!(
        statement.definition().sources()[0].value().parts()[0].value(),
        "t\"q"
    );
    let expression = statement.definition().items()[0].value();
    let root = expression.arena().get(expression.root()).unwrap();
    let Db2ExpressionKind::Column(column) = root.kind() else {
        panic!("expected a transferred column expression");
    };
    assert_eq!(column.parts()[0].value(), "a\"b");
    assert_eq!(
        &source[root.span().start_byte..root.span().end_byte],
        "\"a\"\"b\""
    );
    assert_eq!(root.span().start, Db2SourceLocation { line: 2, column: 8 });
    assert_eq!(statement.span().end_byte, source.len());
}

#[test]
fn public_insert_rejects_nested_column_references_and_width_drift() {
    for source in [
        "INSERT INTO t VALUES (F(a))",
        "INSERT INTO t VALUES (CAST(a AS INTEGER))",
        "INSERT INTO t VALUES (1),(1,2)",
        "INSERT INTO t (a,b) VALUES (1)",
        "INSERT INTO t VALUES (1);;",
    ] {
        assert!(
            parse_db2_insert_values(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
                .is_err()
        );
    }
    let problem = parse_db2_insert_values(
        "INSERT INTO t (a,\"A\") VALUES (1,2)",
        Db2SyntaxLimits::default(),
        Db2AstLimits::default(),
    )
    .unwrap_err();
    assert_eq!(problem.code, Db2SyntaxDiagnosticCode::DuplicateClause);
}

#[test]
fn public_index_and_view_keep_unsupported_families_explicit() {
    for source in [
        "CREATE INDEX i ON t (a,\"A \")",
        "CREATE INDEX i ON t (F(a))",
        "CREATE INDEX i ON t (a) INCLUDE (b)",
    ] {
        assert!(
            parse_db2_create_index_statement(
                source,
                Db2SyntaxLimits::default(),
                Db2AstLimits::default()
            )
            .is_err()
        );
    }
    for source in [
        "CREATE VIEW v AS SELECT * FROM t",
        "CREATE VIEW v AS SELECT a,a FROM t",
        "CREATE VIEW v(x) AS SELECT UNPACK(a) FROM t",
        "CREATE VIEW v(x) AS SELECT ? FROM t",
    ] {
        assert!(
            parse_db2_create_view_statement(
                source,
                Db2SyntaxLimits::default(),
                Db2AstLimits::default()
            )
            .is_err()
        );
    }
    let problem = parse_db2_create_view_statement(
        "-- lead\nCREATE VIEW v(x) AS\nSELECT :h FROM t",
        Db2SyntaxLimits::default(),
        Db2AstLimits::default(),
    )
    .unwrap_err();
    assert_eq!(
        problem.code,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );
    assert_eq!(problem.location, Db2SourceLocation { line: 3, column: 8 });
}

#[test]
fn public_syntax_enforces_configured_aggregate_budgets() {
    let nodes = Db2AstLimits {
        max_expression_nodes: 1,
        ..Db2AstLimits::default()
    };
    assert!(
        parse_db2_insert_values(
            "INSERT INTO t VALUES (1,2)",
            Db2SyntaxLimits::default(),
            nodes
        )
        .is_err()
    );
    let keys = Db2AstLimits {
        max_list_items: 1,
        ..Db2AstLimits::default()
    };
    assert!(
        parse_db2_create_index_statement(
            "CREATE INDEX i ON t (a,b)",
            Db2SyntaxLimits::default(),
            keys
        )
        .is_err()
    );
    let view = Db2AstLimits {
        max_list_items: 2,
        ..Db2AstLimits::default()
    };
    assert!(
        parse_db2_create_view_statement(
            "CREATE VIEW v(x) AS SELECT a FROM t",
            Db2SyntaxLimits::default(),
            view
        )
        .is_err()
    );
}
