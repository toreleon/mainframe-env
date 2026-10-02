//! Pure numeric arithmetic metadata, never an evaluator or SQLCA authority.
//!
//! Sources: ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_witharithmeticoperators.html, 37064 bytes,
//!   83a3db8b0d913dcbaff86c47624fda48fd2d462988499f83e73a6965d0f7d459;
//! - db2z_constantsintro.html, 17915 bytes,
//!   bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8;
//! - db2z_datatypesintro.html, 22904 bytes,
//!   a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570.
//! Language elements have no standalone statement-catalog row. Existing
//! resolved types own validity; context is explicitly resolved, never guessed.
//! Constants use the existing located classifier, without lifting lexer fences.
//! No expression/list backend, binding, value, execution, SQLCA, licensed or
//! official coverage claim is made. Decimal/binary-float mixing with DECFLOAT
//! remains explicitly unresolved: the arithmetic pin supplies no temporary
//! precision rule. The CASE/set-operation table excludes arithmetic, and the
//! DECFLOAT scalar-function default is not an arithmetic conversion authority.

use crate::{
    Db2AstLimits, Db2BinaryOperator, Db2BuiltInDataType, Db2BuiltInType, Db2DataType,
    Db2Nullability, Db2NumericConstantError, Db2NumericConstantLimits, Db2ResolvedType,
    Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2UnaryOperator,
    classify_db2_numeric_constant, resolve_db2_type,
};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2DecimalArithmetic {
    Dec15,
    Dec31,
}

/// Selected by the caller's static/dynamic SQL context owner, with no default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ArithmeticContext {
    decimal: Db2DecimalArithmetic,
    minimum_divide_scale: Option<u32>,
}

impl Db2ArithmeticContext {
    /// Zero means no configured minimum; 1..=9 is an explicit minimum.
    pub fn new(
        decimal: Db2DecimalArithmetic,
        minimum_divide_scale: u32,
        span: Db2SourceSpan,
    ) -> Result<Self, Db2ArithmeticError> {
        if minimum_divide_scale > 9 {
            return Err(error(
                Db2ArithmeticErrorCode::InvalidContext,
                span,
                "resolved minimum divide scale must be zero or 1 through 9",
            ));
        }
        Ok(Self {
            decimal,
            minimum_divide_scale: (minimum_divide_scale != 0).then_some(minimum_divide_scale),
        })
    }
    #[must_use]
    pub const fn decimal(self) -> Db2DecimalArithmetic {
        self.decimal
    }
    #[must_use]
    pub const fn minimum_divide_scale(self) -> Option<u32> {
        self.minimum_divide_scale
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ArithmeticErrorCode {
    InvalidContext,
    InvalidSpan,
    OutsideNumericSubset,
    UnsupportedOperator,
    UnresolvedDecFloatConversion,
    NegativeDivisionScale,
    InvalidLimits,
    OperandLimit,
    ScanLimit,
    SourceLimit,
    InvalidResultType,
}

/// Fixed located diagnostic; retains no borrowed source or arbitrary strings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ArithmeticError {
    pub code: Db2ArithmeticErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

fn error(
    code: Db2ArithmeticErrorCode,
    span: Db2SourceSpan,
    message: &'static str,
) -> Db2ArithmeticError {
    Db2ArithmeticError {
        code,
        span,
        message,
    }
}
impl fmt::Display for Db2ArithmeticError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}
impl std::error::Error for Db2ArithmeticError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2ArithmeticConstantError {
    Arithmetic(Db2ArithmeticError),
    Constant(Db2NumericConstantError),
}

/// Opaque provenance: callers cannot attach an arbitrary digit count to a type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ArithmeticOperand {
    resolved_type: Db2ResolvedType,
    span: Db2SourceSpan,
    integer_constant_digits: Option<u32>,
}

impl Db2ArithmeticOperand {
    /// A binder-supplied validated type and original-source span. This creates
    /// a nonconstant operand, even when its type matches a constant's type.
    pub fn resolved(
        resolved_type: Db2ResolvedType,
        span: Db2SourceSpan,
    ) -> Result<Self, Db2ArithmeticError> {
        validate_span(span)?;
        if !is_integer(resolved_type.scalar())
            && !is_float(resolved_type.scalar())
            && !matches!(
                resolved_type.scalar(),
                Db2ScalarType::Decimal { .. } | Db2ScalarType::DecFloat { .. }
            )
        {
            return Err(error(
                Db2ArithmeticErrorCode::OutsideNumericSubset,
                span,
                "operand is outside this numeric subset; datetime and string contexts require their own rules",
            ));
        }
        Ok(Self {
            resolved_type,
            span,
            integer_constant_digits: None,
        })
    }
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.resolved_type
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
    #[must_use]
    pub const fn integer_constant_digits(&self) -> Option<u32> {
        self.integer_constant_digits
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ArithmeticConstantLimits {
    pub constants: Db2NumericConstantLimits,
    pub max_operands: usize,
    /// Aggregate upper bound for borrowed-source scans across the entire batch.
    pub max_scan_bytes: usize,
}
impl Default for Db2ArithmeticConstantLimits {
    fn default() -> Self {
        Self {
            constants: Db2NumericConstantLimits::default(),
            max_operands: 128,
            max_scan_bytes: 32 * 1024 * 1024,
        }
    }
}

/// Classify a bounded batch from one original source. Owned output survives
/// source destruction. Repeated/overlapping spans count against the same scan
/// budget; callers pass the entire statement's constant batch in one call.
pub fn classify_db2_arithmetic_constants(
    source: &str,
    spans: &[Db2SourceSpan],
    limits: Db2ArithmeticConstantLimits,
) -> Result<Vec<Db2ArithmeticOperand>, Db2ArithmeticConstantError> {
    use Db2ArithmeticConstantError as Error;
    use Db2ArithmeticErrorCode as Code;
    let origin = Db2SourceSpan {
        start_byte: 0,
        end_byte: 0,
        start: Db2SourceLocation::START,
        end: Db2SourceLocation::START,
    };
    if limits.max_operands == 0
        || limits.max_operands > 1024
        || limits.max_scan_bytes == 0
        || limits.max_scan_bytes > 64 * 1024 * 1024
        || limits.constants.max_source_bytes == 0
        || limits.constants.max_source_bytes > 8 * 1024 * 1024
        || limits.constants.ast.validate().is_err()
    {
        return Err(Error::Arithmetic(error(
            Code::InvalidLimits,
            origin,
            "arithmetic constant limits are zero or exceed compiled ceilings",
        )));
    }
    if spans.len() > limits.max_operands {
        return Err(Error::Arithmetic(error(
            Code::OperandLimit,
            origin,
            "constant batch exceeds the aggregate operand limit",
        )));
    }
    if source.len() > limits.constants.max_source_bytes {
        return Err(Error::Arithmetic(error(
            Code::SourceLimit,
            origin,
            "original source exceeds the configured byte limit",
        )));
    }
    // Two full-source location scans per classifier, plus a conservative eight
    // spelling traversals for classification, magnitude comparison and counting.
    let mut scan_bytes = 0usize;
    for span in spans {
        let cost = source.len().checked_mul(2).and_then(|prefix| {
            span.end_byte
                .checked_sub(span.start_byte)
                .and_then(|length| length.checked_mul(8))
                .and_then(|spelling| prefix.checked_add(spelling))
        });
        scan_bytes = cost
            .and_then(|cost| scan_bytes.checked_add(cost))
            .ok_or_else(|| {
                Error::Arithmetic(error(
                    Code::ScanLimit,
                    *span,
                    "aggregate constant scan size overflows",
                ))
            })?;
        if scan_bytes > limits.max_scan_bytes {
            return Err(Error::Arithmetic(error(
                Code::ScanLimit,
                *span,
                "constant batch exceeds the aggregate borrowed-source scan limit",
            )));
        }
    }
    spans
        .iter()
        .map(|span| {
            let constant = classify_db2_numeric_constant(source, *span, limits.constants)
                .map_err(Error::Constant)?;
            let resolved_type = constant.resolved_type().clone();
            let integer_constant_digits = if is_integer(resolved_type.scalar()) {
                Some(
                    source[span.start_byte..span.end_byte]
                        .bytes()
                        .filter(u8::is_ascii_digit)
                        .count() as u32,
                )
            } else {
                None
            };
            Ok(Db2ArithmeticOperand {
                resolved_type,
                span: *span,
                integer_constant_digits,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ArithmeticSide {
    Left,
    Right,
}

/// A runtime conversion obligation, not a performed truncation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2DecimalTruncation {
    pub operand: Db2ArithmeticSide,
    pub original_precision: u32,
    pub original_scale: u32,
    pub temporary_precision: u32,
    pub temporary_scale: u32,
    /// Reject at runtime if the integral part needs more significant digits.
    pub maximum_integral_significant_digits: u32,
    /// SQLWARN7=W is required if removed digits include a nonzero digit.
    pub sqlwarn7_if_nonzero_digits_removed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2DecimalMultiplyCheck {
    pub operand: Db2ArithmeticSide,
    /// Leading zeros in this operand's 31-digit representation must be
    /// STRICTLY greater than this precision, after temporary truncation.
    pub leading_zeros_must_exceed: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ArithmeticRangeCheck {
    None,
    IntegerResult,
    DecimalResult,
    FloatingPointResult,
    /// Includes rounding-mode, underflow/overflow, NaN/infinity and sign rules.
    DecimalFloatingPoint,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ArithmeticZeroCheck {
    None,
    DivisorMustBeNonzero,
    /// DECFLOAT division has its own zero/special-value runtime conditions,
    /// including infinity/NaN, rather than universal integer-style rejection.
    DecimalFloatingPointDivision,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ArithmeticSignRule {
    Unchanged,
    ReverseNonzero,
    ReverseIncludingDecFloatZeroAndSpecial,
    BinaryRuntime,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ArithmeticObligations {
    pub range: Db2ArithmeticRangeCheck,
    pub zero: Db2ArithmeticZeroCheck,
    pub sign: Db2ArithmeticSignRule,
    pub truncation: Option<Db2DecimalTruncation>,
    pub multiplication: Option<Db2DecimalMultiplyCheck>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ArithmeticResult {
    resolved_type: Db2ResolvedType,
    span: Db2SourceSpan,
    obligations: Db2ArithmeticObligations,
    /// Signed table scale, before applying the configured minimum.
    calculated_division_scale: Option<i32>,
}
impl Db2ArithmeticResult {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.resolved_type
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
    #[must_use]
    pub const fn obligations(&self) -> Db2ArithmeticObligations {
        self.obligations
    }
    #[must_use]
    pub const fn calculated_division_scale(&self) -> Option<i32> {
        self.calculated_division_scale
    }
    /// Intermediate results never inherit literal digit provenance.
    #[must_use]
    pub fn into_operand(self) -> Db2ArithmeticOperand {
        Db2ArithmeticOperand {
            resolved_type: self.resolved_type,
            span: self.span,
            integer_constant_digits: None,
        }
    }
}

pub fn resolve_db2_unary_arithmetic(
    operator: Db2UnaryOperator,
    operand: &Db2ArithmeticOperand,
    span: Db2SourceSpan,
) -> Result<Db2ArithmeticResult, Db2ArithmeticError> {
    contains(span, operand.span)?;
    let mut obligations = obligations(Db2ArithmeticRangeCheck::None);
    obligations.sign = Db2ArithmeticSignRule::Unchanged;
    let resolved_type = match operator {
        Db2UnaryOperator::Positive => operand.resolved_type.clone(),
        Db2UnaryOperator::Negative => {
            obligations.sign = if matches!(
                operand.resolved_type.scalar(),
                Db2ScalarType::DecFloat { .. }
            ) {
                Db2ArithmeticSignRule::ReverseIncludingDecFloatZeroAndSpecial
            } else {
                Db2ArithmeticSignRule::ReverseNonzero
            };
            if is_integer(operand.resolved_type.scalar()) {
                obligations.range = Db2ArithmeticRangeCheck::IntegerResult;
            }
            if matches!(operand.resolved_type.scalar(), Db2ScalarType::SmallInt) {
                make_type(
                    Db2BuiltInType::Integer,
                    &[],
                    operand.resolved_type.nullability(),
                    span,
                )?
            } else {
                operand.resolved_type.clone()
            }
        }
        Db2UnaryOperator::Not => {
            return Err(error(
                Db2ArithmeticErrorCode::UnsupportedOperator,
                span,
                "operator is outside unary numeric arithmetic",
            ));
        }
    };
    Ok(Db2ArithmeticResult {
        resolved_type,
        span,
        obligations,
        calculated_division_scale: None,
    })
}

pub fn resolve_db2_binary_arithmetic(
    operator: Db2BinaryOperator,
    left: &Db2ArithmeticOperand,
    right: &Db2ArithmeticOperand,
    context: Db2ArithmeticContext,
    span: Db2SourceSpan,
) -> Result<Db2ArithmeticResult, Db2ArithmeticError> {
    use Db2ScalarType as Type;
    contains(span, left.span)?;
    contains(span, right.span)?;
    if !matches!(
        operator,
        Db2BinaryOperator::Add
            | Db2BinaryOperator::Subtract
            | Db2BinaryOperator::Multiply
            | Db2BinaryOperator::Divide
    ) {
        return Err(error(
            Db2ArithmeticErrorCode::UnsupportedOperator,
            span,
            "operator is outside binary numeric arithmetic",
        ));
    }
    let nullability = if left.resolved_type.nullability() == Db2Nullability::Nullable
        || right.resolved_type.nullability() == Db2Nullability::Nullable
    {
        Db2Nullability::Nullable
    } else {
        Db2Nullability::NotNull
    };
    let a = left.resolved_type.scalar();
    let b = right.resolved_type.scalar();
    let (kind, arguments, range) =
        if matches!(a, Type::DecFloat { .. }) || matches!(b, Type::DecFloat { .. }) {
            let precision = decfloat_precision(a, span)?.max(decfloat_precision(b, span)?);
            (
                Db2BuiltInType::DecFloat,
                vec![precision],
                Db2ArithmeticRangeCheck::DecimalFloatingPoint,
            )
        } else if is_float(a) || is_float(b) {
            (
                Db2BuiltInType::Double,
                vec![],
                Db2ArithmeticRangeCheck::FloatingPointResult,
            )
        } else if is_integer(a) && is_integer(b) {
            let kind = if matches!(a, Type::BigInt) || matches!(b, Type::BigInt) {
                Db2BuiltInType::BigInt
            } else {
                Db2BuiltInType::Integer
            };
            (kind, vec![], Db2ArithmeticRangeCheck::IntegerResult)
        } else {
            return decimal_result(operator, left, right, context, nullability, span);
        };
    let mut obligations = obligations(range);
    if operator == Db2BinaryOperator::Divide {
        obligations.zero = if range == Db2ArithmeticRangeCheck::DecimalFloatingPoint {
            Db2ArithmeticZeroCheck::DecimalFloatingPointDivision
        } else {
            Db2ArithmeticZeroCheck::DivisorMustBeNonzero
        };
    }
    Ok(Db2ArithmeticResult {
        resolved_type: make_type(kind, &arguments, nullability, span)?,
        span,
        obligations,
        calculated_division_scale: None,
    })
}

fn decfloat_precision(
    scalar: &Db2ScalarType,
    span: Db2SourceSpan,
) -> Result<u32, Db2ArithmeticError> {
    match scalar {
        Db2ScalarType::DecFloat { precision } => Ok(*precision),
        Db2ScalarType::SmallInt | Db2ScalarType::Integer => Ok(16),
        Db2ScalarType::BigInt => Ok(34),
        _ => Err(error(
            Db2ArithmeticErrorCode::UnresolvedDecFloatConversion,
            span,
            "decimal or binary-float mixing with DECFLOAT needs a pinned temporary-conversion precision rule",
        )),
    }
}

fn decimal_result(
    operator: Db2BinaryOperator,
    left: &Db2ArithmeticOperand,
    right: &Db2ArithmeticOperand,
    context: Db2ArithmeticContext,
    nullability: Db2Nullability,
    span: Db2SourceSpan,
) -> Result<Db2ArithmeticResult, Db2ArithmeticError> {
    let (mut p, mut s) = decimal_shape(left);
    let (mut q, mut t) = decimal_shape(right);
    let n = if context.decimal == Db2DecimalArithmetic::Dec31 || p > 15 || q > 15 {
        31
    } else {
        15
    };
    let mut obligations = obligations(Db2ArithmeticRangeCheck::DecimalResult);
    let mut calculated_division_scale = None;
    let (precision, scale) = match operator {
        Db2BinaryOperator::Add | Db2BinaryOperator::Subtract => {
            (n.min((p - s).max(q - t) + s.max(t) + 1), s.max(t))
        }
        Db2BinaryOperator::Multiply => {
            let selected = if p < q {
                Db2ArithmeticSide::Left
            } else {
                Db2ArithmeticSide::Right
            };
            if p > 15 && q > 15 {
                let (precision, scale) = if selected == Db2ArithmeticSide::Left {
                    (p, s)
                } else {
                    (q, t)
                };
                let truncated = truncation(selected, precision, scale);
                if selected == Db2ArithmeticSide::Left {
                    p = 15;
                    s = truncated.temporary_scale;
                } else {
                    q = 15;
                    t = truncated.temporary_scale;
                }
                obligations.truncation = Some(truncated);
            }
            obligations.multiplication = Some(Db2DecimalMultiplyCheck {
                operand: if selected == Db2ArithmeticSide::Left {
                    Db2ArithmeticSide::Right
                } else {
                    Db2ArithmeticSide::Left
                },
                leading_zeros_must_exceed: p.min(q),
            });
            (n.min(p + q), n.min(s + t))
        }
        Db2BinaryOperator::Divide => {
            obligations.zero = Db2ArithmeticZeroCheck::DivisorMustBeNonzero;
            let base = if q > 15 {
                let truncated = truncation(Db2ArithmeticSide::Right, q, t);
                t = truncated.temporary_scale;
                obligations.truncation = Some(truncated);
                15
            } else if n == 15 {
                15
            } else if q % 2 == 1 {
                30 - q
            } else {
                29 - q
            };
            let table_scale = base as i32 - (p - s + t) as i32;
            calculated_division_scale = Some(table_scale);
            let effective = context
                .minimum_divide_scale
                .map_or(table_scale, |minimum| table_scale.max(minimum as i32));
            if effective < 0 {
                return Err(error(
                    Db2ArithmeticErrorCode::NegativeDivisionScale,
                    span,
                    "decimal division table gives negative scale without a configured minimum",
                ));
            }
            (n, effective as u32)
        }
        _ => {
            return Err(error(
                Db2ArithmeticErrorCode::UnsupportedOperator,
                span,
                "operator is outside decimal arithmetic",
            ));
        }
    };
    Ok(Db2ArithmeticResult {
        resolved_type: make_type(
            Db2BuiltInType::Decimal,
            &[precision, scale],
            nullability,
            span,
        )?,
        span,
        obligations,
        calculated_division_scale,
    })
}

fn truncation(operand: Db2ArithmeticSide, precision: u32, scale: u32) -> Db2DecimalTruncation {
    Db2DecimalTruncation {
        operand,
        original_precision: precision,
        original_scale: scale,
        temporary_precision: 15,
        temporary_scale: scale.saturating_sub(precision - 15),
        maximum_integral_significant_digits: 15,
        sqlwarn7_if_nonzero_digits_removed: true,
    }
}

fn decimal_shape(operand: &Db2ArithmeticOperand) -> (u32, u32) {
    match operand.resolved_type.scalar() {
        Db2ScalarType::Decimal { precision, scale } => (*precision, *scale),
        Db2ScalarType::SmallInt => (5, 0),
        Db2ScalarType::Integer => (
            operand
                .integer_constant_digits
                .map_or(11, |digits| digits.max(5)),
            0,
        ),
        Db2ScalarType::BigInt => (
            operand
                .integer_constant_digits
                .map_or(19, |digits| digits.max(5)),
            0,
        ),
        _ => unreachable!("numeric constructors and promotion route decimal/integer operands here"),
    }
}
fn obligations(range: Db2ArithmeticRangeCheck) -> Db2ArithmeticObligations {
    Db2ArithmeticObligations {
        range,
        zero: Db2ArithmeticZeroCheck::None,
        sign: Db2ArithmeticSignRule::BinaryRuntime,
        truncation: None,
        multiplication: None,
    }
}
fn make_type(
    kind: Db2BuiltInType,
    arguments: &[u32],
    nullability: Db2Nullability,
    span: Db2SourceSpan,
) -> Result<Db2ResolvedType, Db2ArithmeticError> {
    let syntax = Db2BuiltInDataType::new(kind, arguments.to_vec(), false, Db2AstLimits::default())
        .map_err(|_| {
            error(
                Db2ArithmeticErrorCode::InvalidResultType,
                span,
                "result syntax exceeds existing constructor bounds",
            )
        })?;
    resolve_db2_type(&Db2DataType::BuiltIn(syntax), nullability).map_err(|_| {
        error(
            Db2ArithmeticErrorCode::InvalidResultType,
            span,
            "result rejected by the existing resolved-type authority",
        )
    })
}
fn is_integer(scalar: &Db2ScalarType) -> bool {
    matches!(
        scalar,
        Db2ScalarType::SmallInt | Db2ScalarType::Integer | Db2ScalarType::BigInt
    )
}
fn is_float(scalar: &Db2ScalarType) -> bool {
    matches!(
        scalar,
        Db2ScalarType::Real | Db2ScalarType::Double | Db2ScalarType::Float { .. }
    )
}
fn validate_span(span: Db2SourceSpan) -> Result<(), Db2ArithmeticError> {
    if span.start_byte >= span.end_byte
        || span.start.line == 0
        || span.start.column == 0
        || span.end.line == 0
        || span.end.column == 0
        || span.start.line > span.end.line
        || (span.start.line == span.end.line && span.start.column >= span.end.column)
    {
        Err(error(
            Db2ArithmeticErrorCode::InvalidSpan,
            span,
            "metadata requires a nonempty original byte and line/column span",
        ))
    } else {
        Ok(())
    }
}
fn contains(outer: Db2SourceSpan, inner: Db2SourceSpan) -> Result<(), Db2ArithmeticError> {
    validate_span(outer)?;
    if outer.start_byte > inner.start_byte
        || outer.end_byte < inner.end_byte
        || (outer.start.line, outer.start.column) > (inner.start.line, inner.start.column)
        || (outer.end.line, outer.end.column) < (inner.end.line, inner.end.column)
        || (outer.start_byte == inner.start_byte && outer.start != inner.start)
        || (outer.end_byte == inner.end_byte && outer.end != inner.end)
    {
        Err(error(
            Db2ArithmeticErrorCode::InvalidSpan,
            outer,
            "arithmetic span does not contain operand bytes and locations",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db2NumericConstantErrorCode;
    use Db2BinaryOperator::{Add, Divide, Multiply, Subtract};
    use Db2BuiltInType as Kind;
    use Db2DecimalArithmetic::{Dec15, Dec31};
    use Db2ScalarType as Type;

    fn location(source: &str, end: usize) -> Db2SourceLocation {
        let mut at = Db2SourceLocation::START;
        let mut cr = false;
        for ch in source[..end].chars() {
            if ch == '\r' || (ch == '\n' && !cr) {
                at.line += 1;
                at.column = 1;
            } else if ch != '\n' || !cr {
                at.column += 1;
            }
            cr = ch == '\r';
        }
        at
    }
    fn located(source: &str, start: usize, end: usize) -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: start,
            end_byte: end,
            start: location(source, start),
            end: location(source, end),
        }
    }
    fn span() -> Db2SourceSpan {
        located("x", 0, 1)
    }
    fn context(mode: Db2DecimalArithmetic, minimum: u32) -> Db2ArithmeticContext {
        Db2ArithmeticContext::new(mode, minimum, span()).unwrap()
    }
    fn operand(kind: Kind, arguments: &[u32], nullable: bool) -> Db2ArithmeticOperand {
        Db2ArithmeticOperand::resolved(
            make_type(
                kind,
                arguments,
                if nullable {
                    Db2Nullability::Nullable
                } else {
                    Db2Nullability::NotNull
                },
                span(),
            )
            .unwrap(),
            span(),
        )
        .unwrap()
    }
    fn decimal(p: u32, s: u32) -> Db2ArithmeticOperand {
        operand(Kind::Decimal, &[p, s], false)
    }
    fn binary(
        op: Db2BinaryOperator,
        a: &Db2ArithmeticOperand,
        b: &Db2ArithmeticOperand,
        mode: Db2DecimalArithmetic,
        minimum: u32,
    ) -> Db2ArithmeticResult {
        resolve_db2_binary_arithmetic(op, a, b, context(mode, minimum), span()).unwrap()
    }
    fn constant(text: &str) -> Db2ArithmeticOperand {
        classify_db2_arithmetic_constants(
            text,
            &[located(text, 0, text.len())],
            Db2ArithmeticConstantLimits::default(),
        )
        .unwrap()
        .remove(0)
    }
    fn expect_decimal(result: &Db2ArithmeticResult, p: u32, s: u32) {
        assert_eq!(
            result.resolved_type().scalar(),
            &Type::Decimal {
                precision: p,
                scale: s
            }
        );
    }

    #[test]
    fn integer_and_floating_promotion_commuted_matrix() {
        let cases = [
            (
                Kind::SmallInt,
                vec![],
                Kind::SmallInt,
                vec![],
                Type::Integer,
            ),
            (Kind::SmallInt, vec![], Kind::Integer, vec![], Type::Integer),
            (Kind::Integer, vec![], Kind::Integer, vec![], Type::Integer),
            (Kind::BigInt, vec![], Kind::SmallInt, vec![], Type::BigInt),
            (Kind::BigInt, vec![], Kind::Integer, vec![], Type::BigInt),
            (Kind::BigInt, vec![], Kind::BigInt, vec![], Type::BigInt),
            (Kind::Real, vec![], Kind::Real, vec![], Type::Double),
            (Kind::Double, vec![], Kind::SmallInt, vec![], Type::Double),
            (Kind::Float, vec![1], Kind::BigInt, vec![], Type::Double),
            (
                Kind::Float,
                vec![21],
                Kind::Decimal,
                vec![31, 31],
                Type::Double,
            ),
            (Kind::Float, vec![22], Kind::Real, vec![], Type::Double),
            (Kind::Float, vec![53], Kind::Float, vec![21], Type::Double),
            (
                Kind::DecFloat,
                vec![16],
                Kind::SmallInt,
                vec![],
                Type::DecFloat { precision: 16 },
            ),
            (
                Kind::DecFloat,
                vec![16],
                Kind::Integer,
                vec![],
                Type::DecFloat { precision: 16 },
            ),
            (
                Kind::DecFloat,
                vec![16],
                Kind::BigInt,
                vec![],
                Type::DecFloat { precision: 34 },
            ),
            (
                Kind::DecFloat,
                vec![16],
                Kind::DecFloat,
                vec![34],
                Type::DecFloat { precision: 34 },
            ),
            (
                Kind::DecFloat,
                vec![16],
                Kind::DecFloat,
                vec![16],
                Type::DecFloat { precision: 16 },
            ),
            (
                Kind::DecFloat,
                vec![34],
                Kind::DecFloat,
                vec![34],
                Type::DecFloat { precision: 34 },
            ),
        ];
        for (ka, aa, kb, ab, expected) in cases {
            let a = operand(ka, &aa, false);
            let b = operand(kb, &ab, false);
            for op in [Add, Subtract, Multiply, Divide] {
                for (left, right) in [(&a, &b), (&b, &a)] {
                    let result = binary(op, left, right, Dec15, 0);
                    assert_eq!(result.resolved_type().scalar(), &expected);
                    assert_eq!(
                        result.resolved_type().nullability(),
                        Db2Nullability::NotNull
                    );
                    assert!(result.obligations().truncation.is_none());
                    assert!(result.calculated_division_scale().is_none());
                }
            }
        }
    }

    #[test]
    fn unary_promotion_sign_zero_and_owned_intermediates() {
        for (kind, args) in [
            (Kind::SmallInt, vec![]),
            (Kind::Integer, vec![]),
            (Kind::BigInt, vec![]),
            (Kind::Decimal, vec![31, 31]),
            (Kind::Real, vec![]),
            (Kind::Double, vec![]),
            (Kind::Float, vec![21]),
            (Kind::DecFloat, vec![16]),
            (Kind::DecFloat, vec![34]),
        ] {
            let a = operand(kind, &args, true);
            let plus =
                resolve_db2_unary_arithmetic(Db2UnaryOperator::Positive, &a, span()).unwrap();
            assert_eq!(plus.resolved_type(), a.resolved_type());
            assert_eq!(plus.obligations().sign, Db2ArithmeticSignRule::Unchanged);
            let minus =
                resolve_db2_unary_arithmetic(Db2UnaryOperator::Negative, &a, span()).unwrap();
            let expected = if kind == Kind::SmallInt {
                Type::Integer
            } else {
                a.resolved_type().scalar().clone()
            };
            assert_eq!(minus.resolved_type().scalar(), &expected);
            assert_eq!(
                minus.resolved_type().nullability(),
                Db2Nullability::Nullable
            );
            assert_eq!(
                minus.obligations().sign,
                if kind == Kind::DecFloat {
                    Db2ArithmeticSignRule::ReverseIncludingDecFloatZeroAndSpecial
                } else {
                    Db2ArithmeticSignRule::ReverseNonzero
                }
            );
            assert_eq!(
                minus.obligations().range,
                if is_integer(a.resolved_type().scalar()) {
                    Db2ArithmeticRangeCheck::IntegerResult
                } else {
                    Db2ArithmeticRangeCheck::None
                }
            );
        }
        for text in ["0", "-0", "+00000"] {
            let a = constant(text);
            assert!(a.integer_constant_digits().is_some());
            let result =
                resolve_db2_unary_arithmetic(Db2UnaryOperator::Negative, &a, a.span()).unwrap();
            assert!(result.into_operand().integer_constant_digits().is_none());
        }
        let owned = {
            let text = String::from("000001");
            constant(&text)
        };
        assert_eq!(owned.integer_constant_digits(), Some(6));
        assert_eq!(owned.span().end_byte, 6);
    }

    #[test]
    fn integer_decimal_nonconstant_and_verified_digit_provenance() {
        let d = decimal(1, 0);
        for (kind, p) in [(Kind::SmallInt, 5), (Kind::Integer, 11), (Kind::BigInt, 19)] {
            let a = operand(kind, &[], false);
            for (left, right) in [(&a, &d), (&d, &a)] {
                expect_decimal(&binary(Add, left, right, Dec31, 0), p + 1, 0);
                expect_decimal(&binary(Multiply, left, right, Dec31, 0), p + 1, 0);
            }
        }
        for (text, p) in [
            ("1", 5),
            ("-00001", 5),
            ("+000001", 6),
            ("2147483647", 10),
            ("-2147483648", 10),
            ("2147483648", 10),
            ("9223372036854775807", 19),
            ("-9223372036854775808", 19),
        ] {
            let a = constant(text);
            assert_eq!(decimal_shape(&a), (p, 0));
            let relocated_d =
                Db2ArithmeticOperand::resolved(d.resolved_type.clone(), a.span()).unwrap();
            let result =
                resolve_db2_binary_arithmetic(Add, &a, &relocated_d, context(Dec31, 0), a.span())
                    .unwrap();
            expect_decimal(&result, p + 1, 0);
            let variable =
                Db2ArithmeticOperand::resolved(a.resolved_type.clone(), a.span()).unwrap();
            assert_eq!(
                decimal_shape(&variable).0,
                if matches!(a.resolved_type.scalar(), Type::BigInt) {
                    19
                } else {
                    11
                }
            );
        }
        for text in ["9223372036854775808", "000.00", "-15."] {
            let a = constant(text);
            assert_eq!(a.integer_constant_digits(), None);
            assert!(matches!(a.resolved_type.scalar(), Type::Decimal { .. }));
        }
        // A nonzero-independent metadata result keeps zero checks pending.
        let zero = constant("-0");
        let result =
            resolve_db2_binary_arithmetic(Divide, &zero, &zero, context(Dec15, 0), zero.span())
                .unwrap();
        assert_eq!(
            result.obligations().zero,
            Db2ArithmeticZeroCheck::DivisorMustBeNonzero
        );
    }

    #[test]
    fn decimal_formulas_and_context_fixtures() {
        let a = decimal(8, 2);
        let b = decimal(7, 4);
        for mode in [Dec15, Dec31] {
            for op in [Add, Subtract] {
                expect_decimal(&binary(op, &a, &b, mode, 0), 11, 4);
                expect_decimal(&binary(op, &b, &a, mode, 0), 11, 4);
            }
            expect_decimal(&binary(Multiply, &a, &b, mode, 0), 15, 6);
        }
        expect_decimal(
            &binary(Add, &decimal(15, 0), &decimal(15, 0), Dec15, 0),
            15,
            0,
        );
        expect_decimal(
            &binary(Add, &decimal(15, 0), &decimal(15, 0), Dec31, 0),
            16,
            0,
        );
        expect_decimal(
            &binary(Add, &decimal(16, 0), &decimal(1, 0), Dec15, 0),
            17,
            0,
        );
        expect_decimal(
            &binary(Multiply, &decimal(15, 15), &decimal(15, 15), Dec15, 0),
            15,
            15,
        );
        expect_decimal(
            &binary(Multiply, &decimal(15, 15), &decimal(15, 15), Dec31, 0),
            30,
            30,
        );
        expect_decimal(
            &binary(Multiply, &decimal(31, 31), &decimal(31, 31), Dec15, 0),
            31,
            31,
        );
        for mode in [Dec15, Dec31] {
            assert_eq!(context(mode, 0).minimum_divide_scale(), None);
            for minimum in 1..=9 {
                let ctx = context(mode, minimum);
                assert_eq!(ctx.decimal(), mode);
                assert_eq!(ctx.minimum_divide_scale(), Some(minimum));
            }
            assert_eq!(
                Db2ArithmeticContext::new(mode, 10, span())
                    .unwrap_err()
                    .code,
                Db2ArithmeticErrorCode::InvalidContext
            );
        }
    }

    #[test]
    fn multiplication_truncation_selection_ties_and_runtime_checks() {
        for (p, s, q, t, side, copied_p, copied_s, temporary_s, result_s) in [
            (16, 5, 20, 8, Db2ArithmeticSide::Left, 16, 5, 4, 12),
            (20, 8, 16, 5, Db2ArithmeticSide::Right, 16, 5, 4, 12),
            (20, 8, 20, 3, Db2ArithmeticSide::Right, 20, 3, 0, 8),
            (20, 3, 20, 8, Db2ArithmeticSide::Right, 20, 8, 3, 6),
            (31, 0, 31, 0, Db2ArithmeticSide::Right, 31, 0, 0, 0),
            (16, 16, 31, 31, Db2ArithmeticSide::Left, 16, 16, 15, 31),
        ] {
            let result = binary(Multiply, &decimal(p, s), &decimal(q, t), Dec15, 0);
            expect_decimal(&result, 31, result_s);
            let trunc = result.obligations().truncation.unwrap();
            assert_eq!(
                trunc,
                Db2DecimalTruncation {
                    operand: side,
                    original_precision: copied_p,
                    original_scale: copied_s,
                    temporary_precision: 15,
                    temporary_scale: temporary_s,
                    maximum_integral_significant_digits: 15,
                    sqlwarn7_if_nonzero_digits_removed: true
                }
            );
            let check = result.obligations().multiplication.unwrap();
            assert_eq!(
                check.operand,
                if side == Db2ArithmeticSide::Left {
                    Db2ArithmeticSide::Right
                } else {
                    Db2ArithmeticSide::Left
                }
            );
            assert_eq!(check.leading_zeros_must_exceed, 15);
        }
        let result = binary(Multiply, &decimal(26, 0), &decimal(5, 0), Dec31, 0);
        expect_decimal(&result, 31, 0);
        assert!(result.obligations().truncation.is_none());
        assert_eq!(
            result.obligations().multiplication.unwrap(),
            Db2DecimalMultiplyCheck {
                operand: Db2ArithmeticSide::Left,
                leading_zeros_must_exceed: 5
            }
        );
        // The source's overflow example remains an obligation, not success or
        // a value-dependent rejection inferred from declared capacity.
        assert_eq!(
            result.obligations().range,
            Db2ArithmeticRangeCheck::DecimalResult
        );
    }

    #[test]
    fn division_table_all_rows_odd_even_and_negative_minimum() {
        for (p, s, q, t, mode, result_p, table_s, trunc_s) in [
            (8, 2, 7, 4, Dec15, 15, 5, None),
            (8, 2, 7, 4, Dec31, 31, 13, None),
            (8, 2, 8, 4, Dec31, 31, 11, None),
            (16, 2, 7, 4, Dec15, 31, 5, None),
            (16, 2, 8, 4, Dec31, 31, 3, None),
            (8, 2, 16, 5, Dec15, 31, 5, Some(4)),
            (16, 2, 16, 1, Dec31, 31, 1, Some(0)),
            (31, 0, 31, 31, Dec15, 31, -31, Some(15)),
            (31, 0, 15, 15, Dec31, 31, -31, None),
            (15, 0, 15, 0, Dec15, 15, 0, None),
        ] {
            let a = decimal(p, s);
            let b = decimal(q, t);
            for minimum in 0..=9 {
                let result =
                    resolve_db2_binary_arithmetic(Divide, &a, &b, context(mode, minimum), span());
                if table_s < 0 && minimum == 0 {
                    assert_eq!(
                        result.unwrap_err().code,
                        Db2ArithmeticErrorCode::NegativeDivisionScale
                    );
                    continue;
                }
                let result = result.unwrap();
                assert_eq!(result.calculated_division_scale(), Some(table_s));
                expect_decimal(&result, result_p, table_s.max(minimum as i32) as u32);
                assert_eq!(
                    result.obligations().zero,
                    Db2ArithmeticZeroCheck::DivisorMustBeNonzero
                );
                assert_eq!(
                    result
                        .obligations()
                        .truncation
                        .map(|trunc| trunc.temporary_scale),
                    trunc_s
                );
                if let Some(trunc) = result.obligations().truncation {
                    assert_eq!(trunc.operand, Db2ArithmeticSide::Right);
                    assert_eq!(trunc.maximum_integral_significant_digits, 15);
                    assert!(trunc.sqlwarn7_if_nonzero_digits_removed);
                }
            }
        }
        expect_decimal(
            &binary(Divide, &decimal(1, 1), &decimal(1, 0), Dec31, 0),
            31,
            29,
        );
        expect_decimal(
            &binary(Divide, &decimal(1, 1), &decimal(2, 0), Dec31, 0),
            31,
            27,
        );
    }

    #[test]
    fn every_decimal_precision_and_scale_boundary_is_validated_by_owner() {
        // All precisions and every scale; boundary partner precisions avoid a
        // Cartesian release campaign while crossing every formula threshold.
        for p in 1..=31 {
            for s in 0..=p {
                for q in [1, 15, 16, 31] {
                    for t in [0, q] {
                        for mode in [Dec15, Dec31] {
                            let a = decimal(p, s);
                            let b = decimal(q, t);
                            for op in [Add, Subtract, Multiply, Divide] {
                                let result = binary(op, &a, &b, mode, 1);
                                let Type::Decimal { precision, scale } =
                                    result.resolved_type().scalar()
                                else {
                                    panic!("decimal result required")
                                };
                                assert!((1..=31).contains(precision));
                                assert!(scale <= precision);
                                if op != Divide && !(op == Multiply && p == q && p > 15 && s != t) {
                                    // Equal-precision multiplication deliberately
                                    // selects the second operand, so asymmetric
                                    // scales can have asymmetric result metadata.
                                    let reversed = binary(op, &b, &a, mode, 1);
                                    assert_eq!(result.resolved_type(), reversed.resolved_type());
                                }
                            }
                        }
                    }
                }
            }
        }
        for (p, s) in [(0, 0), (32, 0), (15, 16), (31, 32)] {
            assert!(make_type(Kind::Decimal, &[p, s], Db2Nullability::NotNull, span()).is_err());
        }
    }

    #[test]
    fn nullability_and_runtime_obligations_matrix() {
        for (ka, aa, kb, ab, range, division) in [
            (
                Kind::Integer,
                vec![],
                Kind::BigInt,
                vec![],
                Db2ArithmeticRangeCheck::IntegerResult,
                Db2ArithmeticZeroCheck::DivisorMustBeNonzero,
            ),
            (
                Kind::Integer,
                vec![],
                Kind::Decimal,
                vec![8, 2],
                Db2ArithmeticRangeCheck::DecimalResult,
                Db2ArithmeticZeroCheck::DivisorMustBeNonzero,
            ),
            (
                Kind::Real,
                vec![],
                Kind::Decimal,
                vec![8, 2],
                Db2ArithmeticRangeCheck::FloatingPointResult,
                Db2ArithmeticZeroCheck::DivisorMustBeNonzero,
            ),
            (
                Kind::DecFloat,
                vec![16],
                Kind::BigInt,
                vec![],
                Db2ArithmeticRangeCheck::DecimalFloatingPoint,
                Db2ArithmeticZeroCheck::DecimalFloatingPointDivision,
            ),
        ] {
            for na in [false, true] {
                for nb in [false, true] {
                    let a = operand(ka, &aa, na);
                    let b = operand(kb, &ab, nb);
                    for op in [Add, Subtract, Multiply, Divide] {
                        let result = binary(op, &a, &b, Dec31, 1);
                        assert_eq!(
                            result.resolved_type().nullability(),
                            if na || nb {
                                Db2Nullability::Nullable
                            } else {
                                Db2Nullability::NotNull
                            }
                        );
                        assert_eq!(result.obligations().range, range);
                        assert_eq!(
                            result.obligations().zero,
                            if op == Divide {
                                division
                            } else {
                                Db2ArithmeticZeroCheck::None
                            }
                        );
                        assert_eq!(
                            result.obligations().sign,
                            Db2ArithmeticSignRule::BinaryRuntime
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn nonnumeric_operator_and_unpinned_conversion_diagnostics() {
        for (kind, args) in [
            (Kind::Date, vec![]),
            (Kind::Time, vec![]),
            (Kind::Character, vec![1]),
            (Kind::VarChar, vec![1]),
            (Kind::Binary, vec![1]),
        ] {
            let ty = make_type(kind, &args, Db2Nullability::NotNull, span()).unwrap();
            let err = Db2ArithmeticOperand::resolved(ty, span()).unwrap_err();
            assert_eq!(err.code, Db2ArithmeticErrorCode::OutsideNumericSubset);
            assert_eq!(err.span, span());
            assert!(err.message.contains("subset"));
        }
        let a = operand(Kind::Integer, &[], false);
        for op in [
            Db2BinaryOperator::Concatenate,
            Db2BinaryOperator::Equal,
            Db2BinaryOperator::And,
            Db2BinaryOperator::Or,
        ] {
            assert_eq!(
                resolve_db2_binary_arithmetic(op, &a, &a, context(Dec15, 0), span())
                    .unwrap_err()
                    .code,
                Db2ArithmeticErrorCode::UnsupportedOperator
            );
        }
        assert_eq!(
            resolve_db2_unary_arithmetic(Db2UnaryOperator::Not, &a, span())
                .unwrap_err()
                .code,
            Db2ArithmeticErrorCode::UnsupportedOperator
        );
        let df = operand(Kind::DecFloat, &[16], false);
        for other in [
            decimal(1, 0),
            decimal(16, 0),
            decimal(17, 0),
            operand(Kind::Real, &[], false),
            operand(Kind::Double, &[], false),
            operand(Kind::Float, &[53], false),
        ] {
            for op in [Add, Subtract, Multiply, Divide] {
                for (left, right) in [(&df, &other), (&other, &df)] {
                    assert_eq!(
                        resolve_db2_binary_arithmetic(op, left, right, context(Dec31, 0), span())
                            .unwrap_err()
                            .code,
                        Db2ArithmeticErrorCode::UnresolvedDecFloatConversion
                    );
                }
            }
        }
        let diag = error(Db2ArithmeticErrorCode::InvalidContext, span(), "fixed");
        assert!(format!("{diag}").contains("1:1"));
    }

    #[test]
    fn original_utf8_crlf_comment_whitespace_locations_and_owned_results() {
        for prefix in ["", "  ", "-- comment\n", "/* é 😀 */\r\n", "é\r", "😀\n\n"] {
            let source = format!("{prefix}-000001 /* gap */ 25.50");
            let start = prefix.len();
            let second = source.find("25.50").unwrap();
            let spans = [
                located(&source, start, start + 7),
                located(&source, second, second + 5),
            ];
            let operands = classify_db2_arithmetic_constants(
                &source,
                &spans,
                Db2ArithmeticConstantLimits::default(),
            )
            .unwrap();
            assert_eq!(operands[0].integer_constant_digits(), Some(6));
            let outer = located(&source, start, source.len());
            let result = resolve_db2_binary_arithmetic(
                Add,
                &operands[0],
                &operands[1],
                context(Dec31, 0),
                outer,
            )
            .unwrap();
            drop(source);
            assert_eq!(result.span(), outer);
            expect_decimal(&result, 9, 2);
            assert_eq!(operands[1].span(), spans[1]);
        }
        let a = operand(Kind::Integer, &[], false);
        let mut invalid = span();
        invalid.start.line = 0;
        assert_eq!(
            Db2ArithmeticOperand::resolved(a.resolved_type.clone(), invalid)
                .unwrap_err()
                .code,
            Db2ArithmeticErrorCode::InvalidSpan
        );
        invalid = span();
        invalid.end_byte = 0;
        assert!(Db2ArithmeticOperand::resolved(a.resolved_type.clone(), invalid).is_err());
        invalid = span();
        invalid.end.column = 2;
        invalid.end_byte = 2;
        assert!(resolve_db2_unary_arithmetic(Db2UnaryOperator::Positive, &a, invalid).is_ok());
        invalid = span();
        invalid.start.column = 2;
        invalid.end.column = 3;
        assert_eq!(
            resolve_db2_unary_arithmetic(Db2UnaryOperator::Positive, &a, invalid)
                .unwrap_err()
                .code,
            Db2ArithmeticErrorCode::InvalidSpan
        );
    }

    #[test]
    fn malformed_trailing_and_forged_provenance_fail_closed() {
        for text in [
            "",
            "+",
            "--1",
            "1 2",
            "1/*x*/",
            "1;",
            "1.2.3",
            "1+2",
            "１２",
            "1e0",
            "NaN",
            "1,2",
            "000000000000000000001",
            "12345678901234567890123456789012",
        ] {
            let result = classify_db2_arithmetic_constants(
                text,
                &[located(text, 0, text.len())],
                Db2ArithmeticConstantLimits::default(),
            );
            assert!(
                matches!(result, Err(Db2ArithmeticConstantError::Constant(_))),
                "{text:?}"
            );
        }
        let text = "é\r\n-12345";
        let mut invalid = located(text, 4, text.len());
        invalid.start.column += 1;
        let result = classify_db2_arithmetic_constants(
            text,
            &[invalid],
            Db2ArithmeticConstantLimits::default(),
        );
        let Err(Db2ArithmeticConstantError::Constant(err)) = result else {
            panic!("classifier diagnostic expected")
        };
        assert_eq!(err.code, Db2NumericConstantErrorCode::InvalidSourceSpan);
        assert_eq!(err.span, invalid);
        let invalid = Db2SourceSpan {
            start_byte: 1,
            ..located(text, 0, text.len())
        };
        assert!(
            classify_db2_arithmetic_constants(
                text,
                &[invalid],
                Db2ArithmeticConstantLimits::default()
            )
            .is_err()
        );
    }

    #[test]
    fn exact_and_one_beyond_aggregate_and_classifier_bounds() {
        let source = "1";
        let spans = [span(), span()];
        let limits = Db2ArithmeticConstantLimits {
            max_operands: 2,
            max_scan_bytes: 20,
            ..Default::default()
        };
        assert_eq!(
            classify_db2_arithmetic_constants(source, &spans, limits)
                .unwrap()
                .len(),
            2
        );
        let beyond = Db2ArithmeticConstantLimits {
            max_operands: 1,
            ..limits
        };
        assert!(matches!(
            classify_db2_arithmetic_constants(source, &spans, beyond),
            Err(Db2ArithmeticConstantError::Arithmetic(Db2ArithmeticError {
                code: Db2ArithmeticErrorCode::OperandLimit,
                ..
            }))
        ));
        let beyond = Db2ArithmeticConstantLimits {
            max_scan_bytes: 19,
            ..limits
        };
        assert!(matches!(
            classify_db2_arithmetic_constants(source, &spans, beyond),
            Err(Db2ArithmeticConstantError::Arithmetic(Db2ArithmeticError {
                code: Db2ArithmeticErrorCode::ScanLimit,
                ..
            }))
        ));
        for limits in [
            Db2ArithmeticConstantLimits {
                max_operands: 0,
                ..limits
            },
            Db2ArithmeticConstantLimits {
                max_operands: 1025,
                ..limits
            },
            Db2ArithmeticConstantLimits {
                max_scan_bytes: 0,
                ..limits
            },
            Db2ArithmeticConstantLimits {
                max_scan_bytes: 64 * 1024 * 1024 + 1,
                ..limits
            },
        ] {
            assert!(matches!(
                classify_db2_arithmetic_constants(source, &[], limits),
                Err(Db2ArithmeticConstantError::Arithmetic(Db2ArithmeticError {
                    code: Db2ArithmeticErrorCode::InvalidLimits,
                    ..
                }))
            ));
        }
        let mut limits = Db2ArithmeticConstantLimits::default();
        limits.constants.max_source_bytes = 1;
        assert!(classify_db2_arithmetic_constants(source, &[span()], limits).is_ok());
        assert!(matches!(
            classify_db2_arithmetic_constants("11", &[], limits),
            Err(Db2ArithmeticConstantError::Arithmetic(Db2ArithmeticError {
                code: Db2ArithmeticErrorCode::SourceLimit,
                ..
            }))
        ));
        limits.constants.max_source_bytes = 32;
        limits.constants.ast.max_literal_bytes = 2;
        assert!(classify_db2_arithmetic_constants("12", &[located("12", 0, 2)], limits).is_ok());
        assert!(matches!(
            classify_db2_arithmetic_constants("123", &[located("123", 0, 3)], limits),
            Err(Db2ArithmeticConstantError::Constant(
                Db2NumericConstantError {
                    code: Db2NumericConstantErrorCode::ConstantTooLarge,
                    ..
                }
            ))
        ));
        limits.constants.ast.max_literal_bytes = 32;
        limits.constants.ast.max_list_items = 1;
        assert!(classify_db2_arithmetic_constants("1", &[span()], limits).is_ok());
        assert!(matches!(
            classify_db2_arithmetic_constants("1.", &[located("1.", 0, 2)], limits),
            Err(Db2ArithmeticConstantError::Constant(
                Db2NumericConstantError {
                    code: Db2NumericConstantErrorCode::TypeArgumentsLimit,
                    ..
                }
            ))
        ));
    }
}
