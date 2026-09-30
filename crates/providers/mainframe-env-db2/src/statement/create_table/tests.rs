use super::*;

fn parse(source: &str) -> Result<Db2CreateTableStatement, Db2SyntaxDiagnostic> {
    parse_db2_create_table_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
}

fn foreign(constraint: &Db2CreateTableConstraint) -> &Db2ForeignKeyConstraint {
    let Db2TableConstraintKind::ForeignKey(foreign) = constraint.kind() else {
        panic!("expected FOREIGN KEY")
    };
    foreign
}

#[test]
fn create_table_builds_an_owned_bounded_ast() {
    let statement = parse(
        "CREATE TABLE app.orders (\
             id INTEGER NOT NULL, \
             amount DECIMAL(9,2) WITH DEFAULT -12.5, \
             state app.order_state WITH DEFAULT 'new', \
             note VARCHAR(20) DEFAULT NULL)",
    )
    .unwrap();
    assert_eq!(statement.table_name().parts()[0].value(), "APP");
    assert_eq!(statement.table_name().parts()[1].value(), "ORDERS");
    assert_eq!(statement.columns().len(), 4);
    assert!(statement.columns()[0].is_not_null());
    let Db2DataType::BuiltIn(decimal) = statement.columns()[1].data_type() else {
        panic!("expected DECIMAL")
    };
    assert_eq!(decimal.kind(), Db2BuiltInType::Decimal);
    assert_eq!(decimal.arguments(), &[9, 2]);
    assert_eq!(
        statement.columns()[1].default().unwrap().value(),
        &Db2Literal::Number("-12.5".into())
    );
    assert_eq!(
        statement.columns()[1].default().unwrap().spelling(),
        Db2DefaultSpelling::WithDefault
    );
    assert!(matches!(
        statement.columns()[2].data_type(),
        Db2DataType::Distinct(name) if name.parts().len() == 2
    ));
    assert_eq!(
        statement.columns()[2].default().unwrap().spelling(),
        Db2DefaultSpelling::WithDefault
    );
    assert_eq!(
        statement.columns()[3].default().unwrap().value(),
        &Db2Literal::Null
    );
    assert!(statement.constraints().is_empty());
}

#[test]
fn built_in_aliases_and_time_zone_shape_match_owned_type_syntax() {
    let statement = parse(
        "CREATE TABLE types (\
             a SMALLINT, b INT, c BIGINT, d NUMERIC(7,3), e FLOAT(24), \
             f REAL, g DOUBLE PRECISION, h DECFLOAT(16), \
             i CHARACTER VARYING(12), j BINARY VARYING(8), \
             k TIMESTAMP(9) WITH TIME ZONE, l TIME WITHOUT TIME ZONE, \
             m CLOB(20), n GRAPHIC(4), o VARGRAPHIC(5), p DBCLOB(6), \
             q VARBINARY(7), r BLOB(8), s DATE, t ROWID, u XML)",
    )
    .unwrap();
    assert_eq!(statement.columns().len(), 21);
    let Db2DataType::BuiltIn(varying) = statement.columns()[8].data_type() else {
        panic!("expected varying character")
    };
    assert_eq!(varying.kind(), Db2BuiltInType::VarChar);
    let Db2DataType::BuiltIn(timestamp) = statement.columns()[10].data_type() else {
        panic!("expected timestamp")
    };
    assert!(timestamp.with_time_zone());
    let Db2DataType::BuiltIn(time) = statement.columns()[11].data_type() else {
        panic!("expected time")
    };
    assert!(!time.with_time_zone());
}

#[test]
fn named_table_constraints_and_references_are_preserved() {
    let statement = parse(
        "CREATE TABLE app.child (\
             id INTEGER NOT NULL, code CHAR(4) NOT NULL, parent_id INTEGER NOT NULL, \
             optional_parent INTEGER, \
             CONSTRAINT pk_child PRIMARY KEY (id), \
             CONSTRAINT uq_child UNIQUE (code), \
             CONSTRAINT fk_child FOREIGN KEY (parent_id) \
               REFERENCES owner.parent (id) ON DELETE CASCADE, \
             FOREIGN KEY (optional_parent) REFERENCES parent ON DELETE SET NULL)",
    )
    .unwrap();
    assert_eq!(statement.constraints().len(), 4);
    assert_eq!(
        statement.constraints()[0].name().unwrap().value(),
        "PK_CHILD"
    );
    assert!(matches!(
        statement.constraints()[0].kind(),
        Db2TableConstraintKind::PrimaryKey(columns) if columns[0].value() == "ID"
    ));
    assert!(matches!(
        statement.constraints()[1].kind(),
        Db2TableConstraintKind::Unique(columns) if columns[0].value() == "CODE"
    ));
    let first = foreign(&statement.constraints()[2]);
    assert_eq!(first.referenced_table().parts().len(), 2);
    assert_eq!(first.referenced_columns().unwrap()[0].value(), "ID");
    assert_eq!(first.on_delete(), Some(Db2OnDeleteAction::Cascade));
    let second = foreign(&statement.constraints()[3]);
    assert!(second.referenced_columns().is_none());
    assert_eq!(second.on_delete(), Some(Db2OnDeleteAction::SetNull));
}

#[test]
fn every_common_on_delete_action_is_typed() {
    for (spelling, expected) in [
        ("RESTRICT", Db2OnDeleteAction::Restrict),
        ("NO ACTION", Db2OnDeleteAction::NoAction),
        ("CASCADE", Db2OnDeleteAction::Cascade),
        ("SET NULL", Db2OnDeleteAction::SetNull),
    ] {
        let source = format!(
            "CREATE TABLE child (id INT, FOREIGN KEY (id) REFERENCES parent (id) ON DELETE {spelling})"
        );
        let statement = parse(&source).unwrap();
        assert_eq!(
            foreign(&statement.constraints()[0]).on_delete(),
            Some(expected)
        );
    }
    let statement =
        parse("CREATE TABLE child (id INT, FOREIGN KEY (id) REFERENCES parent (id))").unwrap();
    assert_eq!(foreign(&statement.constraints()[0]).on_delete(), None);
}

#[test]
fn unsupported_column_and_table_forms_fail_explicitly() {
    for source in [
        "CREATE TABLE t (a INT GENERATED ALWAYS AS IDENTITY)",
        "CREATE TABLE t (a CHAR(8) NOT NULL DEFAULT 'x' AS SECURITY LABEL)",
        "CREATE TABLE t (a INT IMPLICITLY HIDDEN)",
        "CREATE TABLE t (a INT FIELDPROC p)",
        "CREATE TABLE t (a INT INLINE LENGTH 4)",
        "CREATE TABLE t (a INT PRIMARY KEY)",
        "CREATE TABLE t (a INT UNIQUE)",
        "CREATE TABLE t (a INT REFERENCES parent(id))",
        "CREATE TABLE t (a INT, CHECK (a > 0))",
        "CREATE TABLE t (a DATE, b DATE, PERIOD FOR BUSINESS_TIME (a,b))",
    ] {
        assert_eq!(
            parse(source).unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "{source}"
        );
    }
    for source in [
        "CREATE TABLE t LIKE source",
        "CREATE TABLE t (a INT) AS (SELECT 1) WITH NO DATA",
        "CREATE TABLE t (a INT) MATERIALIZED QUERY",
        "CREATE TABLE t (a INT) IN db.ts",
        "CREATE TABLE t (a INT) PARTITION BY SIZE",
        "CREATE TABLE t (a INT) ORGANIZE BY HASH a",
        "CREATE TABLE t (a INT) COMPRESS YES",
    ] {
        assert_eq!(
            parse(source).unwrap_err().code,
            Db2SyntaxDiagnosticCode::UnsupportedStatement,
            "{source}"
        );
    }
}

#[test]
fn duplicate_columns_options_names_and_keys_are_rejected() {
    for source in [
        "CREATE TABLE t (a INT, A INT)",
        "CREATE TABLE t (a INT NOT NULL NOT NULL)",
        "CREATE TABLE t (a INT DEFAULT 1 WITH DEFAULT 2)",
        "CREATE TABLE t (a INT NOT NULL, CONSTRAINT p PRIMARY KEY(a), CONSTRAINT p UNIQUE(a))",
        "CREATE TABLE t (a INT NOT NULL, PRIMARY KEY(a), PRIMARY KEY(a))",
        "CREATE TABLE t (a INT NOT NULL, UNIQUE(a), UNIQUE(a))",
        "CREATE TABLE t (a INT, UNIQUE(a,a))",
        "CREATE TABLE t (a INT, FOREIGN KEY(a) REFERENCES p(id), FOREIGN KEY(a) REFERENCES p(other))",
    ] {
        assert_eq!(
            parse(source).unwrap_err().code,
            Db2SyntaxDiagnosticCode::DuplicateClause,
            "{source}"
        );
    }
}

#[test]
fn semantic_conflicts_visible_inside_the_definition_are_rejected() {
    for source in [
        "CREATE TABLE t (a INT NOT NULL DEFAULT NULL)",
        "CREATE TABLE t (a INT DEFAULT NULL NOT NULL)",
        "CREATE TABLE t (a INT, PRIMARY KEY(a))",
        "CREATE TABLE t (a INT, UNIQUE(a))",
        "CREATE TABLE t (a INT, FOREIGN KEY(missing) REFERENCES p(id))",
        "CREATE TABLE t (a INT, FOREIGN KEY(a) REFERENCES p(id, other))",
        "CREATE TABLE t (a INT NOT NULL, FOREIGN KEY(a) REFERENCES p(id) ON DELETE SET NULL)",
    ] {
        assert_eq!(
            parse(source).unwrap_err().code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            "{source}"
        );
    }
}

#[test]
fn malformed_and_missing_operands_fail_closed() {
    for source in [
        "CREATE",
        "CREATE TABLE",
        "CREATE TABLE t",
        "CREATE TABLE t ()",
        "CREATE TABLE t (,a INT)",
        "CREATE TABLE t (a INT,)",
        "CREATE TABLE t (a)",
        "CREATE TABLE t (a INT b INT)",
        "CREATE TABLE t (a INT DEFAULT)",
        "CREATE TABLE t (a INT WITH DEFAULT)",
        "CREATE TABLE t (a INT DEFAULT CURRENT_DATE)",
        "CREATE TABLE t (a INT, CONSTRAINT c)",
        "CREATE TABLE t (a INT, PRIMARY (a))",
        "CREATE TABLE t (a INT, PRIMARY KEY ())",
        "CREATE TABLE t (a INT, FOREIGN KEY (a))",
        "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES)",
        "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p ())",
        "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p ON DELETE)",
        "CREATE TABLE t (a INT, FOREIGN KEY (a) REFERENCES p ON DELETE SET DEFAULT)",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn column_options_are_an_unordered_repeat_group() {
    // The pinned column-definition diagram repeats its options in any order;
    // its note forbids only repeating the same clause.
    for source in [
        "CREATE TABLE t (a INT NOT NULL WITH DEFAULT 1)",
        "CREATE TABLE t (a INT WITH DEFAULT 1 NOT NULL)",
        "CREATE TABLE t (a INT DEFAULT 1 NOT NULL)",
    ] {
        let statement = parse(source).unwrap();
        let column = &statement.columns()[0];
        assert!(column.is_not_null(), "{source}");
        assert_eq!(
            column.default().unwrap().value(),
            &Db2Literal::Number("1".into()),
            "{source}"
        );
    }
}

#[test]
fn built_in_type_arguments_follow_the_pinned_diagram() {
    for source in [
        "CREATE TABLE t (a SMALLINT(1))",
        "CREATE TABLE t (a VARCHAR)",
        "CREATE TABLE t (a DATE(1))",
        "CREATE TABLE t (a DECFLOAT(8))",
        "CREATE TABLE t (a TIMESTAMP(1,2))",
        "CREATE TABLE t (a INT WITH TIME ZONE)",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn republished_topic_forms_remain_fenced() {
    for source in [
        "CREATE TABLE t (a FLOAT DEFAULT 1E3)",
        "CREATE TABLE t (a DECFLOAT DEFAULT 1E3)",
        "CREATE TABLE t (a INT DEFAULT TRUE)",
        "CREATE TABLE t (a INT DEFAULT FALSE)",
        "CREATE TABLE t (a INT) IN db.ts",
        "CREATE TABLE t (a INT) IN DATABASE db",
        "ALTER TABLE t ADD COLUMN b INT",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn optional_table_parts_are_present_or_absent_once() {
    let plain = parse("CREATE TABLE t (a INT)").unwrap();
    assert_eq!(plain.table_name().parts().len(), 1);
    assert!(plain.columns()[0].default().is_none());
    assert!(plain.constraints().is_empty());

    let complete = parse("CREATE TABLE s.t (a INT NOT NULL DEFAULT +1, b INT, CONSTRAINT p PRIMARY KEY(a), FOREIGN KEY(b) REFERENCES s.parent ON DELETE NO ACTION);").unwrap();
    assert_eq!(complete.table_name().parts().len(), 2);
    assert_eq!(
        complete.columns()[0].default().unwrap().spelling(),
        Db2DefaultSpelling::Default
    );
    assert_eq!(complete.constraints().len(), 2);
    assert_eq!(
        foreign(&complete.constraints()[1]).referenced_columns(),
        None
    );

    for source in [
        "CREATE TABLE t (a INT DEFAULT 1 DEFAULT 2)",
        "CREATE TABLE t (a INT NOT NULL DEFAULT 1 NOT NULL)",
        "CREATE TABLE t (a INT, FOREIGN KEY(a) REFERENCES p ON DELETE CASCADE ON DELETE RESTRICT)",
        "CREATE TABLE t (a INT) EXTRA",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn extra_and_multiple_statements_are_never_accepted() {
    assert_eq!(
        parse("CREATE TABLE t (a INT); CREATE TABLE u (b INT)")
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::UnexpectedToken
    );
    assert_eq!(
        parse("CREATE TABLE t (a INT);;").unwrap_err().code,
        Db2SyntaxDiagnosticCode::UnexpectedToken
    );
    assert_eq!(
        parse("SELECT 1").unwrap_err().code,
        Db2SyntaxDiagnosticCode::UnsupportedStatement
    );
    assert_eq!(
        parse("CREATE VIEW v AS SELECT 1").unwrap_err().code,
        Db2SyntaxDiagnosticCode::UnsupportedStatement
    );
}

#[test]
fn unicode_identifiers_and_multiline_spans_are_preserved() {
    let statement =
        parse("CREATE TABLE \"模式\".\"表\" (\n  \"列\" VARCHAR(4) NOT NULL\n);").unwrap();
    assert_eq!(statement.table_name().parts()[0].value(), "模式");
    assert_eq!(statement.table_name().parts()[1].value(), "表");
    assert_eq!(statement.span().start, Db2SourceLocation::START);
    assert_eq!(
        statement.columns()[0].span().start,
        Db2SourceLocation { line: 2, column: 3 }
    );
    assert_eq!(statement.columns()[0].span().end.line, 2);
    assert_eq!(statement.span().end.line, 3);
}

#[test]
fn identifier_element_key_and_literal_limits_are_enforced() {
    let syntax = Db2SyntaxLimits::default();

    let limits = Db2AstLimits {
        max_name_parts: 1,
        ..Default::default()
    };
    assert_eq!(
        parse_db2_create_table_statement("CREATE TABLE s.t (a INT)", syntax, limits)
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );

    let limits = Db2AstLimits {
        max_list_items: 2,
        ..Default::default()
    };
    for source in [
        "CREATE TABLE t (a INT, b INT, c INT)",
        "CREATE TABLE t (a INT NOT NULL, b INT NOT NULL, c INT NOT NULL, UNIQUE(a,b,c))",
    ] {
        assert_eq!(
            parse_db2_create_table_statement(source, syntax, limits)
                .unwrap_err()
                .code,
            Db2SyntaxDiagnosticCode::InvalidStatementOperand,
            "{source}"
        );
    }

    let limits = Db2AstLimits {
        max_identifier_bytes: 3,
        ..Default::default()
    };
    assert_eq!(
        parse_db2_create_table_statement("CREATE TABLE long (a INT)", syntax, limits)
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );

    let limits = Db2AstLimits {
        max_literal_bytes: 3,
        ..Default::default()
    };
    assert_eq!(
        parse_db2_create_table_statement(
            "CREATE TABLE t (a VARCHAR(10) DEFAULT 'four')",
            syntax,
            limits,
        )
        .unwrap_err()
        .code,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );
}

#[test]
fn lexer_resource_bounds_are_propagated() {
    let syntax = Db2SyntaxLimits {
        max_statement_bytes: 12,
        ..Default::default()
    };
    assert_eq!(
            parse_db2_create_table_statement(
                "CREATE TABLE t (a INT)",
                syntax,
                Db2AstLimits::default(),
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::StatementTooLarge
        );

    let syntax = Db2SyntaxLimits {
        max_tokens: 4,
        ..Default::default()
    };
    assert_eq!(
            parse_db2_create_table_statement(
                "CREATE TABLE t (a INT)",
                syntax,
                Db2AstLimits::default(),
            )
            .unwrap_err()
            .code,
            Db2SyntaxDiagnosticCode::TooManyTokens
        );
}
