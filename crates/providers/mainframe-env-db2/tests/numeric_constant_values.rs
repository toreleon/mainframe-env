//! Natural exact literal values, not target assignment/default conversion.

use mainframe_env_db2::{
    Db2MaterializedNumericConstant, Db2Nullability, Db2NumericConstantErrorCode,
    Db2NumericConstantLimits, Db2NumericConstantValue, Db2NumericConstantValueErrorCode,
    Db2ScalarType, Db2SourceLocation, Db2SourceSpan, materialize_db2_numeric_constant,
};

fn span(text: &str) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: 0,
        end_byte: text.len(),
        start: Db2SourceLocation::START,
        end: Db2SourceLocation {
            line: 1,
            column: text.chars().count() as u32 + 1,
        },
    }
}

fn materialize(text: &str) -> Db2MaterializedNumericConstant {
    materialize_db2_numeric_constant(text, span(text), Default::default()).unwrap()
}

#[test]
fn public_natural_binary_values_keep_signed_endpoints_and_type_transitions() {
    for (text, expected) in [
        ("2147483647", 2_147_483_647),
        ("-2147483648", -2_147_483_648),
        ("+0000000000000000001", 1),
        ("-0", 0),
        ("32767", 32_767),
    ] {
        let result = materialize(text);
        assert_eq!(result.value(), &Db2NumericConstantValue::Integer(expected));
        assert_eq!(result.resolved_type().scalar(), &Db2ScalarType::Integer);
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(result.span(), span(text));
    }
    for (text, expected) in [
        ("2147483648", 2_147_483_648),
        ("-2147483649", -2_147_483_649),
        ("9223372036854775807", 9_223_372_036_854_775_807),
        ("-9223372036854775808", -9_223_372_036_854_775_808),
    ] {
        let result = materialize(text);
        assert_eq!(result.value(), &Db2NumericConstantValue::BigInt(expected));
        assert_eq!(result.resolved_type().scalar(), &Db2ScalarType::BigInt);
    }
}

#[test]
fn public_decimal_values_have_independent_exact_coefficients_precision_and_scale() {
    for (text, coefficient, precision, scale) in [
        ("025.50", 2_550, 5, 2),
        ("-000.00100", -100, 8, 5),
        ("-.0", 0, 1, 1),
        ("-000.00", 0, 5, 2),
        ("5.", 5, 1, 0),
        ("9223372036854775808", 9_223_372_036_854_775_808, 19, 0),
        ("-9223372036854775809", -9_223_372_036_854_775_809, 19, 0),
        (
            "9999999999999999999999999999999.",
            9_999_999_999_999_999_999_999_999_999_999,
            31,
            0,
        ),
        (
            "-.1234567890123456789012345678901",
            -1_234_567_890_123_456_789_012_345_678_901,
            31,
            31,
        ),
    ] {
        let result = materialize(text);
        let Db2NumericConstantValue::Decimal(value) = result.value() else {
            panic!("expected exact DECIMAL for {text}");
        };
        assert_eq!(value.coefficient(), coefficient, "{text}");
        assert_eq!(u32::from(value.scale()), scale, "{text}");
        assert_eq!(
            result.resolved_type().scalar(),
            &Db2ScalarType::Decimal { precision, scale }
        );
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::NotNull
        );
    }
}

#[test]
fn public_results_and_errors_own_original_utf8_crlf_locations() {
    let (clone, error, expected_span) = {
        let prefix = "/*é😀*/\r\n  ";
        let text = "-000.00100";
        let source = format!("{prefix}{text}; -- trailing\r\n");
        let expected_span = Db2SourceSpan {
            start_byte: prefix.len(),
            end_byte: prefix.len() + text.len(),
            start: Db2SourceLocation { line: 2, column: 3 },
            end: Db2SourceLocation {
                line: 2,
                column: 13,
            },
        };
        let result =
            materialize_db2_numeric_constant(&source, expected_span, Default::default()).unwrap();
        let source = String::from("1E0");
        let error = materialize_db2_numeric_constant(&source, span(&source), Default::default())
            .unwrap_err();
        (result.clone(), error, expected_span)
    };
    assert_eq!(clone.span(), expected_span);
    let Db2NumericConstantValue::Decimal(value) = clone.value() else {
        panic!("DECIMAL");
    };
    assert_eq!(value.coefficient(), -100);
    assert_eq!(value.scale(), 5);
    assert_eq!(
        error.code,
        Db2NumericConstantValueErrorCode::Classification(
            Db2NumericConstantErrorCode::UnsupportedExponent
        )
    );
    assert_eq!(error.span, span("1E0"));
    assert!(error.message.len() <= 256);
}

#[test]
fn public_materialization_preserves_classifier_rejection_fences() {
    for (text, expected) in [
        ("1E0", Db2NumericConstantErrorCode::UnsupportedExponent),
        (
            "-SNAN",
            Db2NumericConstantErrorCode::UnsupportedSpecialValue,
        ),
        ("1,2", Db2NumericConstantErrorCode::UnsupportedCommaDecimal),
        (
            "00000000000000000001",
            Db2NumericConstantErrorCode::UnsupportedLongInteger,
        ),
        (
            "99999999999999999999999999999999",
            Db2NumericConstantErrorCode::TooManyDigits,
        ),
        ("NULL", Db2NumericConstantErrorCode::InvalidSpelling),
        ("'1'", Db2NumericConstantErrorCode::InvalidSpelling),
        ("1 2", Db2NumericConstantErrorCode::InvalidSpelling),
        ("1;2", Db2NumericConstantErrorCode::InvalidSpelling),
        ("１２", Db2NumericConstantErrorCode::InvalidSpelling),
    ] {
        let error =
            materialize_db2_numeric_constant(text, span(text), Default::default()).unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(expected),
            "{text}"
        );
        assert_eq!(error.span, span(text));
    }
}

#[test]
fn public_limits_cover_entire_original_source_and_selected_spelling() {
    let limits = Db2NumericConstantLimits {
        max_source_bytes: 4,
        ..Default::default()
    };
    assert!(materialize_db2_numeric_constant("+1.0", span("+1.0"), limits).is_ok());
    assert_eq!(
        materialize_db2_numeric_constant("+1.00", span("+1.00"), limits)
            .unwrap_err()
            .code,
        Db2NumericConstantValueErrorCode::Classification(
            Db2NumericConstantErrorCode::SourceTooLarge
        )
    );
    let selected = Db2SourceSpan {
        start_byte: 3,
        end_byte: 4,
        start: Db2SourceLocation { line: 1, column: 4 },
        end: Db2SourceLocation { line: 1, column: 5 },
    };
    assert!(materialize_db2_numeric_constant("abc1", selected, limits).is_ok());
    assert!(materialize_db2_numeric_constant("abc1x", selected, limits).is_err());
    let mut limits = Db2NumericConstantLimits::default();
    limits.ast.max_literal_bytes = 4;
    assert!(materialize_db2_numeric_constant("+1.0", span("+1.0"), limits).is_ok());
    assert_eq!(
        materialize_db2_numeric_constant("+1.00", span("+1.00"), limits)
            .unwrap_err()
            .code,
        Db2NumericConstantValueErrorCode::Classification(
            Db2NumericConstantErrorCode::ConstantTooLarge
        )
    );
    limits.ast.max_list_items = 1;
    assert_eq!(
        materialize_db2_numeric_constant(".1", span(".1"), limits)
            .unwrap_err()
            .code,
        Db2NumericConstantValueErrorCode::Classification(
            Db2NumericConstantErrorCode::TypeArgumentsLimit
        )
    );
    assert!(materialize_db2_numeric_constant("1", span("1"), limits).is_ok());
}

#[test]
fn public_invalid_locations_fail_before_any_value_is_constructed() {
    let source = "α1";
    let valid = Db2SourceSpan {
        start_byte: 2,
        end_byte: 3,
        start: Db2SourceLocation { line: 1, column: 2 },
        end: Db2SourceLocation { line: 1, column: 3 },
    };
    assert!(materialize_db2_numeric_constant(source, valid, Default::default()).is_ok());
    for invalid in [
        Db2SourceSpan {
            start_byte: 1,
            ..valid
        },
        Db2SourceSpan {
            end_byte: 4,
            ..valid
        },
        Db2SourceSpan {
            start: Db2SourceLocation { line: 0, column: 2 },
            ..valid
        },
        Db2SourceSpan {
            end: Db2SourceLocation { line: 2, column: 1 },
            ..valid
        },
    ] {
        let error =
            materialize_db2_numeric_constant(source, invalid, Default::default()).unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(
                Db2NumericConstantErrorCode::InvalidSourceSpan
            )
        );
        assert_eq!(error.span, invalid);
    }
}
