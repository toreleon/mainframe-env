//! Public exact assignment, without binder/default or execution claims.

use mainframe_env_db2::{
    Db2AssignedNumericConstant, Db2AssignedNumericValue, Db2AssignmentCompatibility,
    Db2AssignmentNullability, Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType, Db2ConversionKind,
    Db2DataType, Db2MaterializedNumericConstant, Db2Nullability, Db2NumericAssignmentErrorCode,
    Db2ResolvedType, Db2ScalarType, Db2SourceLocation, Db2SourceSpan, assign_db2_numeric_constant,
    materialize_db2_numeric_constant, resolve_db2_type,
};

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

fn literal(text: &str) -> Db2MaterializedNumericConstant {
    materialize_db2_numeric_constant(text, span(text), Default::default()).unwrap()
}

fn target(kind: Db2BuiltInType, args: &[u32], nullable: bool) -> Db2ResolvedType {
    let syntax =
        Db2BuiltInDataType::new(kind, args.to_vec(), false, Db2AstLimits::default()).unwrap();
    resolve_db2_type(
        &Db2DataType::BuiltIn(syntax),
        if nullable {
            Db2Nullability::Nullable
        } else {
            Db2Nullability::NotNull
        },
    )
    .unwrap()
}

fn assign(text: &str, kind: Db2BuiltInType, args: &[u32]) -> Db2AssignedNumericConstant {
    assign_db2_numeric_constant(&literal(text), &target(kind, args, false)).unwrap()
}

fn decimal(result: &Db2AssignedNumericConstant, coefficient: i128, scale: u8) {
    let Db2AssignedNumericValue::Decimal(value) = result.value() else {
        panic!("expected DECIMAL target value");
    };
    assert_eq!(value.coefficient(), coefficient);
    assert_eq!(value.scale(), scale);
}

#[test]
fn public_binary_target_bounds_follow_fraction_elimination_not_rounding() {
    use Db2AssignedNumericValue as Value;
    use Db2BuiltInType as Kind;
    for (text, kind, expected) in [
        ("32767.999", Kind::SmallInt, Value::SmallInt(i16::MAX)),
        ("-32768.999", Kind::SmallInt, Value::SmallInt(i16::MIN)),
        ("2147483647.999", Kind::Integer, Value::Integer(i32::MAX)),
        ("-2147483648.999", Kind::Integer, Value::Integer(i32::MIN)),
        (
            "9223372036854775807.999",
            Kind::BigInt,
            Value::BigInt(i64::MAX),
        ),
        (
            "-9223372036854775808.999",
            Kind::BigInt,
            Value::BigInt(i64::MIN),
        ),
        ("-.999", Kind::SmallInt, Value::SmallInt(0)),
    ] {
        let result = assign(text, kind, &[]);
        assert_eq!(result.value(), &expected, "{text}");
        assert_eq!(result.resolved_type(), &target(kind, &[], false));
        assert_eq!(result.span(), span(text));
        assert_eq!(result.conversion().discarded_fractional_digits(), 3);
        assert!(result.conversion().discarded_nonzero());
        assert!(result.conversion().temporary_decimal().is_none());
    }
    for (text, kind) in [
        ("32768.000", Kind::SmallInt),
        ("-32769.001", Kind::SmallInt),
        ("2147483648.000", Kind::Integer),
        ("-2147483649.001", Kind::Integer),
        ("9223372036854775808.000", Kind::BigInt),
        ("-9223372036854775809.001", Kind::BigInt),
    ] {
        let error =
            assign_db2_numeric_constant(&literal(text), &target(kind, &[], false)).unwrap_err();
        assert_eq!(error.code, Db2NumericAssignmentErrorCode::ValueOutOfRange);
        assert_eq!(error.span, span(text));
    }
}

#[test]
fn public_decimal31_bounds_and_scale_expansion_preserve_whole_parts() {
    use Db2BuiltInType::Decimal;
    let maximum = 9_999_999_999_999_999_999_999_999_999_999;
    for (text, precision, scale, coefficient) in [
        (".9999999999999999999999999999999", 31, 31, maximum),
        ("-.9999999999999999999999999999999", 31, 31, -maximum),
        ("9999999999999999999999999999999.", 31, 0, maximum),
        ("-0", 31, 31, 0),
        ("9", 31, 30, 9_000_000_000_000_000_000_000_000_000_000),
        ("-1.20", 5, 4, -12_000),
        ("99.999", 4, 2, 9_999),
    ] {
        let result = assign(text, Decimal, &[precision, scale]);
        decimal(&result, coefficient, scale as u8);
        assert_eq!(
            result.resolved_type().scalar(),
            &Db2ScalarType::Decimal { precision, scale }
        );
    }
    for (text, args) in [
        ("1", [31, 31]),
        ("-1.001", [31, 31]),
        ("10", [31, 30]),
        ("-10", [31, 30]),
        ("100.000", [4, 2]),
        ("9999999999999999999999999999999.", [31, 31]),
    ] {
        let error = assign_db2_numeric_constant(&literal(text), &target(Decimal, &args, false))
            .unwrap_err();
        assert_eq!(
            error.code,
            Db2NumericAssignmentErrorCode::ValueOutOfRange,
            "{text}"
        );
        assert_eq!(error.span, span(text));
    }
}

#[test]
fn public_fraction_metadata_distinguishes_zeroes_from_nonzero_loss() {
    for (text, coefficient, count, nonzero) in [
        ("-000.00100", -1, 2, false),
        ("-000.00109", -1, 2, true),
        ("-0.00000", 0, 2, false),
        (".00001", 0, 2, true),
        ("1.20", 1_200, 0, false),
    ] {
        let result = assign(text, Db2BuiltInType::Decimal, &[5, 3]);
        decimal(&result, coefficient, 3);
        assert_eq!(result.conversion().discarded_fractional_digits(), count);
        assert_eq!(result.conversion().discarded_nonzero(), nonzero);
    }
}

#[test]
fn public_temporary_types_follow_natural_kind_and_nullable_target_metadata() {
    for (text, precision, coefficient) in [
        ("1", 11, 100),
        ("-2147483648", 11, -214_748_364_800),
        ("2147483648", 19, 214_748_364_800),
        ("-9223372036854775808", 19, -922_337_203_685_477_580_800),
    ] {
        let target = target(Db2BuiltInType::Decimal, &[21, 2], true);
        let source = literal(text);
        let result = assign_db2_numeric_constant(&source, &target).unwrap();
        decimal(&result, coefficient, 2);
        assert_eq!(result.resolved_type(), &target);
        let temporary = result.conversion().temporary_decimal().unwrap();
        assert_eq!(
            temporary.scalar(),
            &Db2ScalarType::Decimal {
                precision,
                scale: 0
            }
        );
        assert_eq!(temporary.nullability(), Db2Nullability::NotNull);
        assert_eq!(
            result.conversion().compatibility(),
            Db2AssignmentCompatibility::Compatible {
                conversion: Db2ConversionKind::Numeric,
                nullability: Db2AssignmentNullability::Safe,
            }
        );
    }
    let result = assign("9223372036854775808", Db2BuiltInType::Decimal, &[19, 0]);
    assert!(result.conversion().temporary_decimal().is_none());
    decimal(&assign("1", Db2BuiltInType::Decimal, &[1, 0]), 1, 0);
}

#[test]
fn public_unimplemented_conversions_are_distinct_from_incompatibility() {
    use Db2BuiltInType as Kind;
    use Db2NumericAssignmentErrorCode as Code;
    for (kind, args, expected) in [
        (Kind::Real, &[][..], Code::UnsupportedFloatingTarget),
        (Kind::Float, &[53][..], Code::UnsupportedFloatingTarget),
        (Kind::DecFloat, &[34][..], Code::UnsupportedDecFloatTarget),
        (Kind::VarChar, &[10][..], Code::UnsupportedNonNumericTarget),
        (
            Kind::VarGraphic,
            &[10][..],
            Code::UnsupportedNonNumericTarget,
        ),
        (Kind::Binary, &[10][..], Code::IncompatibleTypes),
        (Kind::Date, &[][..], Code::IncompatibleTypes),
    ] {
        let error =
            assign_db2_numeric_constant(&literal("1.25"), &target(kind, args, true)).unwrap_err();
        assert_eq!(error.code, expected);
        assert_eq!(error.span, span("1.25"));
        assert!(error.message.len() <= 256);
    }
}

#[test]
fn public_owned_assignment_keeps_verified_utf8_crlf_source_span() {
    let expected_span = Db2SourceSpan {
        start_byte: 14,
        end_byte: 24,
        start: Db2SourceLocation { line: 2, column: 3 },
        end: Db2SourceLocation {
            line: 2,
            column: 13,
        },
    };
    let (result, error) = {
        let source = String::from("/*é😀*/\r\n  -000.00109; -- trailing");
        // Original prefix is 14 bytes; neither UTF-8 nor CRLF is normalized.
        let literal =
            materialize_db2_numeric_constant(&source, expected_span, Default::default()).unwrap();
        let result =
            assign_db2_numeric_constant(&literal, &target(Db2BuiltInType::Decimal, &[3, 3], true))
                .unwrap();
        let error_literal = materialize_db2_numeric_constant(
            "/*é😀*/\r\n  -32769.001;",
            expected_span,
            Default::default(),
        )
        .unwrap();
        let error = assign_db2_numeric_constant(
            &error_literal,
            &target(Db2BuiltInType::SmallInt, &[], false),
        )
        .unwrap_err();
        (result, error)
    };
    assert_eq!(result.span(), expected_span);
    decimal(&result, -1, 3);
    assert_eq!(
        result.resolved_type().nullability(),
        Db2Nullability::Nullable
    );
    assert_eq!(result.conversion().discarded_fractional_digits(), 2);
    assert!(result.conversion().discarded_nonzero());
    assert_eq!(result.clone(), result);
    assert_eq!(error.span, expected_span);
    assert_eq!(error.code, Db2NumericAssignmentErrorCode::ValueOutOfRange);
    assert!(error.to_string().contains("2:3"));
}
