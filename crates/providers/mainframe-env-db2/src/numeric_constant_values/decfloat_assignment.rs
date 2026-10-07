//! Finite DECFLOAT assignment from existing exact literal proofs.
//!
//! Source identities and dependency review are recorded in
//! docs/delivery/subsystems/db2/decfloat-primitive-decision.md. This language-element
//! kernel has no standalone catalog row or SQL execution/SQLCA authority.
//! Explicit caller context does not establish package/register selection.

use super::{Db2MaterializedNumericConstant, Db2NumericConstantValue};
use crate::{Db2AssignmentCompatibility, Db2ResolvedType, Db2ScalarType, Db2SourceSpan};
use std::fmt::{self, Write};

/// Closed Db2 rounding domain; no default, host inference or ZeroFiveUp.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DecFloatRoundingMode {
    Ceiling,
    Down,
    Floor,
    HalfDown,
    HalfEven,
    HalfUp,
    Up,
}

impl Db2DecFloatRoundingMode {
    fn primitive(self) -> dec::Rounding {
        match self {
            Self::Ceiling => dec::Rounding::Ceiling,
            Self::Down => dec::Rounding::Down,
            Self::Floor => dec::Rounding::Floor,
            Self::HalfDown => dec::Rounding::HalfDown,
            Self::HalfEven => dec::Rounding::HalfEven,
            Self::HalfUp => dec::Rounding::HalfUp,
            Self::Up => dec::Rounding::Up,
        }
    }
}

/// Caller-selected policy origin, not evidence of integrated policy selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DecFloatRoundingContext {
    StaticBindOption,
    NativeProcedureOption,
    DynamicRegister,
    StaticCreateViewRegister,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2DecFloatRounding {
    pub mode: Db2DecFloatRoundingMode,
    pub context: Db2DecFloatRoundingContext,
}

/// Canonical source proofs never supply negative zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2FiniteDecFloatSign {
    Positive,
    Negative,
}

/// Owned finite coefficient × 10^exponent, with signed coefficient and matching sign.
/// No primitive representation, serialization or raw-value constructor escapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2AssignedFiniteDecFloat {
    coefficient: i128,
    exponent: i32,
    sign: Db2FiniteDecFloatSign,
    precision: u32,
    target: Db2ResolvedType,
    source: Db2MaterializedNumericConstant,
    rounding: Db2DecFloatRounding,
    inexact: bool,
    compatibility: Db2AssignmentCompatibility,
    temporary_decimal: Option<Db2ResolvedType>,
}

impl Db2AssignedFiniteDecFloat {
    #[must_use]
    pub const fn coefficient(&self) -> i128 {
        self.coefficient
    }

    #[must_use]
    pub const fn exponent(&self) -> i32 {
        self.exponent
    }

    #[must_use]
    pub const fn sign(&self) -> Db2FiniteDecFloatSign {
        self.sign
    }

    #[must_use]
    pub const fn precision(&self) -> u32 {
        self.precision
    }

    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.target
    }

    #[must_use]
    pub const fn source(&self) -> &Db2MaterializedNumericConstant {
        &self.source
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.source.span()
    }

    #[must_use]
    pub const fn rounding(&self) -> Db2DecFloatRounding {
        self.rounding
    }

    /// Nonzero discarded digits, never a SQLCA warning-policy decision.
    #[must_use]
    pub const fn inexact(&self) -> bool {
        self.inexact
    }

    #[must_use]
    pub const fn compatibility(&self) -> Db2AssignmentCompatibility {
        self.compatibility
    }

    /// Integer temporary DECIMAL(11,0)/(19,0) attributes from the existing type owner.
    #[must_use]
    pub const fn temporary_decimal(&self) -> Option<&Db2ResolvedType> {
        self.temporary_decimal.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DecFloatAssignmentErrorCode {
    IncompatibleTypes,
    WrongTarget,
    InputBounds,
    AllocationFailure,
    PrimitiveFailure,
    PrimitiveStatus,
    NonFinite,
    InvalidResult,
    InvalidTemporaryType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2DecFloatAssignmentError {
    pub code: Db2DecFloatAssignmentErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl fmt::Display for Db2DecFloatAssignmentError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2DecFloatAssignmentError {}

/// Assign only an opaque exact literal proof to a resolved DECFLOAT(16/34) target.
/// Each invocation requires an explicit mode and origin; this never selects a
/// package, procedure, register or installation policy on the caller's behalf.
pub fn assign_db2_finite_decfloat_constant(
    source: &Db2MaterializedNumericConstant,
    target: &Db2ResolvedType,
    rounding: Db2DecFloatRounding,
) -> Result<Db2AssignedFiniteDecFloat, Db2DecFloatAssignmentError> {
    use Db2DecFloatAssignmentErrorCode as Code;
    let fail = |code| failure(code, source.span());
    let compatibility = crate::classify_db2_assignment(source.resolved_type(), target);
    if !matches!(compatibility, Db2AssignmentCompatibility::Compatible { .. }) {
        return Err(fail(Code::IncompatibleTypes));
    }
    let precision = match target.scalar() {
        Db2ScalarType::DecFloat { precision: 16 } => 16,
        Db2ScalarType::DecFloat { precision: 34 } => 34,
        _ => return Err(fail(Code::WrongTarget)),
    };
    let (coefficient, scale, temporary_precision) = match source.value() {
        Db2NumericConstantValue::Integer(value) => (i128::from(*value), 0, Some(11)),
        Db2NumericConstantValue::BigInt(value) => (i128::from(*value), 0, Some(19)),
        Db2NumericConstantValue::Decimal(value) => (value.coefficient(), value.scale(), None),
    };
    // Opaque proofs already guarantee these bounds. Guard before any formatting
    // or parser allocation so widening the owner cannot silently widen this adapter.
    if coefficient.unsigned_abs() >= 10u128.pow(31) || scale > 31 {
        return Err(fail(Code::InputBounds));
    }
    let input = primitive_input(coefficient, scale).map_err(fail)?;
    let (assigned, exponent, negative, inexact) =
        convert_primitive(input, precision, rounding.mode).map_err(fail)?;
    admit_result(assigned, exponent, negative, precision, coefficient).map_err(fail)?;
    let temporary_decimal = temporary_precision
        .map(|precision| {
            let syntax = crate::Db2BuiltInDataType::new(
                crate::Db2BuiltInType::Decimal,
                vec![precision, 0],
                false,
                crate::Db2AstLimits::default(),
            )
            .map_err(|_| fail(Code::InvalidTemporaryType))?;
            crate::resolve_db2_type(
                &crate::Db2DataType::BuiltIn(syntax),
                source.resolved_type().nullability(),
            )
            .map_err(|_| fail(Code::InvalidTemporaryType))
        })
        .transpose()?;
    Ok(Db2AssignedFiniteDecFloat {
        coefficient: assigned,
        exponent,
        sign: if negative {
            Db2FiniteDecFloatSign::Negative
        } else {
            Db2FiniteDecFloatSign::Positive
        },
        precision,
        target: target.clone(),
        source: source.clone(),
        rounding,
        inexact,
        compatibility,
        temporary_decimal,
    })
}

fn failure(
    code: Db2DecFloatAssignmentErrorCode,
    span: Db2SourceSpan,
) -> Db2DecFloatAssignmentError {
    use Db2DecFloatAssignmentErrorCode as Code;
    let message = match code {
        Code::IncompatibleTypes => "source and target are not assignment compatible",
        Code::WrongTarget => "finite assignment requires a resolved DECFLOAT(16/34) target",
        Code::InputBounds => "proven coefficient or scale exceeds the finite adapter bound",
        Code::AllocationFailure => "bounded primitive input reservation failed",
        Code::PrimitiveFailure => "decimal primitive rejected the proven input",
        Code::PrimitiveStatus => "decimal primitive reported an exceptional or unknown status",
        Code::NonFinite => "decimal primitive returned a special or nonfinite value",
        Code::InvalidResult => "decimal primitive output violates finite proof bounds",
        Code::InvalidTemporaryType => "type owner rejected integer temporary decimal attributes",
    };
    Db2DecFloatAssignmentError {
        code,
        span,
        message,
    }
}

// Stack formatting is a transport of proven exact values, not a SQL parser or
// classifier. The largest input is sign + 31 digits + E + sign + 2 scale digits.
struct PrimitiveInput {
    bytes: [u8; 40],
    len: usize,
}

impl Write for PrimitiveInput {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let output = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        output.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn primitive_input(
    coefficient: i128,
    scale: u8,
) -> Result<Vec<u8>, Db2DecFloatAssignmentErrorCode> {
    let mut input = PrimitiveInput {
        bytes: [0; 40],
        len: 0,
    };
    write!(input, "{coefficient}E{}", -i32::from(scale))
        .map_err(|_| Db2DecFloatAssignmentErrorCode::InputBounds)?;
    let mut bytes = Vec::new();
    // Reserve the terminator too before handing ownership to CString parsing.
    bytes
        .try_reserve_exact(input.len + 1)
        .map_err(|_| Db2DecFloatAssignmentErrorCode::AllocationFailure)?;
    bytes.extend_from_slice(&input.bytes[..input.len]);
    Ok(bytes)
}

fn admit_status(status: dec::Status) -> Result<bool, Db2DecFloatAssignmentErrorCode> {
    let mut allowed = dec::Status::default();
    allowed.set_inexact();
    allowed.set_rounded();
    // Equality also rejects unknown flags instead of silently ignoring them.
    if status & allowed != status {
        return Err(Db2DecFloatAssignmentErrorCode::PrimitiveStatus);
    }
    Ok(status.inexact())
}

// Only primitive_input supplies production buffers. Keeping the extraction
// boundary private permits fault regressions without a raw public constructor.
fn convert_primitive(
    input: Vec<u8>,
    precision: u32,
    mode: Db2DecFloatRoundingMode,
) -> Result<(i128, i32, bool, bool), Db2DecFloatAssignmentErrorCode> {
    use Db2DecFloatAssignmentErrorCode as Code;
    if input.len() > 36 {
        return Err(Code::InputBounds);
    }
    match precision {
        16 => {
            let mut context = dec::Context::<dec::Decimal64>::default();
            context.set_rounding(mode.primitive());
            let value = context.parse(input).map_err(|_| Code::PrimitiveFailure)?;
            let inexact = admit_status(context.status())?;
            if !value.is_finite() {
                return Err(Code::NonFinite);
            }
            Ok((
                i128::from(value.coefficient()),
                value.exponent(),
                value.is_signed(),
                inexact,
            ))
        }
        34 => {
            let mut context = dec::Context::<dec::Decimal128>::default();
            context.set_rounding(mode.primitive());
            let value = context.parse(input).map_err(|_| Code::PrimitiveFailure)?;
            let inexact = admit_status(context.status())?;
            if !value.is_finite() {
                return Err(Code::NonFinite);
            }
            Ok((
                value.coefficient(),
                value.exponent(),
                value.is_signed(),
                inexact,
            ))
        }
        _ => Err(Code::WrongTarget),
    }
}

fn admit_result(
    coefficient: i128,
    exponent: i32,
    negative: bool,
    precision: u32,
    original: i128,
) -> Result<(), Db2DecFloatAssignmentErrorCode> {
    // The admitted source envelope is much narrower than either IEEE exponent
    // range: scale <=31, digits <=31; a 16-digit carry reaches exponent 16.
    if !matches!(precision, 16 | 34)
        || coefficient.unsigned_abs() >= 10u128.pow(precision)
        || !(-31..=16).contains(&exponent)
        || negative != (coefficient < 0)
        || negative != (original < 0)
        || (coefficient == 0) != (original == 0)
    {
        return Err(Db2DecFloatAssignmentErrorCode::InvalidResult);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{materialize_db2_located_numeric_operand, materialize_db2_numeric_constant};
    use super::*;
    use crate::{Db2Nullability, Db2NumericConstantLimits, Db2SourceLocation};

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

    fn source(text: &str) -> Db2MaterializedNumericConstant {
        materialize_db2_numeric_constant(text, span(text), Db2NumericConstantLimits::default())
            .unwrap()
    }

    fn target(
        kind: crate::Db2BuiltInType,
        args: Vec<u32>,
        nullable: Db2Nullability,
    ) -> Db2ResolvedType {
        let syntax =
            crate::Db2BuiltInDataType::new(kind, args, false, crate::Db2AstLimits::default())
                .unwrap();
        crate::resolve_db2_type(&crate::Db2DataType::BuiltIn(syntax), nullable).unwrap()
    }

    fn assign(
        text: &str,
        precision: u32,
        mode: Db2DecFloatRoundingMode,
    ) -> Db2AssignedFiniteDecFloat {
        assign_db2_finite_decfloat_constant(
            &source(text),
            &target(
                crate::Db2BuiltInType::DecFloat,
                vec![precision],
                Db2Nullability::NotNull,
            ),
            Db2DecFloatRounding {
                mode,
                context: Db2DecFloatRoundingContext::StaticBindOption,
            },
        )
        .unwrap()
    }

    fn expect(
        text: &str,
        precision: u32,
        mode: Db2DecFloatRoundingMode,
        coefficient: i128,
        exponent: i32,
        inexact: bool,
    ) {
        let result = assign(text, precision, mode);
        assert_eq!(
            (result.coefficient(), result.exponent(), result.inexact()),
            (coefficient, exponent, inexact),
            "{text}, {precision}, {mode:?}"
        );
        assert_eq!(
            result.sign(),
            if coefficient < 0 {
                Db2FiniteDecFloatSign::Negative
            } else {
                Db2FiniteDecFloatSign::Positive
            }
        );
        assert_eq!(result.precision(), precision);
        assert_eq!(result.source(), &source(text));
        assert_eq!(result.span(), span(text));
    }

    #[test]
    fn finite_decfloat_half_even_tie_regression() {
        expect(
            "12345678901234565",
            16,
            Db2DecFloatRoundingMode::HalfEven,
            1_234_567_890_123_456,
            1,
            true,
        );
    }

    #[test]
    fn seven_modes_even_and_odd_ties_both_signs() {
        use Db2DecFloatRoundingMode::*;
        // Expected signed coefficients are authored independently of the primitive.
        for (mode, positive_even, positive_odd, negative_even, negative_odd) in [
            (
                Ceiling,
                1_234_567_890_123_457,
                1_234_567_890_123_458,
                -1_234_567_890_123_456,
                -1_234_567_890_123_457,
            ),
            (
                Down,
                1_234_567_890_123_456,
                1_234_567_890_123_457,
                -1_234_567_890_123_456,
                -1_234_567_890_123_457,
            ),
            (
                Floor,
                1_234_567_890_123_456,
                1_234_567_890_123_457,
                -1_234_567_890_123_457,
                -1_234_567_890_123_458,
            ),
            (
                HalfDown,
                1_234_567_890_123_456,
                1_234_567_890_123_457,
                -1_234_567_890_123_456,
                -1_234_567_890_123_457,
            ),
            (
                HalfEven,
                1_234_567_890_123_456,
                1_234_567_890_123_458,
                -1_234_567_890_123_456,
                -1_234_567_890_123_458,
            ),
            (
                HalfUp,
                1_234_567_890_123_457,
                1_234_567_890_123_458,
                -1_234_567_890_123_457,
                -1_234_567_890_123_458,
            ),
            (
                Up,
                1_234_567_890_123_457,
                1_234_567_890_123_458,
                -1_234_567_890_123_457,
                -1_234_567_890_123_458,
            ),
        ] {
            for (text, coefficient) in [
                ("12345678901234565", positive_even),
                ("12345678901234575", positive_odd),
                ("-12345678901234565", negative_even),
                ("-12345678901234575", negative_odd),
            ] {
                expect(text, 16, mode, coefficient, 1, true);
            }
        }
    }

    #[test]
    fn seven_modes_below_and_above_half_both_signs() {
        use Db2DecFloatRoundingMode::*;
        for (mode, below, above, negative_below, negative_above) in [
            (
                Ceiling,
                1_234_567_890_123_457,
                1_234_567_890_123_457,
                -1_234_567_890_123_456,
                -1_234_567_890_123_456,
            ),
            (
                Down,
                1_234_567_890_123_456,
                1_234_567_890_123_456,
                -1_234_567_890_123_456,
                -1_234_567_890_123_456,
            ),
            (
                Floor,
                1_234_567_890_123_456,
                1_234_567_890_123_456,
                -1_234_567_890_123_457,
                -1_234_567_890_123_457,
            ),
            (
                HalfDown,
                1_234_567_890_123_456,
                1_234_567_890_123_457,
                -1_234_567_890_123_456,
                -1_234_567_890_123_457,
            ),
            (
                HalfEven,
                1_234_567_890_123_456,
                1_234_567_890_123_457,
                -1_234_567_890_123_456,
                -1_234_567_890_123_457,
            ),
            (
                HalfUp,
                1_234_567_890_123_456,
                1_234_567_890_123_457,
                -1_234_567_890_123_456,
                -1_234_567_890_123_457,
            ),
            (
                Up,
                1_234_567_890_123_457,
                1_234_567_890_123_457,
                -1_234_567_890_123_457,
                -1_234_567_890_123_457,
            ),
        ] {
            for (text, coefficient) in [
                ("12345678901234564", below),
                ("12345678901234566", above),
                ("-12345678901234564", negative_below),
                ("-12345678901234566", negative_above),
            ] {
                expect(text, 16, mode, coefficient, 1, true);
            }
        }
    }

    #[test]
    fn discarded_zero_digits_are_exact_for_every_mode_and_sign() {
        use Db2DecFloatRoundingMode::*;
        for mode in [Ceiling, Down, Floor, HalfDown, HalfEven, HalfUp, Up] {
            expect(
                "123456789012345600",
                16,
                mode,
                1_234_567_890_123_456,
                2,
                false,
            );
            expect(
                "-123456789012345600",
                16,
                mode,
                -1_234_567_890_123_456,
                2,
                false,
            );
            expect(
                "1234567890123456.00",
                16,
                mode,
                1_234_567_890_123_456,
                0,
                false,
            );
            expect(
                "-1234567890123456.00",
                16,
                mode,
                -1_234_567_890_123_456,
                0,
                false,
            );
        }
        expect(
            "123456789012345601",
            16,
            Down,
            1_234_567_890_123_456,
            2,
            true,
        );
        expect(
            "-123456789012345601",
            16,
            Down,
            -1_234_567_890_123_456,
            2,
            true,
        );
        expect("123456789012345601", 16, Up, 1_234_567_890_123_457, 2, true);
    }

    #[test]
    fn significant_digit_carry_retains_primitive_quantum() {
        use Db2DecFloatRoundingMode::*;
        expect(
            "99999999999999995",
            16,
            HalfEven,
            1_000_000_000_000_000,
            2,
            true,
        );
        expect(
            "-99999999999999995",
            16,
            HalfUp,
            -1_000_000_000_000_000,
            2,
            true,
        );
        expect(
            "99999999999999995",
            16,
            Down,
            9_999_999_999_999_999,
            1,
            true,
        );
        expect(
            "9999999999999999999999999999999",
            16,
            HalfUp,
            1_000_000_000_000_000,
            16,
            true,
        );
        expect(
            "-9999999999999999999999999999999",
            16,
            HalfUp,
            -1_000_000_000_000_000,
            16,
            true,
        );
    }

    #[test]
    fn exact_precision_targets_and_admitted_extrema() {
        use Db2DecFloatRoundingMode::HalfEven;
        for precision in [16, 34] {
            expect(
                "1234567890123456",
                precision,
                HalfEven,
                1_234_567_890_123_456,
                0,
                false,
            );
            expect(
                "1234567890.123456",
                precision,
                HalfEven,
                1_234_567_890_123_456,
                -6,
                false,
            );
            expect(
                ".0000000000000000000000000000001",
                precision,
                HalfEven,
                1,
                -31,
                false,
            );
            expect(
                "-.0000000000000000000000000000001",
                precision,
                HalfEven,
                -1,
                -31,
                false,
            );
        }
        expect(
            "9999999999999999999999999999999",
            34,
            HalfEven,
            9_999_999_999_999_999_999_999_999_999_999,
            0,
            false,
        );
        expect(
            "-9999999999999999999999999999999",
            34,
            HalfEven,
            -9_999_999_999_999_999_999_999_999_999_999,
            0,
            false,
        );
        expect(
            ".1234567890123456789012345678901",
            34,
            HalfEven,
            1_234_567_890_123_456_789_012_345_678_901,
            -31,
            false,
        );
    }

    #[test]
    fn integer_endpoints_and_temporary_decimal_attributes() {
        use Db2DecFloatRoundingMode::HalfEven;
        for (text, coefficient, temporary_precision) in [
            ("2147483647", 2_147_483_647, 11),
            ("-2147483648", -2_147_483_648, 11),
            ("9223372036854775807", 9_223_372_036_854_775_807, 19),
            ("-9223372036854775808", -9_223_372_036_854_775_808, 19),
        ] {
            expect(text, 34, HalfEven, coefficient, 0, false);
            let result = assign(text, 34, HalfEven);
            assert_eq!(
                result.temporary_decimal().unwrap().scalar(),
                &Db2ScalarType::Decimal {
                    precision: temporary_precision,
                    scale: 0
                }
            );
            assert_eq!(
                result.temporary_decimal().unwrap().nullability(),
                Db2Nullability::NotNull
            );
        }
        expect("2147483647", 16, HalfEven, 2_147_483_647, 0, false);
        expect("-2147483648", 16, HalfEven, -2_147_483_648, 0, false);
        expect(
            "9223372036854775807",
            16,
            HalfEven,
            9_223_372_036_854_776,
            3,
            true,
        );
        expect(
            "-9223372036854775808",
            16,
            HalfEven,
            -9_223_372_036_854_776,
            3,
            true,
        );
    }

    #[test]
    fn source_precision_is_not_coefficient_significance() {
        use Db2DecFloatRoundingMode::HalfEven;
        for precision in [16, 34] {
            expect(
                "000000000000000.1234567890123456",
                precision,
                HalfEven,
                1_234_567_890_123_456,
                -16,
                false,
            );
            expect("000.00100", precision, HalfEven, 100, -5, false);
            expect("025.50", precision, HalfEven, 2_550, -2, false);
            let result = assign("000.00100", precision, HalfEven);
            assert_eq!(
                result.source().resolved_type().scalar(),
                &Db2ScalarType::Decimal {
                    precision: 8,
                    scale: 5
                }
            );
            assert_eq!(result.temporary_decimal(), None);
        }
    }

    #[test]
    fn zero_quantum_and_canonical_negative_zero() {
        use Db2DecFloatRoundingMode::*;
        for mode in [Ceiling, Down, Floor, HalfDown, HalfEven, HalfUp, Up] {
            for precision in [16, 34] {
                for text in ["0", "-0", "+0"] {
                    expect(text, precision, mode, 0, 0, false);
                }
                for text in ["0.00000", "-0.00000", "+0.00000"] {
                    expect(text, precision, mode, 0, -5, false);
                }
                expect(
                    ".0000000000000000000000000000000",
                    precision,
                    mode,
                    0,
                    -31,
                    false,
                );
            }
        }
    }

    #[test]
    fn owned_original_utf8_crlf_trivia_span_and_nullable_target() {
        let result = {
            let text = String::from("é\r\n  - /*x*/ 025.50 tail");
            let located = |start_byte, end_byte, start_column, end_column| Db2SourceSpan {
                start_byte,
                end_byte,
                start: Db2SourceLocation {
                    line: 2,
                    column: start_column,
                },
                end: Db2SourceLocation {
                    line: 2,
                    column: end_column,
                },
            };
            let proof = materialize_db2_located_numeric_operand(
                &text,
                located(6, 20, 3, 17),
                Some(located(6, 7, 3, 4)),
                located(14, 20, 11, 17),
                Db2NumericConstantLimits::default(),
                crate::Db2SyntaxLimits::default(),
            )
            .unwrap();
            let target = target(
                crate::Db2BuiltInType::DecFloat,
                vec![34],
                Db2Nullability::Nullable,
            );
            assign_db2_finite_decfloat_constant(
                &proof,
                &target,
                Db2DecFloatRounding {
                    mode: Db2DecFloatRoundingMode::HalfDown,
                    context: Db2DecFloatRoundingContext::StaticCreateViewRegister,
                },
            )
            .unwrap()
            .clone()
        };
        assert_eq!(
            (result.coefficient(), result.exponent(), result.inexact()),
            (-2550, -2, false)
        );
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::Nullable
        );
        assert_eq!(
            result.source().resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!((result.span().start_byte, result.span().end_byte), (6, 20));
        assert_eq!(
            result.span().start,
            Db2SourceLocation { line: 2, column: 3 }
        );
        assert_eq!(
            result.span().end,
            Db2SourceLocation {
                line: 2,
                column: 17
            }
        );
        assert_eq!(
            result.rounding(),
            Db2DecFloatRounding {
                mode: Db2DecFloatRoundingMode::HalfDown,
                context: Db2DecFloatRoundingContext::StaticCreateViewRegister
            }
        );
        assert!(matches!(
            result.compatibility(),
            Db2AssignmentCompatibility::Compatible { .. }
        ));
        assert_eq!(
            result.source().value(),
            &Db2NumericConstantValue::Decimal(
                mainframe_env_encoding::DecimalValue::new(-2550, 2).unwrap()
            )
        );
    }

    #[test]
    fn explicit_origins_do_not_change_caller_mode_or_leak_status() {
        use Db2DecFloatRoundingContext::*;
        for context in [
            StaticBindOption,
            NativeProcedureOption,
            DynamicRegister,
            StaticCreateViewRegister,
        ] {
            for (mode, coefficient) in [
                (Db2DecFloatRoundingMode::Up, 1_234_567_890_123_457),
                (Db2DecFloatRoundingMode::Down, 1_234_567_890_123_456),
            ] {
                let rounding = Db2DecFloatRounding { mode, context };
                let result = assign_db2_finite_decfloat_constant(
                    &source("12345678901234561"),
                    &target(
                        crate::Db2BuiltInType::DecFloat,
                        vec![16],
                        Db2Nullability::NotNull,
                    ),
                    rounding,
                )
                .unwrap();
                assert_eq!(result.coefficient(), coefficient);
                assert_eq!(result.rounding(), rounding);
                assert!(result.inexact());
            }
            let result = assign_db2_finite_decfloat_constant(
                &source("1.00"),
                &target(
                    crate::Db2BuiltInType::DecFloat,
                    vec![34],
                    Db2Nullability::NotNull,
                ),
                Db2DecFloatRounding {
                    mode: Db2DecFloatRoundingMode::Down,
                    context,
                },
            )
            .unwrap();
            assert!(!result.inexact());
            assert_eq!((result.coefficient(), result.exponent()), (100, -2));
        }
    }

    #[test]
    fn incompatible_and_compatible_wrong_targets_fail_before_conversion() {
        use crate::Db2BuiltInType::*;
        let proof = source("1");
        let rounding = Db2DecFloatRounding {
            mode: Db2DecFloatRoundingMode::Up,
            context: Db2DecFloatRoundingContext::DynamicRegister,
        };
        for (kind, args, expected) in [
            (
                Binary,
                vec![1],
                Db2DecFloatAssignmentErrorCode::IncompatibleTypes,
            ),
            (
                Date,
                vec![],
                Db2DecFloatAssignmentErrorCode::IncompatibleTypes,
            ),
            (Integer, vec![], Db2DecFloatAssignmentErrorCode::WrongTarget),
            (
                Decimal,
                vec![31, 0],
                Db2DecFloatAssignmentErrorCode::WrongTarget,
            ),
            (Real, vec![], Db2DecFloatAssignmentErrorCode::WrongTarget),
            (Double, vec![], Db2DecFloatAssignmentErrorCode::WrongTarget),
            (
                VarChar,
                vec![10],
                Db2DecFloatAssignmentErrorCode::WrongTarget,
            ),
        ] {
            let error = assign_db2_finite_decfloat_constant(
                &proof,
                &target(kind, args, Db2Nullability::NotNull),
                rounding,
            )
            .unwrap_err();
            assert_eq!(error.code, expected);
            assert_eq!(error.span, proof.span());
            assert!(!error.to_string().is_empty());
        }
        // Existing exact assignment remains fenced; this child did not lift it.
        assert_eq!(
            super::super::assign_db2_numeric_constant(
                &proof,
                &target(DecFloat, vec![16], Db2Nullability::NotNull)
            )
            .unwrap_err()
            .code,
            super::super::Db2NumericAssignmentErrorCode::UnsupportedDecFloatTarget
        );
    }

    #[test]
    fn primitive_status_and_result_guards_reject_exceptional_outputs() {
        for flag in [
            dec::Status::set_conversion_syntax,
            dec::Status::set_division_by_zero,
            dec::Status::set_division_impossible,
            dec::Status::set_division_undefined,
            dec::Status::set_insufficient_storage,
            dec::Status::set_invalid_context,
            dec::Status::set_invalid_operation,
            dec::Status::set_overflow,
            dec::Status::set_clamped,
            dec::Status::set_subnormal,
            dec::Status::set_underflow,
        ] {
            let mut status = dec::Status::default();
            flag(&mut status);
            assert_eq!(
                admit_status(status),
                Err(Db2DecFloatAssignmentErrorCode::PrimitiveStatus)
            );
            status.set_inexact();
            assert_eq!(
                admit_status(status),
                Err(Db2DecFloatAssignmentErrorCode::PrimitiveStatus)
            );
        }
        let mut rounded = dec::Status::default();
        rounded.set_rounded();
        assert_eq!(admit_status(rounded), Ok(false));
        rounded.set_inexact();
        assert_eq!(admit_status(rounded), Ok(true));
        for (coefficient, exponent, negative, precision, original) in [
            (10_000_000_000_000_000, 0, false, 16, 1),
            (1, -32, false, 16, 1),
            (1, 17, false, 34, 1),
            (1, 0, true, 16, 1),
            (-1, 0, true, 16, 1),
            (0, 0, false, 16, 1),
            (1, 0, false, 16, 0),
            (1, 0, false, 17, 1),
            (0, 0, true, 16, 0),
        ] {
            assert_eq!(
                admit_result(coefficient, exponent, negative, precision, original),
                Err(Db2DecFloatAssignmentErrorCode::InvalidResult)
            );
        }
    }

    #[test]
    fn primitive_input_is_bounded_exact_value_transport() {
        assert_eq!(
            primitive_input(-9_999_999_999_999_999_999_999_999_999_999, 31).unwrap(),
            b"-9999999999999999999999999999999E-31"
        );
        assert_eq!(primitive_input(0, 31).unwrap(), b"0E-31");
        let mut input = PrimitiveInput {
            bytes: [0; 40],
            len: 39,
        };
        assert!(input.write_str("00").is_err());
        assert_eq!(input.len, 39);
    }

    #[test]
    fn primitive_boundary_rejects_special_parse_and_range_failures() {
        use Db2DecFloatAssignmentErrorCode as Code;
        for precision in [16, 34] {
            for (text, expected) in [
                ("NaN", Code::NonFinite),
                ("sNaN", Code::NonFinite),
                ("Infinity", Code::NonFinite),
                ("-Infinity", Code::NonFinite),
                ("invalid", Code::PrimitiveFailure),
                ("1\0E0", Code::PrimitiveFailure),
                ("1E99999", Code::PrimitiveStatus),
                ("1E-99999", Code::PrimitiveStatus),
            ] {
                assert_eq!(
                    convert_primitive(
                        text.as_bytes().to_vec(),
                        precision,
                        Db2DecFloatRoundingMode::HalfEven
                    ),
                    Err(expected),
                    "{precision}, {text}"
                );
            }
        }
        assert_eq!(
            convert_primitive(vec![b'1'; 37], 16, Db2DecFloatRoundingMode::Down),
            Err(Code::InputBounds)
        );
        assert_eq!(
            convert_primitive(b"1E0".to_vec(), 17, Db2DecFloatRoundingMode::Down),
            Err(Code::WrongTarget)
        );
    }
}
