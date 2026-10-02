//! Independent public vectors for column-only default admission, not DDL execution.

use mainframe_env_db2::{
    Db2AssignedNumericValue, Db2BoundColumnDefault as DefaultValue,
    Db2BoundCreateTableColumnDefaults, Db2ColumnDefaultBindingErrorCode as Code,
    Db2ColumnDefaultBindingLimits as Limits, Db2ColumnDefaultStringContexts, Db2MixedData,
    Db2Nullability, Db2SourceLocation, Db2SourceSpan, Db2StringConstantContext,
    Db2StringStorageContext, Db2StringStorageErrorCode, Db2SyntaxDiagnosticCode,
    Db2SystemDefaultProducer as Producer, Db2TimeZone, bind_db2_create_table_column_defaults,
};

fn contexts() -> Db2ColumnDefaultStringContexts {
    Db2ColumnDefaultStringContexts {
        source: Db2StringConstantContext::UnicodeUtf8 {
            mixed_data: Db2MixedData::No,
        },
        character_target: Db2StringStorageContext::UnicodeUtf8Mixed,
        binary_target: Db2StringStorageContext::Binary,
    }
}

fn bind(sql: &str) -> Db2BoundCreateTableColumnDefaults {
    bind_db2_create_table_column_defaults(sql, Limits::default(), contexts()).unwrap()
}

#[test]
fn public_clause_intents_and_assigned_values_remain_distinct() {
    let result = bind(
        "CREATE TABLE T(A INTEGER, B INTEGER NOT NULL, C INTEGER DEFAULT NULL, D INTEGER DEFAULT, E DECIMAL(5,2) NOT NULL DEFAULT -12.349)",
    );
    assert_eq!(result.columns()[0].default(), &DefaultValue::ImplicitNull);
    assert_eq!(
        result.columns()[1].default(),
        &DefaultValue::MissingDefaultObligation
    );
    assert_eq!(result.columns()[2].default(), &DefaultValue::ExplicitNull);
    assert_eq!(
        result.columns()[3].default(),
        &DefaultValue::System(Producer::NumericZero)
    );
    let DefaultValue::Numeric(value) = result.columns()[4].default() else {
        panic!("numeric proof")
    };
    let Db2AssignedNumericValue::Decimal(decimal) = value.value() else {
        panic!("decimal")
    };
    assert_eq!((decimal.coefficient(), decimal.scale()), (-1234, 2));
    assert_eq!(value.resolved_type(), result.columns()[4].resolved_type());
    assert_eq!(value.resolved_type().nullability(), Db2Nullability::NotNull);
    assert_eq!(value.conversion().discarded_fractional_digits(), 1);
    assert!(value.conversion().discarded_nonzero());
}

#[test]
fn public_owned_original_utf8_crlf_proofs_keep_fixed_coordinates() {
    let result = {
        let sql = String::from(
            "/*é*/\r\nCREATE TABLE T(A SMALLINT DEFAULT - /*外*/\r\n32768, B CHAR(4) DEFAULT 'é''')",
        );
        bind(&sql)
    };
    let DefaultValue::Numeric(number) = result.columns()[0].default() else {
        panic!("numeric proof")
    };
    assert_eq!(number.value(), &Db2AssignedNumericValue::SmallInt(-32768));
    assert_eq!(
        number.span(),
        Db2SourceSpan {
            start_byte: 42,
            end_byte: 58,
            start: Db2SourceLocation {
                line: 2,
                column: 35
            },
            end: Db2SourceLocation { line: 3, column: 6 },
        }
    );
    assert_eq!(&result.source()[42..58], "- /*外*/\r\n32768");
    let DefaultValue::String(text) = result.columns()[1].default() else {
        panic!("string proof")
    };
    assert_eq!(text.value().bytes(), "é' ".as_bytes());
    assert_eq!(
        text.span(),
        Db2SourceSpan {
            start_byte: 78,
            end_byte: 84,
            start: Db2SourceLocation {
                line: 3,
                column: 26
            },
            end: Db2SourceLocation {
                line: 3,
                column: 31
            },
        }
    );
    assert_eq!(text.source().span(), text.span());
}

#[test]
fn public_character_binary_and_pre_trim_default_cap_are_independent() {
    let result = bind(
        "CREATE TABLE T(A BINARY(4) DEFAULT BX'ff00', B VARCHAR(2) NOT NULL DEFAULT '', C CHAR(1) DEFAULT X'00')",
    );
    for (index, expected) in [(0, &[255, 0, 0, 0][..]), (1, &b""[..]), (2, &[0][..])] {
        let DefaultValue::String(value) = result.columns()[index].default() else {
            panic!("string proof")
        };
        assert_eq!(value.value().bytes(), expected);
    }
    for (size, allowed) in [(1536, true), (1537, false)] {
        let sql = format!("CREATE TABLE T(A CHAR(1) DEFAULT '{}')", " ".repeat(size));
        let result = bind_db2_create_table_column_defaults(&sql, Limits::default(), contexts());
        if allowed {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err().code, Code::CharacterDefaultTooLong);
        }
    }
    let sql = format!(
        "CREATE TABLE T(A VARBINARY(1537) DEFAULT BX'{}')",
        "00".repeat(1537)
    );
    let result = bind(&sql);
    let DefaultValue::String(value) = result.columns()[0].default() else {
        panic!("binary proof")
    };
    assert_eq!(value.value().bytes().len(), 1537);
}

#[test]
fn public_system_producers_keep_runtime_precision_and_zone_without_values() {
    let result = bind(
        "CREATE TABLE T(A DATE DEFAULT, B TIME DEFAULT NOT NULL, C TIMESTAMP(12) WITH TIME ZONE DEFAULT, D TIMESTAMP(0) DEFAULT, E GRAPHIC(3) DEFAULT, F VARBINARY(7) DEFAULT)",
    );
    for (index, expected) in [
        (0, Producer::CurrentDate),
        (1, Producer::CurrentTime),
        (
            2,
            Producer::CurrentTimestamp {
                precision: 12,
                time_zone: Db2TimeZone::WithTimeZone,
            },
        ),
        (
            3,
            Producer::CurrentTimestamp {
                precision: 0,
                time_zone: Db2TimeZone::WithoutTimeZone,
            },
        ),
        (4, Producer::FixedGraphicBlanks { length: 3 }),
        (5, Producer::VaryingEmpty),
    ] {
        assert_eq!(
            result.columns()[index].default(),
            &DefaultValue::System(expected)
        );
    }
    assert_eq!(
        result.columns()[1].resolved_type().nullability(),
        Db2Nullability::NotNull
    );
}

#[test]
fn public_pending_and_invalid_cases_keep_owner_diagnostics() {
    let pending = bind_db2_create_table_column_defaults(
        "CREATE TABLE T(A CHAR(2) WITH DEFAULT)",
        Limits::default(),
        contexts(),
    )
    .unwrap_err();
    assert_eq!(pending.code, Code::WithDefaultSourcePending);
    assert!(pending.span.is_some());
    let conflict = bind_db2_create_table_column_defaults(
        "CREATE TABLE T(A INTEGER DEFAULT NULL NOT NULL)",
        Limits::default(),
        contexts(),
    )
    .unwrap_err();
    assert!(matches!(conflict.code, Code::Syntax(_)));
    let excess = bind_db2_create_table_column_defaults(
        "CREATE TABLE T(A CHAR(1) DEFAULT 'ax')",
        Limits::default(),
        contexts(),
    )
    .unwrap_err();
    assert_eq!(
        excess.code,
        Code::StringStorage(Db2StringStorageErrorCode::NonBlankExcess)
    );
}

#[test]
fn public_source_and_output_limits_fail_without_partial_results() {
    let sql = "CREATE TABLE T(A CHAR(4) DEFAULT 'a')";
    let mut limits = Limits::default();
    limits.syntax.max_statement_bytes = sql.len();
    limits.storage.max_output_bytes = 4;
    assert!(bind_db2_create_table_column_defaults(sql, limits, contexts()).is_ok());
    limits.syntax.max_statement_bytes -= 1;
    assert_eq!(
        bind_db2_create_table_column_defaults(sql, limits, contexts())
            .unwrap_err()
            .code,
        Code::Syntax(Db2SyntaxDiagnosticCode::StatementTooLarge)
    );
    limits.syntax.max_statement_bytes = sql.len();
    limits.storage.max_output_bytes = 3;
    assert_eq!(
        bind_db2_create_table_column_defaults(sql, limits, contexts())
            .unwrap_err()
            .code,
        Code::StringStorage(Db2StringStorageErrorCode::OutputTooLarge)
    );
}
