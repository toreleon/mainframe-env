//! Pure ordered result-type combination of already validated metadata.
//!
//! Db2 13 baseline ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_rules4resultdatatypes.html, 36969 bytes,
//!   7c23258e9e9f63a5be873ca3ce6f87a2f9b893d4f0aab7d0799b7f02e6bcc206;
//! - db2z_caseexpression.html, 39911 bytes,
//!   fac861b774379be825c697a83d9447f40c125fbda877f716714b0e8ea930cd4a;
//! - db2z_datatypesintro.html, 22904 bytes,
//!   a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570;
//! - db2z_bif_coalesce.html, 12698 bytes,
//!   6f6c80fad7a56d48a50b66fee4c33b6d1ea5d29f4d24ed11fb314a36528941a4.
//!
//! Language elements have no standalone statement-catalog row. This is neither
//! a CASE evaluator nor a function resolver, parser or backend. Callers provide
//! validated types and actual application context, including CASE ELSE presence.
//! No generic nullability override can establish CASE or COALESCE semantics.
//! LOB/XML/ROWID/distinct types cannot enter through the existing resolved-type
//! authority; their combination remains pending, not universally invalid IBM.
//! Character/graphic, datetime strings and FLOAT(n) aliases require further
//! attributes or source-backed alias binding. Storage byte widths alone do not
//! prove FLOAT alias equivalence. No shared lexer/expression fences are changed.

use crate::{
    Db2AssignmentCompatibility, Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType,
    Db2ConversionKind, Db2DataType, Db2Nullability, Db2ResolvedType, Db2ScalarType,
    Db2SourceLocation, Db2SourceSpan, Db2TimeZone, classify_db2_assignment, resolve_db2_type,
};
use std::fmt;

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_OPERANDS: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ResultCombinationLimits {
    /// Entire original UTF-8 source, including text outside the combination.
    pub max_source_bytes: usize,
    /// Aggregate number of typed and explicit untyped NULL entries.
    pub max_operands: usize,
}

impl Default for Db2ResultCombinationLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 1024 * 1024,
            max_operands: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2CaseElse {
    /// The final input entry is the explicit ELSE result (including ELSE NULL).
    Present,
    /// Inputs contain THEN results only; the implicit ELSE NULL adds nullability.
    Omitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ResultCombinationContext {
    /// Non-CASE contexts governed by the ordinary result rules. This does not
    /// establish any function overload or alias identity.
    OperandRules,
    Case {
        else_clause: Db2CaseElse,
    },
    /// Caller has resolved the actual COALESCE application, not just a name.
    Coalesce,
}

/// Borrowed input is bounded before any element traversal or output allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ResultTypeOperand<'a> {
    Typed {
        resolved_type: &'a Db2ResolvedType,
        span: Db2SourceSpan,
    },
    UntypedNull {
        span: Db2SourceSpan,
    },
}

impl Db2ResultTypeOperand<'_> {
    #[must_use]
    pub const fn span(self) -> Db2SourceSpan {
        match self {
            Self::Typed { span, .. } | Self::UntypedNull { span } => span,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ResultCombinationErrorCode {
    InvalidLimits,
    SourceTooLarge,
    TooManyOperands,
    EmptyOperands,
    MissingApplicationContext,
    InvalidApplicationContext,
    InvalidSourceSpan,
    InvalidOperandOrder,
    AllUntypedNull,
    CharacterContextRequired,
    DatetimeStringContextRequired,
    FloatAliasBindingRequired,
    IncompatibleTypes,
    InvalidResolvedType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ResultCombinationError {
    pub code: Db2ResultCombinationErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl fmt::Display for Db2ResultCombinationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2ResultCombinationError {}

fn error(code: Db2ResultCombinationErrorCode, span: Db2SourceSpan) -> Db2ResultCombinationError {
    use Db2ResultCombinationErrorCode as Code;
    let message = match code {
        Code::InvalidLimits => "limits are zero or exceed compiled ceilings",
        Code::SourceTooLarge => "original source exceeds the byte limit",
        Code::TooManyOperands => "aggregate operand count exceeds the limit",
        Code::EmptyOperands => "at least one result operand is required",
        Code::MissingApplicationContext => "actual application context is required for nullability",
        Code::InvalidApplicationContext => {
            "COALESCE or CASE with ELSE requires at least two entries"
        }
        Code::InvalidSourceSpan => "nonempty byte and line/column spans must match original source",
        Code::InvalidOperandOrder => "operand spans must be nonoverlapping and in source order",
        Code::AllUntypedNull => "at least one operand must have a defined type",
        Code::CharacterContextRequired => {
            "character/graphic combination needs CCSID/collation binding"
        }
        Code::DatetimeStringContextRequired => {
            "datetime strings need value/encoding/conversion context"
        }
        Code::FloatAliasBindingRequired => {
            "FLOAT(n) aliases need source-backed REAL/DOUBLE binding"
        }
        Code::IncompatibleTypes => "types are incompatible within the declared result-rule subset",
        Code::InvalidResolvedType => "existing type authority rejected the result shape",
    };
    Db2ResultCombinationError {
        code,
        span,
        message,
    }
}

/// Obligations, not evidence that a runtime conversion succeeded. Numeric
/// assignment must error if the whole part cannot be preserved, even after the
/// metadata precision is capped to 31. Fractional rounding/truncation and
/// timestamp zone conversion still belong to the runtime assignment authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2ResultConversionObligation {
    /// None for an explicit untyped NULL; it acquires no independent scalar type.
    pub conversion: Option<Db2ConversionKind>,
    pub whole_part_must_be_preserved: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CombinedResultOperand {
    pub resolved_type: Option<Db2ResolvedType>,
    pub span: Db2SourceSpan,
    pub obligation: Db2ResultConversionObligation,
}

/// Each typed pair in the ordered fold. Untyped NULLs do not invent pair types.
/// Candidate assignment obligations apply if this intermediate shape is
/// materialized. Runtime operands convert to the final result; this trace does
/// not require intermediate value conversion. Earlier caps remain observable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ResultCombinationStep {
    pub right_operand_index: usize,
    pub result_type: Db2ResolvedType,
    pub left_obligation: Db2ResultConversionObligation,
    pub right_obligation: Db2ResultConversionObligation,
    pub uncapped_decimal_precision: Option<u32>,
    pub required_whole_digits: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2CombinedResultType {
    resolved_type: Db2ResolvedType,
    span: Db2SourceSpan,
    context: Db2ResultCombinationContext,
    operands: Vec<Db2CombinedResultOperand>,
    steps: Vec<Db2ResultCombinationStep>,
}

impl Db2CombinedResultType {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.resolved_type
    }
    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }
    #[must_use]
    pub const fn context(&self) -> Db2ResultCombinationContext {
        self.context
    }
    #[must_use]
    pub fn operands(&self) -> &[Db2CombinedResultOperand] {
        &self.operands
    }
    #[must_use]
    pub fn steps(&self) -> &[Db2ResultCombinationStep] {
        &self.steps
    }
}

/// Fold exactly the supplied source order. Source text verifies locations only:
/// it is never parsed or used to infer application context, types or NULLs.
/// CASE inputs contain THEN entries followed by the ELSE entry when present.
/// Implicit ELSE NULL has no fabricated source span. Expression node/depth/list
/// budgets are owned by the upstream binder; this flat metadata kernel claims
/// only an aggregate operand bound, not an expression/list backend.
pub fn combine_db2_result_types(
    source: &str,
    span: Db2SourceSpan,
    operands: &[Db2ResultTypeOperand<'_>],
    context: Option<Db2ResultCombinationContext>,
    limits: Db2ResultCombinationLimits,
) -> Result<Db2CombinedResultType, Db2ResultCombinationError> {
    use Db2ResultCombinationErrorCode as Code;
    let start = Db2SourceSpan {
        start_byte: 0,
        end_byte: 0,
        start: Db2SourceLocation::START,
        end: Db2SourceLocation::START,
    };
    if limits.max_source_bytes == 0
        || limits.max_source_bytes > MAX_SOURCE_BYTES
        || limits.max_operands == 0
        || limits.max_operands > MAX_OPERANDS
    {
        return Err(error(Code::InvalidLimits, start));
    }
    if source.len() > limits.max_source_bytes {
        return Err(error(Code::SourceTooLarge, start));
    }
    // Inspect only slice length before traversing spans/types or allocating.
    if operands.len() > limits.max_operands {
        return Err(error(Code::TooManyOperands, start));
    }
    if operands.is_empty() {
        return Err(error(Code::EmptyOperands, start));
    }
    let context = context.ok_or_else(|| error(Code::MissingApplicationContext, start))?;
    if operands.len() < 2
        && matches!(
            context,
            Db2ResultCombinationContext::Coalesce
                | Db2ResultCombinationContext::Case {
                    else_clause: Db2CaseElse::Present
                }
        )
    {
        return Err(error(Code::InvalidApplicationContext, start));
    }
    validate_spans(source, span, operands)?;
    let mut steps = Vec::with_capacity(operands.len().saturating_sub(1));
    let mut accumulated: Option<Db2ResolvedType> = None;
    let mut any_nullable = false;
    let mut all_nullable = true;
    for (index, operand) in operands.iter().copied().enumerate() {
        let Db2ResultTypeOperand::Typed {
            resolved_type,
            span: operand_span,
        } = operand
        else {
            any_nullable = true;
            continue;
        };
        let nullable = resolved_type.nullability() == Db2Nullability::Nullable;
        any_nullable |= nullable;
        all_nullable &= nullable;
        let nullability = result_nullability(context, any_nullable, all_nullable);
        if let Some(left) = accumulated {
            let pair = combine_pair(left.scalar(), resolved_type.scalar(), operand_span)?;
            let result_type = construct_type(&pair.scalar, nullability, operand_span)?;
            steps.push(Db2ResultCombinationStep {
                right_operand_index: index,
                left_obligation: conversion_obligation(Some(&left), &result_type, operand_span)?,
                right_obligation: conversion_obligation(
                    Some(resolved_type),
                    &result_type,
                    operand_span,
                )?,
                result_type: result_type.clone(),
                uncapped_decimal_precision: pair.uncapped_decimal_precision,
                required_whole_digits: pair.required_whole_digits,
            });
            accumulated = Some(result_type);
        } else {
            accumulated = Some(resolved_type.clone());
        }
    }
    let accumulated = accumulated.ok_or_else(|| error(Code::AllUntypedNull, span))?;
    validate_single_shape(accumulated.scalar(), span)?;
    let resolved_type = construct_type(
        accumulated.scalar(),
        result_nullability(context, any_nullable, all_nullable),
        span,
    )?;
    let owned_operands = operands
        .iter()
        .copied()
        .map(|operand| {
            let input = match operand {
                Db2ResultTypeOperand::Typed { resolved_type, .. } => Some(resolved_type),
                Db2ResultTypeOperand::UntypedNull { .. } => None,
            };
            Ok(Db2CombinedResultOperand {
                resolved_type: input.cloned(),
                span: operand.span(),
                obligation: conversion_obligation(input, &resolved_type, operand.span())?,
            })
        })
        .collect::<Result<Vec<_>, Db2ResultCombinationError>>()?;
    Ok(Db2CombinedResultType {
        resolved_type,
        span,
        context,
        operands: owned_operands,
        steps,
    })
}

fn result_nullability(
    context: Db2ResultCombinationContext,
    any: bool,
    all: bool,
) -> Db2Nullability {
    let nullable = match context {
        Db2ResultCombinationContext::Coalesce => all,
        Db2ResultCombinationContext::Case {
            else_clause: Db2CaseElse::Omitted,
        } => true,
        _ => any,
    };
    if nullable {
        Db2Nullability::Nullable
    } else {
        Db2Nullability::NotNull
    }
}

fn conversion_obligation(
    input: Option<&Db2ResolvedType>,
    result: &Db2ResolvedType,
    span: Db2SourceSpan,
) -> Result<Db2ResultConversionObligation, Db2ResultCombinationError> {
    let conversion = match input {
        Some(input) => match classify_db2_assignment(input, result) {
            Db2AssignmentCompatibility::Compatible { conversion, .. } => Some(conversion),
            _ => {
                return Err(error(
                    Db2ResultCombinationErrorCode::InvalidResolvedType,
                    span,
                ));
            }
        },
        None => None,
    };
    Ok(Db2ResultConversionObligation {
        whole_part_must_be_preserved: conversion == Some(Db2ConversionKind::Numeric),
        conversion,
    })
}

struct SourceCursor<'a> {
    source: &'a str,
    offset: usize,
    location: Db2SourceLocation,
    previous_cr: bool,
}

impl SourceCursor<'_> {
    fn advance(&mut self, end: usize) -> Db2SourceLocation {
        for character in self.source[self.offset..end].chars() {
            if character == '\r' || (character == '\n' && !self.previous_cr) {
                self.location.line += 1;
                self.location.column = 1;
            } else if character != '\n' || !self.previous_cr {
                self.location.column += 1;
            }
            self.previous_cr = character == '\r';
        }
        self.offset = end;
        self.location
    }
}

fn validate_spans(
    source: &str,
    span: Db2SourceSpan,
    operands: &[Db2ResultTypeOperand<'_>],
) -> Result<(), Db2ResultCombinationError> {
    use Db2ResultCombinationErrorCode as Code;
    let valid_bytes = |s: Db2SourceSpan| {
        s.start_byte < s.end_byte && source.get(s.start_byte..s.end_byte).is_some()
    };
    if !valid_bytes(span) {
        return Err(error(Code::InvalidSourceSpan, span));
    }
    let mut cursor = SourceCursor {
        source,
        offset: 0,
        location: Db2SourceLocation::START,
        previous_cr: false,
    };
    if cursor.advance(span.start_byte) != span.start {
        return Err(error(Code::InvalidSourceSpan, span));
    }
    for operand in operands.iter().copied() {
        let s = operand.span();
        if !valid_bytes(s) || s.start_byte < span.start_byte || s.end_byte > span.end_byte {
            return Err(error(Code::InvalidSourceSpan, s));
        }
        if s.start_byte < cursor.offset {
            return Err(error(Code::InvalidOperandOrder, s));
        }
        if cursor.advance(s.start_byte) != s.start || cursor.advance(s.end_byte) != s.end {
            return Err(error(Code::InvalidSourceSpan, s));
        }
    }
    if cursor.advance(span.end_byte) != span.end {
        return Err(error(Code::InvalidSourceSpan, span));
    }
    Ok(())
}

struct PairResult {
    scalar: Db2ScalarType,
    uncapped_decimal_precision: Option<u32>,
    required_whole_digits: Option<u32>,
}

fn character(scalar: &Db2ScalarType) -> bool {
    matches!(
        scalar,
        Db2ScalarType::Character { .. }
            | Db2ScalarType::VarChar { .. }
            | Db2ScalarType::Graphic { .. }
            | Db2ScalarType::VarGraphic { .. }
    )
}

fn datetime(scalar: &Db2ScalarType) -> bool {
    matches!(
        scalar,
        Db2ScalarType::Date | Db2ScalarType::Time | Db2ScalarType::Timestamp { .. }
    )
}

fn validate_single_shape(
    scalar: &Db2ScalarType,
    span: Db2SourceSpan,
) -> Result<(), Db2ResultCombinationError> {
    use Db2ResultCombinationErrorCode as Code;
    if character(scalar) {
        return Err(error(Code::CharacterContextRequired, span));
    }
    if matches!(scalar, Db2ScalarType::Float { .. }) {
        return Err(error(Code::FloatAliasBindingRequired, span));
    }
    Ok(())
}

fn integral(scalar: &Db2ScalarType) -> Option<(u32, u32)> {
    match scalar {
        Db2ScalarType::SmallInt => Some((0, 5)),
        Db2ScalarType::Integer => Some((1, 11)),
        Db2ScalarType::BigInt => Some((2, 19)),
        _ => None,
    }
}

fn fixed_decimal(scalar: &Db2ScalarType) -> Option<(u32, u32)> {
    if let Db2ScalarType::Decimal { precision, scale } = scalar {
        Some((*precision, *scale))
    } else {
        integral(scalar).map(|(_, digits)| (digits, 0))
    }
}

fn numeric(scalar: &Db2ScalarType) -> bool {
    fixed_decimal(scalar).is_some()
        || matches!(
            scalar,
            Db2ScalarType::Real | Db2ScalarType::Double | Db2ScalarType::DecFloat { .. }
        )
}

fn combine_pair(
    left: &Db2ScalarType,
    right: &Db2ScalarType,
    span: Db2SourceSpan,
) -> Result<PairResult, Db2ResultCombinationError> {
    use Db2ResultCombinationErrorCode as Code;
    use Db2ScalarType as S;
    if (character(left) && datetime(right)) || (datetime(left) && character(right)) {
        return Err(error(Code::DatetimeStringContextRequired, span));
    }
    validate_single_shape(left, span)?;
    validate_single_shape(right, span)?;
    let mut result = PairResult {
        scalar: left.clone(),
        uncapped_decimal_precision: None,
        required_whole_digits: None,
    };
    result.scalar = match (left, right) {
        (S::DecFloat { precision }, other) | (other, S::DecFloat { precision })
            if numeric(other) =>
        {
            let needed = match other {
                S::BigInt
                | S::Decimal {
                    precision: 17.., ..
                } => 34,
                S::DecFloat { precision } => *precision,
                _ => 16,
            };
            S::DecFloat {
                precision: (*precision).max(needed),
            }
        }
        (S::Double, other) | (other, S::Double) if numeric(other) => S::Double,
        (S::Real, S::Real) => S::Real,
        (S::Real, other) | (other, S::Real) if numeric(other) => S::Double,
        (a, b) if integral(a).is_some() && integral(b).is_some() => {
            match integral(a).unwrap().0.max(integral(b).unwrap().0) {
                0 => S::SmallInt,
                1 => S::Integer,
                _ => S::BigInt,
            }
        }
        (a, b) if fixed_decimal(a).is_some() && fixed_decimal(b).is_some() => {
            let (p1, s1) = fixed_decimal(a).unwrap();
            let (p2, s2) = fixed_decimal(b).unwrap();
            let scale = s1.max(s2);
            let whole = (p1 - s1).max(p2 - s2);
            result.uncapped_decimal_precision = Some(scale + whole);
            result.required_whole_digits = Some(whole);
            S::Decimal {
                precision: (scale + whole).min(31),
                scale,
            }
        }
        (S::Binary { length: a }, S::Binary { length: b }) => S::Binary {
            length: (*a).max(*b),
        },
        (S::VarBinary { length: a }, S::Binary { length: b } | S::VarBinary { length: b })
        | (S::Binary { length: a }, S::VarBinary { length: b }) => S::VarBinary {
            length: (*a).max(*b),
        },
        (S::Date, S::Date) => S::Date,
        (S::Time, S::Time) => S::Time,
        (
            S::Timestamp {
                precision: a,
                time_zone: za,
            },
            S::Timestamp {
                precision: b,
                time_zone: zb,
            },
        ) => S::Timestamp {
            precision: (*a).max(*b),
            time_zone: if *za == Db2TimeZone::WithTimeZone || *zb == Db2TimeZone::WithTimeZone {
                Db2TimeZone::WithTimeZone
            } else {
                Db2TimeZone::WithoutTimeZone
            },
        },
        _ => return Err(error(Code::IncompatibleTypes, span)),
    };
    Ok(result)
}

/// Only the existing validated syntax/type authority constructs resolved types.
fn construct_type(
    scalar: &Db2ScalarType,
    nullability: Db2Nullability,
    span: Db2SourceSpan,
) -> Result<Db2ResolvedType, Db2ResultCombinationError> {
    use Db2BuiltInType as B;
    use Db2ScalarType as S;
    let (kind, arguments, zone) = match scalar {
        S::SmallInt => (B::SmallInt, vec![], false),
        S::Integer => (B::Integer, vec![], false),
        S::BigInt => (B::BigInt, vec![], false),
        S::Decimal { precision, scale } => (B::Decimal, vec![*precision, *scale], false),
        S::Real => (B::Real, vec![], false),
        S::Double => (B::Double, vec![], false),
        S::DecFloat { precision } => (B::DecFloat, vec![*precision], false),
        S::Binary { length } => (B::Binary, vec![*length], false),
        S::VarBinary { length } => (B::VarBinary, vec![*length], false),
        S::Date => (B::Date, vec![], false),
        S::Time => (B::Time, vec![], false),
        S::Timestamp {
            precision,
            time_zone,
        } => (
            B::Timestamp,
            vec![*precision],
            *time_zone == Db2TimeZone::WithTimeZone,
        ),
        _ => {
            return Err(error(
                Db2ResultCombinationErrorCode::InvalidResolvedType,
                span,
            ));
        }
    };
    let syntax = Db2BuiltInDataType::new(kind, arguments, zone, Db2AstLimits::default())
        .map_err(|_| error(Db2ResultCombinationErrorCode::InvalidResolvedType, span))?;
    resolve_db2_type(&Db2DataType::BuiltIn(syntax), nullability)
        .map_err(|_| error(Db2ResultCombinationErrorCode::InvalidResolvedType, span))
}

#[cfg(test)]
mod tests {
    use super::*;
    use Db2ResultCombinationContext as Context;
    use Db2ResultCombinationErrorCode as Code;
    use Db2ScalarType as S;
    use Db2TimeZone::{WithTimeZone as With, WithoutTimeZone as Without};

    fn location(source: &str, byte: usize) -> Db2SourceLocation {
        SourceCursor {
            source,
            offset: 0,
            location: Db2SourceLocation::START,
            previous_cr: false,
        }
        .advance(byte)
    }

    fn span(source: &str, start: usize, end: usize) -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: start,
            end_byte: end,
            start: location(source, start),
            end: location(source, end),
        }
    }

    fn decimal(precision: u32, scale: u32) -> S {
        S::Decimal { precision, scale }
    }
    fn decfloat(precision: u32) -> S {
        S::DecFloat { precision }
    }
    fn timestamp(precision: u32, time_zone: Db2TimeZone) -> S {
        S::Timestamp {
            precision,
            time_zone,
        }
    }

    fn resolved(scalar: &S, nullability: Db2Nullability) -> Db2ResolvedType {
        construct_type(scalar, nullability, span("x", 0, 1)).unwrap()
    }

    fn syntax_type(
        kind: Db2BuiltInType,
        arguments: Vec<u32>,
        zone: bool,
        nullability: Db2Nullability,
    ) -> Result<Db2ResolvedType, crate::Db2TypeError> {
        resolve_db2_type(
            &Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(kind, arguments, zone, Db2AstLimits::default()).unwrap(),
            ),
            nullability,
        )
    }

    fn run(
        types: &[Option<Db2ResolvedType>],
        context: Option<Context>,
    ) -> Result<Db2CombinedResultType, Db2ResultCombinationError> {
        let source = vec!["x"; types.len()].join(",");
        let operands = types
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let span = span(&source, index * 2, index * 2 + 1);
                match ty {
                    Some(resolved_type) => Db2ResultTypeOperand::Typed {
                        resolved_type,
                        span,
                    },
                    None => Db2ResultTypeOperand::UntypedNull { span },
                }
            })
            .collect::<Vec<_>>();
        combine_db2_result_types(
            &source,
            span(&source, 0, source.len()),
            &operands,
            context,
            Db2ResultCombinationLimits::default(),
        )
    }

    fn fold(shapes: &[S]) -> Db2CombinedResultType {
        let types = shapes
            .iter()
            .map(|s| Some(resolved(s, Db2Nullability::NotNull)))
            .collect::<Vec<_>>();
        run(&types, Some(Context::OperandRules)).unwrap()
    }

    #[test]
    fn every_numeric_table_pair_in_both_directions() {
        // Independent explicit expectations from result-rules Table 1. INTEGER
        // requires 11 whole digits here, not its constant classifier's 10.
        let d12 = decimal(12, 2);
        let d17 = decimal(17, 4);
        let f16 = decfloat(16);
        let f34 = decfloat(34);
        let shapes = [
            S::SmallInt,
            S::Integer,
            S::BigInt,
            d12.clone(),
            d17.clone(),
            S::Real,
            S::Double,
            f16.clone(),
            f34.clone(),
        ];
        let expected = [
            [
                S::SmallInt,
                S::Integer,
                S::BigInt,
                d12.clone(),
                d17.clone(),
                S::Double,
                S::Double,
                f16.clone(),
                f34.clone(),
            ],
            [
                S::Integer,
                S::Integer,
                S::BigInt,
                decimal(13, 2),
                d17.clone(),
                S::Double,
                S::Double,
                f16.clone(),
                f34.clone(),
            ],
            [
                S::BigInt,
                S::BigInt,
                S::BigInt,
                decimal(21, 2),
                decimal(23, 4),
                S::Double,
                S::Double,
                f34.clone(),
                f34.clone(),
            ],
            [
                d12.clone(),
                decimal(13, 2),
                decimal(21, 2),
                d12.clone(),
                d17.clone(),
                S::Double,
                S::Double,
                f16.clone(),
                f34.clone(),
            ],
            [
                d17.clone(),
                d17.clone(),
                decimal(23, 4),
                d17.clone(),
                d17.clone(),
                S::Double,
                S::Double,
                f34.clone(),
                f34.clone(),
            ],
            [
                S::Double,
                S::Double,
                S::Double,
                S::Double,
                S::Double,
                S::Real,
                S::Double,
                f16.clone(),
                f34.clone(),
            ],
            [
                S::Double,
                S::Double,
                S::Double,
                S::Double,
                S::Double,
                S::Double,
                S::Double,
                f16.clone(),
                f34.clone(),
            ],
            [
                f16.clone(),
                f16.clone(),
                f34.clone(),
                f16.clone(),
                f34.clone(),
                f16.clone(),
                f16.clone(),
                f16.clone(),
                f34.clone(),
            ],
            [
                f34.clone(),
                f34.clone(),
                f34.clone(),
                f34.clone(),
                f34.clone(),
                f34.clone(),
                f34.clone(),
                f34.clone(),
                f34.clone(),
            ],
        ];
        for (i, left) in shapes.iter().enumerate() {
            for (j, right) in shapes.iter().enumerate() {
                let result = fold(&[left.clone(), right.clone()]);
                assert_eq!(
                    result.resolved_type().scalar(),
                    &expected[i][j],
                    "pair {i},{j}"
                );
                assert_eq!(
                    result.resolved_type().nullability(),
                    Db2Nullability::NotNull
                );
                assert_eq!(result.steps().len(), 1);
                assert_eq!(result.operands().len(), 2);
            }
        }
    }

    #[test]
    fn decimal_boundaries_and_whole_part_obligations() {
        for (shapes, expected, raw, whole) in [
            ([decimal(1, 1), S::SmallInt], decimal(6, 1), 6, 5),
            ([decimal(1, 1), S::Integer], decimal(12, 1), 12, 11),
            ([decimal(1, 1), S::BigInt], decimal(20, 1), 20, 19),
            ([decimal(30, 30), S::SmallInt], decimal(31, 30), 35, 5),
            ([decimal(31, 31), S::BigInt], decimal(31, 31), 50, 19),
            ([decimal(31, 0), decimal(31, 31)], decimal(31, 31), 62, 31),
            ([decimal(16, 0), decimal(15, 15)], decimal(31, 15), 31, 16),
            ([decimal(16, 0), decimal(16, 16)], decimal(31, 16), 32, 16),
        ] {
            let result = fold(&shapes);
            assert_eq!(result.resolved_type().scalar(), &expected);
            let step = &result.steps()[0];
            assert_eq!(step.uncapped_decimal_precision, Some(raw));
            assert_eq!(step.required_whole_digits, Some(whole));
            assert!(
                step.left_obligation.whole_part_must_be_preserved
                    || step.right_obligation.whole_part_must_be_preserved
            );
        }
        let result = fold(&[decimal(31, 31), S::BigInt, S::Double]);
        assert_eq!(result.resolved_type().scalar(), &S::Double);
        assert_eq!(result.steps()[0].uncapped_decimal_precision, Some(50));
        assert!(
            result.steps()[0]
                .right_obligation
                .whole_part_must_be_preserved
        );
        assert_eq!(result.steps()[1].uncapped_decimal_precision, None);
    }

    #[test]
    fn ordered_fold_is_not_sorted_or_reassociated() {
        let a = fold(&[decfloat(16), decimal(16, 0), decimal(16, 16)]);
        let b = fold(&[decimal(16, 0), decimal(16, 16), decfloat(16)]);
        assert_eq!(a.resolved_type().scalar(), &decfloat(16));
        assert_eq!(b.resolved_type().scalar(), &decfloat(34));
        assert_eq!(a.steps()[0].right_operand_index, 1);
        assert_eq!(a.steps()[1].right_operand_index, 2);
        assert_eq!(b.steps()[0].uncapped_decimal_precision, Some(32));
        // DECFLOAT promotion is based on operand precision, not whole digits.
        for p in [1, 16, 17, 31] {
            let expected = decfloat(if p <= 16 { 16 } else { 34 });
            assert_eq!(
                fold(&[decimal(p, p), decfloat(16)])
                    .resolved_type()
                    .scalar(),
                &expected
            );
        }
    }

    #[test]
    fn binary_lengths_and_fixed_varying_promotions() {
        let shapes = [
            S::Binary { length: 1 },
            S::Binary { length: 255 },
            S::VarBinary { length: 1 },
            S::VarBinary { length: 32_704 },
        ];
        let expected = [
            [
                S::Binary { length: 1 },
                S::Binary { length: 255 },
                S::VarBinary { length: 1 },
                S::VarBinary { length: 32_704 },
            ],
            [
                S::Binary { length: 255 },
                S::Binary { length: 255 },
                S::VarBinary { length: 255 },
                S::VarBinary { length: 32_704 },
            ],
            [
                S::VarBinary { length: 1 },
                S::VarBinary { length: 255 },
                S::VarBinary { length: 1 },
                S::VarBinary { length: 32_704 },
            ],
            [
                S::VarBinary { length: 32_704 },
                S::VarBinary { length: 32_704 },
                S::VarBinary { length: 32_704 },
                S::VarBinary { length: 32_704 },
            ],
        ];
        for (i, left) in shapes.iter().enumerate() {
            for (j, right) in shapes.iter().enumerate() {
                assert_eq!(
                    fold(&[left.clone(), right.clone()])
                        .resolved_type()
                        .scalar(),
                    &expected[i][j]
                );
            }
        }
        let result = fold(&[shapes[0].clone(), shapes[1].clone(), shapes[3].clone()]);
        assert_eq!(
            result.steps()[0].right_obligation.conversion,
            Some(Db2ConversionKind::Identity)
        );
        assert_eq!(
            result.operands()[0].obligation.conversion,
            Some(Db2ConversionKind::BinaryString)
        );
        assert!(!result.operands()[0].obligation.whole_part_must_be_preserved);
    }

    #[test]
    fn date_time_timestamp_precision_and_zone_matrix() {
        assert_eq!(fold(&[S::Date, S::Date]).resolved_type().scalar(), &S::Date);
        assert_eq!(fold(&[S::Time, S::Time]).resolved_type().scalar(), &S::Time);
        for a in [0, 6, 12] {
            for b in [0, 6, 12] {
                for za in [Without, With] {
                    for zb in [Without, With] {
                        let result = fold(&[timestamp(a, za), timestamp(b, zb)]);
                        let zone = if za == With || zb == With {
                            With
                        } else {
                            Without
                        };
                        assert_eq!(result.resolved_type().scalar(), &timestamp(a.max(b), zone));
                    }
                }
            }
        }
        let result = fold(&[
            timestamp(0, Without),
            timestamp(6, With),
            timestamp(12, Without),
        ]);
        assert_eq!(result.resolved_type().scalar(), &timestamp(12, With));
        assert_eq!(
            result.operands()[0].obligation.conversion,
            Some(Db2ConversionKind::Timestamp)
        );
    }

    #[test]
    fn explicit_context_nullability_matrix() {
        let not_null = resolved(&S::Integer, Db2Nullability::NotNull);
        let nullable = resolved(&S::Integer, Db2Nullability::Nullable);
        for (types, ordinary, coalesce) in [
            (
                vec![Some(not_null.clone()), Some(not_null.clone())],
                Db2Nullability::NotNull,
                Db2Nullability::NotNull,
            ),
            (
                vec![Some(nullable.clone()), Some(not_null.clone())],
                Db2Nullability::Nullable,
                Db2Nullability::NotNull,
            ),
            (
                vec![Some(not_null.clone()), Some(nullable.clone())],
                Db2Nullability::Nullable,
                Db2Nullability::NotNull,
            ),
            (
                vec![Some(nullable.clone()), Some(nullable.clone())],
                Db2Nullability::Nullable,
                Db2Nullability::Nullable,
            ),
            (
                vec![None, Some(not_null.clone())],
                Db2Nullability::Nullable,
                Db2Nullability::NotNull,
            ),
            (
                vec![Some(not_null.clone()), None],
                Db2Nullability::Nullable,
                Db2Nullability::NotNull,
            ),
            (
                vec![None, Some(nullable.clone()), None],
                Db2Nullability::Nullable,
                Db2Nullability::Nullable,
            ),
        ] {
            for context in [
                Context::OperandRules,
                Context::Case {
                    else_clause: Db2CaseElse::Present,
                },
            ] {
                assert_eq!(
                    run(&types, Some(context))
                        .unwrap()
                        .resolved_type()
                        .nullability(),
                    ordinary
                );
            }
            assert_eq!(
                run(&types, Some(Context::Coalesce))
                    .unwrap()
                    .resolved_type()
                    .nullability(),
                coalesce
            );
            assert_eq!(
                run(
                    &types,
                    Some(Context::Case {
                        else_clause: Db2CaseElse::Omitted
                    })
                )
                .unwrap()
                .resolved_type()
                .nullability(),
                Db2Nullability::Nullable
            );
            assert_eq!(
                run(&types, None).unwrap_err().code,
                Code::MissingApplicationContext
            );
        }
        let single = [Some(not_null)];
        let result = run(
            &single,
            Some(Context::Case {
                else_clause: Db2CaseElse::Omitted,
            }),
        )
        .unwrap();
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::Nullable
        );
        assert_eq!(result.operands().len(), 1); // No fabricated ELSE location.
        for context in [
            Context::Coalesce,
            Context::Case {
                else_clause: Db2CaseElse::Present,
            },
        ] {
            assert_eq!(
                run(&single, Some(context)).unwrap_err().code,
                Code::InvalidApplicationContext
            );
        }
    }

    #[test]
    fn untyped_nulls_keep_positions_without_a_scalar_type() {
        let types = [
            None,
            Some(resolved(&S::SmallInt, Db2Nullability::NotNull)),
            None,
            Some(resolved(&S::BigInt, Db2Nullability::NotNull)),
            None,
        ];
        let result = run(
            &types,
            Some(Context::Case {
                else_clause: Db2CaseElse::Present,
            }),
        )
        .unwrap();
        assert_eq!(result.resolved_type().scalar(), &S::BigInt);
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::Nullable
        );
        assert_eq!(result.steps().len(), 1);
        assert_eq!(result.steps()[0].right_operand_index, 3);
        for i in [0, 2, 4] {
            assert_eq!(result.operands()[i].resolved_type, None);
            assert_eq!(result.operands()[i].obligation.conversion, None);
            assert!(!result.operands()[i].obligation.whole_part_must_be_preserved);
        }
        for context in [
            Context::OperandRules,
            Context::Coalesce,
            Context::Case {
                else_clause: Db2CaseElse::Present,
            },
            Context::Case {
                else_clause: Db2CaseElse::Omitted,
            },
        ] {
            assert_eq!(
                run(&[None, None], Some(context)).unwrap_err().code,
                Code::AllUntypedNull
            );
        }
        assert_eq!(
            run(&[], Some(Context::OperandRules)).unwrap_err().code,
            Code::EmptyOperands
        );
    }

    #[test]
    fn incompatible_and_pending_shapes_are_located() {
        for (left, right) in [
            (S::SmallInt, S::Date),
            (S::Date, S::Time),
            (S::Date, timestamp(6, Without)),
            (S::Time, timestamp(6, With)),
            (S::Binary { length: 1 }, S::Integer),
            (S::VarBinary { length: 1 }, S::Date),
        ] {
            for shapes in [[left.clone(), right.clone()], [right.clone(), left.clone()]] {
                let types = shapes
                    .iter()
                    .map(|s| Some(resolved(s, Db2Nullability::NotNull)))
                    .collect::<Vec<_>>();
                let failure = run(&types, Some(Context::OperandRules)).unwrap_err();
                assert_eq!(failure.code, Code::IncompatibleTypes);
                assert_eq!(failure.span, span("x,x", 2, 3));
            }
        }
        for kind in [
            Db2BuiltInType::Character,
            Db2BuiltInType::VarChar,
            Db2BuiltInType::Graphic,
            Db2BuiltInType::VarGraphic,
        ] {
            let string = syntax_type(kind, vec![1], false, Db2Nullability::NotNull).unwrap();
            assert_eq!(
                run(&[Some(string.clone())], Some(Context::OperandRules))
                    .unwrap_err()
                    .code,
                Code::CharacterContextRequired
            );
            for context in [
                Context::OperandRules,
                Context::Coalesce,
                Context::Case {
                    else_clause: Db2CaseElse::Present,
                },
            ] {
                let date = resolved(&S::Date, Db2Nullability::NotNull);
                for types in [
                    [Some(string.clone()), Some(date.clone())],
                    [Some(date.clone()), Some(string.clone())],
                ] {
                    assert_eq!(
                        run(&types, Some(context)).unwrap_err().code,
                        Code::DatetimeStringContextRequired
                    );
                }
                assert_eq!(
                    run(&[Some(string.clone()), Some(string.clone())], Some(context))
                        .unwrap_err()
                        .code,
                    Code::CharacterContextRequired
                );
            }
        }
        for precision in [1, 21, 22, 53] {
            let alias = syntax_type(
                Db2BuiltInType::Float,
                vec![precision],
                false,
                Db2Nullability::NotNull,
            )
            .unwrap();
            assert_eq!(
                run(&[Some(alias.clone())], Some(Context::OperandRules))
                    .unwrap_err()
                    .code,
                Code::FloatAliasBindingRequired
            );
            let integer = resolved(&S::Integer, Db2Nullability::NotNull);
            for types in [
                [Some(alias.clone()), Some(integer.clone())],
                [Some(integer), Some(alias)],
            ] {
                assert_eq!(
                    run(&types, Some(Context::OperandRules)).unwrap_err().code,
                    Code::FloatAliasBindingRequired
                );
            }
        }
    }

    #[test]
    fn upstream_type_authority_retains_negative_families_and_bounds() {
        use crate::Db2TypeErrorCode as T;
        use Db2BuiltInType as B;
        for (kind, arguments, zone, code) in [
            (B::Decimal, vec![0], false, T::InvalidPrecision),
            (B::Decimal, vec![32], false, T::InvalidPrecision),
            (B::Decimal, vec![1, 2], false, T::InvalidScale),
            (B::Binary, vec![0], false, T::InvalidLength),
            (B::Binary, vec![256], false, T::InvalidLength),
            (B::VarBinary, vec![32_705], false, T::InvalidLength),
            (B::Timestamp, vec![13], false, T::InvalidPrecision),
            (B::Date, vec![], true, T::InvalidTimeZone),
            (B::DecFloat, vec![17], false, T::InvalidPrecision),
            (B::Real, vec![1], false, T::InvalidArguments),
            (B::Blob, vec![1], false, T::UnsupportedLob),
            (B::Clob, vec![1], false, T::UnsupportedLob),
            (B::DbClob, vec![1], false, T::UnsupportedLob),
            (B::Xml, vec![], false, T::UnsupportedXml),
            (B::RowId, vec![], false, T::UnsupportedRowId),
        ] {
            assert_eq!(
                syntax_type(kind, arguments, zone, Db2Nullability::NotNull)
                    .unwrap_err()
                    .code,
                code
            );
        }
        let name = crate::Db2QualifiedName::new(
            vec![crate::Db2Identifier::new("a", false, Db2AstLimits::default()).unwrap()],
            Db2AstLimits::default(),
        )
        .unwrap();
        assert_eq!(
            resolve_db2_type(&Db2DataType::Distinct(name), Db2Nullability::NotNull)
                .unwrap_err()
                .code,
            T::UnsupportedDistinct
        );
    }

    #[test]
    fn relocation_comments_whitespace_and_owned_output() {
        for prefix in ["", " ", "-- hé\n", "/* é */\r\n\t", "\r", "\n", "\r\n"] {
            let result = {
                let source =
                    format!("{prefix}CASE WHEN p THEN a /* trivia */ ELSE b END ; trailing");
                let first = prefix.len() + "CASE WHEN p THEN ".len();
                let second = prefix.len() + "CASE WHEN p THEN a /* trivia */ ELSE ".len();
                let end = second + "b END".len();
                let small = resolved(&S::SmallInt, Db2Nullability::NotNull);
                let big = resolved(&S::BigInt, Db2Nullability::Nullable);
                let operands = [
                    Db2ResultTypeOperand::Typed {
                        resolved_type: &small,
                        span: span(&source, first, first + 1),
                    },
                    Db2ResultTypeOperand::Typed {
                        resolved_type: &big,
                        span: span(&source, second, second + 1),
                    },
                ];
                let context = Context::Case {
                    else_clause: Db2CaseElse::Present,
                };
                combine_db2_result_types(
                    &source,
                    span(&source, prefix.len(), end),
                    &operands,
                    Some(context),
                    Db2ResultCombinationLimits::default(),
                )
                .unwrap()
            }; // Original source, context and resolved inputs have been dropped.
            assert_eq!(result.resolved_type().scalar(), &S::BigInt);
            assert_eq!(
                result.resolved_type().nullability(),
                Db2Nullability::Nullable
            );
            assert_eq!(
                result.context(),
                Context::Case {
                    else_clause: Db2CaseElse::Present
                }
            );
            assert_eq!(result.span().start_byte, prefix.len());
            assert_eq!(result.operands()[0].span.start_byte, prefix.len() + 17);
            assert_eq!(
                result.operands()[0]
                    .resolved_type
                    .as_ref()
                    .unwrap()
                    .scalar(),
                &S::SmallInt
            );
            assert_eq!(result.steps()[0].result_type.scalar(), &S::BigInt);
        }
        // Metadata typing never parses or repairs malformed/trailing SQL text.
        let ty = resolved(&S::Integer, Db2Nullability::NotNull);
        for source in [
            "garbage; ELSE END ???",
            "CASE WHEN /* not SQL */",
            "  \t\r\n",
        ] {
            let operand = Db2ResultTypeOperand::Typed {
                resolved_type: &ty,
                span: span(source, 0, source.len()),
            };
            assert!(
                combine_db2_result_types(
                    source,
                    span(source, 0, source.len()),
                    &[operand],
                    Some(Context::OperandRules),
                    Db2ResultCombinationLimits::default()
                )
                .is_ok()
            );
        }
    }

    #[test]
    fn malformed_spans_and_source_order_fail() {
        let source = "é\r\na,b";
        let ty = resolved(&S::Integer, Db2Nullability::NotNull);
        let envelope = span(source, 4, 7);
        let a = span(source, 4, 5);
        let b = span(source, 6, 7);
        let operand = |span| Db2ResultTypeOperand::Typed {
            resolved_type: &ty,
            span,
        };
        let call = |envelope, operands: &[Db2ResultTypeOperand<'_>]| {
            combine_db2_result_types(
                source,
                envelope,
                operands,
                Some(Context::OperandRules),
                Db2ResultCombinationLimits::default(),
            )
        };
        for bad in [
            Db2SourceSpan { end_byte: 8, ..b },
            Db2SourceSpan { start_byte: 5, ..a },
            Db2SourceSpan { start_byte: 6, ..a },
            Db2SourceSpan {
                start: Db2SourceLocation::START,
                ..a
            },
            Db2SourceSpan {
                end: Db2SourceLocation::START,
                ..a
            },
            span(source, 0, 2),
            Db2SourceSpan { start_byte: 1, ..a },
        ] {
            let failure = call(envelope, &[operand(bad)]).unwrap_err();
            assert_eq!(failure.code, Code::InvalidSourceSpan);
            assert_eq!(failure.span, bad);
        }
        for bad in [
            Db2SourceSpan {
                end_byte: source.len() + 1,
                ..envelope
            },
            Db2SourceSpan {
                start_byte: 1,
                ..envelope
            },
            Db2SourceSpan {
                end: Db2SourceLocation::START,
                ..envelope
            },
        ] {
            assert_eq!(
                call(bad, &[operand(a)]).unwrap_err().code,
                Code::InvalidSourceSpan
            );
        }
        for operands in [[operand(b), operand(a)], [operand(a), operand(a)]] {
            assert_eq!(
                call(envelope, &operands).unwrap_err().code,
                Code::InvalidOperandOrder
            );
        }
        assert_eq!(
            call(envelope, &[operand(a), operand(b)])
                .unwrap()
                .operands()[1]
                .span
                .start,
            Db2SourceLocation { line: 2, column: 3 }
        );
    }

    #[test]
    fn limits_precede_traversal_and_bound_all_entries() {
        let ty = resolved(&S::Integer, Db2Nullability::NotNull);
        let source = "x,x,x";
        let valid = Db2ResultTypeOperand::Typed {
            resolved_type: &ty,
            span: span(source, 0, 1),
        };
        let invalid = Db2ResultTypeOperand::UntypedNull {
            span: Db2SourceSpan {
                start_byte: usize::MAX,
                ..valid.span()
            },
        };
        let envelope = span(source, 0, source.len());
        let call = |operands: &[Db2ResultTypeOperand<'_>], limits| {
            combine_db2_result_types(
                source,
                envelope,
                operands,
                Some(Context::OperandRules),
                limits,
            )
        };
        for limits in [
            Db2ResultCombinationLimits {
                max_source_bytes: 0,
                ..Default::default()
            },
            Db2ResultCombinationLimits {
                max_source_bytes: MAX_SOURCE_BYTES + 1,
                ..Default::default()
            },
            Db2ResultCombinationLimits {
                max_operands: 0,
                ..Default::default()
            },
            Db2ResultCombinationLimits {
                max_operands: MAX_OPERANDS + 1,
                ..Default::default()
            },
        ] {
            assert_eq!(
                call(&[invalid], limits).unwrap_err().code,
                Code::InvalidLimits
            );
        }
        assert_eq!(
            call(
                &[invalid],
                Db2ResultCombinationLimits {
                    max_source_bytes: source.len() - 1,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            Code::SourceTooLarge
        );
        assert!(
            call(
                &[valid],
                Db2ResultCombinationLimits {
                    max_source_bytes: source.len(),
                    max_operands: 1
                }
            )
            .is_ok()
        );
        assert_eq!(
            call(
                &[valid, invalid],
                Db2ResultCombinationLimits {
                    max_operands: 1,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            Code::TooManyOperands
        );
        let operands = [
            valid,
            Db2ResultTypeOperand::UntypedNull {
                span: span(source, 2, 3),
            },
            Db2ResultTypeOperand::Typed {
                resolved_type: &ty,
                span: span(source, 4, 5),
            },
        ];
        assert!(
            call(
                &operands,
                Db2ResultCombinationLimits {
                    max_operands: 3,
                    ..Default::default()
                }
            )
            .is_ok()
        );
        assert_eq!(
            call(
                &operands,
                Db2ResultCombinationLimits {
                    max_operands: 2,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            Code::TooManyOperands
        );
        // Exact compiled ceiling and one beyond, including untyped entries.
        let large_source = "x,".repeat(MAX_OPERANDS);
        let operands = (0..MAX_OPERANDS)
            .map(|i| Db2ResultTypeOperand::Typed {
                resolved_type: &ty,
                span: Db2SourceSpan {
                    start_byte: i * 2,
                    end_byte: i * 2 + 1,
                    start: Db2SourceLocation {
                        line: 1,
                        column: (i * 2 + 1) as u32,
                    },
                    end: Db2SourceLocation {
                        line: 1,
                        column: (i * 2 + 2) as u32,
                    },
                },
            })
            .collect::<Vec<_>>();
        let large_envelope = span(&large_source, 0, large_source.len());
        let limits = Db2ResultCombinationLimits {
            max_operands: MAX_OPERANDS,
            ..Default::default()
        };
        let result = combine_db2_result_types(
            &large_source,
            large_envelope,
            &operands,
            Some(Context::OperandRules),
            limits,
        )
        .unwrap();
        assert_eq!(result.operands().len(), MAX_OPERANDS);
        assert_eq!(result.steps().len(), MAX_OPERANDS - 1);
        let mut oversized = operands;
        oversized.push(invalid);
        assert_eq!(
            combine_db2_result_types(&large_source, large_envelope, &oversized, None, limits)
                .unwrap_err()
                .code,
            Code::TooManyOperands
        );
    }
}
