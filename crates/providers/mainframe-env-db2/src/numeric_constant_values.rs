//! Exact natural numeric literal values through the existing classifier and decimal primitive.
//!
//! Sources: ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_constantsintro.html, 17915 bytes,
//!   bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8;
//! - db2z_datatypesintro.html, 22904 bytes,
//!   a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570.
//! - db2z_numericassignments.html, 18430 bytes,
//!   3f6ba8a8290190c2e36590aa348fb816fff6a166c7483b97d1b51cc53bbd8302;
//! - db2z_assignmentandcomparison.html, 40605 bytes,
//!   2cc975449a25d1d825cbd8a6551f5ea9c6dfc6ed5dc9956643f0e1dc9948af8a.
//!
//! Language elements have no standalone statement-catalog row. This private
//! kernel materializes natural INTEGER/BIGINT/DECIMAL constants and assigns only
//! their opaque proofs to resolved exact numeric targets, with no expression
//! evaluation, default legality, wire format, catalog
//! identity, cell persistence, backend, execution or coverage claim. The sole
//! spelling/span/type authority remains the existing numeric classifier, including
//! its deferred exponent, special, comma, long unpointed in-range and >31-digit
//! forms. Long unpointed spellings are outside this subset, not universally
//! invalid IBM constants. No shared lexer or expression fence is lifted.

use crate::numeric_constant_types::classify_db2_located_numeric_operand;
use crate::{
    Db2NumericConstantError, Db2NumericConstantErrorCode, Db2NumericConstantLimits,
    Db2NumericConstantType, Db2ResolvedType, Db2ScalarType, Db2SourceSpan,
    classify_db2_numeric_constant,
};
use mainframe_env_encoding::DecimalValue;
use std::fmt;

pub mod decfloat_assignment;

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
    let constant_type = classify_db2_numeric_constant(source, span, limits)?;
    // Classification already verified these UTF-8 boundaries and endpoints.
    let spelling = &source[span.start_byte..span.end_byte];
    let (negative, unsigned) = match spelling.as_bytes().first() {
        Some(b'-') => (true, &spelling[1..]),
        Some(b'+') => (false, &spelling[1..]),
        _ => (false, spelling),
    };
    materialize_parts(constant_type, unsigned, negative)
}

/// Materialize an original located sign/number operand, permitting only the
/// existing lexer's intervening trivia. The classifier verifies the complete
/// source envelope, limits, component endpoints, ordering and actual tokens
/// before coefficient accumulation. Output owns its natural type/value and the
/// original combined operand span; it does not establish default legality.
pub fn materialize_db2_located_numeric_operand(
    source: &str,
    operand: Db2SourceSpan,
    sign: Option<Db2SourceSpan>,
    number: Db2SourceSpan,
    numeric_limits: Db2NumericConstantLimits,
    syntax_limits: crate::Db2SyntaxLimits,
) -> Result<Db2MaterializedNumericConstant, Db2NumericConstantValueError> {
    let constant_type = classify_db2_located_numeric_operand(
        source,
        operand,
        sign,
        number,
        numeric_limits,
        syntax_limits,
    )?;
    // The classifier proved both components against the same original source.
    let negative = sign.is_some_and(|span| source.as_bytes()[span.start_byte] == b'-');
    materialize_parts(
        constant_type,
        &source[number.start_byte..number.end_byte],
        negative,
    )
}

fn materialize_parts(
    constant_type: Db2NumericConstantType,
    unsigned: &str,
    negative: bool,
) -> Result<Db2MaterializedNumericConstant, Db2NumericConstantValueError> {
    use Db2NumericConstantValueErrorCode as Code;
    let span = constant_type.span();
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

/// Matching exact target value, not a SQL datatype validation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2AssignedNumericValue {
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Decimal(DecimalValue),
}

/// Conversion observations only; no SQLCA warning or SQLCODE is inferred.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NumericAssignmentConversion {
    compatibility: crate::Db2AssignmentCompatibility,
    temporary_decimal: Option<Db2ResolvedType>,
    discarded_fractional_digits: u32,
    discarded_nonzero: bool,
}

impl Db2NumericAssignmentConversion {
    #[must_use]
    pub const fn compatibility(&self) -> crate::Db2AssignmentCompatibility {
        self.compatibility
    }

    /// Integer-to-decimal intermediate attributes, resolved by the type owner.
    #[must_use]
    pub const fn temporary_decimal(&self) -> Option<&Db2ResolvedType> {
        self.temporary_decimal.as_ref()
    }

    #[must_use]
    pub const fn discarded_fractional_digits(&self) -> u32 {
        self.discarded_fractional_digits
    }

    #[must_use]
    pub const fn discarded_nonzero(&self) -> bool {
        self.discarded_nonzero
    }
}

/// Owned validated target/value pairing and the literal's verified original span.
/// Only assignment from a materialized literal can construct this proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2AssignedNumericConstant {
    target_type: Db2ResolvedType,
    value: Db2AssignedNumericValue,
    span: Db2SourceSpan,
    conversion: Db2NumericAssignmentConversion,
}

impl Db2AssignedNumericConstant {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.target_type
    }

    #[must_use]
    pub const fn value(&self) -> &Db2AssignedNumericValue {
        &self.value
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }

    #[must_use]
    pub const fn conversion(&self) -> &Db2NumericAssignmentConversion {
        &self.conversion
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2NumericAssignmentErrorCode {
    IncompatibleTypes,
    UnsupportedFloatingTarget,
    UnsupportedDecFloatTarget,
    UnsupportedNonNumericTarget,
    ValueOutOfRange,
    InvalidDecimalValue,
    InvalidTemporaryType,
}

/// Fixed-size located conversion failure, independent of source/context lifetime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2NumericAssignmentError {
    pub code: Db2NumericAssignmentErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl fmt::Display for Db2NumericAssignmentError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2NumericAssignmentError {}

/// Assign an opaque natural literal proof to an existing validated exact target.
/// Fractional digits are eliminated toward zero, never rounded. Whole-part and
/// target coefficient bounds are checked before scale-up multiplication or output
/// admission. No source, arbitrary coefficient, type syntax, or provenance count
/// is accepted here. Binder applicability and default precision restrictions are
/// separate obligations; this does not execute SQL or assign host memory.
pub fn assign_db2_numeric_constant(
    source: &Db2MaterializedNumericConstant,
    target: &Db2ResolvedType,
) -> Result<Db2AssignedNumericConstant, Db2NumericAssignmentError> {
    use Db2NumericAssignmentErrorCode as Code;
    let failure = |code, message| Db2NumericAssignmentError {
        code,
        span: source.span(),
        message,
    };
    let compatibility = crate::classify_db2_assignment(source.resolved_type(), target);
    if !matches!(
        compatibility,
        crate::Db2AssignmentCompatibility::Compatible { .. }
    ) {
        return Err(failure(
            Code::IncompatibleTypes,
            "literal and target types are incompatible",
        ));
    }
    let target_scale = match target.scalar() {
        Db2ScalarType::SmallInt | Db2ScalarType::Integer | Db2ScalarType::BigInt => 0,
        Db2ScalarType::Decimal { scale, .. } => *scale,
        Db2ScalarType::Real | Db2ScalarType::Double | Db2ScalarType::Float { .. } => {
            return Err(failure(
                Code::UnsupportedFloatingTarget,
                "floating assignment is not implemented",
            ));
        }
        Db2ScalarType::DecFloat { .. } => {
            return Err(failure(
                Code::UnsupportedDecFloatTarget,
                "DECFLOAT assignment is not implemented",
            ));
        }
        _ => {
            return Err(failure(
                Code::UnsupportedNonNumericTarget,
                "compatible nonnumeric conversion is not implemented",
            ));
        }
    };
    let (coefficient, source_scale) = match source.value() {
        Db2NumericConstantValue::Integer(value) => (i128::from(*value), 0),
        Db2NumericConstantValue::BigInt(value) => (i128::from(*value), 0),
        Db2NumericConstantValue::Decimal(value) => (value.coefficient(), u32::from(value.scale())),
    };
    let overflow = || {
        failure(
            Code::ValueOutOfRange,
            "whole part or exact coefficient exceeds the target range",
        )
    };
    // Both scales/precision are bounded by the existing classifier and resolver
    // to 31. 10^31 fits i128; no unbounded power, string or allocation is needed.
    let whole = coefficient / 10i128.pow(source_scale);
    if let Db2ScalarType::Decimal { precision, scale } = target.scalar()
        && whole.unsigned_abs() >= 10u128.pow(precision - scale)
    {
        return Err(overflow());
    }
    let discarded_fractional_digits = source_scale.saturating_sub(target_scale);
    let divisor = 10i128.pow(discarded_fractional_digits);
    let discarded_nonzero = coefficient % divisor != 0;
    let mut assigned = coefficient / divisor;
    if let Db2ScalarType::Decimal { precision, .. } = target.scalar() {
        let maximum = 10i128.pow(*precision) - 1;
        let multiplier = 10i128.pow(target_scale.saturating_sub(source_scale));
        // Reject before multiplication, including a 31-digit whole coefficient
        // assigned to scale 31, which would otherwise exceed i128 capacity.
        if assigned.unsigned_abs() > (maximum / multiplier) as u128 {
            return Err(overflow());
        }
        assigned = assigned.checked_mul(multiplier).ok_or_else(overflow)?;
    }
    let value = match target.scalar() {
        Db2ScalarType::SmallInt => {
            Db2AssignedNumericValue::SmallInt(i16::try_from(assigned).map_err(|_| overflow())?)
        }
        Db2ScalarType::Integer => {
            Db2AssignedNumericValue::Integer(i32::try_from(assigned).map_err(|_| overflow())?)
        }
        Db2ScalarType::BigInt => {
            Db2AssignedNumericValue::BigInt(i64::try_from(assigned).map_err(|_| overflow())?)
        }
        Db2ScalarType::Decimal { .. } => Db2AssignedNumericValue::Decimal(
            DecimalValue::new(assigned, target_scale as u8).map_err(|_| {
                failure(
                    Code::InvalidDecimalValue,
                    "decimal primitive rejected the assigned value",
                )
            })?,
        ),
        _ => unreachable!("unsupported target rejected before conversion"),
    };
    let temporary_precision = if matches!(target.scalar(), Db2ScalarType::Decimal { .. }) {
        match source.value() {
            Db2NumericConstantValue::Integer(_) => Some(11),
            Db2NumericConstantValue::BigInt(_) => Some(19),
            Db2NumericConstantValue::Decimal(_) => None,
        }
    } else {
        None
    };
    // Trace attributes come from natural source kind, never the value's size.
    // All range checks precede this bounded two-argument syntax allocation.
    let temporary_decimal = temporary_precision
        .map(|precision| {
            let syntax = crate::Db2BuiltInDataType::new(
                crate::Db2BuiltInType::Decimal,
                vec![precision, 0],
                false,
                crate::Db2AstLimits::default(),
            )
            .map_err(|_| {
                failure(
                    Code::InvalidTemporaryType,
                    "temporary decimal syntax was rejected",
                )
            })?;
            crate::resolve_db2_type(
                &crate::Db2DataType::BuiltIn(syntax),
                source.resolved_type().nullability(),
            )
            .map_err(|_| {
                failure(
                    Code::InvalidTemporaryType,
                    "temporary decimal type was rejected",
                )
            })
        })
        .transpose()?;
    Ok(Db2AssignedNumericConstant {
        target_type: target.clone(),
        value,
        span: source.span(),
        conversion: Db2NumericAssignmentConversion {
            compatibility,
            temporary_decimal,
            discarded_fractional_digits,
            discarded_nonzero,
        },
    })
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
        let mut limits = Db2NumericConstantLimits {
            max_source_bytes: 4,
            ..Db2NumericConstantLimits::default()
        };
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

    fn assignment_target(
        kind: crate::Db2BuiltInType,
        arguments: &[u32],
        nullability: Db2Nullability,
    ) -> Db2ResolvedType {
        let syntax = crate::Db2BuiltInDataType::new(
            kind,
            arguments.to_vec(),
            false,
            crate::Db2AstLimits::default(),
        )
        .unwrap();
        crate::resolve_db2_type(&crate::Db2DataType::BuiltIn(syntax), nullability).unwrap()
    }

    fn assign(
        text: &str,
        kind: crate::Db2BuiltInType,
        arguments: &[u32],
    ) -> Result<Db2AssignedNumericConstant, Db2NumericAssignmentError> {
        assign_db2_numeric_constant(
            &materialize(text).unwrap(),
            &assignment_target(kind, arguments, Db2Nullability::NotNull),
        )
    }

    fn expect_assignment_error(
        text: &str,
        kind: crate::Db2BuiltInType,
        arguments: &[u32],
        code: Db2NumericAssignmentErrorCode,
    ) {
        let error = assign(text, kind, arguments).unwrap_err();
        assert_eq!(error.code, code, "{text:?} -> {kind:?}{arguments:?}");
        assert_eq!(error.span, full_span(text));
        assert!(error.message.len() <= 256);
        assert!(error.to_string().contains("1:1"));
    }

    #[test]
    fn assignment_binary_integer_endpoints_and_natural_source_kinds() {
        use crate::Db2BuiltInType as Kind;
        use Db2AssignedNumericValue as Value;
        for (text, kind, expected) in [
            ("32767", Kind::SmallInt, Value::SmallInt(32_767)),
            ("-32768", Kind::SmallInt, Value::SmallInt(-32_768)),
            ("2147483647", Kind::Integer, Value::Integer(2_147_483_647)),
            ("-2147483648", Kind::Integer, Value::Integer(-2_147_483_648)),
            ("9223372036854775807", Kind::BigInt, Value::BigInt(i64::MAX)),
            (
                "-9223372036854775808",
                Kind::BigInt,
                Value::BigInt(i64::MIN),
            ),
            ("1", Kind::SmallInt, Value::SmallInt(1)),
            ("-1", Kind::Integer, Value::Integer(-1)),
            ("1", Kind::BigInt, Value::BigInt(1)),
            (
                "+0000000002147483648",
                Kind::BigInt,
                Value::BigInt(2_147_483_648),
            ),
            (
                "-0000000002147483649",
                Kind::BigInt,
                Value::BigInt(-2_147_483_649),
            ),
        ] {
            let result = assign(text, kind, &[]).unwrap();
            assert_eq!(result.value(), &expected);
            assert_eq!(
                result.resolved_type(),
                &assignment_target(kind, &[], Db2Nullability::NotNull)
            );
            assert_eq!(result.span(), full_span(text));
            assert_eq!(result.conversion().discarded_fractional_digits(), 0);
            assert!(!result.conversion().discarded_nonzero());
            assert!(result.conversion().temporary_decimal().is_none());
        }
        for (kind, texts) in [
            (Kind::SmallInt, ["32768", "-32769"]),
            (Kind::Integer, ["2147483648", "-2147483649"]),
            (
                Kind::BigInt,
                ["9223372036854775808", "-9223372036854775809"],
            ),
        ] {
            for text in texts {
                expect_assignment_error(
                    text,
                    kind,
                    &[],
                    Db2NumericAssignmentErrorCode::ValueOutOfRange,
                );
            }
        }
    }

    #[test]
    fn assignment_fractional_integer_endpoint_neighborhoods_truncate_before_range_checks() {
        use crate::Db2BuiltInType as Kind;
        use Db2AssignedNumericValue as Value;
        for (kind, cases) in [
            (
                Kind::SmallInt,
                [
                    ("32767.999", Value::SmallInt(32_767)),
                    ("32766.999", Value::SmallInt(32_766)),
                    ("-32768.999", Value::SmallInt(-32_768)),
                    ("-32767.999", Value::SmallInt(-32_767)),
                ],
            ),
            (
                Kind::Integer,
                [
                    ("2147483647.999", Value::Integer(i32::MAX)),
                    ("2147483646.999", Value::Integer(2_147_483_646)),
                    ("-2147483648.999", Value::Integer(i32::MIN)),
                    ("-2147483647.999", Value::Integer(-2_147_483_647)),
                ],
            ),
            (
                Kind::BigInt,
                [
                    ("9223372036854775807.999", Value::BigInt(i64::MAX)),
                    (
                        "9223372036854775806.999",
                        Value::BigInt(9_223_372_036_854_775_806),
                    ),
                    ("-9223372036854775808.999", Value::BigInt(i64::MIN)),
                    (
                        "-9223372036854775807.999",
                        Value::BigInt(-9_223_372_036_854_775_807),
                    ),
                ],
            ),
        ] {
            for (text, expected) in cases {
                let result = assign(text, kind, &[]).unwrap();
                assert_eq!(result.value(), &expected, "{text}");
                assert_eq!(result.conversion().discarded_fractional_digits(), 3);
                assert!(result.conversion().discarded_nonzero());
            }
        }
        for (kind, texts) in [
            (
                Kind::SmallInt,
                ["32768.000", "-32769.000", "32768.001", "-32769.001"],
            ),
            (
                Kind::Integer,
                [
                    "2147483648.000",
                    "-2147483649.000",
                    "2147483648.001",
                    "-2147483649.001",
                ],
            ),
            (
                Kind::BigInt,
                [
                    "9223372036854775808.000",
                    "-9223372036854775809.000",
                    "9223372036854775808.001",
                    "-9223372036854775809.001",
                ],
            ),
        ] {
            for text in texts {
                expect_assignment_error(
                    text,
                    kind,
                    &[],
                    Db2NumericAssignmentErrorCode::ValueOutOfRange,
                );
            }
        }
    }

    #[test]
    fn assignment_decimal_precision_scale_and_fractional_metadata_are_exact() {
        use crate::Db2BuiltInType as Kind;
        for (text, precision, scale, coefficient, discarded, nonzero) in [
            ("025.50", 4, 2, 2_550, 0, false),
            ("-025.50", 3, 1, -255, 1, false),
            ("000.00100", 3, 3, 1, 2, false),
            ("-000.00109", 3, 3, -1, 2, true),
            ("99.999", 4, 2, 9_999, 1, true),
            ("-99.999", 4, 2, -9_999, 1, true),
            ("9.999", 1, 0, 9, 3, true),
            ("-9.999", 1, 0, -9, 3, true),
            (".999", 2, 2, 99, 1, true),
            ("-.999", 2, 2, -99, 1, true),
            (".001", 1, 1, 0, 2, true),
            ("-.001", 1, 1, 0, 2, true),
            ("1.200", 2, 1, 12, 2, false),
            ("-1.201", 2, 1, -12, 2, true),
            ("1.20", 5, 4, 12_000, 0, false),
            ("-1.20", 5, 4, -12_000, 0, false),
            ("00099.", 2, 0, 99, 0, false),
            (
                "9999999999999999999999999999999.",
                31,
                0,
                9_999_999_999_999_999_999_999_999_999_999,
                0,
                false,
            ),
            (
                "-9999999999999999999999999999999.",
                31,
                0,
                -9_999_999_999_999_999_999_999_999_999_999,
                0,
                false,
            ),
        ] {
            let result = assign(text, Kind::Decimal, &[precision, scale]).unwrap();
            assert_eq!(
                result.value(),
                &Db2AssignedNumericValue::Decimal(
                    DecimalValue::new(coefficient, scale as u8).unwrap()
                ),
                "{text}"
            );
            assert_eq!(
                result.resolved_type().scalar(),
                &Db2ScalarType::Decimal { precision, scale }
            );
            assert_eq!(result.conversion().discarded_fractional_digits(), discarded);
            assert_eq!(result.conversion().discarded_nonzero(), nonzero);
            assert!(result.conversion().temporary_decimal().is_none());
        }
        for (text, arguments) in [
            ("100.000", [4, 2]),
            ("-100.001", [4, 2]),
            ("10.", [1, 0]),
            ("-10.", [1, 0]),
            ("1.", [31, 31]),
            ("-1.001", [31, 31]),
            ("99.", [3, 2]),
            ("-99.", [3, 2]),
            ("9999999999999999999999999999999.", [31, 31]),
            ("-9999999999999999999999999999999.", [31, 30]),
        ] {
            expect_assignment_error(
                text,
                Kind::Decimal,
                &arguments,
                Db2NumericAssignmentErrorCode::ValueOutOfRange,
            );
        }
    }

    #[test]
    fn assignment_decimal31_zero_whole_part_and_scale_expansion_are_bounded() {
        use crate::Db2BuiltInType as Kind;
        const MAXIMUM: i128 = 9_999_999_999_999_999_999_999_999_999_999;
        for (text, coefficient) in [
            (".9999999999999999999999999999999", MAXIMUM),
            ("-.9999999999999999999999999999999", -MAXIMUM),
            (".0000000000000000000000000000001", 1),
            ("-.0000000000000000000000000000001", -1),
            (".1", 1_000_000_000_000_000_000_000_000_000_000),
            ("-.1", -1_000_000_000_000_000_000_000_000_000_000),
            ("0", 0),
            ("-0", 0),
            ("0.", 0),
            ("-.0000000000000000000000000000000", 0),
        ] {
            let result = assign(text, Kind::Decimal, &[31, 31]).unwrap();
            assert_eq!(
                result.value(),
                &Db2AssignedNumericValue::Decimal(DecimalValue::new(coefficient, 31).unwrap())
            );
            assert_eq!(
                result.resolved_type().scalar(),
                &Db2ScalarType::Decimal {
                    precision: 31,
                    scale: 31
                }
            );
            assert_eq!(result.conversion().discarded_fractional_digits(), 0);
        }
        for scale in 0..=31 {
            for text in ["0", "-0", "-000.00", ".0000000000000000000000000000000"] {
                let result = assign(text, Kind::Decimal, &[31, scale]).unwrap();
                assert_eq!(
                    result.value(),
                    &Db2AssignedNumericValue::Decimal(DecimalValue::new(0, scale as u8).unwrap())
                );
                assert!(!result.conversion().discarded_nonzero());
            }
        }
        for text in [
            "1",
            "-1",
            "2147483648",
            "-9223372036854775808",
            "9999999999999999999999999999999",
        ] {
            expect_assignment_error(
                text,
                Kind::Decimal,
                &[31, 31],
                Db2NumericAssignmentErrorCode::ValueOutOfRange,
            );
        }
        let expanded = assign("9", Kind::Decimal, &[31, 30]).unwrap();
        assert_eq!(
            expanded.value(),
            &Db2AssignedNumericValue::Decimal(
                DecimalValue::new(9_000_000_000_000_000_000_000_000_000_000, 30).unwrap()
            )
        );
        expect_assignment_error(
            "10",
            Kind::Decimal,
            &[31, 30],
            Db2NumericAssignmentErrorCode::ValueOutOfRange,
        );
        expect_assignment_error(
            "-10",
            Kind::Decimal,
            &[31, 30],
            Db2NumericAssignmentErrorCode::ValueOutOfRange,
        );
    }

    #[test]
    fn assignment_integer_decimal_temporary_types_follow_natural_kind_not_magnitude() {
        use crate::Db2BuiltInType as Kind;
        for (text, precision, coefficient) in [
            ("1", 11, 100),
            ("-1", 11, -100),
            ("32767", 11, 3_276_700),
            ("-32768", 11, -3_276_800),
            ("2147483647", 11, 214_748_364_700),
            ("-2147483648", 11, -214_748_364_800),
            ("2147483648", 19, 214_748_364_800),
            ("-2147483649", 19, -214_748_364_900),
            ("9223372036854775807", 19, 922_337_203_685_477_580_700),
            ("-9223372036854775808", 19, -922_337_203_685_477_580_800),
        ] {
            let result = assign(text, Kind::Decimal, &[21, 2]).unwrap();
            assert_eq!(
                result.value(),
                &Db2AssignedNumericValue::Decimal(DecimalValue::new(coefficient, 2).unwrap())
            );
            let temporary = result.conversion().temporary_decimal().unwrap();
            assert_eq!(
                temporary.scalar(),
                &Db2ScalarType::Decimal {
                    precision,
                    scale: 0
                }
            );
            assert_eq!(temporary.nullability(), Db2Nullability::NotNull);
            assert_eq!(result.conversion().discarded_fractional_digits(), 0);
        }
        // Temporary precision 11 does not impose a target precision minimum.
        assert_eq!(
            assign("1", Kind::Decimal, &[1, 0]).unwrap().value(),
            &Db2AssignedNumericValue::Decimal(DecimalValue::new(1, 0).unwrap())
        );
        let decimal = assign("9223372036854775808", Kind::Decimal, &[19, 0]).unwrap();
        assert!(decimal.conversion().temporary_decimal().is_none());
        for text in ["2147483647", "-2147483648", "2147483648", "-2147483649"] {
            expect_assignment_error(
                text,
                Kind::Decimal,
                &[9, 0],
                Db2NumericAssignmentErrorCode::ValueOutOfRange,
            );
        }
    }

    #[test]
    fn assignment_fractional_zero_removal_is_distinct_from_losing_nonzero_digits() {
        use crate::Db2BuiltInType as Kind;
        for (text, expected, discarded, nonzero) in [
            ("2000004.5", 2_000_004, 1, true),
            ("-2000004.5", -2_000_004, 1, true),
            ("200000555.0", 200_000_555, 1, false),
            ("-200000555.000", -200_000_555, 3, false),
            (".999", 0, 3, true),
            ("-.999", 0, 3, true),
            ("-0.000", 0, 3, false),
            ("5.", 5, 0, false),
        ] {
            let result = assign(text, Kind::Integer, &[]).unwrap();
            assert_eq!(result.value(), &Db2AssignedNumericValue::Integer(expected));
            assert_eq!(result.conversion().discarded_fractional_digits(), discarded);
            assert_eq!(result.conversion().discarded_nonzero(), nonzero);
            assert!(result.conversion().temporary_decimal().is_none());
        }
        let tiny = assign("-.0000000000000000000000000000001", Kind::SmallInt, &[]).unwrap();
        assert_eq!(tiny.value(), &Db2AssignedNumericValue::SmallInt(0));
        assert_eq!(tiny.conversion().discarded_fractional_digits(), 31);
        assert!(tiny.conversion().discarded_nonzero());
    }

    #[test]
    fn assignment_compatibility_and_nullable_targets_retain_owner_metadata() {
        use crate::Db2BuiltInType as Kind;
        for nullability in [Db2Nullability::Nullable, Db2Nullability::NotNull] {
            for (text, kind, args, conversion) in [
                (
                    "1",
                    Kind::Integer,
                    &[][..],
                    crate::Db2ConversionKind::Identity,
                ),
                (
                    "1",
                    Kind::SmallInt,
                    &[][..],
                    crate::Db2ConversionKind::Numeric,
                ),
                (
                    "1.0",
                    Kind::Decimal,
                    &[2, 1][..],
                    crate::Db2ConversionKind::Identity,
                ),
                (
                    "1.0",
                    Kind::Decimal,
                    &[3, 1][..],
                    crate::Db2ConversionKind::Numeric,
                ),
                (
                    "1",
                    Kind::Decimal,
                    &[1, 0][..],
                    crate::Db2ConversionKind::Numeric,
                ),
            ] {
                let literal = materialize(text).unwrap();
                let target = assignment_target(kind, args, nullability);
                let result = assign_db2_numeric_constant(&literal, &target).unwrap();
                assert_eq!(result.resolved_type(), &target);
                assert_eq!(
                    literal.resolved_type().nullability(),
                    Db2Nullability::NotNull
                );
                assert_eq!(
                    result.conversion().compatibility(),
                    crate::Db2AssignmentCompatibility::Compatible {
                        conversion,
                        nullability: crate::Db2AssignmentNullability::Safe,
                    }
                );
            }
        }
    }

    #[test]
    fn assignment_unsupported_converter_errors_do_not_claim_type_incompatibility() {
        use crate::Db2BuiltInType as Kind;
        use Db2NumericAssignmentErrorCode as Code;
        for text in ["1", "2147483648", "1.2"] {
            for (kind, arguments, code) in [
                (Kind::Real, &[][..], Code::UnsupportedFloatingTarget),
                (Kind::Double, &[][..], Code::UnsupportedFloatingTarget),
                (Kind::Float, &[21][..], Code::UnsupportedFloatingTarget),
                (Kind::Float, &[53][..], Code::UnsupportedFloatingTarget),
                (Kind::DecFloat, &[16][..], Code::UnsupportedDecFloatTarget),
                (Kind::DecFloat, &[34][..], Code::UnsupportedDecFloatTarget),
                (
                    Kind::Character,
                    &[10][..],
                    Code::UnsupportedNonNumericTarget,
                ),
                (Kind::VarChar, &[10][..], Code::UnsupportedNonNumericTarget),
                (Kind::Graphic, &[10][..], Code::UnsupportedNonNumericTarget),
                (
                    Kind::VarGraphic,
                    &[10][..],
                    Code::UnsupportedNonNumericTarget,
                ),
                (Kind::Binary, &[10][..], Code::IncompatibleTypes),
                (Kind::VarBinary, &[10][..], Code::IncompatibleTypes),
                (Kind::Date, &[][..], Code::IncompatibleTypes),
                (Kind::Time, &[][..], Code::IncompatibleTypes),
                (Kind::Timestamp, &[6][..], Code::IncompatibleTypes),
            ] {
                expect_assignment_error(text, kind, arguments, code);
            }
        }
    }

    #[test]
    fn assignment_proofs_and_errors_preserve_complete_original_spans_after_input_drop() {
        use crate::Db2BuiltInType as Kind;
        for (prefix, line, column) in [
            ("", 1, 1),
            ("  ", 1, 3),
            ("α😀 ", 1, 4),
            ("-- α\n", 2, 1),
            ("/* α */\r\n  ", 2, 3),
            ("/* α */\r  ", 2, 3),
            ("/* α */\n\n  ", 3, 3),
        ] {
            let (result, error, span) = {
                let text = "-32768.999";
                let source = format!("{prefix}{text}; -- trailing α\r\n");
                let span = Db2SourceSpan {
                    start_byte: prefix.len(),
                    end_byte: prefix.len() + text.len(),
                    start: Db2SourceLocation { line, column },
                    end: Db2SourceLocation {
                        line,
                        column: column + text.len() as u32,
                    },
                };
                let literal = materialize_db2_numeric_constant(
                    &source,
                    span,
                    Db2NumericConstantLimits::default(),
                )
                .unwrap();
                let target = assignment_target(Kind::SmallInt, &[], Db2Nullability::Nullable);
                let result = assign_db2_numeric_constant(&literal, &target).unwrap();
                let error = assign_db2_numeric_constant(
                    &literal,
                    &assignment_target(Kind::Decimal, &[4, 0], Db2Nullability::NotNull),
                )
                .unwrap_err();
                (result, error, span)
            };
            assert_eq!(result.span(), span);
            assert_eq!(result.value(), &Db2AssignedNumericValue::SmallInt(-32_768));
            assert_eq!(
                result.resolved_type().nullability(),
                Db2Nullability::Nullable
            );
            assert_eq!(result.conversion().discarded_fractional_digits(), 3);
            assert!(result.conversion().discarded_nonzero());
            assert_eq!(result.clone(), result);
            assert_eq!(error.span, span);
            assert_eq!(error.code, Db2NumericAssignmentErrorCode::ValueOutOfRange);
            assert!(error.to_string().contains(&format!("{line}:{column}")));
        }
    }

    mod located_operand_tests {
        use super::*;
        use crate::{Db2SourceLocation, Db2SyntaxDiagnosticCode, Db2SyntaxLimits};

        fn span(start: usize, end: usize, from: (u32, u32), to: (u32, u32)) -> Db2SourceSpan {
            Db2SourceSpan {
                start_byte: start,
                end_byte: end,
                start: Db2SourceLocation {
                    line: from.0,
                    column: from.1,
                },
                end: Db2SourceLocation {
                    line: to.0,
                    column: to.1,
                },
            }
        }

        fn line_span(start: usize, end: usize) -> Db2SourceSpan {
            span(
                start,
                end,
                (1, (start as u32).saturating_add(1)),
                (1, (end as u32).saturating_add(1)),
            )
        }

        fn selected(
            text: &str,
            sign: bool,
            number_start: usize,
        ) -> Result<Db2MaterializedNumericConstant, Db2NumericConstantValueError> {
            materialize_db2_located_numeric_operand(
                text,
                line_span(0, text.len()),
                sign.then(|| line_span(0, 1)),
                line_span(number_start, text.len()),
                Db2NumericConstantLimits::default(),
                Db2SyntaxLimits::default(),
            )
        }

        #[test]
        fn located_fixed_natural_types_and_coefficients() {
            for (text, start, scalar, value) in [
                (
                    "- /*c*/ 2147483648",
                    8,
                    Db2ScalarType::Integer,
                    Db2NumericConstantValue::Integer(-2_147_483_648),
                ),
                (
                    "- /*c*/ 9223372036854775808",
                    8,
                    Db2ScalarType::BigInt,
                    Db2NumericConstantValue::BigInt(-9_223_372_036_854_775_808),
                ),
                (
                    "+ /*c*/ 2147483648",
                    8,
                    Db2ScalarType::BigInt,
                    Db2NumericConstantValue::BigInt(2_147_483_648),
                ),
                (
                    "- /*c*/ 9223372036854775809",
                    8,
                    Db2ScalarType::Decimal {
                        precision: 19,
                        scale: 0,
                    },
                    Db2NumericConstantValue::Decimal(
                        DecimalValue::new(-9_223_372_036_854_775_809, 0).unwrap(),
                    ),
                ),
                (
                    "- /*c*/ 000.00100",
                    8,
                    Db2ScalarType::Decimal {
                        precision: 8,
                        scale: 5,
                    },
                    Db2NumericConstantValue::Decimal(DecimalValue::new(-100, 5).unwrap()),
                ),
                (
                    "- /*c*/ 000.00",
                    8,
                    Db2ScalarType::Decimal {
                        precision: 5,
                        scale: 2,
                    },
                    Db2NumericConstantValue::Decimal(DecimalValue::new(0, 2).unwrap()),
                ),
                (
                    "- /*c*/ .0000000000000000000000000000001",
                    8,
                    Db2ScalarType::Decimal {
                        precision: 31,
                        scale: 31,
                    },
                    Db2NumericConstantValue::Decimal(DecimalValue::new(-1, 31).unwrap()),
                ),
                (
                    "- /*c*/ 000000000000000000000000000000.1",
                    8,
                    Db2ScalarType::Decimal {
                        precision: 31,
                        scale: 1,
                    },
                    Db2NumericConstantValue::Decimal(DecimalValue::new(-1, 1).unwrap()),
                ),
                (
                    "000.00",
                    0,
                    Db2ScalarType::Decimal {
                        precision: 5,
                        scale: 2,
                    },
                    Db2NumericConstantValue::Decimal(DecimalValue::new(0, 2).unwrap()),
                ),
            ] {
                let result = selected(text, start != 0, start).unwrap();
                assert_eq!(result.resolved_type().scalar(), &scalar, "{text}");
                assert_eq!(
                    result.resolved_type().nullability(),
                    crate::Db2Nullability::NotNull
                );
                assert_eq!(result.value(), &value, "{text}");
                assert_eq!(result.span(), line_span(0, text.len()));
            }
        }

        #[test]
        fn located_trivia_sign_omission_and_adjacency() {
            for (text, start) in [
                ("-1", 1),
                ("- 1", 2),
                ("-\t\x0c1", 3),
                ("-/*c*/1", 6),
                ("- /*a /*b*/ c*/ 1", 16),
            ] {
                assert_eq!(
                    selected(text, true, start).unwrap().value(),
                    &Db2NumericConstantValue::Integer(-1)
                );
            }
            for text in ["1", "2147483647"] {
                assert!(selected(text, false, 0).is_ok());
            }
            assert_eq!(
                selected("+1", true, 1).unwrap().value(),
                &Db2NumericConstantValue::Integer(1)
            );
            for (text, start) in [("- 1", 2), ("-/*c*/1", 6)] {
                let error = materialize_db2_numeric_constant(
                    text,
                    line_span(0, text.len()),
                    Db2NumericConstantLimits::default(),
                )
                .unwrap_err();
                assert_eq!(
                    error.code,
                    Db2NumericConstantValueErrorCode::Classification(
                        Db2NumericConstantErrorCode::InvalidSpelling
                    )
                );
                assert!(selected(text, true, start).is_ok());
            }
        }

        #[test]
        fn located_fixed_utf8_crlf_provenance_and_lifetime_without_surrounding_lexing() {
            let operand = span(8, 34, (2, 1), (3, 13));
            let sign = span(8, 9, (2, 1), (2, 2));
            let number = span(24, 34, (3, 3), (3, 13));
            let result = {
                // Neither surrounding invalid SQL nor the suffix NUL is selected.
                let source = String::from("α😀\r\n- /*α*/ --b\r\n  2147483648\0???");
                materialize_db2_located_numeric_operand(
                    &source,
                    operand,
                    Some(sign),
                    number,
                    Db2NumericConstantLimits::default(),
                    Db2SyntaxLimits::default(),
                )
                .unwrap()
            };
            assert_eq!(result.span(), operand);
            assert_eq!(result.resolved_type().scalar(), &Db2ScalarType::Integer);
            assert_eq!(
                result.value(),
                &Db2NumericConstantValue::Integer(-2_147_483_648)
            );
            assert_eq!(result.clone(), result);
            for (text, end_line) in [("- --c\n1", 2), ("- --c\r1", 2), ("- --c\r\n1", 2)] {
                let end = text.len();
                let result = materialize_db2_located_numeric_operand(
                    text,
                    span(0, end, (1, 1), (end_line, 2)),
                    Some(line_span(0, 1)),
                    span(end - 1, end, (end_line, 1), (end_line, 2)),
                    Db2NumericConstantLimits::default(),
                    Db2SyntaxLimits::default(),
                )
                .unwrap();
                assert_eq!(result.value(), &Db2NumericConstantValue::Integer(-1));
            }
        }

        #[test]
        fn located_rejects_nontrivia_extra_tokens_and_malformed_comments() {
            for (text, start) in [
                ("- +1", 3),
                ("- x 1", 4),
                ("- 1 2", 4),
                ("- (1)", 3),
                ("- /** 1", 6),
                ("- /*c* / 1", 9),
                ("--comment 1", 10),
                ("- --comment 1", 12),
                ("-\0 1", 3),
            ] {
                // Number span excludes the trailing ')' only in that negative case.
                let number_end = if text == "- (1)" { 4 } else { text.len() };
                assert!(
                    materialize_db2_located_numeric_operand(
                        text,
                        line_span(0, text.len()),
                        Some(line_span(0, 1)),
                        line_span(start, number_end),
                        Db2NumericConstantLimits::default(),
                        Db2SyntaxLimits::default()
                    )
                    .is_err(),
                    "{text:?}"
                );
            }
            for (text, start, code) in [
                (
                    "- /*c*/ 1E0",
                    8,
                    Db2NumericConstantErrorCode::UnsupportedExponent,
                ),
                (
                    "- /*c*/ NAN",
                    8,
                    Db2NumericConstantErrorCode::UnsupportedSpecialValue,
                ),
                (
                    "- /*c*/ 1,2",
                    8,
                    Db2NumericConstantErrorCode::UnsupportedCommaDecimal,
                ),
                (
                    "- /*c*/ 00000000000000000001",
                    8,
                    Db2NumericConstantErrorCode::UnsupportedLongInteger,
                ),
                (
                    "- /*c*/ .00000000000000000000000000000001",
                    8,
                    Db2NumericConstantErrorCode::TooManyDigits,
                ),
                (
                    "- /*c*/ +1",
                    8,
                    Db2NumericConstantErrorCode::InvalidSpelling,
                ),
                (
                    "- /*c*/ -1",
                    8,
                    Db2NumericConstantErrorCode::InvalidSpelling,
                ),
            ] {
                let error = selected(text, true, start).unwrap_err();
                assert_eq!(
                    error.code,
                    Db2NumericConstantValueErrorCode::Classification(code)
                );
                assert_eq!(error.span, line_span(0, text.len()));
            }
            // A classifier-valid trailing point is not one actual lexer Number token.
            assert!(selected("- /*c*/ 5.", true, 8).is_err());
            assert!(selected("-1", false, 0).is_err());
        }

        #[test]
        fn located_component_spans_cannot_be_forged() {
            let text = "- /*c*/ 12";
            let operand = line_span(0, 10);
            let sign = line_span(0, 1);
            let number = line_span(8, 10);
            let mut cases = vec![
                (operand, None, number),
                (number, Some(sign), number),
                (operand, Some(number), sign),
                (operand, Some(line_span(0, 9)), number),
                (operand, Some(line_span(0, 0)), number),
                (operand, Some(sign), line_span(10, 8)),
                (operand, Some(sign), line_span(8, 9)),
                (operand, Some(sign), line_span(8, usize::MAX)),
                (line_span(0, 11), Some(sign), number),
            ];
            for component in 0..3 {
                for endpoint in 0..2 {
                    let (mut a, mut b, mut c) = (operand, sign, number);
                    let altered = match component {
                        0 => &mut a,
                        1 => &mut b,
                        _ => &mut c,
                    };
                    if endpoint == 0 {
                        altered.start.column += 1;
                    } else {
                        altered.end.line += 1;
                    }
                    cases.push((a, Some(b), c));
                }
            }
            for (operand, sign, number) in cases {
                let error = materialize_db2_located_numeric_operand(
                    text,
                    operand,
                    sign,
                    number,
                    Db2NumericConstantLimits::default(),
                    Db2SyntaxLimits::default(),
                )
                .unwrap_err();
                assert_eq!(
                    error.code,
                    Db2NumericConstantValueErrorCode::Classification(
                        Db2NumericConstantErrorCode::InvalidSourceSpan
                    )
                );
                assert_eq!(error.span, operand);
            }
            let text = "α- 1";
            let operand = span(2, 5, (1, 2), (1, 5));
            let number = span(4, 5, (1, 4), (1, 5));
            for (operand, sign, number) in [
                (
                    Db2SourceSpan {
                        start_byte: 1,
                        ..operand
                    },
                    Some(span(2, 3, (1, 2), (1, 3))),
                    number,
                ),
                (operand, Some(span(1, 2, (1, 1), (1, 2))), number),
                (
                    operand,
                    Some(span(2, 3, (1, 2), (1, 3))),
                    Db2SourceSpan {
                        start_byte: 1,
                        ..number
                    },
                ),
            ] {
                assert!(
                    materialize_db2_located_numeric_operand(
                        text,
                        operand,
                        sign,
                        number,
                        Db2NumericConstantLimits::default(),
                        Db2SyntaxLimits::default()
                    )
                    .is_err()
                );
            }
            // Valid coordinates selecting an actual nonsign cannot establish a sign.
            assert!(selected("x 1", true, 2).is_err());
        }

        #[test]
        fn located_default_parser_spans_supplement_fixed_vectors() {
            let source = "CREATE TABLE T (A INTEGER DEFAULT - /*c*/ 2147483648, B DECIMAL(5,2) WITH DEFAULT + --c\r\n000.00)";
            let table = crate::parse_db2_create_table_statement(
                source,
                Db2SyntaxLimits::default(),
                crate::Db2AstLimits::default(),
            )
            .unwrap();
            for (column, expected) in table.columns().iter().zip([
                Db2NumericConstantValue::Integer(-2_147_483_648),
                Db2NumericConstantValue::Decimal(DecimalValue::new(0, 2).unwrap()),
            ]) {
                let default = column.default().unwrap();
                let operand = default.value_span().unwrap();
                let result = materialize_db2_located_numeric_operand(
                    source,
                    operand,
                    default.numeric_sign_span(),
                    default.numeric_token_span().unwrap(),
                    Db2NumericConstantLimits::default(),
                    Db2SyntaxLimits::default(),
                )
                .unwrap();
                assert_eq!(result.value(), &expected);
                assert_eq!(result.span(), operand);
            }
        }

        #[test]
        fn located_exact_and_one_beyond_source_operand_token_count_and_type_argument_bounds() {
            let source = "- /*c*/ 1.0";
            let operand = line_span(0, 11);
            let sign = Some(line_span(0, 1));
            let number = line_span(8, 11);
            let numeric = Db2NumericConstantLimits {
                max_source_bytes: 11,
                ast: crate::Db2AstLimits {
                    max_literal_bytes: 11,
                    max_list_items: 2,
                    ..crate::Db2AstLimits::default()
                },
            };
            let syntax = Db2SyntaxLimits {
                max_statement_bytes: 11,
                max_token_bytes: 3,
                max_tokens: 2,
                max_nesting: 1,
            };
            let run = |source: &str, numeric, syntax| {
                materialize_db2_located_numeric_operand(
                    source, operand, sign, number, numeric, syntax,
                )
            };
            assert!(run(source, numeric, syntax).is_ok());
            for (numeric, syntax, code) in [
                (
                    Db2NumericConstantLimits {
                        max_source_bytes: 10,
                        ..numeric
                    },
                    syntax,
                    Db2NumericConstantErrorCode::SourceTooLarge,
                ),
                (
                    numeric,
                    Db2SyntaxLimits {
                        max_statement_bytes: 10,
                        ..syntax
                    },
                    Db2NumericConstantErrorCode::SourceTooLarge,
                ),
                (
                    Db2NumericConstantLimits {
                        ast: crate::Db2AstLimits {
                            max_literal_bytes: 10,
                            ..numeric.ast
                        },
                        ..numeric
                    },
                    syntax,
                    Db2NumericConstantErrorCode::ConstantTooLarge,
                ),
                (
                    numeric,
                    Db2SyntaxLimits {
                        max_token_bytes: 2,
                        ..syntax
                    },
                    Db2NumericConstantErrorCode::Syntax(Db2SyntaxDiagnosticCode::TokenTooLarge),
                ),
                (
                    numeric,
                    Db2SyntaxLimits {
                        max_tokens: 1,
                        ..syntax
                    },
                    Db2NumericConstantErrorCode::Syntax(Db2SyntaxDiagnosticCode::TooManyTokens),
                ),
                (
                    Db2NumericConstantLimits {
                        ast: crate::Db2AstLimits {
                            max_list_items: 1,
                            ..numeric.ast
                        },
                        ..numeric
                    },
                    syntax,
                    Db2NumericConstantErrorCode::TypeArgumentsLimit,
                ),
            ] {
                assert_eq!(
                    run(source, numeric, syntax).unwrap_err().code,
                    Db2NumericConstantValueErrorCode::Classification(code)
                );
            }
            // Unselected suffix still consumes both full-source budgets.
            assert_eq!(
                run("- /*c*/ 1.0x", numeric, syntax).unwrap_err().code,
                Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::SourceTooLarge
                )
            );
            let text = "1";
            assert!(
                materialize_db2_located_numeric_operand(
                    text,
                    line_span(0, 1),
                    None,
                    line_span(0, 1),
                    Db2NumericConstantLimits::default(),
                    Db2SyntaxLimits {
                        max_tokens: 1,
                        max_token_bytes: 1,
                        ..syntax
                    }
                )
                .is_ok()
            );
            let text = "- /*a /*b*/ c*/ 1";
            let error = materialize_db2_located_numeric_operand(
                text,
                line_span(0, 17),
                Some(line_span(0, 1)),
                line_span(16, 17),
                Db2NumericConstantLimits::default(),
                Db2SyntaxLimits {
                    max_nesting: 1,
                    ..Db2SyntaxLimits::default()
                },
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::Syntax(
                        Db2SyntaxDiagnosticCode::UnbalancedDelimiter
                    )
                )
            );
        }

        #[test]
        fn located_all_compiled_budget_ceilings_and_invalid_limits_precede_spans() {
            let numeric = Db2NumericConstantLimits {
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
            let syntax = Db2SyntaxLimits {
                max_statement_bytes: 8 * 1024 * 1024,
                max_tokens: 262_144,
                max_token_bytes: 1024 * 1024,
                max_nesting: 1024,
            };
            let run = |numeric, syntax| {
                materialize_db2_located_numeric_operand(
                    "1",
                    line_span(0, 1),
                    None,
                    line_span(0, 1),
                    numeric,
                    syntax,
                )
            };
            assert!(run(numeric, syntax).is_ok());
            let mut invalid = Vec::new();
            for field in 0..11 {
                for beyond in [false, true] {
                    let (mut n, mut s) = (numeric, syntax);
                    let value = match field {
                        0 => &mut n.max_source_bytes,
                        1 => &mut n.ast.max_identifier_bytes,
                        2 => &mut n.ast.max_name_parts,
                        3 => &mut n.ast.max_literal_bytes,
                        4 => &mut n.ast.max_expression_nodes,
                        5 => &mut n.ast.max_list_items,
                        6 => &mut n.ast.max_expression_depth,
                        7 => &mut s.max_statement_bytes,
                        8 => &mut s.max_tokens,
                        9 => &mut s.max_token_bytes,
                        _ => &mut s.max_nesting,
                    };
                    *value = if beyond { *value + 1 } else { 0 };
                    invalid.push((n, s));
                }
            }
            for (n, s) in invalid {
                let error = materialize_db2_located_numeric_operand(
                    "1",
                    line_span(0, usize::MAX),
                    Some(line_span(0, usize::MAX)),
                    line_span(0, usize::MAX),
                    n,
                    s,
                )
                .unwrap_err();
                assert_eq!(
                    error.code,
                    Db2NumericConstantValueErrorCode::Classification(
                        Db2NumericConstantErrorCode::InvalidLimits
                    )
                );
                assert_eq!(error.span, line_span(0, 0));
            }
            // Exact compiled source and combined-operand envelopes, with bounded
            // selected tokens. The eight-megabyte trivia is never normalized/copied.
            let mut source = String::from("-");
            source.push_str(&" ".repeat(numeric.max_source_bytes - 2));
            source.push('1');
            let operand = line_span(0, source.len());
            let number = line_span(source.len() - 1, source.len());
            let result = materialize_db2_located_numeric_operand(
                &source,
                operand,
                Some(line_span(0, 1)),
                number,
                numeric,
                syntax,
            )
            .unwrap();
            assert_eq!(result.value(), &Db2NumericConstantValue::Integer(-1));
            source.push('x');
            let error = materialize_db2_located_numeric_operand(
                &source,
                operand,
                Some(line_span(0, 1)),
                number,
                numeric,
                syntax,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                Db2NumericConstantValueErrorCode::Classification(
                    Db2NumericConstantErrorCode::SourceTooLarge
                )
            );
            assert_eq!(error.span, line_span(0, 0));
        }
    }
}
