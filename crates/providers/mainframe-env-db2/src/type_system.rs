//! Owned common Db2 scalar-type and compatibility semantics.
//!
//! This module resolves parser-level type syntax into validated semantic shapes.
//! It deliberately does not infer storage, perform conversions, or resolve the
//! context-sensitive rules that belong to a later binder.

use crate::ast::{Db2BuiltInDataType, Db2BuiltInType, Db2DataType};
use std::fmt;

const MAX_DECIMAL_PRECISION: u32 = 31;
const MAX_FLOAT_PRECISION: u32 = 53;
const MAX_CHARACTER_LENGTH: u32 = 255;
const MAX_VARCHAR_LENGTH: u32 = 32_704;
const MAX_GRAPHIC_LENGTH: u32 = 127;
const MAX_VARGRAPHIC_LENGTH: u32 = 16_352;
const MAX_BINARY_LENGTH: u32 = 255;
const MAX_VARBINARY_LENGTH: u32 = 32_704;
const MAX_TIMESTAMP_PRECISION: u32 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2Nullability {
    NotNull,
    Nullable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2TimeZone {
    WithoutTimeZone,
    WithTimeZone,
}

/// A validated common scalar shape, distinct from parser-level type syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2ScalarType {
    SmallInt,
    Integer,
    BigInt,
    Decimal {
        precision: u32,
        scale: u32,
    },
    Float {
        precision: u32,
    },
    Real,
    Double,
    DecFloat {
        precision: u32,
    },
    Character {
        length: u32,
    },
    VarChar {
        length: u32,
    },
    Graphic {
        length: u32,
    },
    VarGraphic {
        length: u32,
    },
    Binary {
        length: u32,
    },
    VarBinary {
        length: u32,
    },
    Date,
    Time,
    Timestamp {
        precision: u32,
        time_zone: Db2TimeZone,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ResolvedType {
    scalar: Db2ScalarType,
    nullability: Db2Nullability,
}

impl Db2ResolvedType {
    #[must_use]
    pub const fn scalar(&self) -> &Db2ScalarType {
        &self.scalar
    }

    #[must_use]
    pub const fn nullability(&self) -> Db2Nullability {
        self.nullability
    }
}

/// Parser-owned syntax does not yet carry these attributes. This explicit
/// boundary lets later syntax work fail closed instead of silently discarding
/// an explicit CCSID or collation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Db2TypeAttributes {
    explicit_ccsid: bool,
    explicit_collation: bool,
}

impl Db2TypeAttributes {
    #[must_use]
    pub const fn new(explicit_ccsid: bool, explicit_collation: bool) -> Self {
        Self {
            explicit_ccsid,
            explicit_collation,
        }
    }

    #[must_use]
    pub const fn implicit() -> Self {
        Self::new(false, false)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2TypeErrorCode {
    InvalidArguments,
    InvalidPrecision,
    InvalidScale,
    InvalidLength,
    InvalidTimeZone,
    UnsupportedDistinct,
    UnsupportedLob,
    UnsupportedRowId,
    UnsupportedXml,
    UnsupportedCcsid,
    UnsupportedCollation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2TypeError {
    pub code: Db2TypeErrorCode,
    pub message: &'static str,
}

impl Db2TypeError {
    const fn new(code: Db2TypeErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for Db2TypeError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for Db2TypeError {}

pub fn resolve_db2_type(
    syntax: &Db2DataType,
    nullability: Db2Nullability,
) -> Result<Db2ResolvedType, Db2TypeError> {
    resolve_db2_type_with_attributes(syntax, nullability, Db2TypeAttributes::implicit())
}

pub fn resolve_db2_type_with_attributes(
    syntax: &Db2DataType,
    nullability: Db2Nullability,
    attributes: Db2TypeAttributes,
) -> Result<Db2ResolvedType, Db2TypeError> {
    if attributes.explicit_ccsid {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::UnsupportedCcsid,
            "explicit CCSID semantics are outside the common type slice",
        ));
    }
    if attributes.explicit_collation {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::UnsupportedCollation,
            "explicit collation semantics are outside the common type slice",
        ));
    }
    let Db2DataType::BuiltIn(syntax) = syntax else {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::UnsupportedDistinct,
            "distinct types require catalog identity and are not resolved here",
        ));
    };
    let scalar = resolve_built_in(syntax)?;
    Ok(Db2ResolvedType {
        scalar,
        nullability,
    })
}

fn resolve_built_in(syntax: &Db2BuiltInDataType) -> Result<Db2ScalarType, Db2TypeError> {
    use Db2BuiltInType as Syntax;

    match syntax.kind() {
        Syntax::SmallInt => no_arguments(syntax, Db2ScalarType::SmallInt),
        Syntax::Integer => no_arguments(syntax, Db2ScalarType::Integer),
        Syntax::BigInt => no_arguments(syntax, Db2ScalarType::BigInt),
        Syntax::Decimal => resolve_decimal(syntax),
        Syntax::Float => resolve_float(syntax),
        Syntax::Real => no_arguments(syntax, Db2ScalarType::Real),
        Syntax::Double => no_arguments(syntax, Db2ScalarType::Double),
        Syntax::DecFloat => resolve_decfloat(syntax),
        Syntax::Character => resolve_length(syntax, 1, MAX_CHARACTER_LENGTH, |length| {
            Db2ScalarType::Character { length }
        }),
        Syntax::VarChar => resolve_required_length(syntax, MAX_VARCHAR_LENGTH, |length| {
            Db2ScalarType::VarChar { length }
        }),
        Syntax::Graphic => resolve_length(syntax, 1, MAX_GRAPHIC_LENGTH, |length| {
            Db2ScalarType::Graphic { length }
        }),
        Syntax::VarGraphic => resolve_required_length(syntax, MAX_VARGRAPHIC_LENGTH, |length| {
            Db2ScalarType::VarGraphic { length }
        }),
        Syntax::Binary => resolve_length(syntax, 1, MAX_BINARY_LENGTH, |length| {
            Db2ScalarType::Binary { length }
        }),
        Syntax::VarBinary => resolve_required_length(syntax, MAX_VARBINARY_LENGTH, |length| {
            Db2ScalarType::VarBinary { length }
        }),
        Syntax::Date => no_arguments(syntax, Db2ScalarType::Date),
        Syntax::Time => no_arguments(syntax, Db2ScalarType::Time),
        Syntax::Timestamp => resolve_timestamp(syntax),
        Syntax::Clob | Syntax::DbClob | Syntax::Blob => Err(Db2TypeError::new(
            Db2TypeErrorCode::UnsupportedLob,
            "LOB types are outside the common scalar type slice",
        )),
        Syntax::RowId => Err(Db2TypeError::new(
            Db2TypeErrorCode::UnsupportedRowId,
            "ROWID requires a separately owned semantic family",
        )),
        Syntax::Xml => Err(Db2TypeError::new(
            Db2TypeErrorCode::UnsupportedXml,
            "XML requires its dedicated type and comparison semantics",
        )),
    }
}

fn reject_time_zone(syntax: &Db2BuiltInDataType) -> Result<(), Db2TypeError> {
    if syntax.with_time_zone() {
        Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidTimeZone,
            "WITH TIME ZONE is supported only for TIMESTAMP in this slice",
        ))
    } else {
        Ok(())
    }
}

fn no_arguments(
    syntax: &Db2BuiltInDataType,
    scalar: Db2ScalarType,
) -> Result<Db2ScalarType, Db2TypeError> {
    reject_time_zone(syntax)?;
    if syntax.arguments().is_empty() {
        Ok(scalar)
    } else {
        Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidArguments,
            "this Db2 type does not accept numeric arguments",
        ))
    }
}

fn resolve_decimal(syntax: &Db2BuiltInDataType) -> Result<Db2ScalarType, Db2TypeError> {
    reject_time_zone(syntax)?;
    let (precision, scale) = match syntax.arguments() {
        [] => (5, 0),
        [precision] => (*precision, 0),
        [precision, scale] => (*precision, *scale),
        _ => {
            return Err(Db2TypeError::new(
                Db2TypeErrorCode::InvalidArguments,
                "DECIMAL accepts at most precision and scale",
            ));
        }
    };
    if !(1..=MAX_DECIMAL_PRECISION).contains(&precision) {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidPrecision,
            "DECIMAL precision must be from 1 through 31",
        ));
    }
    if scale > precision {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidScale,
            "DECIMAL scale cannot exceed precision",
        ));
    }
    Ok(Db2ScalarType::Decimal { precision, scale })
}

fn resolve_float(syntax: &Db2BuiltInDataType) -> Result<Db2ScalarType, Db2TypeError> {
    reject_time_zone(syntax)?;
    let precision = optional_single_argument(syntax, 53, "FLOAT accepts at most one precision")?;
    if !(1..=MAX_FLOAT_PRECISION).contains(&precision) {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidPrecision,
            "FLOAT precision must be from 1 through 53",
        ));
    }
    Ok(Db2ScalarType::Float { precision })
}

fn resolve_decfloat(syntax: &Db2BuiltInDataType) -> Result<Db2ScalarType, Db2TypeError> {
    reject_time_zone(syntax)?;
    let precision = optional_single_argument(syntax, 34, "DECFLOAT accepts at most one precision")?;
    if !matches!(precision, 16 | 34) {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidPrecision,
            "DECFLOAT precision must be 16 or 34",
        ));
    }
    Ok(Db2ScalarType::DecFloat { precision })
}

fn resolve_length(
    syntax: &Db2BuiltInDataType,
    default: u32,
    maximum: u32,
    constructor: fn(u32) -> Db2ScalarType,
) -> Result<Db2ScalarType, Db2TypeError> {
    reject_time_zone(syntax)?;
    let length = optional_single_argument(syntax, default, "type accepts at most one length")?;
    checked_length(length, maximum, constructor)
}

fn resolve_required_length(
    syntax: &Db2BuiltInDataType,
    maximum: u32,
    constructor: fn(u32) -> Db2ScalarType,
) -> Result<Db2ScalarType, Db2TypeError> {
    reject_time_zone(syntax)?;
    let [length] = syntax.arguments() else {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidArguments,
            "varying string types require exactly one length",
        ));
    };
    checked_length(*length, maximum, constructor)
}

fn checked_length(
    length: u32,
    maximum: u32,
    constructor: fn(u32) -> Db2ScalarType,
) -> Result<Db2ScalarType, Db2TypeError> {
    if length == 0 || length > maximum {
        Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidLength,
            "string length is zero or exceeds the Db2 type maximum",
        ))
    } else {
        Ok(constructor(length))
    }
}

fn optional_single_argument(
    syntax: &Db2BuiltInDataType,
    default: u32,
    message: &'static str,
) -> Result<u32, Db2TypeError> {
    match syntax.arguments() {
        [] => Ok(default),
        [value] => Ok(*value),
        _ => Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidArguments,
            message,
        )),
    }
}

fn resolve_timestamp(syntax: &Db2BuiltInDataType) -> Result<Db2ScalarType, Db2TypeError> {
    let precision = optional_single_argument(syntax, 6, "TIMESTAMP accepts at most one precision")?;
    if precision > MAX_TIMESTAMP_PRECISION {
        return Err(Db2TypeError::new(
            Db2TypeErrorCode::InvalidPrecision,
            "TIMESTAMP precision must be from 0 through 12",
        ));
    }
    Ok(Db2ScalarType::Timestamp {
        precision,
        time_zone: if syntax.with_time_zone() {
            Db2TimeZone::WithTimeZone
        } else {
            Db2TimeZone::WithoutTimeZone
        },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ConversionKind {
    Identity,
    Numeric,
    NumericCharacterString,
    NumericGraphicString,
    CharacterString,
    GraphicString,
    CharacterGraphicString,
    BinaryString,
    DatetimeCharacterString,
    DatetimeGraphicString,
    Timestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2AssignmentContext {
    CharacterStringToDatetime,
    GraphicStringToDatetime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2AssignmentNullability {
    Safe,
    RequiresRuntimeNullCheck,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2AssignmentCompatibility {
    Compatible {
        conversion: Db2ConversionKind,
        nullability: Db2AssignmentNullability,
    },
    ContextDependent {
        requirement: Db2AssignmentContext,
        nullability: Db2AssignmentNullability,
    },
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ComparisonContext {
    DatetimeCharacterString,
    DatetimeGraphicString,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ComparisonCompatibility {
    Compatible {
        conversion: Db2ConversionKind,
        result_nullability: Db2Nullability,
    },
    ContextDependent {
        requirement: Db2ComparisonContext,
        result_nullability: Db2Nullability,
    },
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeFamily {
    BinaryInteger,
    Decimal,
    FloatingPoint,
    DecimalFloatingPoint,
    Character,
    Graphic,
    Binary,
    Date,
    Time,
    Timestamp,
}

impl Db2ScalarType {
    const fn family(&self) -> TypeFamily {
        match self {
            Self::SmallInt | Self::Integer | Self::BigInt => TypeFamily::BinaryInteger,
            Self::Decimal { .. } => TypeFamily::Decimal,
            Self::Float { .. } | Self::Real | Self::Double => TypeFamily::FloatingPoint,
            Self::DecFloat { .. } => TypeFamily::DecimalFloatingPoint,
            Self::Character { .. } | Self::VarChar { .. } => TypeFamily::Character,
            Self::Graphic { .. } | Self::VarGraphic { .. } => TypeFamily::Graphic,
            Self::Binary { .. } | Self::VarBinary { .. } => TypeFamily::Binary,
            Self::Date => TypeFamily::Date,
            Self::Time => TypeFamily::Time,
            Self::Timestamp { .. } => TypeFamily::Timestamp,
        }
    }
}

impl TypeFamily {
    const fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::BinaryInteger | Self::Decimal | Self::FloatingPoint | Self::DecimalFloatingPoint
        )
    }

    const fn is_datetime(self) -> bool {
        matches!(self, Self::Date | Self::Time | Self::Timestamp)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaseAssignment {
    Compatible(Db2ConversionKind),
    ContextDependent(Db2AssignmentContext),
    Incompatible,
}

pub fn classify_db2_assignment(
    source: &Db2ResolvedType,
    target: &Db2ResolvedType,
) -> Db2AssignmentCompatibility {
    let nullability = if source.nullability == Db2Nullability::Nullable
        && target.nullability == Db2Nullability::NotNull
    {
        Db2AssignmentNullability::RequiresRuntimeNullCheck
    } else {
        Db2AssignmentNullability::Safe
    };
    match classify_assignment_scalars(&source.scalar, &target.scalar) {
        BaseAssignment::Compatible(conversion) => Db2AssignmentCompatibility::Compatible {
            conversion,
            nullability,
        },
        BaseAssignment::ContextDependent(requirement) => {
            Db2AssignmentCompatibility::ContextDependent {
                requirement,
                nullability,
            }
        }
        BaseAssignment::Incompatible => Db2AssignmentCompatibility::Incompatible,
    }
}

fn classify_assignment_scalars(source: &Db2ScalarType, target: &Db2ScalarType) -> BaseAssignment {
    use Db2AssignmentContext as Context;
    use Db2ConversionKind as Conversion;
    use TypeFamily as Family;

    if source == target {
        return BaseAssignment::Compatible(Conversion::Identity);
    }
    let source = source.family();
    let target = target.family();
    match (source, target) {
        (source, target) if source.is_numeric() && target.is_numeric() => {
            BaseAssignment::Compatible(Conversion::Numeric)
        }
        (Family::Character, Family::Character) => {
            BaseAssignment::Compatible(Conversion::CharacterString)
        }
        (Family::Graphic, Family::Graphic) => BaseAssignment::Compatible(Conversion::GraphicString),
        (Family::Binary, Family::Binary) => BaseAssignment::Compatible(Conversion::BinaryString),
        (Family::Timestamp, Family::Timestamp) => BaseAssignment::Compatible(Conversion::Timestamp),
        (source, Family::Character) | (Family::Character, source) if source.is_numeric() => {
            BaseAssignment::Compatible(Conversion::NumericCharacterString)
        }
        (source, Family::Graphic) | (Family::Graphic, source) if source.is_numeric() => {
            BaseAssignment::Compatible(Conversion::NumericGraphicString)
        }
        (Family::Character, Family::Graphic) | (Family::Graphic, Family::Character) => {
            BaseAssignment::Compatible(Conversion::CharacterGraphicString)
        }
        (source, Family::Character) if source.is_datetime() => {
            BaseAssignment::Compatible(Conversion::DatetimeCharacterString)
        }
        (Family::Character, target) if target.is_datetime() => {
            BaseAssignment::ContextDependent(Context::CharacterStringToDatetime)
        }
        (source, Family::Graphic) if source.is_datetime() => {
            BaseAssignment::Compatible(Conversion::DatetimeGraphicString)
        }
        (Family::Graphic, target) if target.is_datetime() => {
            BaseAssignment::ContextDependent(Context::GraphicStringToDatetime)
        }
        _ => BaseAssignment::Incompatible,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaseComparison {
    Compatible(Db2ConversionKind),
    ContextDependent(Db2ComparisonContext),
    Incompatible,
}

pub fn classify_db2_comparison(
    left: &Db2ResolvedType,
    right: &Db2ResolvedType,
) -> Db2ComparisonCompatibility {
    let result_nullability = if left.nullability == Db2Nullability::Nullable
        || right.nullability == Db2Nullability::Nullable
    {
        Db2Nullability::Nullable
    } else {
        Db2Nullability::NotNull
    };
    match classify_comparison_scalars(&left.scalar, &right.scalar) {
        BaseComparison::Compatible(conversion) => Db2ComparisonCompatibility::Compatible {
            conversion,
            result_nullability,
        },
        BaseComparison::ContextDependent(requirement) => {
            Db2ComparisonCompatibility::ContextDependent {
                requirement,
                result_nullability,
            }
        }
        BaseComparison::Incompatible => Db2ComparisonCompatibility::Incompatible,
    }
}

fn classify_comparison_scalars(left: &Db2ScalarType, right: &Db2ScalarType) -> BaseComparison {
    use Db2ComparisonContext as Context;
    use Db2ConversionKind as Conversion;
    use TypeFamily as Family;

    if left == right {
        return BaseComparison::Compatible(Conversion::Identity);
    }
    let left = left.family();
    let right = right.family();
    match (left, right) {
        (left, right) if left.is_numeric() && right.is_numeric() => {
            BaseComparison::Compatible(Conversion::Numeric)
        }
        (Family::Character, Family::Character) => {
            BaseComparison::Compatible(Conversion::CharacterString)
        }
        (Family::Graphic, Family::Graphic) => BaseComparison::Compatible(Conversion::GraphicString),
        (Family::Binary, Family::Binary) => BaseComparison::Compatible(Conversion::BinaryString),
        (Family::Timestamp, Family::Timestamp) => BaseComparison::Compatible(Conversion::Timestamp),
        (Family::Character, Family::Graphic) | (Family::Graphic, Family::Character) => {
            BaseComparison::Compatible(Conversion::CharacterGraphicString)
        }
        (left, Family::Character) | (Family::Character, left) if left.is_numeric() => {
            BaseComparison::Compatible(Conversion::NumericCharacterString)
        }
        (left, Family::Graphic) | (Family::Graphic, left) if left.is_numeric() => {
            BaseComparison::Compatible(Conversion::NumericGraphicString)
        }
        (left, Family::Character) | (Family::Character, left) if left.is_datetime() => {
            BaseComparison::ContextDependent(Context::DatetimeCharacterString)
        }
        (left, Family::Graphic) | (Family::Graphic, left) if left.is_datetime() => {
            BaseComparison::ContextDependent(Context::DatetimeGraphicString)
        }
        _ => BaseComparison::Incompatible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        Db2AstErrorCode, Db2AstLimits, Db2BuiltInDataType, Db2Identifier, Db2QualifiedName,
    };

    fn syntax(kind: Db2BuiltInType, arguments: &[u32], with_time_zone: bool) -> Db2DataType {
        Db2DataType::BuiltIn(
            Db2BuiltInDataType::new(
                kind,
                arguments.to_vec(),
                with_time_zone,
                Db2AstLimits::default(),
            )
            .unwrap(),
        )
    }

    fn resolve(kind: Db2BuiltInType, arguments: &[u32], with_time_zone: bool) -> Db2ResolvedType {
        resolve_db2_type(
            &syntax(kind, arguments, with_time_zone),
            Db2Nullability::NotNull,
        )
        .unwrap()
    }

    fn resolved(scalar: Db2ScalarType, nullability: Db2Nullability) -> Db2ResolvedType {
        Db2ResolvedType {
            scalar,
            nullability,
        }
    }

    #[test]
    fn resolves_common_scalar_shapes_and_preserves_parameters() {
        let cases = [
            (
                Db2BuiltInType::SmallInt,
                vec![],
                false,
                Db2ScalarType::SmallInt,
            ),
            (
                Db2BuiltInType::Integer,
                vec![],
                false,
                Db2ScalarType::Integer,
            ),
            (Db2BuiltInType::BigInt, vec![], false, Db2ScalarType::BigInt),
            (
                Db2BuiltInType::Decimal,
                vec![],
                false,
                Db2ScalarType::Decimal {
                    precision: 5,
                    scale: 0,
                },
            ),
            (
                Db2BuiltInType::Decimal,
                vec![31, 31],
                false,
                Db2ScalarType::Decimal {
                    precision: 31,
                    scale: 31,
                },
            ),
            (
                Db2BuiltInType::Float,
                vec![21],
                false,
                Db2ScalarType::Float { precision: 21 },
            ),
            (Db2BuiltInType::Real, vec![], false, Db2ScalarType::Real),
            (Db2BuiltInType::Double, vec![], false, Db2ScalarType::Double),
            (
                Db2BuiltInType::DecFloat,
                vec![16],
                false,
                Db2ScalarType::DecFloat { precision: 16 },
            ),
            (
                Db2BuiltInType::Character,
                vec![255],
                false,
                Db2ScalarType::Character { length: 255 },
            ),
            (
                Db2BuiltInType::VarChar,
                vec![32_704],
                false,
                Db2ScalarType::VarChar { length: 32_704 },
            ),
            (
                Db2BuiltInType::Graphic,
                vec![127],
                false,
                Db2ScalarType::Graphic { length: 127 },
            ),
            (
                Db2BuiltInType::VarGraphic,
                vec![16_352],
                false,
                Db2ScalarType::VarGraphic { length: 16_352 },
            ),
            (
                Db2BuiltInType::Binary,
                vec![255],
                false,
                Db2ScalarType::Binary { length: 255 },
            ),
            (
                Db2BuiltInType::VarBinary,
                vec![32_704],
                false,
                Db2ScalarType::VarBinary { length: 32_704 },
            ),
            (Db2BuiltInType::Date, vec![], false, Db2ScalarType::Date),
            (Db2BuiltInType::Time, vec![], false, Db2ScalarType::Time),
            (
                Db2BuiltInType::Timestamp,
                vec![12],
                true,
                Db2ScalarType::Timestamp {
                    precision: 12,
                    time_zone: Db2TimeZone::WithTimeZone,
                },
            ),
        ];

        for (kind, arguments, with_time_zone, expected) in cases {
            let actual = resolve(kind, &arguments, with_time_zone);
            assert_eq!(actual.scalar(), &expected);
            assert_eq!(actual.nullability(), Db2Nullability::NotNull);
        }
    }

    #[test]
    fn applies_only_context_free_defaults() {
        let cases = [
            (
                Db2BuiltInType::Float,
                Db2ScalarType::Float { precision: 53 },
            ),
            (
                Db2BuiltInType::DecFloat,
                Db2ScalarType::DecFloat { precision: 34 },
            ),
            (
                Db2BuiltInType::Character,
                Db2ScalarType::Character { length: 1 },
            ),
            (
                Db2BuiltInType::Graphic,
                Db2ScalarType::Graphic { length: 1 },
            ),
            (Db2BuiltInType::Binary, Db2ScalarType::Binary { length: 1 }),
            (
                Db2BuiltInType::Timestamp,
                Db2ScalarType::Timestamp {
                    precision: 6,
                    time_zone: Db2TimeZone::WithoutTimeZone,
                },
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(resolve(kind, &[], false).scalar(), &expected);
        }
        for kind in [
            Db2BuiltInType::VarChar,
            Db2BuiltInType::VarGraphic,
            Db2BuiltInType::VarBinary,
        ] {
            assert_eq!(
                resolve_db2_type(&syntax(kind, &[], false), Db2Nullability::Nullable)
                    .unwrap_err()
                    .code,
                Db2TypeErrorCode::InvalidArguments
            );
        }
    }

    #[test]
    fn rejects_invalid_precision_scale_length_argument_and_zone_matrix() {
        let cases = [
            (
                Db2BuiltInType::SmallInt,
                vec![1],
                false,
                Db2TypeErrorCode::InvalidArguments,
            ),
            (
                Db2BuiltInType::Decimal,
                vec![0],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::Decimal,
                vec![32],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::Decimal,
                vec![4, 5],
                false,
                Db2TypeErrorCode::InvalidScale,
            ),
            (
                Db2BuiltInType::Float,
                vec![0],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::Float,
                vec![54],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::DecFloat,
                vec![15],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::DecFloat,
                vec![17],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::Character,
                vec![0],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::Character,
                vec![256],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::VarChar,
                vec![32_705],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::Graphic,
                vec![128],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::VarGraphic,
                vec![16_353],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::Binary,
                vec![256],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::VarBinary,
                vec![32_705],
                false,
                Db2TypeErrorCode::InvalidLength,
            ),
            (
                Db2BuiltInType::Timestamp,
                vec![13],
                false,
                Db2TypeErrorCode::InvalidPrecision,
            ),
            (
                Db2BuiltInType::Date,
                vec![],
                true,
                Db2TypeErrorCode::InvalidTimeZone,
            ),
            (
                Db2BuiltInType::Time,
                vec![],
                true,
                Db2TypeErrorCode::InvalidTimeZone,
            ),
        ];

        for (kind, arguments, with_time_zone, expected) in cases {
            assert_eq!(
                resolve_db2_type(
                    &syntax(kind, &arguments, with_time_zone),
                    Db2Nullability::Nullable,
                )
                .unwrap_err()
                .code,
                expected,
                "kind={kind:?}, arguments={arguments:?}, with_time_zone={with_time_zone}",
            );
        }
    }

    #[test]
    fn rejects_unsupported_type_families_and_string_attributes() {
        let unsupported = [
            (Db2BuiltInType::Clob, Db2TypeErrorCode::UnsupportedLob),
            (Db2BuiltInType::DbClob, Db2TypeErrorCode::UnsupportedLob),
            (Db2BuiltInType::Blob, Db2TypeErrorCode::UnsupportedLob),
            (Db2BuiltInType::RowId, Db2TypeErrorCode::UnsupportedRowId),
            (Db2BuiltInType::Xml, Db2TypeErrorCode::UnsupportedXml),
        ];
        for (kind, expected) in unsupported {
            assert_eq!(
                resolve_db2_type(&syntax(kind, &[], false), Db2Nullability::Nullable)
                    .unwrap_err()
                    .code,
                expected
            );
        }

        let identifier = Db2Identifier::new("MONEY", false, Db2AstLimits::default()).unwrap();
        let name = Db2QualifiedName::new(vec![identifier], Db2AstLimits::default()).unwrap();
        assert_eq!(
            resolve_db2_type(&Db2DataType::Distinct(name), Db2Nullability::NotNull)
                .unwrap_err()
                .code,
            Db2TypeErrorCode::UnsupportedDistinct
        );

        let varchar = syntax(Db2BuiltInType::VarChar, &[10], false);
        for (attributes, expected) in [
            (
                Db2TypeAttributes::new(true, false),
                Db2TypeErrorCode::UnsupportedCcsid,
            ),
            (
                Db2TypeAttributes::new(false, true),
                Db2TypeErrorCode::UnsupportedCollation,
            ),
        ] {
            assert_eq!(
                resolve_db2_type_with_attributes(&varchar, Db2Nullability::Nullable, attributes,)
                    .unwrap_err()
                    .code,
                expected
            );
        }
    }

    #[test]
    fn resource_bounds_hold_at_ast_and_resolved_shape_boundaries() {
        assert_eq!(
            Db2BuiltInDataType::new(
                Db2BuiltInType::Decimal,
                vec![31, 0, 0],
                false,
                Db2AstLimits::default(),
            )
            .unwrap_err()
            .code,
            Db2AstErrorCode::InvalidDataType
        );

        let boundaries = [
            (Db2BuiltInType::Character, 255, 256),
            (Db2BuiltInType::VarChar, 32_704, 32_705),
            (Db2BuiltInType::Graphic, 127, 128),
            (Db2BuiltInType::VarGraphic, 16_352, 16_353),
            (Db2BuiltInType::Binary, 255, 256),
            (Db2BuiltInType::VarBinary, 32_704, 32_705),
        ];
        for (kind, maximum, too_large) in boundaries {
            assert!(
                resolve_db2_type(&syntax(kind, &[maximum], false), Db2Nullability::NotNull,)
                    .is_ok()
            );
            assert_eq!(
                resolve_db2_type(&syntax(kind, &[too_large], false), Db2Nullability::NotNull,)
                    .unwrap_err()
                    .code,
                Db2TypeErrorCode::InvalidLength
            );
        }
    }

    fn numeric_types() -> Vec<Db2ScalarType> {
        vec![
            Db2ScalarType::SmallInt,
            Db2ScalarType::Integer,
            Db2ScalarType::BigInt,
            Db2ScalarType::Decimal {
                precision: 13,
                scale: 2,
            },
            Db2ScalarType::Float { precision: 21 },
            Db2ScalarType::Real,
            Db2ScalarType::Double,
            Db2ScalarType::DecFloat { precision: 34 },
        ]
    }

    #[test]
    fn every_common_numeric_family_is_assignment_and_comparison_compatible() {
        for left in numeric_types() {
            for right in numeric_types() {
                let left = resolved(left.clone(), Db2Nullability::NotNull);
                let right = resolved(right.clone(), Db2Nullability::NotNull);
                assert!(matches!(
                    classify_db2_assignment(&left, &right),
                    Db2AssignmentCompatibility::Compatible { .. }
                ));
                assert!(matches!(
                    classify_db2_comparison(&left, &right),
                    Db2ComparisonCompatibility::Compatible { .. }
                ));
            }
        }
    }

    #[test]
    fn assignment_distinguishes_compatible_contextual_and_incompatible_pairs() {
        let char_10 = resolved(
            Db2ScalarType::Character { length: 10 },
            Db2Nullability::NotNull,
        );
        let varchar_20 = resolved(
            Db2ScalarType::VarChar { length: 20 },
            Db2Nullability::NotNull,
        );
        let binary = resolved(
            Db2ScalarType::Binary { length: 10 },
            Db2Nullability::NotNull,
        );
        let varbinary = resolved(
            Db2ScalarType::VarBinary { length: 20 },
            Db2Nullability::NotNull,
        );
        let timestamp_without = resolved(
            Db2ScalarType::Timestamp {
                precision: 6,
                time_zone: Db2TimeZone::WithoutTimeZone,
            },
            Db2Nullability::NotNull,
        );
        let timestamp_with = resolved(
            Db2ScalarType::Timestamp {
                precision: 12,
                time_zone: Db2TimeZone::WithTimeZone,
            },
            Db2Nullability::NotNull,
        );

        assert_eq!(
            classify_db2_assignment(&char_10, &varchar_20),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::CharacterString,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
        assert_eq!(
            classify_db2_assignment(&binary, &varbinary),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::BinaryString,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
        assert_eq!(
            classify_db2_assignment(&timestamp_without, &timestamp_with),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::Timestamp,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
        assert_eq!(
            classify_db2_assignment(&char_10, &binary),
            Db2AssignmentCompatibility::Incompatible
        );
        assert_eq!(
            classify_db2_assignment(
                &resolved(Db2ScalarType::Date, Db2Nullability::NotNull),
                &resolved(Db2ScalarType::Time, Db2Nullability::NotNull),
            ),
            Db2AssignmentCompatibility::Incompatible
        );
    }

    #[test]
    fn assignment_classification_preserves_direction_and_context() {
        let numeric = resolved(Db2ScalarType::Integer, Db2Nullability::Nullable);
        let character = resolved(
            Db2ScalarType::VarChar { length: 20 },
            Db2Nullability::NotNull,
        );
        let graphic = resolved(
            Db2ScalarType::VarGraphic { length: 20 },
            Db2Nullability::NotNull,
        );
        let date = resolved(Db2ScalarType::Date, Db2Nullability::NotNull);

        assert_eq!(
            classify_db2_assignment(&numeric, &character),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::NumericCharacterString,
                nullability: Db2AssignmentNullability::RequiresRuntimeNullCheck,
            }
        );
        assert_eq!(
            classify_db2_assignment(&character, &numeric),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::NumericCharacterString,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
        assert!(matches!(
            classify_db2_assignment(&numeric, &graphic),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::NumericGraphicString,
                ..
            }
        ));
        assert!(matches!(
            classify_db2_assignment(&character, &graphic),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::CharacterGraphicString,
                ..
            }
        ));
        assert_eq!(
            classify_db2_assignment(&date, &character),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::DatetimeCharacterString,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
        assert_eq!(
            classify_db2_assignment(&character, &date),
            Db2AssignmentCompatibility::ContextDependent {
                requirement: Db2AssignmentContext::CharacterStringToDatetime,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
        assert_ne!(
            classify_db2_assignment(&numeric, &character),
            classify_db2_assignment(&character, &numeric)
        );
    }

    #[test]
    fn assignment_nullability_matrix_requires_only_the_unsafe_direction_to_check() {
        let scalar = Db2ScalarType::Integer;
        let cases = [
            (
                Db2Nullability::NotNull,
                Db2Nullability::NotNull,
                Db2AssignmentNullability::Safe,
            ),
            (
                Db2Nullability::NotNull,
                Db2Nullability::Nullable,
                Db2AssignmentNullability::Safe,
            ),
            (
                Db2Nullability::Nullable,
                Db2Nullability::Nullable,
                Db2AssignmentNullability::Safe,
            ),
            (
                Db2Nullability::Nullable,
                Db2Nullability::NotNull,
                Db2AssignmentNullability::RequiresRuntimeNullCheck,
            ),
        ];

        for (source_nullability, target_nullability, expected) in cases {
            assert_eq!(
                classify_db2_assignment(
                    &resolved(scalar.clone(), source_nullability),
                    &resolved(scalar.clone(), target_nullability),
                ),
                Db2AssignmentCompatibility::Compatible {
                    conversion: Db2ConversionKind::Identity,
                    nullability: expected,
                }
            );
        }
    }

    fn comparison_representatives() -> Vec<Db2ResolvedType> {
        vec![
            resolved(Db2ScalarType::Integer, Db2Nullability::NotNull),
            resolved(
                Db2ScalarType::Decimal {
                    precision: 9,
                    scale: 2,
                },
                Db2Nullability::Nullable,
            ),
            resolved(
                Db2ScalarType::VarChar { length: 20 },
                Db2Nullability::NotNull,
            ),
            resolved(
                Db2ScalarType::VarGraphic { length: 20 },
                Db2Nullability::Nullable,
            ),
            resolved(
                Db2ScalarType::VarBinary { length: 20 },
                Db2Nullability::NotNull,
            ),
            resolved(Db2ScalarType::Date, Db2Nullability::NotNull),
            resolved(Db2ScalarType::Time, Db2Nullability::Nullable),
            resolved(
                Db2ScalarType::Timestamp {
                    precision: 6,
                    time_zone: Db2TimeZone::WithoutTimeZone,
                },
                Db2Nullability::NotNull,
            ),
            resolved(
                Db2ScalarType::Timestamp {
                    precision: 12,
                    time_zone: Db2TimeZone::WithTimeZone,
                },
                Db2Nullability::Nullable,
            ),
        ]
    }

    #[test]
    fn comparison_matrix_is_symmetric() {
        let types = comparison_representatives();
        for left in &types {
            for right in &types {
                assert_eq!(
                    classify_db2_comparison(left, right),
                    classify_db2_comparison(right, left),
                    "left={left:?}, right={right:?}",
                );
            }
        }
    }

    #[test]
    fn comparison_classifies_context_and_datetime_boundaries() {
        let character = resolved(
            Db2ScalarType::VarChar { length: 20 },
            Db2Nullability::NotNull,
        );
        let graphic = resolved(
            Db2ScalarType::VarGraphic { length: 20 },
            Db2Nullability::Nullable,
        );
        let integer = resolved(Db2ScalarType::Integer, Db2Nullability::NotNull);
        let date = resolved(Db2ScalarType::Date, Db2Nullability::NotNull);
        let time = resolved(Db2ScalarType::Time, Db2Nullability::NotNull);
        let timestamp_without = resolved(
            Db2ScalarType::Timestamp {
                precision: 6,
                time_zone: Db2TimeZone::WithoutTimeZone,
            },
            Db2Nullability::NotNull,
        );
        let timestamp_with = resolved(
            Db2ScalarType::Timestamp {
                precision: 12,
                time_zone: Db2TimeZone::WithTimeZone,
            },
            Db2Nullability::Nullable,
        );

        assert_eq!(
            classify_db2_comparison(&character, &graphic),
            Db2ComparisonCompatibility::Compatible {
                conversion: Db2ConversionKind::CharacterGraphicString,
                result_nullability: Db2Nullability::Nullable,
            }
        );
        assert_eq!(
            classify_db2_comparison(&integer, &character),
            Db2ComparisonCompatibility::Compatible {
                conversion: Db2ConversionKind::NumericCharacterString,
                result_nullability: Db2Nullability::NotNull,
            }
        );
        assert_eq!(
            classify_db2_comparison(&date, &graphic),
            Db2ComparisonCompatibility::ContextDependent {
                requirement: Db2ComparisonContext::DatetimeGraphicString,
                result_nullability: Db2Nullability::Nullable,
            }
        );
        assert_eq!(
            classify_db2_comparison(&timestamp_without, &timestamp_with),
            Db2ComparisonCompatibility::Compatible {
                conversion: Db2ConversionKind::Timestamp,
                result_nullability: Db2Nullability::Nullable,
            }
        );
        assert_eq!(
            classify_db2_comparison(&date, &time),
            Db2ComparisonCompatibility::Incompatible
        );
    }
}
