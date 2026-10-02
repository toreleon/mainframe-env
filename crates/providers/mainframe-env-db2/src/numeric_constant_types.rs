//! Pure typing of the declared integer/decimal constant subset, without evaluation.
//!
//! Sources: ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_constantsintro.html, 17915 bytes,
//!   bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8;
//! - db2z_datatypesintro.html, 22904 bytes,
//!   a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570.
//!
//! These are language elements, with no standalone statement-catalog row or
//! execution/coverage claim. This kernel neither invokes nor changes the lexer,
//! AST/expression parsers, conversions, or binding. Longer unpointed spellings
//! whose value fits BIGINT are outside this initial subset, not universally
//! invalid IBM constants. Exponents, special values, comma decimal conventions,
//! and more than 31 digits require later declared constant families.

use crate::{
    Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType, Db2DataType, Db2Nullability, Db2ResolvedType,
    Db2SourceLocation, Db2SourceSpan, resolve_db2_type,
};
use std::fmt;

// Same bounded source envelope as the owned syntax API; no token budget or
// expression/list backend is claimed by this single-spelling typing kernel.
const MAX_SOURCE_BYTES_CEILING: usize = 8 * 1024 * 1024;
const MAX_INTEGER_DIGITS: u32 = 19;
const MAX_DECIMAL_DIGITS: u32 = 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2NumericConstantLimits {
    /// Bounds the entire original source, including text outside the constant.
    pub max_source_bytes: usize,
    /// Existing AST limits also bound spelling bytes and type arguments.
    pub ast: Db2AstLimits,
}

impl Default for Db2NumericConstantLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            ast: Db2AstLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2NumericConstantErrorCode {
    InvalidLimits,
    SourceTooLarge,
    InvalidSourceSpan,
    ConstantTooLarge,
    InvalidSpelling,
    TooManyDigits,
    UnsupportedExponent,
    UnsupportedSpecialValue,
    UnsupportedCommaDecimal,
    UnsupportedLongInteger,
    TypeArgumentsLimit,
    InvalidResolvedType,
}

/// Fixed-size diagnostic: no source text or unbounded error string is retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NumericConstantError {
    pub code: Db2NumericConstantErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl Db2NumericConstantError {
    const fn new(
        code: Db2NumericConstantErrorCode,
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

impl fmt::Display for Db2NumericConstantError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2NumericConstantError {}

/// Owned existing semantic type and its verified original-source location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NumericConstantType {
    resolved_type: Db2ResolvedType,
    span: Db2SourceSpan,
}

impl Db2NumericConstantType {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.resolved_type
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
}

/// Classify exactly `source[span.start_byte..span.end_byte]`, with at most one
/// adjacent leading sign. The caller selects the spelling, not a statement or
/// expression: surrounding text is untouched and embedded trivia fails. Both
/// byte offsets and line/column endpoints must match the original UTF-8 source.
/// Invalid-span errors preserve the supplied span for caller diagnosis; a
/// source-envelope/limit error uses the bounded source-start point instead.
pub fn classify_db2_numeric_constant(
    source: &str,
    span: Db2SourceSpan,
    limits: Db2NumericConstantLimits,
) -> Result<Db2NumericConstantType, Db2NumericConstantError> {
    use Db2NumericConstantErrorCode as Code;
    let source_start = Db2SourceSpan {
        start_byte: 0,
        end_byte: 0,
        start: Db2SourceLocation::START,
        end: Db2SourceLocation::START,
    };
    if limits.max_source_bytes == 0
        || limits.max_source_bytes > MAX_SOURCE_BYTES_CEILING
        || limits.ast.validate().is_err()
    {
        return Err(Db2NumericConstantError::new(
            Code::InvalidLimits,
            source_start,
            "numeric constant limits are zero or exceed compiled ceilings",
        ));
    }
    if source.len() > limits.max_source_bytes {
        return Err(Db2NumericConstantError::new(
            Code::SourceTooLarge,
            source_start,
            "original source exceeds the configured byte limit",
        ));
    }
    let spelling = source.get(span.start_byte..span.end_byte).ok_or_else(|| {
        Db2NumericConstantError::new(
            Code::InvalidSourceSpan,
            span,
            "constant byte span is outside the source or splits a UTF-8 character",
        )
    })?;
    if location_at(source, span.start_byte) != span.start
        || location_at(source, span.end_byte) != span.end
    {
        return Err(Db2NumericConstantError::new(
            Code::InvalidSourceSpan,
            span,
            "constant line/column endpoints do not match the original source",
        ));
    }
    if spelling.len() > limits.ast.max_literal_bytes {
        return Err(Db2NumericConstantError::new(
            Code::ConstantTooLarge,
            span,
            "constant spelling exceeds the configured AST literal byte limit",
        ));
    }
    let (negative, unsigned) = match spelling.as_bytes().first() {
        Some(b'-') => (true, &spelling[1..]),
        Some(b'+') => (false, &spelling[1..]),
        _ => (false, spelling),
    };
    if ["INF", "INFINITY", "NAN", "SNAN"]
        .iter()
        .any(|special| unsigned.eq_ignore_ascii_case(special))
    {
        return Err(Db2NumericConstantError::new(
            Code::UnsupportedSpecialValue,
            span,
            "decimal floating-point special values are outside this constant subset",
        ));
    }
    let mut precision = 0;
    let mut scale = 0;
    let mut pointed = false;
    for byte in unsigned.bytes() {
        match byte {
            b'0'..=b'9' => {
                precision += 1;
                if pointed {
                    scale += 1;
                }
                if precision > MAX_DECIMAL_DIGITS {
                    return Err(Db2NumericConstantError::new(
                        Code::TooManyDigits,
                        span,
                        "more than 31 digits is outside the integer/decimal constant subset",
                    ));
                }
            }
            b'.' if !pointed => pointed = true,
            b'E' | b'e' => {
                return Err(Db2NumericConstantError::new(
                    Code::UnsupportedExponent,
                    span,
                    "exponent forms require a later floating-point constant subset",
                ));
            }
            b',' => {
                return Err(Db2NumericConstantError::new(
                    Code::UnsupportedCommaDecimal,
                    span,
                    "comma decimal conventions require context outside this subset",
                ));
            }
            _ => {
                return Err(Db2NumericConstantError::new(
                    Code::InvalidSpelling,
                    span,
                    "expected ASCII digits, one decimal point and an optional adjacent sign",
                ));
            }
        }
    }
    if precision == 0 {
        return Err(Db2NumericConstantError::new(
            Code::InvalidSpelling,
            span,
            "a numeric constant must contain at least one ASCII digit",
        ));
    }
    let (kind, arguments) = if pointed {
        (Db2BuiltInType::Decimal, vec![precision, scale])
    } else {
        // Comparing bounded magnitudes avoids evaluation, overflow and sign
        // normalization. Leading zeros affect decimal precision, not range.
        let magnitude = unsigned.trim_start_matches('0');
        let integer_bound = if negative { "2147483648" } else { "2147483647" };
        let bigint_bound = if negative {
            "9223372036854775808"
        } else {
            "9223372036854775807"
        };
        if within_magnitude(magnitude, bigint_bound) {
            if precision > MAX_INTEGER_DIGITS {
                return Err(Db2NumericConstantError::new(
                    Code::UnsupportedLongInteger,
                    span,
                    "unpointed in-range spellings longer than 19 digits are outside this initial subset",
                ));
            }
            let kind = if within_magnitude(magnitude, integer_bound) {
                Db2BuiltInType::Integer
            } else {
                Db2BuiltInType::BigInt
            };
            (kind, Vec::new())
        } else {
            (Db2BuiltInType::Decimal, vec![precision, 0])
        }
    };
    let syntax = Db2BuiltInDataType::new(kind, arguments, false, limits.ast).map_err(|_| {
        Db2NumericConstantError::new(
            Code::TypeArgumentsLimit,
            span,
            "constant type arguments exceed the configured AST list limit",
        )
    })?;
    let resolved_type = resolve_db2_type(&Db2DataType::BuiltIn(syntax), Db2Nullability::NotNull)
        .map_err(|_| {
            Db2NumericConstantError::new(
                Code::InvalidResolvedType,
                span,
                "constant shape could not be constructed by the existing type authority",
            )
        })?;
    Ok(Db2NumericConstantType {
        resolved_type,
        span,
    })
}

fn within_magnitude(magnitude: &str, bound: &str) -> bool {
    magnitude.len() < bound.len() || (magnitude.len() == bound.len() && magnitude <= bound)
}

fn location_at(source: &str, end_byte: usize) -> Db2SourceLocation {
    let mut location = Db2SourceLocation::START;
    let mut previous_cr = false;
    for character in source[..end_byte].chars() {
        if character == '\r' || (character == '\n' && !previous_cr) {
            location.line += 1;
            location.column = 1;
        } else if character != '\n' || !previous_cr {
            location.column += 1;
        }
        previous_cr = character == '\r';
    }
    location
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Db2ScalarType, Db2SyntaxDiagnosticCode, Db2SyntaxLimits, lex_db2};

    fn full_span(spelling: &str) -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: 0,
            end_byte: spelling.len(),
            start: Db2SourceLocation::START,
            end: location_at(spelling, spelling.len()),
        }
    }

    fn classify(spelling: &str) -> Result<Db2NumericConstantType, Db2NumericConstantError> {
        classify_db2_numeric_constant(
            spelling,
            full_span(spelling),
            Db2NumericConstantLimits::default(),
        )
    }

    fn expect_type(spelling: &str, scalar: Db2ScalarType) {
        let result = classify(spelling).unwrap_or_else(|error| panic!("{spelling:?}: {error}"));
        assert_eq!(result.resolved_type().scalar(), &scalar, "{spelling:?}");
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(result.span(), full_span(spelling));
    }

    #[test]
    fn inclusive_integer_and_bigint_boundaries() {
        for spelling in [
            "2147483647",
            "+2147483647",
            "-2147483648",
            "32767",
            "-32768",
            "1",
        ] {
            expect_type(spelling, Db2ScalarType::Integer);
        }
        for spelling in [
            "2147483648",
            "+2147483648",
            "-2147483649",
            "9223372036854775807",
            "+9223372036854775807",
            "-9223372036854775808",
        ] {
            expect_type(spelling, Db2ScalarType::BigInt);
        }
        for spelling in [
            "9223372036854775808",
            "+9223372036854775808",
            "-9223372036854775809",
        ] {
            expect_type(
                spelling,
                Db2ScalarType::Decimal {
                    precision: 19,
                    scale: 0,
                },
            );
        }
    }

    #[test]
    fn zero_sign_and_leading_zeros_do_not_change_binary_range() {
        for digits in 1..=19 {
            for sign in ["", "+", "-"] {
                expect_type(
                    &format!("{sign}{}", "0".repeat(digits)),
                    Db2ScalarType::Integer,
                );
            }
        }
        for spelling in [
            "0000000000000000001",
            "+0000000002147483647",
            "-0000000002147483648",
        ] {
            expect_type(spelling, Db2ScalarType::Integer);
        }
        for spelling in ["0000000002147483648", "-0000000002147483649"] {
            expect_type(spelling, Db2ScalarType::BigInt);
        }
        for digits in 20..=31 {
            for sign in ["", "+", "-"] {
                for suffix in ["0", "1", "9223372036854775807"] {
                    let text = format!("{sign}{}{suffix}", "0".repeat(digits - suffix.len()));
                    assert_eq!(
                        classify(&text).unwrap_err().code,
                        Db2NumericConstantErrorCode::UnsupportedLongInteger
                    );
                }
            }
        }
        assert_eq!(
            classify("-09223372036854775808").unwrap_err().code,
            Db2NumericConstantErrorCode::UnsupportedLongInteger
        );
        expect_type(
            "09223372036854775808",
            Db2ScalarType::Decimal {
                precision: 20,
                scale: 0,
            },
        );
    }

    #[test]
    fn decimal_precision_and_scale_include_every_zero() {
        for (text, precision, scale) in [
            (".5", 1, 1),
            ("5.", 1, 0),
            ("000.00", 5, 2),
            ("025.50", 5, 2),
            ("1000.", 4, 0),
            ("15.", 2, 0),
            ("375893333333333333333.33", 23, 2),
            ("0.00000", 6, 5),
        ] {
            for sign in ["", "+", "-"] {
                expect_type(
                    &format!("{sign}{text}"),
                    Db2ScalarType::Decimal { precision, scale },
                );
            }
        }
        for digits in 20..=31 {
            for sign in ["", "+", "-"] {
                expect_type(
                    &format!("{sign}{}", "9".repeat(digits)),
                    Db2ScalarType::Decimal {
                        precision: digits as u32,
                        scale: 0,
                    },
                );
            }
        }
    }

    #[test]
    fn exact_and_one_beyond_digit_bounds_at_every_decimal_position() {
        for digits in [31, 32] {
            for point in 0..=digits {
                for sign in ["", "+", "-"] {
                    let text =
                        format!("{sign}{}.{}", "0".repeat(point), "0".repeat(digits - point));
                    if digits == 31 {
                        expect_type(
                            &text,
                            Db2ScalarType::Decimal {
                                precision: 31,
                                scale: (digits - point) as u32,
                            },
                        );
                    } else {
                        assert_eq!(
                            classify(&text).unwrap_err().code,
                            Db2NumericConstantErrorCode::TooManyDigits
                        );
                    }
                }
            }
        }
        for sign in ["", "+", "-"] {
            assert_eq!(
                classify(&format!("{sign}{}", "9".repeat(32)))
                    .unwrap_err()
                    .code,
                Db2NumericConstantErrorCode::TooManyDigits
            );
        }
    }

    #[test]
    fn malformed_and_undeclared_spellings_fail_with_fixed_located_errors() {
        use Db2NumericConstantErrorCode as Code;
        for (texts, code) in [
            (
                vec![
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
                    "1 2",
                    " 1",
                    "1 ",
                    "+ 1",
                    "1\n2",
                    "1\t2",
                    "1/*c*/2",
                    "-/*c*/1",
                    "1--c\n",
                    "１２",
                    "١٢",
                    "−1",
                    "1_0",
                    "1\0",
                    "DECFLOAT(16)",
                    "NULL",
                    "'1'",
                    "(1)",
                ],
                Code::InvalidSpelling,
            ),
            (
                vec![
                    "1E0", "1e0", "+15E1", "2.E5", "-2.2E-1", "+5.E+2", "1E", "1e+",
                ],
                Code::UnsupportedExponent,
            ),
            (
                vec!["INF", "-INFINITY", "+inf", "Nan", "-snAN", "+NAN"],
                Code::UnsupportedSpecialValue,
            ),
            (vec!["1,2", ",5", "5,"], Code::UnsupportedCommaDecimal),
        ] {
            for text in texts {
                let error = classify(text).unwrap_err();
                assert_eq!(error.code, code, "{text:?}");
                assert_eq!(error.span, full_span(text));
                assert!(error.message.len() <= 256);
                assert!(error.to_string().contains("1:1"));
            }
        }
    }

    #[test]
    fn relocated_spans_match_utf8_lf_cr_and_crlf_original_source() {
        for prefix in [
            "",
            "  ",
            "-- context\n",
            "/* α */\n  ",
            "/* α */\r  ",
            "/* α */\r\n  ",
            "/* α */\r\n\r\n  ",
        ] {
            for text in ["-2147483648", "+9223372036854775808", "000.00", "1e2"] {
                let source = format!("{prefix}{text}; -- trailing context");
                let span = Db2SourceSpan {
                    start_byte: prefix.len(),
                    end_byte: prefix.len() + text.len(),
                    start: location_at(&source, prefix.len()),
                    end: location_at(&source, prefix.len() + text.len()),
                };
                let actual = classify_db2_numeric_constant(
                    &source,
                    span,
                    Db2NumericConstantLimits::default(),
                );
                match actual {
                    Ok(result) => {
                        assert_eq!(result.span(), span);
                        assert_eq!(
                            result.resolved_type(),
                            classify(text).unwrap().resolved_type()
                        );
                    }
                    Err(error) => {
                        assert_eq!(text, "1e2");
                        assert_eq!(error.span, span);
                        assert_eq!(error.code, Db2NumericConstantErrorCode::UnsupportedExponent);
                    }
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
        assert_eq!(
            classify_db2_numeric_constant(source, span, Db2NumericConstantLimits::default())
                .unwrap()
                .span(),
            span
        );
    }

    #[test]
    fn invalid_byte_and_line_column_spans_fail_closed() {
        let source = "α1";
        let span = Db2SourceSpan {
            start_byte: 2,
            end_byte: 3,
            start: Db2SourceLocation { line: 1, column: 2 },
            end: Db2SourceLocation { line: 1, column: 3 },
        };
        assert!(
            classify_db2_numeric_constant(source, span, Db2NumericConstantLimits::default())
                .is_ok()
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
            let error =
                classify_db2_numeric_constant(source, invalid, Db2NumericConstantLimits::default())
                    .unwrap_err();
            assert_eq!(error.code, Db2NumericConstantErrorCode::InvalidSourceSpan);
            assert_eq!(error.span, invalid);
        }
    }

    #[test]
    fn configured_source_literal_and_type_argument_bounds() {
        let mut limits = Db2NumericConstantLimits {
            max_source_bytes: 4,
            ..Db2NumericConstantLimits::default()
        };
        assert!(classify_db2_numeric_constant("+1.0", full_span("+1.0"), limits).is_ok());
        assert_eq!(
            classify_db2_numeric_constant("+1.00", full_span("+1.00"), limits)
                .unwrap_err()
                .code,
            Db2NumericConstantErrorCode::SourceTooLarge
        );
        let span = Db2SourceSpan {
            start_byte: 3,
            end_byte: 4,
            start: Db2SourceLocation { line: 1, column: 4 },
            end: Db2SourceLocation { line: 1, column: 5 },
        };
        assert_eq!(
            classify_db2_numeric_constant("abc1x", span, limits)
                .unwrap_err()
                .code,
            Db2NumericConstantErrorCode::SourceTooLarge
        );
        limits.max_source_bytes = 100;
        limits.ast.max_literal_bytes = 4;
        assert!(classify_db2_numeric_constant("+1.0", full_span("+1.0"), limits).is_ok());
        assert_eq!(
            classify_db2_numeric_constant("+1.00", full_span("+1.00"), limits)
                .unwrap_err()
                .code,
            Db2NumericConstantErrorCode::ConstantTooLarge
        );
        limits.ast.max_list_items = 2;
        assert!(classify_db2_numeric_constant(".1", full_span(".1"), limits).is_ok());
        limits.ast.max_list_items = 1;
        assert_eq!(
            classify_db2_numeric_constant(".1", full_span(".1"), limits)
                .unwrap_err()
                .code,
            Db2NumericConstantErrorCode::TypeArgumentsLimit
        );
        limits.ast.max_expression_nodes = 1;
        limits.ast.max_expression_depth = 1;
        assert!(classify_db2_numeric_constant("1", full_span("1"), limits).is_ok());
    }

    #[test]
    fn compiled_source_and_ast_ceilings_and_invalid_limits() {
        let max_ast = Db2AstLimits {
            max_identifier_bytes: 1024,
            max_name_parts: 16,
            max_literal_bytes: 8 * 1024 * 1024,
            max_expression_nodes: 262_144,
            max_list_items: 65_536,
            max_expression_depth: 1024,
        };
        let max_limits = Db2NumericConstantLimits {
            max_source_bytes: MAX_SOURCE_BYTES_CEILING,
            ast: max_ast,
        };
        assert!(classify_db2_numeric_constant(".1", full_span(".1"), max_limits).is_ok());
        let mut source = " ".repeat(MAX_SOURCE_BYTES_CEILING - 1);
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
        assert!(classify_db2_numeric_constant(&source, span, max_limits).is_ok());
        source.push(' ');
        assert_eq!(
            classify_db2_numeric_constant(&source, span, max_limits)
                .unwrap_err()
                .code,
            Db2NumericConstantErrorCode::SourceTooLarge
        );
        for invalid in [
            Db2NumericConstantLimits {
                max_source_bytes: 0,
                ..max_limits
            },
            Db2NumericConstantLimits {
                max_source_bytes: MAX_SOURCE_BYTES_CEILING + 1,
                ..max_limits
            },
        ] {
            assert_eq!(
                classify_db2_numeric_constant("1", full_span("1"), invalid)
                    .unwrap_err()
                    .code,
                Db2NumericConstantErrorCode::InvalidLimits
            );
        }
        for field in 0..6 {
            for beyond in [false, true] {
                let mut limits = max_limits;
                let value = match field {
                    0 => &mut limits.ast.max_identifier_bytes,
                    1 => &mut limits.ast.max_name_parts,
                    2 => &mut limits.ast.max_literal_bytes,
                    3 => &mut limits.ast.max_expression_nodes,
                    4 => &mut limits.ast.max_list_items,
                    _ => &mut limits.ast.max_expression_depth,
                };
                *value = if beyond { *value + 1 } else { 0 };
                assert_eq!(
                    classify_db2_numeric_constant("1", full_span("1"), limits)
                        .unwrap_err()
                        .code,
                    Db2NumericConstantErrorCode::InvalidLimits
                );
            }
        }
    }

    #[test]
    fn output_and_error_survive_source_and_limits_drop() {
        let (result, error) = {
            let source = String::from("000.00");
            let limits = Db2NumericConstantLimits::default();
            let result =
                classify_db2_numeric_constant(&source, full_span(&source), limits).unwrap();
            let source = String::from("1E0");
            let error =
                classify_db2_numeric_constant(&source, full_span(&source), limits).unwrap_err();
            (result, error)
        };
        assert_eq!(
            result.resolved_type().scalar(),
            &Db2ScalarType::Decimal {
                precision: 5,
                scale: 2
            }
        );
        assert_eq!(error.code, Db2NumericConstantErrorCode::UnsupportedExponent);
        assert_eq!(error.span.end_byte, 3);
    }

    #[test]
    fn typing_does_not_lift_existing_lexer_or_expression_fences() {
        for text in ["1E0", "1e+2", "1D", "1F"] {
            assert_eq!(
                lex_db2(text, Db2SyntaxLimits::default()).unwrap_err().code,
                Db2SyntaxDiagnosticCode::UnsupportedNumericConstant
            );
        }
        assert!(classify("5.").is_ok());
        assert!(
            crate::parse_db2_expression("5.", Db2SyntaxLimits::default(), Db2AstLimits::default())
                .is_err()
        );
        assert!(
            crate::parse_db2_expression(
                "TRUE",
                Db2SyntaxLimits::default(),
                Db2AstLimits::default()
            )
            .is_err()
        );
    }
}
