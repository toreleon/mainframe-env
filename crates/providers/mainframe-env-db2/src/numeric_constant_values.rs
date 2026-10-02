//! Exact natural numeric literal values through the existing classifier and decimal primitive.
//!
//! Sources: ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_constantsintro.html, 17915 bytes,
//!   bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8;
//! - db2z_datatypesintro.html, 22904 bytes,
//!   a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570.
//!
//! Language elements have no standalone statement-catalog row. This private
//! kernel materializes only natural INTEGER/BIGINT/DECIMAL constants, with no
//! expression evaluation, assignment/default conversion, wire format, catalog
//! identity, cell persistence, backend, execution or coverage claim. The sole
//! spelling/span/type authority remains `classify_db2_numeric_constant`, including
//! its deferred exponent, special, comma, long unpointed in-range and >31-digit
//! forms. Long unpointed spellings are outside this subset, not universally
//! invalid IBM constants. No shared lexer or expression fence is lifted.

use crate::{
    Db2NumericConstantError, Db2NumericConstantErrorCode, Db2NumericConstantLimits,
    Db2NumericConstantType, Db2ResolvedType, Db2ScalarType, Db2SourceSpan,
    classify_db2_numeric_constant,
};
use mainframe_env_encoding::DecimalValue;
use std::fmt;

/// Exact natural value; DECIMAL retains its scale even for zero or trailing zeros.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2NumericConstantValue {
    Integer(i32),
    BigInt(i64),
    Decimal(DecimalValue),
}

/// An owned, classifier-verified type/span and its matching exact value.
/// Private fields and the sole materializing constructor prevent forged pairs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2MaterializedNumericConstant {
    constant_type: Db2NumericConstantType,
    value: Db2NumericConstantValue,
}

impl Db2MaterializedNumericConstant {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        self.constant_type.resolved_type()
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.constant_type.span()
    }

    #[must_use]
    pub const fn value(&self) -> &Db2NumericConstantValue {
        &self.value
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2NumericConstantValueErrorCode {
    Classification(Db2NumericConstantErrorCode),
    InvalidCoefficient,
    CoefficientOutOfRange,
    ValueOutOfRange,
    InvalidDecimalValue,
    UnsupportedResolvedType,
}

/// Fixed-size located diagnostic; never retains source text or allocated messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NumericConstantValueError {
    pub code: Db2NumericConstantValueErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl Db2NumericConstantValueError {
    const fn new(
        code: Db2NumericConstantValueErrorCode,
        span: Db2SourceSpan,
        message: &'static str,
    ) -> Self {
        Self {
            code,
            span,
            message,
        }
    }
}

impl From<Db2NumericConstantError> for Db2NumericConstantValueError {
    fn from(error: Db2NumericConstantError) -> Self {
        Self::new(
            Db2NumericConstantValueErrorCode::Classification(error.code),
            error.span,
            error.message,
        )
    }
}

impl fmt::Display for Db2NumericConstantValueError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2NumericConstantValueError {}

/// Materialize exactly the classifier-selected original-source spelling.
/// Limits, adjacent sign, natural type and both byte/line/column endpoints are
/// validated by the existing classifier before any value is accumulated. This
/// does not parse a statement or expression, trim trivia, or convert to a target
/// type. No floats are used; negative zero becomes zero without losing DECIMAL
/// scale or natural precision. Output remains valid after source/limits are dropped.
pub fn materialize_db2_numeric_constant(
    source: &str,
    span: Db2SourceSpan,
    limits: Db2NumericConstantLimits,
) -> Result<Db2MaterializedNumericConstant, Db2NumericConstantValueError> {
    use Db2NumericConstantValueErrorCode as Code;
    let constant_type = classify_db2_numeric_constant(source, span, limits)?;
    // Classification already verified these UTF-8 boundaries and endpoints.
    let spelling = &source[span.start_byte..span.end_byte];
    let (negative, unsigned) = match spelling.as_bytes().first() {
        Some(b'-') => (true, &spelling[1..]),
        Some(b'+') => (false, &spelling[1..]),
        _ => (false, spelling),
    };
    let coefficient = accumulate_coefficient(unsigned, negative, span)?;
    let out_of_range = || {
        Db2NumericConstantValueError::new(
            Code::ValueOutOfRange,
            span,
            "exact coefficient does not fit the classifier's natural value type",
        )
    };
    let value = match constant_type.resolved_type().scalar() {
        Db2ScalarType::Integer => Db2NumericConstantValue::Integer(
            i32::try_from(coefficient).map_err(|_| out_of_range())?,
        ),
        Db2ScalarType::BigInt => {
            Db2NumericConstantValue::BigInt(i64::try_from(coefficient).map_err(|_| out_of_range())?)
        }
        Db2ScalarType::Decimal { scale, .. } => {
            let scale = u8::try_from(*scale).map_err(|_| out_of_range())?;
            let decimal = DecimalValue::new(coefficient, scale).map_err(|_| {
                Db2NumericConstantValueError::new(
                    Code::InvalidDecimalValue,
                    span,
                    "exact coefficient/scale could not be constructed by the decimal primitive",
                )
            })?;
            Db2NumericConstantValue::Decimal(decimal)
        }
        _ => {
            return Err(Db2NumericConstantValueError::new(
                Code::UnsupportedResolvedType,
                span,
                "classifier returned a type outside the exact numeric constant subset",
            ));
        }
    };
    Ok(Db2MaterializedNumericConstant {
        constant_type,
        value,
    })
}

// Consume only classifier-verified ASCII digits, ignoring its decimal point.
// The independent digit/overflow guard bounds materialization; SQL spelling and
// datatype validation belong exclusively to the existing classifier/resolver.
fn accumulate_coefficient(
    unsigned: &str,
    negative: bool,
    span: Db2SourceSpan,
) -> Result<i128, Db2NumericConstantValueError> {
    use Db2NumericConstantValueErrorCode as Code;
    let out_of_range = || {
        Db2NumericConstantValueError::new(
            Code::CoefficientOutOfRange,
            span,
            "coefficient exceeds the 31-digit materialization bound or integer capacity",
        )
    };
    let mut coefficient = 0i128;
    let mut digits = 0;
    for byte in unsigned.bytes() {
        if byte == b'.' {
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(Db2NumericConstantValueError::new(
                Code::InvalidCoefficient,
                span,
                "classified coefficient must contain only ASCII digits and its decimal point",
            ));
        }
        digits += 1;
        if digits > 31 {
            return Err(out_of_range());
        }
        coefficient = coefficient
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(byte - b'0')))
            .ok_or_else(out_of_range)?;
    }
    if digits == 0 {
        return Err(Db2NumericConstantValueError::new(
            Code::InvalidCoefficient,
            span,
            "classified coefficient must contain at least one ASCII digit",
        ));
    }
    if negative {
        coefficient = coefficient.checked_neg().ok_or_else(out_of_range)?;
    }
    Ok(coefficient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Db2Nullability, Db2NumericConstantLimits, Db2ScalarType, Db2SourceLocation, Db2SourceSpan,
    };

    fn full_span(text: &str) -> Db2SourceSpan {
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

    fn materialize(
        text: &str,
    ) -> Result<Db2MaterializedNumericConstant, Db2NumericConstantValueError> {
        materialize_db2_numeric_constant(text, full_span(text), Db2NumericConstantLimits::default())
    }

    fn expect_value(text: &str, scalar: Db2ScalarType, value: Db2NumericConstantValue) {
        let result = materialize(text).unwrap_or_else(|error| panic!("{text:?}: {error}"));
        assert_eq!(result.resolved_type().scalar(), &scalar, "{text:?}");
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(result.value(), &value, "{text:?}");
        assert_eq!(result.span(), full_span(text));
    }

    #[test]
    fn signed_binary_endpoints_and_transitions_have_exact_values() {
        for (text, value) in [
            ("2147483647", 2_147_483_647),
            ("+2147483647", 2_147_483_647),
            ("-2147483648", -2_147_483_648),
            ("-2147483647", -2_147_483_647),
            ("32767", 32_767),
            ("-32768", -32_768),
            ("1", 1),
            ("-1", -1),
        ] {
            expect_value(
                text,
                Db2ScalarType::Integer,
                Db2NumericConstantValue::Integer(value),
            );
        }
        for (text, value) in [
            ("2147483648", 2_147_483_648),
            ("+2147483648", 2_147_483_648),
            ("-2147483649", -2_147_483_649),
            ("9223372036854775807", 9_223_372_036_854_775_807),
            ("+9223372036854775807", 9_223_372_036_854_775_807),
            ("-9223372036854775808", -9_223_372_036_854_775_808),
        ] {
            expect_value(
                text,
                Db2ScalarType::BigInt,
                Db2NumericConstantValue::BigInt(value),
            );
        }
    }

    #[test]
    fn decimal_values_preserve_scale_and_natural_precision() {
        for (text, coefficient, precision, scale) in [
            ("9223372036854775808", 9_223_372_036_854_775_808, 19, 0),
            ("+9223372036854775808", 9_223_372_036_854_775_808, 19, 0),
            ("-9223372036854775809", -9_223_372_036_854_775_809, 19, 0),
            ("09223372036854775808", 9_223_372_036_854_775_808, 20, 0),
            ("-09223372036854775809", -9_223_372_036_854_775_809, 20, 0),
            ("025.50", 2_550, 5, 2),
            ("+025.50", 2_550, 5, 2),
            ("-025.50", -2_550, 5, 2),
            ("1000.", 1_000, 4, 0),
            ("-15.", -15, 2, 0),
            (
                "+375893333333333333333.33",
                37_589_333_333_333_333_333_333,
                23,
                2,
            ),
            (".5", 5, 1, 1),
            ("+.5", 5, 1, 1),
            ("-.5", -5, 1, 1),
            ("5.", 5, 1, 0),
            ("+5.", 5, 1, 0),
            ("-5.", -5, 1, 0),
            ("000.00100", 100, 8, 5),
            ("-0.00000", 0, 6, 5),
        ] {
            expect_value(
                text,
                Db2ScalarType::Decimal { precision, scale },
                Db2NumericConstantValue::Decimal(
                    DecimalValue::new(coefficient, scale as u8).unwrap(),
                ),
            );
        }
    }

    #[test]
    fn leading_zeros_and_negative_zero_keep_natural_types() {
        for digits in 1..=19 {
            for sign in ["", "+", "-"] {
                expect_value(
                    &format!("{sign}{}", "0".repeat(digits)),
                    Db2ScalarType::Integer,
                    Db2NumericConstantValue::Integer(0),
                );
            }
        }
        for (text, value) in [
            ("0000000000000000001", 1),
            ("-0000000000000000001", -1),
            ("+0000000002147483647", 2_147_483_647),
            ("-0000000002147483648", -2_147_483_648),
        ] {
            expect_value(
                text,
                Db2ScalarType::Integer,
                Db2NumericConstantValue::Integer(value),
            );
        }
        for (text, value) in [
            ("0000000002147483648", 2_147_483_648),
            ("-0000000002147483649", -2_147_483_649),
        ] {
            expect_value(
                text,
                Db2ScalarType::BigInt,
                Db2NumericConstantValue::BigInt(value),
            );
        }
        for (text, precision, scale) in [
            ("-0.", 1, 0),
            ("-.0", 1, 1),
            ("-000.00", 5, 2),
            ("+000.00", 5, 2),
            ("000.00", 5, 2),
        ] {
            expect_value(
                text,
                Db2ScalarType::Decimal { precision, scale },
                Db2NumericConstantValue::Decimal(DecimalValue::new(0, scale as u8).unwrap()),
            );
        }
    }

    #[test]
    fn decimal31_coefficients_at_every_point_and_sign_are_exact() {
        const DIGITS: &str = "1234567890123456789012345678901";
        const COEFFICIENT: i128 = 1_234_567_890_123_456_789_012_345_678_901;
        const MAXIMUM: i128 = 9_999_999_999_999_999_999_999_999_999_999;
        for (sign, polarity) in [("", 1), ("+", 1), ("-", -1)] {
            for point in 0..=31 {
                let text = format!("{sign}{}.{}", &DIGITS[..point], &DIGITS[point..]);
                let scale = (31 - point) as u32;
                expect_value(
                    &text,
                    Db2ScalarType::Decimal {
                        precision: 31,
                        scale,
                    },
                    Db2NumericConstantValue::Decimal(
                        DecimalValue::new(polarity * COEFFICIENT, scale as u8).unwrap(),
                    ),
                );
            }
            for text in [
                format!("{sign}{}", "9".repeat(31)),
                format!("{sign}{}.", "9".repeat(31)),
            ] {
                expect_value(
                    &text,
                    Db2ScalarType::Decimal {
                        precision: 31,
                        scale: 0,
                    },
                    Db2NumericConstantValue::Decimal(
                        DecimalValue::new(polarity * MAXIMUM, 0).unwrap(),
                    ),
                );
            }
            for point in 0..=31 {
                let text = format!("{sign}{}.{}", "0".repeat(point), "0".repeat(31 - point));
                expect_value(
                    &text,
                    Db2ScalarType::Decimal {
                        precision: 31,
                        scale: (31 - point) as u32,
                    },
                    Db2NumericConstantValue::Decimal(
                        DecimalValue::new(0, (31 - point) as u8).unwrap(),
                    ),
                );
            }
            for point in 0..=32 {
                let text = format!("{sign}{}.{}", "9".repeat(point), "9".repeat(32 - point));
                expect_classification_error(&text, Db2NumericConstantErrorCode::TooManyDigits);
            }
            expect_classification_error(
                &format!("{sign}{}", "9".repeat(32)),
                Db2NumericConstantErrorCode::TooManyDigits,
            );
        }
    }

    fn expect_classification_error(text: &str, code: Db2NumericConstantErrorCode) {
        let error = materialize(text).unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(code),
            "{text:?}"
        );
        assert_eq!(error.span, full_span(text));
        assert!(error.message.len() <= 256);
        assert!(error.to_string().contains("1:1"));
        // Propagation retains the classifier's diagnostic, not a second interpretation.
        let classified = classify_db2_numeric_constant(
            text,
            full_span(text),
            Db2NumericConstantLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error, Db2NumericConstantValueError::from(classified));
    }

    #[test]
    fn inherited_malformed_and_deferred_forms_remain_rejected() {
        use Db2NumericConstantErrorCode as Code;
        for text in [
            "",
            "+",
            "-",
            ".",
            "+.",
            "-.",
            "1..2",
            ".1.",
            "++1",
            "--1",
            "+-1",
            "-+1",
            "1-",
            "1+",
            "1;",
            "1;2",
            "1 2",
            " 1",
            "1 ",
            "+ 1",
            "1\t2",
            "1/*c*/2",
            "-/*c*/1",
            "/*c*/1",
            "1--c",
            "１２",
            "١٢",
            "−1",
            "1_0",
            "1\0",
            "DECFLOAT(16)",
            "NULL",
            "'1'",
            "(1)",
            "1F",
            "1D",
        ] {
            // Single-line endpoints count Unicode characters, not UTF-8 bytes.
            let span = Db2SourceSpan {
                end: Db2SourceLocation {
                    line: 1,
                    column: text.chars().count() as u32 + 1,
                },
                ..full_span(text)
            };
            let error =
                materialize_db2_numeric_constant(text, span, Db2NumericConstantLimits::default())
                    .unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::Classification(Code::InvalidSpelling),
                "{text:?}"
            );
            assert_eq!(error.span, span);
        }
        for text in [
            "1E0", "1e0", "+15E1", "2.E5", "-2.2E-1", "+5.E+2", "1E", "1e+",
        ] {
            expect_classification_error(text, Code::UnsupportedExponent);
        }
        for text in ["INF", "-INFINITY", "+inf", "Nan", "-snAN", "+NAN"] {
            expect_classification_error(text, Code::UnsupportedSpecialValue);
        }
        for text in ["1,2", ",5", "5,"] {
            expect_classification_error(text, Code::UnsupportedCommaDecimal);
        }
        for digits in 20..=31 {
            for sign in ["", "+", "-"] {
                for suffix in ["0", "1", "9223372036854775807"] {
                    expect_classification_error(
                        &format!("{sign}{}{suffix}", "0".repeat(digits - suffix.len())),
                        Code::UnsupportedLongInteger,
                    );
                }
            }
        }
        expect_classification_error("-09223372036854775808", Code::UnsupportedLongInteger);
        for (text, line, column) in [
            ("1\n2", 2, 2),
            ("1\r2", 2, 2),
            ("1\r\n2", 2, 2),
            ("1--c\n", 2, 1),
        ] {
            let span = Db2SourceSpan {
                end: Db2SourceLocation { line, column },
                ..full_span(text)
            };
            let error =
                materialize_db2_numeric_constant(text, span, Db2NumericConstantLimits::default())
                    .unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::Classification(Code::InvalidSpelling)
            );
            assert_eq!(error.span, span);
        }
    }

    #[test]
    fn original_utf8_crlf_and_comment_context_spans_are_preserved() {
        for (prefix, line, column) in [
            ("", 1, 1),
            ("  ", 1, 3),
            ("-- context\n", 2, 1),
            ("/* α */\n  ", 2, 3),
            ("/* α */\r  ", 2, 3),
            ("/* α */\r\n  ", 2, 3),
            ("/* α */\r\n\r\n  ", 3, 3),
            ("α😀 ", 1, 4),
        ] {
            for text in ["-2147483648", "+9223372036854775808", "000.00", "1e2"] {
                let source = format!("{prefix}{text}; -- trailing α context\r\n");
                let span = Db2SourceSpan {
                    start_byte: prefix.len(),
                    end_byte: prefix.len() + text.len(),
                    start: Db2SourceLocation { line, column },
                    end: Db2SourceLocation {
                        line,
                        column: column + text.len() as u32,
                    },
                };
                let actual = materialize_db2_numeric_constant(
                    &source,
                    span,
                    Db2NumericConstantLimits::default(),
                );
                if text == "1e2" {
                    let error = actual.unwrap_err();
                    assert_eq!(error.span, span);
                    assert_eq!(
                        error.code,
                        Db2NumericConstantValueErrorCode::Classification(
                            Db2NumericConstantErrorCode::UnsupportedExponent
                        )
                    );
                } else {
                    let result = actual.unwrap();
                    assert_eq!(result.span(), span);
                    let expected = materialize(text).unwrap();
                    assert_eq!(result.value(), expected.value());
                    assert_eq!(result.resolved_type(), expected.resolved_type());
                }
            }
        }
        let source = "/* α */\r\n  -1";
        let span = Db2SourceSpan {
            start_byte: 12,
            end_byte: 14,
            start: Db2SourceLocation { line: 2, column: 3 },
            end: Db2SourceLocation { line: 2, column: 5 },
        };
        let result =
            materialize_db2_numeric_constant(source, span, Db2NumericConstantLimits::default())
                .unwrap();
        assert_eq!(result.span(), span);
        assert_eq!(result.value(), &Db2NumericConstantValue::Integer(-1));
    }

    #[test]
    fn invalid_byte_and_line_column_spans_fail_before_materialization() {
        let source = "α1";
        let span = Db2SourceSpan {
            start_byte: 2,
            end_byte: 3,
            start: Db2SourceLocation { line: 1, column: 2 },
            end: Db2SourceLocation { line: 1, column: 3 },
        };
        assert_eq!(
            materialize_db2_numeric_constant(source, span, Db2NumericConstantLimits::default())
                .unwrap()
                .value(),
            &Db2NumericConstantValue::Integer(1)
        );
        for invalid in [
            Db2SourceSpan {
                start_byte: 1,
                ..span
            },
            Db2SourceSpan {
                end_byte: 4,
                ..span
            },
            Db2SourceSpan {
                start_byte: 3,
                end_byte: 2,
                ..span
            },
            Db2SourceSpan {
                start_byte: usize::MAX,
                end_byte: usize::MAX,
                ..span
            },
            Db2SourceSpan {
                start: Db2SourceLocation { line: 0, column: 2 },
                ..span
            },
            Db2SourceSpan {
                start: Db2SourceLocation { line: 1, column: 3 },
                ..span
            },
            Db2SourceSpan {
                end: Db2SourceLocation { line: 2, column: 1 },
                ..span
            },
        ] {
            let error = materialize_db2_numeric_constant(
                source,
                invalid,
                Db2NumericConstantLimits::default(),
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::InvalidSourceSpan
                )
            );
            assert_eq!(error.span, invalid);
        }
    }

    #[test]
    fn exact_and_one_beyond_source_spelling_and_type_argument_limits() {
        use Db2NumericConstantErrorCode as Code;
        let mut limits = Db2NumericConstantLimits::default();
        limits.max_source_bytes = 4;
        assert!(materialize_db2_numeric_constant("+1.0", full_span("+1.0"), limits).is_ok());
        let source_start = Db2SourceSpan {
            start_byte: 0,
            end_byte: 0,
            start: Db2SourceLocation::START,
            end: Db2SourceLocation::START,
        };
        let error =
            materialize_db2_numeric_constant("+1.00", full_span("+1.00"), limits).unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(Code::SourceTooLarge)
        );
        assert_eq!(error.span, source_start);
        // The bound covers the entire source, including text outside the selected constant.
        let span = Db2SourceSpan {
            start_byte: 3,
            end_byte: 4,
            start: Db2SourceLocation { line: 1, column: 4 },
            end: Db2SourceLocation { line: 1, column: 5 },
        };
        assert!(materialize_db2_numeric_constant("abc1", span, limits).is_ok());
        assert_eq!(
            materialize_db2_numeric_constant("abc1x", span, limits)
                .unwrap_err()
                .code,
            Db2NumericConstantValueErrorCode::Classification(Code::SourceTooLarge)
        );
        limits.max_source_bytes = 100;
        limits.ast.max_literal_bytes = 4;
        assert!(materialize_db2_numeric_constant("+1.0", full_span("+1.0"), limits).is_ok());
        let error =
            materialize_db2_numeric_constant("+1.00", full_span("+1.00"), limits).unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(Code::ConstantTooLarge)
        );
        assert_eq!(error.span, full_span("+1.00"));
        limits.ast.max_list_items = 2;
        assert!(materialize_db2_numeric_constant(".1", full_span(".1"), limits).is_ok());
        limits.ast.max_list_items = 1;
        assert_eq!(
            materialize_db2_numeric_constant(".1", full_span(".1"), limits)
                .unwrap_err()
                .code,
            Db2NumericConstantValueErrorCode::Classification(Code::TypeArgumentsLimit)
        );
        limits.ast.max_expression_nodes = 1;
        limits.ast.max_expression_depth = 1;
        assert!(materialize_db2_numeric_constant("1", full_span("1"), limits).is_ok());
        // No expression/list backend is present; only classifier type-argument bounds apply.
    }

    #[test]
    fn compiled_resource_ceilings_and_invalid_limits_are_inherited() {
        let limits = Db2NumericConstantLimits {
            max_source_bytes: 8 * 1024 * 1024,
            ast: crate::Db2AstLimits {
                max_identifier_bytes: 1024,
                max_name_parts: 16,
                max_literal_bytes: 8 * 1024 * 1024,
                max_expression_nodes: 262_144,
                max_list_items: 65_536,
                max_expression_depth: 1024,
            },
        };
        assert!(materialize_db2_numeric_constant(".1", full_span(".1"), limits).is_ok());
        let mut source = " ".repeat(limits.max_source_bytes - 1);
        source.push('1');
        let end = source.len();
        let span = Db2SourceSpan {
            start_byte: end - 1,
            end_byte: end,
            start: Db2SourceLocation {
                line: 1,
                column: end as u32,
            },
            end: Db2SourceLocation {
                line: 1,
                column: end as u32 + 1,
            },
        };
        assert_eq!(
            materialize_db2_numeric_constant(&source, span, limits)
                .unwrap()
                .value(),
            &Db2NumericConstantValue::Integer(1)
        );
        source.push(' ');
        assert_eq!(
            materialize_db2_numeric_constant(&source, span, limits)
                .unwrap_err()
                .code,
            Db2NumericConstantValueErrorCode::Classification(
                Db2NumericConstantErrorCode::SourceTooLarge
            )
        );
        for max_source_bytes in [0, limits.max_source_bytes + 1] {
            assert_invalid_limits(Db2NumericConstantLimits {
                max_source_bytes,
                ..limits
            });
        }
        for field in 0..6 {
            for beyond in [false, true] {
                let mut invalid = limits;
                let value = match field {
                    0 => &mut invalid.ast.max_identifier_bytes,
                    1 => &mut invalid.ast.max_name_parts,
                    2 => &mut invalid.ast.max_literal_bytes,
                    3 => &mut invalid.ast.max_expression_nodes,
                    4 => &mut invalid.ast.max_list_items,
                    _ => &mut invalid.ast.max_expression_depth,
                };
                *value = if beyond { *value + 1 } else { 0 };
                assert_invalid_limits(invalid);
            }
        }
    }

    fn assert_invalid_limits(limits: Db2NumericConstantLimits) {
        let error = materialize_db2_numeric_constant("1", full_span("1"), limits).unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(
                Db2NumericConstantErrorCode::InvalidLimits
            )
        );
        assert_eq!(error.span.start_byte, 0);
        assert_eq!(error.span.end_byte, 0);
        assert_eq!(error.span.start, Db2SourceLocation::START);
        assert_eq!(error.span.end, Db2SourceLocation::START);
    }

    #[test]
    fn owned_outputs_and_fixed_errors_survive_source_drop() {
        let (result, error) = {
            let source = String::from("-000.00100");
            let limits = Db2NumericConstantLimits::default();
            let result =
                materialize_db2_numeric_constant(&source, full_span(&source), limits).unwrap();
            let source = String::from("1E0");
            let error =
                materialize_db2_numeric_constant(&source, full_span(&source), limits).unwrap_err();
            (result, error)
        };
        assert_eq!(
            result.resolved_type().scalar(),
            &Db2ScalarType::Decimal {
                precision: 8,
                scale: 5
            }
        );
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        let Db2NumericConstantValue::Decimal(value) = result.value() else {
            panic!("expected DECIMAL")
        };
        assert_eq!(value.coefficient(), -100);
        assert_eq!(value.scale(), 5);
        assert_eq!(result.span(), full_span("-000.00100"));
        assert_eq!(result.clone(), result);
        assert_eq!(
            error.code,
            Db2NumericConstantValueErrorCode::Classification(
                Db2NumericConstantErrorCode::UnsupportedExponent
            )
        );
        assert_eq!(error.span, full_span("1E0"));
        assert!(error.message.len() <= 256);
    }

    #[test]
    fn coefficient_guard_is_bounded_and_located() {
        let span = full_span("1");
        assert_eq!(accumulate_coefficient("0", true, span).unwrap(), 0);
        for text in ["", ".", "α", "1x", "+1"] {
            let error = accumulate_coefficient(text, false, span).unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::InvalidCoefficient
            );
            assert_eq!(error.span, span);
            assert!(error.message.len() <= 256);
        }
        for text in ["0".repeat(32), "9".repeat(32), "9".repeat(100)] {
            let error = accumulate_coefficient(&text, false, span).unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::CoefficientOutOfRange
            );
            assert_eq!(error.span, span);
        }
    }
}
