//! Owned common Db2 scalar-type and compatibility semantics.
//!
//! This module resolves parser-level type syntax into validated semantic shapes.
//! It deliberately does not infer storage, perform conversions, or resolve the
//! context-sensitive rules that belong to a later binder.

use crate::ast::{Db2BuiltInDataType, Db2BuiltInType, Db2DataType};
use std::fmt;

pub mod string_constants;

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
    // Db2 13 baseline ibm-db2-for-zos-13-2026-08-13, SQL0050:
    // SSEPEK_13.0.0/sqlref/src/tpc/db2z_sql_createtable.html, 874327 bytes,
    // SHA-256 104cc7fd0f43e804819da99c18887de60983cad8fa78b7d550ffaf63dfd299d6,
    // lines 220..227 define FLOAT(1..21) as single precision (REAL) and
    // FLOAT(22..53) as double precision (DOUBLE), with omitted precision 53.
    // Canonicalize semantic shape only; the AST retains FLOAT and its arguments.
    Ok(if precision <= 21 {
        Db2ScalarType::Real
    } else {
        Db2ScalarType::Double
    })
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

mod compatibility;

pub use compatibility::{
    Db2AssignmentCompatibility, Db2AssignmentContext, Db2AssignmentNullability,
    Db2ComparisonCompatibility, Db2ComparisonContext, Db2ConversionKind, classify_db2_assignment,
    classify_db2_comparison,
};

#[cfg(test)]
mod tests;
