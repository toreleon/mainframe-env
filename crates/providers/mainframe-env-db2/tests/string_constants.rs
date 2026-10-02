//! Independent public literal proofs; no assignment or execution claims.

use mainframe_env_db2::{
    Db2AstLimits, Db2BuiltInDataType, Db2BuiltInType, Db2DataType, Db2MixedData, Db2Nullability,
    Db2ScalarType, Db2SourceLocation, Db2SourceSpan, Db2StringConstant, Db2StringConstantContext,
    Db2StringConstantEncoding, Db2StringConstantErrorCode, Db2StringConstantLimits,
    Db2StringConstantValue, Db2StringKind, Db2SyntaxLimits, Db2TokenKind, Db2TypeErrorCode,
    lex_db2, materialize_db2_string_constant, resolve_db2_type,
};

fn context(mixed_data: Db2MixedData) -> Db2StringConstantContext {
    Db2StringConstantContext::UnicodeUtf8 { mixed_data }
}

fn single_line_span(source: &str, end_column: u32) -> Db2SourceSpan {
    Db2SourceSpan {
        start_byte: 0,
        end_byte: source.len(),
        start: Db2SourceLocation::START,
        end: Db2SourceLocation {
            line: 1,
            column: end_column,
        },
    }
}

fn literal(source: &str, end_column: u32) -> Db2StringConstant {
    materialize_db2_string_constant(
        source,
        single_line_span(source, end_column),
        Default::default(),
        context(Db2MixedData::No),
    )
    .unwrap()
}

#[test]
fn public_character_proofs_decode_once_and_retain_utf8_byte_lengths() {
    for (source, column, expected, form) in [
        ("' A''b  '", 10, " A'b  ", Db2StringKind::Character),
        ("'é''A'", 7, "é'A", Db2StringKind::Character),
        ("''''''", 7, "''", Db2StringKind::Character),
        ("x'C3a90031'", 12, "é\0\x31", Db2StringKind::Hex),
        ("X'2727'", 8, "''", Db2StringKind::Hex),
    ] {
        let proof = literal(source, column);
        assert_eq!(
            proof.value(),
            &Db2StringConstantValue::Character(expected.into())
        );
        assert_eq!(proof.value().bytes(), expected.as_bytes());
        assert_eq!(
            proof.resolved_type().scalar(),
            &Db2ScalarType::VarChar {
                length: expected.len() as u32
            }
        );
        assert_eq!(proof.resolved_type().nullability(), Db2Nullability::NotNull);
        assert_eq!(
            proof.encoding(),
            Db2StringConstantEncoding::UnicodeUtf8Mixed
        );
        assert_eq!(proof.encoding().ccsid(), Some(1208));
        assert_eq!(proof.form(), form);
        assert_eq!(proof.span(), single_line_span(source, column));
    }
    let tokens = lex_db2("'a''''b'", Default::default()).unwrap();
    assert!(matches!(&tokens.tokens()[0].kind,
        Db2TokenKind::String { value, .. } if value == "a''''b"));
    assert_eq!(literal("'a''''b'", 9).value().bytes(), b"a''b");
}

#[test]
fn public_binary_and_empty_proofs_are_not_character_data_or_null() {
    let proof = literal("bX'fF00c1'", 11);
    assert_eq!(
        proof.value(),
        &Db2StringConstantValue::Binary(vec![255, 0, 193])
    );
    assert_eq!(
        proof.resolved_type().scalar(),
        &Db2ScalarType::VarBinary { length: 3 }
    );
    assert_eq!(proof.encoding(), Db2StringConstantEncoding::Binary);
    assert_eq!(proof.encoding().ccsid(), None);
    assert_eq!(proof.form(), Db2StringKind::Binary);
    assert!(matches!(
        lex_db2("BX'00'", Default::default()).unwrap().tokens()[0].kind,
        Db2TokenKind::String {
            kind: Db2StringKind::Binary,
            ..
        }
    ));
    for (source, column, scalar) in [
        ("''", 3, Db2ScalarType::VarChar { length: 0 }),
        ("X''", 4, Db2ScalarType::VarChar { length: 0 }),
        ("BX''", 5, Db2ScalarType::VarBinary { length: 0 }),
    ] {
        let proof = literal(source, column);
        assert!(proof.value().bytes().is_empty());
        assert_eq!(proof.resolved_type().scalar(), &scalar);
        assert_eq!(proof.resolved_type().nullability(), Db2Nullability::NotNull);
    }
    for kind in [Db2BuiltInType::VarChar, Db2BuiltInType::VarBinary] {
        let syntax = Db2DataType::BuiltIn(
            Db2BuiltInDataType::new(kind, vec![0], false, Default::default()).unwrap(),
        );
        assert_eq!(
            resolve_db2_type(&syntax, Db2Nullability::Nullable)
                .unwrap_err()
                .code,
            Db2TypeErrorCode::InvalidLength
        );
    }
    let error = materialize_db2_string_constant(
        "NULL",
        single_line_span("NULL", 5),
        Default::default(),
        context(Db2MixedData::No),
    )
    .unwrap_err();
    assert_eq!(error.code, Db2StringConstantErrorCode::UnsupportedForm);
}

#[test]
fn public_mixed_data_context_does_not_infer_unicode_subtype_or_binary_ccsid() {
    for flag in [Db2MixedData::No, Db2MixedData::Yes] {
        for (source, column, encoding) in [
            ("'A'", 4, Db2StringConstantEncoding::UnicodeUtf8Mixed),
            ("X'C3A9'", 8, Db2StringConstantEncoding::UnicodeUtf8Mixed),
            ("BX'ff'", 7, Db2StringConstantEncoding::Binary),
        ] {
            let proof = materialize_db2_string_constant(
                source,
                single_line_span(source, column),
                Default::default(),
                context(flag),
            )
            .unwrap();
            assert_eq!(proof.encoding(), encoding);
        }
    }
    let source = "X'c3a9'";
    let error = materialize_db2_string_constant(
        source,
        single_line_span(source, 8),
        Default::default(),
        context(Db2MixedData::Yes),
    )
    .unwrap_err();
    assert_eq!(
        error.code,
        Db2StringConstantErrorCode::HexCaseRequiresUppercase
    );
    assert_eq!(literal(source, 8).value().bytes(), &[0xc3, 0xa9]);
}

#[test]
fn public_owned_proofs_keep_complete_original_locations_without_lexing_surroundings() {
    let expected = Db2SourceSpan {
        start_byte: 16,
        end_byte: 27,
        start: Db2SourceLocation { line: 2, column: 7 },
        end: Db2SourceLocation { line: 3, column: 6 },
    };
    let clone = {
        let source = String::from("/* é */\r\n1E3 \0 '元\r\na''b' + :hv");
        let proof = materialize_db2_string_constant(
            &source,
            expected,
            Default::default(),
            context(Db2MixedData::Yes),
        )
        .unwrap();
        proof.clone()
    };
    assert_eq!(clone.span(), expected);
    assert_eq!(clone.value().bytes(), "元\r\na'b".as_bytes());
    let source = "é\r\n'a'";
    let valid = Db2SourceSpan {
        start_byte: 4,
        end_byte: 7,
        start: Db2SourceLocation { line: 2, column: 1 },
        end: Db2SourceLocation { line: 2, column: 4 },
    };
    for forged in [
        Db2SourceSpan {
            start_byte: 1,
            ..valid
        },
        Db2SourceSpan {
            end_byte: 8,
            ..valid
        },
        Db2SourceSpan {
            start: Db2SourceLocation::START,
            ..valid
        },
        Db2SourceSpan {
            end: Db2SourceLocation { line: 2, column: 5 },
            ..valid
        },
    ] {
        let error = materialize_db2_string_constant(
            source,
            forged,
            Default::default(),
            context(Db2MixedData::No),
        )
        .unwrap_err();
        assert_eq!(error.code, Db2StringConstantErrorCode::InvalidSourceSpan);
        assert_eq!(error.span, forged);
    }
}

#[test]
fn public_malformed_and_pending_forms_remain_distinct_errors() {
    use Db2StringConstantErrorCode as Code;
    for (source, expected) in [
        ("'a'b'", Code::InvalidSpelling),
        ("'a';", Code::InvalidSpelling),
        ("BX'F'", Code::InvalidHex),
        ("BX'GG'", Code::InvalidHex),
        ("X'FF'", Code::InvalidUtf8InContext),
        ("X'C0AF'", Code::InvalidUtf8InContext),
        ("G'a'", Code::UnsupportedForm),
        ("UX'0041'", Code::UnsupportedForm),
        ("\"a\"", Code::UnsupportedDelimiter),
        ("'\0'", Code::InvalidLexerToken),
    ] {
        let span = single_line_span(source, source.chars().count() as u32 + 1);
        let error = materialize_db2_string_constant(
            source,
            span,
            Default::default(),
            context(Db2MixedData::No),
        )
        .unwrap_err();
        assert_eq!(error.code, expected, "{source}");
        assert_eq!(error.span, span);
        assert!(!error.message.is_empty());
    }
    for unsupported in [
        Db2StringConstantContext::Ascii,
        Db2StringConstantContext::Ebcdic,
        Db2StringConstantContext::UnicodeUtf16,
    ] {
        let error = materialize_db2_string_constant(
            "'a'",
            single_line_span("'a'", 4),
            Default::default(),
            unsupported,
        )
        .unwrap_err();
        assert_eq!(error.code, Code::UnsupportedContext);
    }
    assert_eq!(literal("BX'FF'", 7).value().bytes(), &[255]);
}

#[test]
fn public_source_spelling_token_value_and_publication_bounds_are_independent() {
    use Db2StringConstantErrorCode as Code;
    let source = "'a''β'";
    let span = single_line_span(source, 7);
    let limits = Db2StringConstantLimits {
        syntax: Db2SyntaxLimits {
            max_statement_bytes: 7,
            max_token_bytes: 7,
            max_tokens: 1,
            max_nesting: 1,
        },
        ast: Db2AstLimits {
            max_literal_bytes: 7,
            ..Default::default()
        },
        max_value_bytes: 4,
    };
    assert_eq!(
        materialize_db2_string_constant(source, span, limits, context(Db2MixedData::No))
            .unwrap()
            .value()
            .bytes(),
        "a'β".as_bytes()
    );
    for (changed, expected) in [
        (
            Db2StringConstantLimits {
                syntax: Db2SyntaxLimits {
                    max_statement_bytes: 6,
                    ..limits.syntax
                },
                ..limits
            },
            Code::SourceTooLarge,
        ),
        (
            Db2StringConstantLimits {
                syntax: Db2SyntaxLimits {
                    max_token_bytes: 6,
                    ..limits.syntax
                },
                ..limits
            },
            Code::TokenTooLarge,
        ),
        (
            Db2StringConstantLimits {
                ast: Db2AstLimits {
                    max_literal_bytes: 6,
                    ..limits.ast
                },
                ..limits
            },
            Code::SpellingTooLarge,
        ),
        (
            Db2StringConstantLimits {
                max_value_bytes: 3,
                ..limits
            },
            Code::ValueTooLarge,
        ),
    ] {
        assert_eq!(
            materialize_db2_string_constant(source, span, changed, context(Db2MixedData::No))
                .unwrap_err()
                .code,
            expected
        );
    }
    for (prefix, repeated, natural_length) in [("", "a", 32704), ("BX", "00", 16352)] {
        let source = format!("{prefix}'{}'", repeated.repeat(natural_length));
        let proof = literal(&source, source.len() as u32 + 1);
        assert_eq!(proof.value().bytes().len(), natural_length);
        let too_large = format!("{prefix}'{}'", repeated.repeat(natural_length + 1));
        assert_eq!(
            materialize_db2_string_constant(
                &too_large,
                single_line_span(&too_large, too_large.len() as u32 + 1),
                Default::default(),
                context(Db2MixedData::No)
            )
            .unwrap_err()
            .code,
            Code::BodyTooLarge
        );
    }
}
