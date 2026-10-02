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

// Rows and columns follow the supported subset of the pinned Db2 13
// db2z_assignmentandcomparison.html operand table (SHA-256 2cc97544).
fn compatibility_matrix_types() -> Vec<Db2ResolvedType> {
    [
        Db2ScalarType::Integer,
        Db2ScalarType::Decimal {
            precision: 9,
            scale: 2,
        },
        Db2ScalarType::Float { precision: 21 },
        Db2ScalarType::DecFloat { precision: 34 },
        Db2ScalarType::VarChar { length: 20 },
        Db2ScalarType::VarGraphic { length: 20 },
        Db2ScalarType::VarBinary { length: 20 },
        Db2ScalarType::Date,
        Db2ScalarType::Time,
        Db2ScalarType::Timestamp {
            precision: 6,
            time_zone: Db2TimeZone::WithoutTimeZone,
        },
        Db2ScalarType::Timestamp {
            precision: 6,
            time_zone: Db2TimeZone::WithTimeZone,
        },
    ]
    .into_iter()
    .map(|scalar| resolved(scalar, Db2Nullability::NotNull))
    .collect()
}

#[test]
fn assignment_matrix_covers_every_supported_operand_cell() {
    // Y: compatible; C: string-to-datetime needs a binder context; N: rejected.
    let rows = [
        "YYYYYYNNNNN", // binary integer
        "YYYYYYNNNNN", // decimal
        "YYYYYYNNNNN", // floating point
        "YYYYYYNNNNN", // decimal floating point
        "YYYYYYNCCCC", // character
        "YYYYYYNCCCC", // graphic
        "NNNNNNYNNNN", // binary string
        "NNNNYYNYNNN", // date
        "NNNNYYNNYNN", // time
        "NNNNYYNNNYY", // timestamp without time zone
        "NNNNYYNNNYY", // timestamp with time zone
    ];
    let types = compatibility_matrix_types();
    for (source_index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), types.len());
        for (target_index, expected) in row.bytes().enumerate() {
            let actual = classify_db2_assignment(&types[source_index], &types[target_index]);
            let matches = match expected {
                b'Y' => matches!(actual, Db2AssignmentCompatibility::Compatible { .. }),
                b'C' => matches!(actual, Db2AssignmentCompatibility::ContextDependent { .. }),
                b'N' => actual == Db2AssignmentCompatibility::Incompatible,
                _ => unreachable!(),
            };
            assert!(
                matches,
                "assignment ({source_index}, {target_index}): {actual:?}"
            );
        }
    }
}

#[test]
fn comparison_matrix_covers_every_supported_operand_cell() {
    // C: datetime/string comparison requires representation and binder context.
    let rows = [
        "YYYYYYNNNNN", // binary integer
        "YYYYYYNNNNN", // decimal
        "YYYYYYNNNNN", // floating point
        "YYYYYYNNNNN", // decimal floating point
        "YYYYYYNCCCC", // character
        "YYYYYYNCCCC", // graphic
        "NNNNNNYNNNN", // binary string
        "NNNNCCNYNNN", // date
        "NNNNCCNNYNN", // time
        "NNNNCCNNNYY", // timestamp without time zone
        "NNNNCCNNNYY", // timestamp with time zone
    ];
    let types = compatibility_matrix_types();
    for (left_index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), types.len());
        for (right_index, expected) in row.bytes().enumerate() {
            let actual = classify_db2_comparison(&types[left_index], &types[right_index]);
            let matches = match expected {
                b'Y' => matches!(actual, Db2ComparisonCompatibility::Compatible { .. }),
                b'C' => matches!(actual, Db2ComparisonCompatibility::ContextDependent { .. }),
                b'N' => actual == Db2ComparisonCompatibility::Incompatible,
                _ => unreachable!(),
            };
            assert!(
                matches,
                "comparison ({left_index}, {right_index}): {actual:?}"
            );
        }
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
        (Db2BuiltInType::Float, vec![21], false, Db2ScalarType::Real),
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
        (Db2BuiltInType::Float, Db2ScalarType::Double),
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
            resolve_db2_type(&syntax(kind, &[maximum], false), Db2Nullability::NotNull,).is_ok()
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
