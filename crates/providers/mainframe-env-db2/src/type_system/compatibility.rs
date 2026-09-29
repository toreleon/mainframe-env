use super::{Db2Nullability, Db2ResolvedType, Db2ScalarType};

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
