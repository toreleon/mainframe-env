//! Public pure constant typing; no evaluator or statement-row credit.

use mainframe_env_db2::{
    Db2AstLimits, Db2Nullability, Db2NumericConstantErrorCode, Db2NumericConstantLimits,
    Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2SyntaxDiagnosticCode, Db2SyntaxLimits,
    classify_db2_numeric_constant, lex_db2,
};

fn span(text: &str) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: 0,
        end_byte: text.len(),
        start: Db2SourceLocation::START,
        end: Db2SourceLocation {
            line: 1,
            column: text.len() as u32 + 1,
        },
    }
}

#[test]
fn public_classifier_uses_existing_not_null_types_at_signed_boundaries() {
    for (text, scalar) in [
        ("+2147483647", Db2ScalarType::Integer),
        ("-2147483648", Db2ScalarType::Integer),
        ("2147483648", Db2ScalarType::BigInt),
        ("-2147483649", Db2ScalarType::BigInt),
        ("9223372036854775807", Db2ScalarType::BigInt),
        ("-9223372036854775808", Db2ScalarType::BigInt),
        (
            "9223372036854775808",
            Db2ScalarType::Decimal {
                precision: 19,
                scale: 0,
            },
        ),
        (
            "-9223372036854775809",
            Db2ScalarType::Decimal {
                precision: 19,
                scale: 0,
            },
        ),
        (
            "-000.00",
            Db2ScalarType::Decimal {
                precision: 5,
                scale: 2,
            },
        ),
        (
            ".05",
            Db2ScalarType::Decimal {
                precision: 2,
                scale: 2,
            },
        ),
        (
            "15.",
            Db2ScalarType::Decimal {
                precision: 2,
                scale: 0,
            },
        ),
    ] {
        let result = classify_db2_numeric_constant(text, span(text), Default::default()).unwrap();
        assert_eq!(result.resolved_type().scalar(), &scalar, "{text}");
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(result.span(), span(text));
    }
}

#[test]
fn public_results_own_types_and_verify_utf8_crlf_original_locations() {
    let result = {
        let source = String::from("é\r\nVALUES (-025.50)");
        let located = Db2SourceSpan {
            start_byte: 12,
            end_byte: 19,
            start: Db2SourceLocation { line: 2, column: 9 },
            end: Db2SourceLocation {
                line: 2,
                column: 16,
            },
        };
        let result = classify_db2_numeric_constant(&source, located, Default::default()).unwrap();
        let mut wrong = located;
        wrong.start.column -= 1;
        assert_eq!(
            classify_db2_numeric_constant(&source, wrong, Default::default())
                .unwrap_err()
                .code,
            Db2NumericConstantErrorCode::InvalidSourceSpan
        );
        result
    };
    assert_eq!(
        result.resolved_type().scalar(),
        &Db2ScalarType::Decimal {
            precision: 5,
            scale: 2
        }
    );
    assert_eq!(result.span().start_byte, 12);
}

#[test]
fn public_limits_fail_closed_before_unbounded_work() {
    let limits = Db2NumericConstantLimits {
        max_source_bytes: 2,
        ..Default::default()
    };
    assert_eq!(
        classify_db2_numeric_constant("123", span("123"), limits)
            .unwrap_err()
            .code,
        Db2NumericConstantErrorCode::SourceTooLarge
    );
    let limits = Db2NumericConstantLimits {
        ast: Db2AstLimits {
            max_literal_bytes: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        classify_db2_numeric_constant("123", span("123"), limits)
            .unwrap_err()
            .code,
        Db2NumericConstantErrorCode::ConstantTooLarge
    );
    let limits = Db2NumericConstantLimits {
        ast: Db2AstLimits {
            max_list_items: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        classify_db2_numeric_constant("1.0", span("1.0"), limits)
            .unwrap_err()
            .code,
        Db2NumericConstantErrorCode::TypeArgumentsLimit
    );
    let text = "9".repeat(31);
    assert!(classify_db2_numeric_constant(&text, span(&text), Default::default()).is_ok());
    let text = "9".repeat(32);
    assert_eq!(
        classify_db2_numeric_constant(&text, span(&text), Default::default())
            .unwrap_err()
            .code,
        Db2NumericConstantErrorCode::TooManyDigits
    );
}

#[test]
fn public_deferred_forms_do_not_lift_shared_lexer_fences() {
    for (text, code) in [
        ("1E2", Db2NumericConstantErrorCode::UnsupportedExponent),
        ("-INF", Db2NumericConstantErrorCode::UnsupportedSpecialValue),
        ("1,5", Db2NumericConstantErrorCode::UnsupportedCommaDecimal),
        (
            "00000000000000000001",
            Db2NumericConstantErrorCode::UnsupportedLongInteger,
        ),
        ("- 1", Db2NumericConstantErrorCode::InvalidSpelling),
    ] {
        assert_eq!(
            classify_db2_numeric_constant(text, span(text), Default::default())
                .unwrap_err()
                .code,
            code
        );
    }
    assert_eq!(
        lex_db2("1E2", Db2SyntaxLimits::default()).unwrap_err().code,
        Db2SyntaxDiagnosticCode::UnsupportedNumericConstant
    );
}
