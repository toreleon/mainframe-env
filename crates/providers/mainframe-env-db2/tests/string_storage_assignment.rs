//! Independent storage vectors; no retrieval, SQLCA or durable-cell claims.

use mainframe_env_db2::{
    Db2BuiltInDataType, Db2BuiltInType as Kind, Db2DataType, Db2MixedData, Db2Nullability,
    Db2ResolvedType, Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2StoredStringConstant,
    Db2StringConstant, Db2StringConstantContext, Db2StringConstantValue,
    Db2StringStorageContext as Context, Db2StringStorageErrorCode as Code, Db2StringStorageLimits,
    Db2TypeErrorCode, materialize_db2_string_constant, resolve_db2_type, store_db2_string_constant,
};

const LIMITS: Db2StringStorageLimits = Db2StringStorageLimits {
    max_output_bytes: 32704,
};

fn literal(source: &str, end_column: u32) -> Db2StringConstant {
    materialize_db2_string_constant(
        source,
        Db2SourceSpan {
            start_byte: 0,
            end_byte: source.len(),
            start: Db2SourceLocation::START,
            end: Db2SourceLocation {
                line: 1,
                column: end_column,
            },
        },
        Default::default(),
        Db2StringConstantContext::UnicodeUtf8 {
            mixed_data: Db2MixedData::No,
        },
    )
    .unwrap()
}

fn target(kind: Kind, args: &[u32], nullability: Db2Nullability) -> Db2ResolvedType {
    resolve_db2_type(
        &Db2DataType::BuiltIn(
            Db2BuiltInDataType::new(kind, args.to_vec(), false, Default::default()).unwrap(),
        ),
        nullability,
    )
    .unwrap()
}

fn storage(
    source: &str,
    column: u32,
    kind: Kind,
    length: u32,
    context: Context,
) -> Db2StoredStringConstant {
    store_db2_string_constant(
        &literal(source, column),
        &target(kind, &[length], Db2Nullability::Nullable),
        context,
        LIMITS,
    )
    .unwrap()
}

#[test]
fn public_character_storage_pads_or_removes_only_excess_ascii_blanks() {
    for (source, column, kind, length, expected, padding, truncation) in [
        ("'bA'", 5, Kind::Character, 5, "bA   ", 3, 0),
        ("'bA'", 5, Kind::VarChar, 5, "bA", 0, 0),
        ("'a''B   '", 10, Kind::Character, 4, "a'B ", 0, 2),
        ("'a''B   '", 10, Kind::VarChar, 3, "a'B", 0, 3),
        ("X'00622020'", 12, Kind::Character, 3, "\0b ", 0, 1),
        ("X'0062'", 8, Kind::Character, 4, "\0b  ", 2, 0),
        ("'é  '", 6, Kind::VarChar, 2, "é", 0, 2),
        ("'界'", 4, Kind::Character, 5, "界  ", 2, 0),
    ] {
        let result = storage(source, column, kind, length, Context::UnicodeUtf8Mixed);
        assert_eq!(
            result.value(),
            &Db2StringConstantValue::Character(expected.into())
        );
        assert_eq!(result.value().bytes(), expected.as_bytes());
        assert_eq!(result.padded_bytes(), padding);
        assert_eq!(result.truncated_bytes(), truncation);
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::Nullable
        );
        assert_eq!(
            result.source().resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(result.context(), Context::UnicodeUtf8Mixed);
    }
    for (source, column, length) in [
        ("'AB'", 5, 1),
        ("'A B'", 6, 1),
        ("'A\t'", 5, 1),
        ("X'4100'", 8, 1),
        ("'A　'", 5, 1),
        ("'é '", 5, 1),
        ("'界 '", 5, 2),
        ("'😀 '", 5, 3),
    ] {
        for kind in [Kind::Character, Kind::VarChar] {
            let proof = literal(source, column);
            let before = proof.clone();
            let error = store_db2_string_constant(
                &proof,
                &target(kind, &[length], Db2Nullability::NotNull),
                Context::UnicodeUtf8Mixed,
                LIMITS,
            )
            .unwrap_err();
            assert_eq!(error.code, Code::NonBlankExcess);
            assert_eq!(error.span, proof.span());
            assert_eq!(proof, before);
        }
    }
}

#[test]
fn public_binary_storage_retains_bytes_and_never_truncates_even_zero_excess() {
    for (source, column, kind, length, expected, padding) in [
        ("BX'00ff20'", 11, Kind::Binary, 5, &b"\0\xff \0\0"[..], 2),
        ("BX'00ff20'", 11, Kind::VarBinary, 5, &b"\0\xff "[..], 0),
        ("BX''", 5, Kind::Binary, 2, &b"\0\0"[..], 2),
        ("BX''", 5, Kind::VarBinary, 2, &b""[..], 0),
    ] {
        let result = storage(source, column, kind, length, Context::Binary);
        assert_eq!(
            result.value(),
            &Db2StringConstantValue::Binary(expected.to_vec())
        );
        assert_eq!(result.value().bytes(), expected);
        assert_eq!(result.padded_bytes(), padding);
        assert_eq!(result.truncated_bytes(), 0);
        assert_eq!(result.source().encoding().ccsid(), None);
    }
    for (source, column) in [("BX'4100'", 9), ("BX'4120'", 9), ("BX'0000'", 9)] {
        for kind in [Kind::Binary, Kind::VarBinary] {
            assert_eq!(
                store_db2_string_constant(
                    &literal(source, column),
                    &target(kind, &[1], Db2Nullability::NotNull),
                    Context::Binary,
                    LIMITS,
                )
                .unwrap_err()
                .code,
                Code::BinaryTooLong
            );
        }
    }
}

#[test]
fn public_empty_values_remain_nonnull_and_are_not_declared_length_zero() {
    for (source, column, fixed, varying, context, expected) in [
        (
            "''",
            3,
            Kind::Character,
            Kind::VarChar,
            Context::UnicodeUtf8Mixed,
            &b" "[..],
        ),
        (
            "X''",
            4,
            Kind::Character,
            Kind::VarChar,
            Context::UnicodeUtf8Mixed,
            &b" "[..],
        ),
        (
            "BX''",
            5,
            Kind::Binary,
            Kind::VarBinary,
            Context::Binary,
            &b"\0"[..],
        ),
    ] {
        let proof = literal(source, column);
        assert_eq!(proof.resolved_type().nullability(), Db2Nullability::NotNull);
        assert_eq!(
            storage(source, column, fixed, 1, context).value().bytes(),
            expected
        );
        assert!(
            storage(source, column, varying, 1, context)
                .value()
                .bytes()
                .is_empty()
        );
        assert_eq!(
            store_db2_string_constant(&proof, proof.resolved_type(), context, LIMITS)
                .unwrap_err()
                .code,
            Code::InvalidTargetLength
        );
        let syntax = Db2DataType::BuiltIn(
            Db2BuiltInDataType::new(varying, vec![0], false, Default::default()).unwrap(),
        );
        assert_eq!(
            resolve_db2_type(&syntax, Db2Nullability::Nullable)
                .unwrap_err()
                .code,
            Db2TypeErrorCode::InvalidLength
        );
    }
}

#[test]
fn public_encoding_and_unsupported_conversion_fences_are_explicit() {
    for (source, column, kind, args, context, code) in [
        (
            "X'41'",
            6,
            Kind::Binary,
            &[1][..],
            Context::Binary,
            Code::IncompatibleTypes,
        ),
        (
            "BX'41'",
            7,
            Kind::VarChar,
            &[1][..],
            Context::UnicodeUtf8Mixed,
            Code::IncompatibleTypes,
        ),
        (
            "'A'",
            4,
            Kind::Character,
            &[1][..],
            Context::Binary,
            Code::TargetEncodingMismatch,
        ),
        (
            "BX''",
            5,
            Kind::Binary,
            &[1][..],
            Context::UnicodeUtf8Mixed,
            Code::TargetEncodingMismatch,
        ),
        (
            "'A'",
            4,
            Kind::Character,
            &[1][..],
            Context::Ebcdic,
            Code::UnsupportedCharacterConversion,
        ),
        (
            "'A'",
            4,
            Kind::Character,
            &[1][..],
            Context::BitData,
            Code::UnsupportedBitData,
        ),
        (
            "'1'",
            4,
            Kind::Graphic,
            &[1][..],
            Context::UnicodeUtf8Mixed,
            Code::UnsupportedGraphicTarget,
        ),
        (
            "'1'",
            4,
            Kind::Integer,
            &[][..],
            Context::UnicodeUtf8Mixed,
            Code::UnsupportedNumericTarget,
        ),
        (
            "'1'",
            4,
            Kind::Date,
            &[][..],
            Context::UnicodeUtf8Mixed,
            Code::UnsupportedDatetimeTarget,
        ),
    ] {
        let proof = literal(source, column);
        let error = store_db2_string_constant(
            &proof,
            &target(kind, args, Db2Nullability::NotNull),
            context,
            LIMITS,
        )
        .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.span, proof.span());
    }
}

#[test]
fn public_output_bounds_distinguish_fixed_padding_from_actual_varying_length() {
    for (source, column, kind, length, context, required) in [
        (
            "'A'",
            4,
            Kind::Character,
            255,
            Context::UnicodeUtf8Mixed,
            255,
        ),
        ("BX'41'", 7, Kind::Binary, 255, Context::Binary, 255),
        (
            "'AB'",
            5,
            Kind::VarChar,
            32704,
            Context::UnicodeUtf8Mixed,
            2,
        ),
        ("BX'4100'", 9, Kind::VarBinary, 32704, Context::Binary, 2),
    ] {
        let proof = literal(source, column);
        let declared = target(kind, &[length], Db2Nullability::NotNull);
        let result = store_db2_string_constant(
            &proof,
            &declared,
            context,
            Db2StringStorageLimits {
                max_output_bytes: required,
            },
        )
        .unwrap();
        assert_eq!(result.value().bytes().len(), required);
        assert_eq!(result.resolved_type(), &declared);
        assert_eq!(
            store_db2_string_constant(
                &proof,
                &declared,
                context,
                Db2StringStorageLimits {
                    max_output_bytes: required - 1
                }
            )
            .unwrap_err()
            .code,
            Code::OutputTooLarge
        );
        for budget in [0, 32705, usize::MAX] {
            assert_eq!(
                store_db2_string_constant(
                    &proof,
                    &declared,
                    context,
                    Db2StringStorageLimits {
                        max_output_bytes: budget
                    }
                )
                .unwrap_err()
                .code,
                Code::InvalidLimits
            );
        }
    }
    let source = format!("'{}'", "A".repeat(32704));
    let proof = literal(&source, 32707);
    let declared = target(Kind::VarChar, &[32704], Db2Nullability::Nullable);
    assert_eq!(
        store_db2_string_constant(&proof, &declared, Context::UnicodeUtf8Mixed, LIMITS)
            .unwrap()
            .value()
            .bytes(),
        &[65; 32704]
    );
    assert_eq!(
        store_db2_string_constant(
            &proof,
            &declared,
            Context::UnicodeUtf8Mixed,
            Db2StringStorageLimits {
                max_output_bytes: 32703
            }
        )
        .unwrap_err()
        .code,
        Code::OutputTooLarge
    );
}

#[test]
fn public_owned_storage_keeps_original_provenance_and_both_value_proofs() {
    let expected_span = Db2SourceSpan {
        start_byte: 16,
        end_byte: 26,
        start: Db2SourceLocation { line: 2, column: 7 },
        end: Db2SourceLocation { line: 3, column: 5 },
    };
    let result = {
        let sql = String::from("/* é */\r\n1E3 \0 '元\r\na  ' + :hv");
        let proof = materialize_db2_string_constant(
            &sql,
            expected_span,
            Default::default(),
            Db2StringConstantContext::UnicodeUtf8 {
                mixed_data: Db2MixedData::Yes,
            },
        )
        .unwrap();
        let declared = target(Kind::Character, &[7], Db2Nullability::Nullable);
        store_db2_string_constant(&proof, &declared, Context::UnicodeUtf8Mixed, LIMITS)
            .unwrap()
            .clone()
    };
    assert_eq!(result.span(), expected_span);
    assert_eq!(result.source().value().bytes(), "元\r\na  ".as_bytes());
    assert_eq!(result.value().bytes(), "元\r\na ".as_bytes());
    assert_eq!(
        result.resolved_type().scalar(),
        &Db2ScalarType::Character { length: 7 }
    );
    assert_eq!(
        result.source().resolved_type().scalar(),
        &Db2ScalarType::VarChar { length: 8 }
    );
    assert_eq!(
        result.resolved_type().nullability(),
        Db2Nullability::Nullable
    );
    assert_eq!(result.truncated_bytes(), 1);
    assert_eq!(result.context(), Context::UnicodeUtf8Mixed);
}
