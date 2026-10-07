//! Original located operands; parsed DEFAULT provenance is not default legality.

use mainframe_env_db2::{
    Db2AssignedNumericValue, Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType, Db2DataType,
    Db2MaterializedNumericConstant, Db2Nullability, Db2NumericConstantErrorCode as Code,
    Db2NumericConstantLimits, Db2NumericConstantValue,
    Db2NumericConstantValueErrorCode as ValueCode, Db2ScalarType, Db2SourceLocation, Db2SourceSpan,
    Db2SyntaxDiagnosticCode, Db2SyntaxLimits, assign_db2_numeric_constant,
    classify_db2_located_numeric_operand, materialize_db2_located_numeric_operand,
    materialize_db2_numeric_constant, parse_db2_create_table_statement, resolve_db2_type,
};

fn line_span(start: usize, end: usize) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: start,
        end_byte: end,
        start: Db2SourceLocation {
            line: 1,
            column: (start as u32).saturating_add(1),
        },
        end: Db2SourceLocation {
            line: 1,
            column: (end as u32).saturating_add(1),
        },
    }
}

fn selected(source: &str, number_start: usize) -> Db2MaterializedNumericConstant {
    materialize_db2_located_numeric_operand(
        source,
        line_span(0, source.len()),
        Some(line_span(0, 1)),
        line_span(number_start, source.len()),
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

#[test]
fn public_segmented_natural_types_preserve_signed_endpoints_and_legacy_fences() {
    for (source, expected, scalar) in [
        (
            "- /*x*/ 2147483648",
            Db2NumericConstantValue::Integer(-2147483648),
            Db2ScalarType::Integer,
        ),
        (
            "- /*x*/ 9223372036854775808",
            Db2NumericConstantValue::BigInt(-9223372036854775808),
            Db2ScalarType::BigInt,
        ),
        (
            "+ /*x*/ 2147483648",
            Db2NumericConstantValue::BigInt(2147483648),
            Db2ScalarType::BigInt,
        ),
    ] {
        let value = selected(source, 8);
        assert_eq!(value.value(), &expected);
        assert_eq!(value.resolved_type().scalar(), &scalar);
        assert_eq!(value.resolved_type().nullability(), Db2Nullability::NotNull);
        assert_eq!(value.span(), line_span(0, source.len()));
        let classified = classify_db2_located_numeric_operand(
            source,
            value.span(),
            Some(line_span(0, 1)),
            line_span(8, source.len()),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(classified.resolved_type(), value.resolved_type());
        assert_eq!(
            materialize_db2_numeric_constant(source, value.span(), Default::default())
                .unwrap_err()
                .code,
            ValueCode::Classification(Code::InvalidSpelling)
        );
    }
    let source = "19";
    assert_eq!(
        materialize_db2_located_numeric_operand(
            source,
            line_span(0, 2),
            None,
            line_span(0, 2),
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .value(),
        &Db2NumericConstantValue::Integer(19)
    );
}

#[test]
fn public_decimal_proofs_keep_spelling_precision_and_coefficient_without_rewriting() {
    for (source, number_start, coefficient, precision, scale) in [
        ("- /*d*/ 000.1200", 8, -1200, 7, 4),
        ("+ /*x /*y*/ z*/ 000.1200", 16, 1200, 7, 4),
        ("- /*d*/ 000.00", 8, 0, 5, 2),
        ("- /*d*/ .0000000000000000000000000000001", 8, -1, 31, 31),
    ] {
        let proof = selected(source, number_start);
        let Db2NumericConstantValue::Decimal(value) = proof.value() else {
            panic!("DECIMAL");
        };
        assert_eq!(value.coefficient(), coefficient);
        assert_eq!(u32::from(value.scale()), scale);
        assert_eq!(
            proof.resolved_type().scalar(),
            &Db2ScalarType::Decimal { precision, scale }
        );
    }
}

#[test]
fn public_original_crlf_utf8_proofs_and_existing_assignment_survive_source_scope() {
    let operand = Db2SourceSpan {
        start_byte: 7,
        end_byte: 20,
        start: Db2SourceLocation { line: 2, column: 3 },
        end: Db2SourceLocation {
            line: 2,
            column: 16,
        },
    };
    let (natural, assigned) = {
        let source = String::from("☃\r\n  - /*x*/ 32768 \0+?");
        let proof = materialize_db2_located_numeric_operand(
            &source,
            operand,
            Some(Db2SourceSpan {
                end_byte: 8,
                end: Db2SourceLocation { line: 2, column: 4 },
                ..operand
            }),
            Db2SourceSpan {
                start_byte: 15,
                start: Db2SourceLocation {
                    line: 2,
                    column: 11,
                },
                ..operand
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let target = resolve_db2_type(
            &Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(
                    Db2BuiltInType::SmallInt,
                    vec![],
                    false,
                    Default::default(),
                )
                .unwrap(),
            ),
            Db2Nullability::Nullable,
        )
        .unwrap();
        let assigned = assign_db2_numeric_constant(&proof, &target).unwrap();
        (proof, assigned)
    };
    assert_eq!(natural.span(), operand);
    assert_eq!(natural.value(), &Db2NumericConstantValue::Integer(-32768));
    assert_eq!(assigned.span(), operand);
    assert_eq!(assigned.value(), &Db2AssignedNumericValue::SmallInt(-32768));
    assert_eq!(assigned.resolved_type().scalar(), &Db2ScalarType::SmallInt);
    assert_eq!(
        assigned.resolved_type().nullability(),
        Db2Nullability::Nullable
    );
}

#[test]
fn public_components_trivia_and_pending_numeric_forms_cannot_forge_proofs() {
    let source = "- /*x*/ 12";
    let operand = line_span(0, 10);
    let number = line_span(8, 10);
    for (a, sign, n) in [
        (operand, None, number),
        (operand, Some(line_span(0, 2)), number),
        (operand, Some(line_span(0, 1)), line_span(7, 10)),
        (
            operand,
            Some(line_span(0, 1)),
            Db2SourceSpan {
                end: Db2SourceLocation {
                    line: 2,
                    column: 11,
                },
                ..number
            },
        ),
        (line_span(0, usize::MAX), Some(line_span(0, 1)), number),
    ] {
        assert!(
            materialize_db2_located_numeric_operand(
                source,
                a,
                sign,
                n,
                Default::default(),
                Default::default(),
            )
            .is_err()
        );
    }
    for (source, start) in [("- x 1", 4), ("- /*x 1", 6), ("- +1", 3), ("-1", 0)] {
        assert!(
            materialize_db2_located_numeric_operand(
                source,
                line_span(0, source.len()),
                Some(line_span(0, 1)),
                line_span(start, source.len()),
                Default::default(),
                Default::default(),
            )
            .is_err()
        );
    }
    for (source, code) in [
        ("- /*x*/ 1E0", Code::UnsupportedExponent),
        ("- /*x*/ NAN", Code::UnsupportedSpecialValue),
        ("- /*x*/ 1,2", Code::UnsupportedCommaDecimal),
    ] {
        let error = materialize_db2_located_numeric_operand(
            source,
            line_span(0, source.len()),
            Some(line_span(0, 1)),
            line_span(8, source.len()),
            Default::default(),
            Default::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, ValueCode::Classification(code));
    }
}

#[test]
fn public_numeric_and_syntax_limits_preflight_the_whole_original_source() {
    let source = "- /*x*/ 1.0";
    let numeric = Db2NumericConstantLimits {
        max_source_bytes: 11,
        ast: Db2AstLimits {
            max_literal_bytes: 11,
            ..Default::default()
        },
    };
    let syntax = Db2SyntaxLimits {
        max_statement_bytes: 11,
        max_token_bytes: 3,
        max_tokens: 2,
        max_nesting: 1,
    };
    let run = |numeric, syntax| {
        materialize_db2_located_numeric_operand(
            source,
            line_span(0, 11),
            Some(line_span(0, 1)),
            line_span(8, 11),
            numeric,
            syntax,
        )
    };
    assert!(run(numeric, syntax).is_ok());
    for (n, s, expected) in [
        (
            Db2NumericConstantLimits {
                max_source_bytes: 10,
                ..numeric
            },
            syntax,
            Code::SourceTooLarge,
        ),
        (
            numeric,
            Db2SyntaxLimits {
                max_statement_bytes: 10,
                ..syntax
            },
            Code::SourceTooLarge,
        ),
        (
            Db2NumericConstantLimits {
                ast: Db2AstLimits {
                    max_literal_bytes: 10,
                    ..numeric.ast
                },
                ..numeric
            },
            syntax,
            Code::ConstantTooLarge,
        ),
        (
            numeric,
            Db2SyntaxLimits {
                max_token_bytes: 2,
                ..syntax
            },
            Code::Syntax(Db2SyntaxDiagnosticCode::TokenTooLarge),
        ),
        (
            numeric,
            Db2SyntaxLimits {
                max_tokens: 1,
                ..syntax
            },
            Code::Syntax(Db2SyntaxDiagnosticCode::TooManyTokens),
        ),
        (
            numeric,
            Db2SyntaxLimits {
                max_nesting: 0,
                ..syntax
            },
            Code::InvalidLimits,
        ),
    ] {
        assert_eq!(
            run(n, s).unwrap_err().code,
            ValueCode::Classification(expected)
        );
    }
}

#[test]
fn public_parser_default_locations_feed_existing_proofs_without_accepting_defaults() {
    let source = "CREATE TABLE t (a BIGINT DEFAULT - --gap\r\n9223372036854775808)";
    let statement =
        parse_db2_create_table_statement(source, Default::default(), Default::default()).unwrap();
    let default = statement.columns()[0].default().unwrap();
    let proof = materialize_db2_located_numeric_operand(
        source,
        default.value_span().unwrap(),
        default.numeric_sign_span(),
        default.numeric_token_span().unwrap(),
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        proof.value(),
        &Db2NumericConstantValue::BigInt(-9223372036854775808)
    );
    assert_eq!(proof.span(), default.value_span().unwrap());
}
