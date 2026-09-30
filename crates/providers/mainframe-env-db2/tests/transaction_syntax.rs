use mainframe_env_db2::{
    Db2AstLimits, Db2RollbackTarget, Db2SourceLocation, Db2StatementId, Db2StatementKind,
    Db2SyntaxDiagnosticCode, Db2SyntaxLimits, parse_db2_transaction_statement,
};

fn parse(source: &str) -> mainframe_env_db2::Db2Statement {
    parse_db2_transaction_statement(source, Db2SyntaxLimits::default(), Db2AstLimits::default())
        .unwrap()
}

fn error(source: &str) -> (Db2SyntaxDiagnosticCode, Db2SourceLocation) {
    let diagnostic = parse_db2_transaction_statement(
        source,
        Db2SyntaxLimits::default(),
        Db2AstLimits::default(),
    )
    .unwrap_err();
    (diagnostic.code, diagnostic.location)
}

#[test]
fn commit_forms_own_their_span_and_reject_extra_work() {
    for (source, work) in [("COMMIT", false), ("commit work;", true)] {
        let statement = parse(source);
        assert_eq!(statement.id(), Db2StatementId::SqlCommit);
        let Db2StatementKind::Commit(commit) = statement.kind() else {
            panic!("expected COMMIT AST")
        };
        assert_eq!(commit.has_work_keyword(), work);
        assert_eq!(statement.span().start_byte, 0);
        assert_eq!(statement.span().end_byte, source.len());
    }
    assert_eq!(
        error("COMMIT WORK WORK"),
        (
            Db2SyntaxDiagnosticCode::UnexpectedToken,
            Db2SourceLocation {
                line: 1,
                column: 13
            }
        )
    );
}

#[test]
fn rollback_unit_and_savepoint_forms() {
    for (source, work) in [("ROLLBACK", false), ("ROLLBACK WORK", true)] {
        let statement = parse(source);
        assert_eq!(statement.id(), Db2StatementId::SqlRollback);
        let Db2StatementKind::Rollback(rollback) = statement.kind() else {
            panic!("expected ROLLBACK AST")
        };
        assert_eq!(rollback.has_work_keyword(), work);
        assert_eq!(rollback.target(), &Db2RollbackTarget::UnitOfWork);
    }
    let unnamed = parse("ROLLBACK TO SAVEPOINT");
    let Db2StatementKind::Rollback(rollback) = unnamed.kind() else {
        panic!("expected ROLLBACK AST")
    };
    assert_eq!(rollback.target(), &Db2RollbackTarget::Savepoint(None));

    let named = parse("ROLLBACK WORK TO SAVEPOINT S1;");
    let Db2StatementKind::Rollback(rollback) = named.kind() else {
        panic!("expected ROLLBACK AST")
    };
    let Db2RollbackTarget::Savepoint(Some(name)) = rollback.target() else {
        panic!("expected named savepoint")
    };
    assert_eq!(name.value(), "S1");
    assert_eq!(
        error("ROLLBACK TO OTHER").0,
        Db2SyntaxDiagnosticCode::MissingToken
    );
}

#[test]
fn savepoint_options_and_duplicates() {
    // db2z_sql_savepoint (3e168615...): UNIQUE is optional and follows the
    // name; ON ROLLBACK RETAIN CURSORS is required; ON ROLLBACK RETAIN LOCKS
    // is optional; the two ON ROLLBACK clauses may appear in either order.
    let minimal = parse("SAVEPOINT S1 ON ROLLBACK RETAIN CURSORS");
    let Db2StatementKind::Savepoint(savepoint) = minimal.kind() else {
        panic!("expected SAVEPOINT AST")
    };
    assert!(!savepoint.is_unique());
    assert!(savepoint.retains_cursors());
    assert!(!savepoint.retains_locks());

    for source in [
        "SAVEPOINT S1 UNIQUE ON ROLLBACK RETAIN CURSORS ON ROLLBACK RETAIN LOCKS",
        "SAVEPOINT S1 UNIQUE ON ROLLBACK RETAIN LOCKS ON ROLLBACK RETAIN CURSORS",
    ] {
        let statement = parse(source);
        assert_eq!(statement.id(), Db2StatementId::SqlSavepoint);
        let Db2StatementKind::Savepoint(savepoint) = statement.kind() else {
            panic!("expected SAVEPOINT AST")
        };
        assert_eq!(savepoint.name().value(), "S1");
        assert!(savepoint.is_unique());
        assert!(savepoint.retains_cursors());
        assert!(savepoint.retains_locks());
    }
    for source in [
        "SAVEPOINT S1 UNIQUE UNIQUE ON ROLLBACK RETAIN CURSORS",
        "SAVEPOINT S1 ON ROLLBACK RETAIN CURSORS ON ROLLBACK RETAIN CURSORS",
        "SAVEPOINT S1 ON ROLLBACK RETAIN CURSORS ON ROLLBACK RETAIN LOCKS ON ROLLBACK RETAIN LOCKS",
    ] {
        assert_eq!(error(source).0, Db2SyntaxDiagnosticCode::DuplicateClause);
    }
}

#[test]
fn savepoint_requires_retain_cursors_and_unique_after_the_name() {
    for source in [
        "SAVEPOINT S1",
        "SAVEPOINT S1 UNIQUE",
        "SAVEPOINT S1 ON ROLLBACK RETAIN LOCKS",
    ] {
        assert_eq!(
            error(source).0,
            Db2SyntaxDiagnosticCode::MissingToken,
            "{source}"
        );
    }
    assert_eq!(
        error("SAVEPOINT S1 ON ROLLBACK RETAIN CURSORS UNIQUE").0,
        Db2SyntaxDiagnosticCode::UnexpectedToken
    );
}

#[test]
fn malformed_or_unsupported_statements_fail_at_the_first_offending_token() {
    assert_eq!(
        error("SAVEPOINT SYSPOINT").0,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );
    assert_eq!(
        error("SAVEPOINT \"sysPoint\"").0,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );
    assert_eq!(error("SAVEPOINT").0, Db2SyntaxDiagnosticCode::MissingToken);
    assert_eq!(
        error("ROLLBACK TO SAVEPOINT 1").0,
        Db2SyntaxDiagnosticCode::UnexpectedToken
    );
    assert_eq!(
        error("SAVEPOINT S1 ON ROLLBACK RETAIN ROWS"),
        (
            Db2SyntaxDiagnosticCode::MissingToken,
            Db2SourceLocation {
                line: 1,
                column: 33
            }
        )
    );
    assert_eq!(
        error("COMMIT; ROLLBACK").0,
        Db2SyntaxDiagnosticCode::UnexpectedToken
    );
    assert_eq!(
        error("RELEASE SAVEPOINT S1").0,
        Db2SyntaxDiagnosticCode::UnsupportedStatement
    );
    assert_eq!(
        error("SELECT 1").0,
        Db2SyntaxDiagnosticCode::UnsupportedStatement
    );
}

#[test]
fn configured_bounds_fail_before_a_statement_is_returned() {
    let syntax_limits = Db2SyntaxLimits {
        max_tokens: 1,
        ..Db2SyntaxLimits::default()
    };
    assert_eq!(
        parse_db2_transaction_statement("COMMIT WORK", syntax_limits, Db2AstLimits::default())
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::TooManyTokens
    );
    let ast_limits = Db2AstLimits {
        max_identifier_bytes: 2,
        ..Db2AstLimits::default()
    };
    assert_eq!(
        parse_db2_transaction_statement("SAVEPOINT LONG", Db2SyntaxLimits::default(), ast_limits)
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::InvalidStatementOperand
    );
    let ast_limits = Db2AstLimits {
        max_name_parts: 0,
        ..Db2AstLimits::default()
    };
    assert_eq!(
        parse_db2_transaction_statement("COMMIT", Db2SyntaxLimits::default(), ast_limits)
            .unwrap_err()
            .code,
        Db2SyntaxDiagnosticCode::InvalidLimits
    );
}
