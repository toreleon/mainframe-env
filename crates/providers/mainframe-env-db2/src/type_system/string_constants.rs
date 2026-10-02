//! Located natural string values, inside the existing semantic type owner.
//!
//! Source: ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_constantsintro.html, 17915 bytes,
//!   bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8;
//! - db2z_charstrings.html, 17888 bytes,
//!   54c3f8ec1620479ffb002788e198c788050d8616e1ee4fbac431fb45bb59d835;
//! - db2z_characterstokens.html, 14271 bytes,
//!   ed63dd289fc68abe18ec5863756239f79d93941cd525968ba48f962d1299d4ae;
//! - db2z_binarystringsintro.html, 3946 bytes,
//!   69af1d1645cf137f58d372fe93589e1f299a76937c4e9d1284ad1aa6e2643036;
//! - db2z_datatypesintro.html, 22904 bytes,
//!   a488006755eedd9ef58da3ba8ef9f304a3d79c3910cc39da637dda1f3c38f570;
//! - db2z_apostrophesandquotesindelims.html, 5549 bytes,
//!   94746a42021315234baea45342774882f497fcaa3f8360cdd837c0673d414700.
//!
//! Language elements have no standalone statement-catalog rows. This proof is
//! not assignment/default legality, expression evaluation, conversion, catalog
//! identity, storage or execution. Only caller-selected UTF-8/apostrophe context
//! is admitted; other ordinary families remain pending. Raw NUL source retains
//! the existing lexer rejection; X'00' and BX'00' preserve a decoded NUL byte.

use super::{Db2Nullability, Db2ResolvedType, Db2ScalarType};
use crate::{
    Db2AstLimits, Db2SourceLocation, Db2SourceSpan, Db2StringKind, Db2SyntaxDiagnosticCode,
    Db2SyntaxLimits, Db2TokenKind, lex_db2,
};
use std::fmt;

const MAX_STRING_BODY_BYTES: usize = 32_704;

/// Installation flag, independent of Unicode strings' always-MIXED subtype.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2MixedData {
    No,
    Yes,
}

/// Explicit source interpretation, never inferred from host or string contents.
/// UnicodeUtf8 admits apostrophe-delimited constants only. The other contexts
/// are explicit pending implementations, not aliases for UTF-8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2StringConstantContext {
    UnicodeUtf8 { mixed_data: Db2MixedData },
    UnicodeUtf16,
    Ascii,
    Ebcdic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2StringConstantLimits {
    /// Bounds the complete original source and the selected lexer token.
    pub syntax: Db2SyntaxLimits,
    /// Bounds selected spelling bytes; all existing AST limits are validated.
    /// No expression nodes or lists are constructed by this kernel.
    pub ast: Db2AstLimits,
    /// Independent decoded-byte budget, at most the publication's 32704 cap.
    pub max_value_bytes: usize,
}

impl Default for Db2StringConstantLimits {
    fn default() -> Self {
        Self {
            syntax: Db2SyntaxLimits::default(),
            ast: Db2AstLimits::default(),
            max_value_bytes: MAX_STRING_BODY_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2StringConstantEncoding {
    UnicodeUtf8Mixed,
    Binary,
}

impl Db2StringConstantEncoding {
    #[must_use]
    pub const fn ccsid(self) -> Option<u16> {
        match self {
            Self::UnicodeUtf8Mixed => Some(1208),
            Self::Binary => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2StringConstantValue {
    Character(String),
    Binary(Vec<u8>),
}

impl Db2StringConstantValue {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::Character(value) => value.as_bytes(),
            Self::Binary(value) => value,
        }
    }
}

/// Opaque owned pairing. Callers cannot supply a raw value, type or byte count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2StringConstant {
    resolved_type: Db2ResolvedType,
    span: Db2SourceSpan,
    value: Db2StringConstantValue,
    encoding: Db2StringConstantEncoding,
    form: Db2StringKind,
}

impl Db2StringConstant {
    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.resolved_type
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.span
    }

    #[must_use]
    pub const fn value(&self) -> &Db2StringConstantValue {
        &self.value
    }

    #[must_use]
    pub const fn encoding(&self) -> Db2StringConstantEncoding {
        self.encoding
    }

    #[must_use]
    pub const fn form(&self) -> Db2StringKind {
        self.form
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2StringConstantErrorCode {
    InvalidLimits,
    SourceTooLarge,
    InvalidSourceSpan,
    SpellingTooLarge,
    TokenTooLarge,
    BodyTooLarge,
    ValueTooLarge,
    UnsupportedContext,
    UnsupportedDelimiter,
    UnsupportedForm,
    InvalidSpelling,
    InvalidHex,
    HexCaseRequiresUppercase,
    InvalidUtf8InContext,
    InvalidLexerToken,
}

/// Fixed-size located error; never retains source or an allocated message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2StringConstantError {
    pub code: Db2StringConstantErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl fmt::Display for Db2StringConstantError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2StringConstantError {}

fn error(
    code: Db2StringConstantErrorCode,
    span: Db2SourceSpan,
    message: &'static str,
) -> Db2StringConstantError {
    Db2StringConstantError {
        code,
        span,
        message,
    }
}

/// Verify complete original coordinates and preflight all allocation bounds,
/// then lex only the selected original spelling. Surrounding SQL is not parsed.
pub fn materialize_db2_string_constant(
    source: &str,
    span: Db2SourceSpan,
    limits: Db2StringConstantLimits,
    context: Db2StringConstantContext,
) -> Result<Db2StringConstant, Db2StringConstantError> {
    use Db2StringConstantErrorCode as Code;
    let origin = Db2SourceSpan {
        start_byte: 0,
        end_byte: 0,
        start: Db2SourceLocation::START,
        end: Db2SourceLocation::START,
    };
    // The existing lexer validates syntax limits before rejecting empty input.
    // Reuse that authority without allocating any quoted payload or lexing SQL.
    if !matches!(
        lex_db2("", limits.syntax),
        Err(diagnostic) if diagnostic.code == Db2SyntaxDiagnosticCode::EmptyStatement
    ) || limits.ast.validate().is_err()
        || limits.max_value_bytes == 0
        || limits.max_value_bytes > MAX_STRING_BODY_BYTES
    {
        return Err(error(
            Code::InvalidLimits,
            origin,
            "invalid string constant limits",
        ));
    }
    if source.len() > limits.syntax.max_statement_bytes {
        return Err(error(
            Code::SourceTooLarge,
            origin,
            "original source exceeds byte limit",
        ));
    }
    if span.start_byte > span.end_byte
        || span.end_byte > source.len()
        || !source.is_char_boundary(span.start_byte)
        || !source.is_char_boundary(span.end_byte)
    {
        return Err(error(
            Code::InvalidSourceSpan,
            span,
            "span is outside source or splits UTF-8",
        ));
    }
    if location_at(source, span.start_byte) != span.start
        || location_at(source, span.end_byte) != span.end
    {
        return Err(error(
            Code::InvalidSourceSpan,
            span,
            "original span coordinates do not match",
        ));
    }
    let spelling = &source[span.start_byte..span.end_byte];
    if spelling.len() > limits.ast.max_literal_bytes {
        return Err(error(
            Code::SpellingTooLarge,
            span,
            "selected spelling exceeds literal limit",
        ));
    }
    if spelling.len() > limits.syntax.max_token_bytes {
        return Err(error(
            Code::TokenTooLarge,
            span,
            "selected spelling exceeds token limit",
        ));
    }
    let Db2StringConstantContext::UnicodeUtf8 { mixed_data } = context else {
        return Err(error(
            Code::UnsupportedContext,
            span,
            "only explicit Unicode UTF-8 context is implemented",
        ));
    };
    let (form, body) = selected_body(spelling, span)?;
    let decoded_len = preflight_body(form, body, mixed_data, span)?;
    if decoded_len > limits.max_value_bytes {
        return Err(error(
            Code::ValueTooLarge,
            span,
            "decoded string exceeds value byte limit",
        ));
    }
    let lexed = lex_db2(spelling, limits.syntax).map_err(|_| {
        error(
            Code::InvalidLexerToken,
            span,
            "selected spelling is not an admitted lexer token",
        )
    })?;
    let [token] = lexed.tokens() else {
        return Err(error(
            Code::InvalidLexerToken,
            span,
            "selected spelling must be one string token",
        ));
    };
    let Db2TokenKind::String { kind, value } = &token.kind else {
        return Err(error(
            Code::InvalidLexerToken,
            span,
            "selected token is not a string",
        ));
    };
    if *kind != form || token.span.start_byte != 0 || token.span.end_byte != spelling.len() {
        return Err(error(
            Code::InvalidLexerToken,
            span,
            "string token does not cover selected spelling",
        ));
    }
    let value = if form == Db2StringKind::Character {
        let mut decoded = String::with_capacity(decoded_len);
        let mut characters = value.chars();
        while let Some(character) = characters.next() {
            decoded.push(character);
            if character == '\'' {
                characters.next(); // preflight proved one doubled delimiter
            }
        }
        Db2StringConstantValue::Character(decoded)
    } else {
        let mut decoded = Vec::with_capacity(decoded_len);
        for pair in value.as_bytes().as_chunks::<2>().0 {
            decoded.push((hex_digit(pair[0]) << 4) | hex_digit(pair[1]));
        }
        if form == Db2StringKind::Binary {
            Db2StringConstantValue::Binary(decoded)
        } else {
            Db2StringConstantValue::Character(String::from_utf8(decoded).map_err(|_| {
                error(
                    Code::InvalidUtf8InContext,
                    span,
                    "X bytes are not valid UTF-8 in the explicit context",
                )
            })?)
        }
    };
    let (scalar, encoding) = if form == Db2StringKind::Binary {
        (
            Db2ScalarType::VarBinary {
                length: decoded_len as u32,
            },
            Db2StringConstantEncoding::Binary,
        )
    } else {
        (
            Db2ScalarType::VarChar {
                length: decoded_len as u32,
            },
            Db2StringConstantEncoding::UnicodeUtf8Mixed,
        )
    };
    // Natural literal length zero is valid. This does not relax column syntax
    // or the existing resolved column/type constructor's minimum length one.
    Ok(Db2StringConstant {
        resolved_type: Db2ResolvedType {
            scalar,
            nullability: Db2Nullability::NotNull,
        },
        span,
        value,
        encoding,
        form,
    })
}

fn selected_body(
    spelling: &str,
    span: Db2SourceSpan,
) -> Result<(Db2StringKind, &str), Db2StringConstantError> {
    use Db2StringConstantErrorCode as Code;
    let (form, prefix_len) = if spelling.starts_with('\'') {
        (Db2StringKind::Character, 0)
    } else if spelling
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("BX"))
    {
        (Db2StringKind::Binary, 2)
    } else if spelling
        .get(..1)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("X"))
    {
        (Db2StringKind::Hex, 1)
    } else if spelling.starts_with('"') {
        return Err(error(
            Code::UnsupportedDelimiter,
            span,
            "quotation-mark string context is pending",
        ));
    } else {
        return Err(error(
            Code::UnsupportedForm,
            span,
            "only apostrophe character, X and BX forms are implemented",
        ));
    };
    if spelling.as_bytes().get(prefix_len) == Some(&b'"') {
        return Err(error(
            Code::UnsupportedDelimiter,
            span,
            "quotation-mark string context is pending",
        ));
    }
    if spelling.as_bytes().get(prefix_len) != Some(&b'\'')
        || spelling.len() < prefix_len + 2
        || !spelling.ends_with('\'')
    {
        return Err(error(
            Code::InvalidSpelling,
            span,
            "literal requires adjacent prefix and complete apostrophe delimiters",
        ));
    }
    Ok((form, &spelling[prefix_len + 1..spelling.len() - 1]))
}

fn preflight_body(
    form: Db2StringKind,
    body: &str,
    mixed_data: Db2MixedData,
    span: Db2SourceSpan,
) -> Result<usize, Db2StringConstantError> {
    use Db2StringConstantErrorCode as Code;
    // The pin bounds bytes between ordinary delimiters and hex digit count,
    // independently of decoded natural type length or complete quoted spelling.
    if body.len() > MAX_STRING_BODY_BYTES {
        return Err(error(
            Code::BodyTooLarge,
            span,
            "literal body exceeds publication limit",
        ));
    }
    if form == Db2StringKind::Character {
        let mut escaped = 0;
        let mut bytes = body.bytes();
        while let Some(byte) = bytes.next() {
            if byte == b'\'' {
                if bytes.next() != Some(b'\'') {
                    return Err(error(
                        Code::InvalidSpelling,
                        span,
                        "interior apostrophe must be doubled",
                    ));
                }
                escaped += 1;
            }
        }
        Ok(body.len() - escaped)
    } else {
        if !body.len().is_multiple_of(2) || !body.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(error(
                Code::InvalidHex,
                span,
                "hex body must contain complete hexadecimal pairs",
            ));
        }
        if form == Db2StringKind::Hex
            && mixed_data == Db2MixedData::Yes
            && body.bytes().any(|byte| matches!(byte, b'a'..=b'f'))
        {
            return Err(error(
                Code::HexCaseRequiresUppercase,
                span,
                "X hex requires uppercase with MIXED DATA YES",
            ));
        }
        Ok(body.len() / 2)
    }
}

fn hex_digit(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'A'..=b'F' => byte - b'A' + 10,
        _ => byte - b'a' + 10, // preflight and existing lexer verified hex
    }
}

fn location_at(source: &str, end_byte: usize) -> Db2SourceLocation {
    let mut location = Db2SourceLocation::START;
    let mut previous_cr = false;
    for character in source[..end_byte].chars() {
        if character == '\r' || (character == '\n' && !previous_cr) {
            location.line += 1;
            location.column = 1;
        } else if character != '\n' || !previous_cr {
            location.column += 1;
        }
        previous_cr = character == '\r';
    }
    location
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Db2BuiltInDataType, Db2BuiltInType, Db2DataType, Db2TypeErrorCode, resolve_db2_type,
    };

    fn context(flag: Db2MixedData) -> Db2StringConstantContext {
        Db2StringConstantContext::UnicodeUtf8 { mixed_data: flag }
    }

    fn full_span(source: &str) -> Db2SourceSpan {
        Db2SourceSpan {
            start_byte: 0,
            end_byte: source.len(),
            start: Db2SourceLocation::START,
            end: location_at(source, source.len()),
        }
    }

    fn materialize(source: &str) -> Db2StringConstant {
        materialize_db2_string_constant(
            source,
            full_span(source),
            Db2StringConstantLimits::default(),
            context(Db2MixedData::No),
        )
        .unwrap()
    }

    fn rejected(source: &str, expected: Db2StringConstantErrorCode) {
        let error = materialize_db2_string_constant(
            source,
            full_span(source),
            Db2StringConstantLimits::default(),
            context(Db2MixedData::No),
        )
        .unwrap_err();
        assert_eq!(error.code, expected, "{source:?}");
        assert_eq!(error.span, full_span(source));
        assert!(!error.message.is_empty());
    }

    #[test]
    fn ordinary_values_preserve_case_spaces_and_decode_apostrophes_once() {
        for (source, expected) in [
            ("' MiXeD  '", " MiXeD  "),
            ("'DON''T CHANGE'", "DON'T CHANGE"),
            ("''''", "'"),
            ("''''''", "''"),
            ("'é元🙂'", "é元🙂"),
            ("'a\r\nb\rc\nd\t'", "a\r\nb\rc\nd\t"),
            ("'/*x*/--y'", "/*x*/--y"),
        ] {
            let proof = materialize(source);
            assert_eq!(
                proof.value(),
                &Db2StringConstantValue::Character(expected.into())
            );
            assert_eq!(
                proof.resolved_type().scalar(),
                &Db2ScalarType::VarChar {
                    length: expected.len() as u32
                }
            );
            assert_eq!(proof.resolved_type().nullability(), Db2Nullability::NotNull);
            assert_eq!(proof.form(), Db2StringKind::Character);
            assert_eq!(
                proof.encoding(),
                Db2StringConstantEncoding::UnicodeUtf8Mixed
            );
            assert_eq!(proof.encoding().ccsid(), Some(1208));
        }
        // Existing token payload is inherited raw escape text; only this value
        // proof decodes it. No shared literal/parser authority was changed.
        assert!(
            matches!(&lex_db2("'a''''b'", Db2SyntaxLimits::default()).unwrap().tokens()[0].kind,
            Db2TokenKind::String { value, .. } if value == "a''''b")
        );
        assert_eq!(materialize("'a''''b'").value().bytes(), b"a''b");
    }

    #[test]
    fn character_hex_is_utf8_character_data_with_actual_byte_length() {
        for (source, expected) in [
            ("X'412061'", &b"A a"[..]),
            ("x'C3a9E58583'", &b"\xc3\xa9\xe5\x85\x83"[..]),
            ("X'0031'", &b"\0\x31"[..]),
            ("X'00'", &b"\0"[..]),
            ("X'2727'", &b"''"[..]),
        ] {
            let proof = materialize(source);
            assert_eq!(proof.value().bytes(), expected);
            assert!(matches!(
                proof.value(),
                Db2StringConstantValue::Character(_)
            ));
            assert_eq!(proof.form(), Db2StringKind::Hex);
            assert_eq!(
                proof.resolved_type().scalar(),
                &Db2ScalarType::VarChar {
                    length: expected.len() as u32
                }
            );
            assert_eq!(proof.encoding().ccsid(), Some(1208));
        }
    }

    #[test]
    fn binary_hex_preserves_bytes_without_ccsid_or_character_coercion() {
        for (source, expected) in [
            ("BX'FF00c141'", &b"\xff\0\xc1A"[..]),
            ("bx'00'", &b"\0"[..]),
            ("bX'C3a9'", &b"\xc3\xa9"[..]),
            ("Bx'2727'", &b"''"[..]),
        ] {
            let proof = materialize(source);
            assert_eq!(
                proof.value(),
                &Db2StringConstantValue::Binary(expected.into())
            );
            assert_eq!(proof.form(), Db2StringKind::Binary);
            assert_eq!(
                proof.resolved_type().scalar(),
                &Db2ScalarType::VarBinary {
                    length: expected.len() as u32
                }
            );
            assert_eq!(proof.resolved_type().nullability(), Db2Nullability::NotNull);
            assert_eq!(proof.encoding(), Db2StringConstantEncoding::Binary);
            assert_eq!(proof.encoding().ccsid(), None);
        }
        rejected("X'FF'", Db2StringConstantErrorCode::InvalidUtf8InContext);
        assert_eq!(materialize("BX'FF'").value().bytes(), &[255]);
    }

    #[test]
    fn mixed_data_flag_fences_x_hex_case_without_changing_unicode_subtype() {
        for flag in [Db2MixedData::No, Db2MixedData::Yes] {
            for source in ["'abc'", "X'C3A9'", "BX'c3a9'", "bx'fF'", "X'31'"] {
                let proof = materialize_db2_string_constant(
                    source,
                    full_span(source),
                    Db2StringConstantLimits::default(),
                    context(flag),
                )
                .unwrap();
                let expected = if proof.form() == Db2StringKind::Binary {
                    Db2StringConstantEncoding::Binary
                } else {
                    Db2StringConstantEncoding::UnicodeUtf8Mixed
                };
                assert_eq!(proof.encoding(), expected);
            }
        }
        let error = materialize_db2_string_constant(
            "x'c3a9'",
            full_span("x'c3a9'"),
            Db2StringConstantLimits::default(),
            context(Db2MixedData::Yes),
        )
        .unwrap_err();
        assert_eq!(
            error.code,
            Db2StringConstantErrorCode::HexCaseRequiresUppercase
        );
        assert_eq!(materialize("x'c3a9'").value().bytes(), b"\xc3\xa9");
    }

    #[test]
    fn empty_constants_are_not_null_and_column_length_zero_remains_invalid() {
        for source in ["''", "X''", "BX''"] {
            let proof = materialize(source);
            assert!(proof.value().bytes().is_empty());
            assert_eq!(proof.resolved_type().nullability(), Db2Nullability::NotNull);
            assert_eq!(
                proof.resolved_type().scalar(),
                &if source.starts_with("BX") {
                    Db2ScalarType::VarBinary { length: 0 }
                } else {
                    Db2ScalarType::VarChar { length: 0 }
                }
            );
        }
        rejected("NULL", Db2StringConstantErrorCode::UnsupportedForm);
        for kind in [
            Db2BuiltInType::VarChar,
            Db2BuiltInType::VarBinary,
            Db2BuiltInType::Character,
            Db2BuiltInType::Binary,
        ] {
            let syntax = Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(kind, vec![0], false, Db2AstLimits::default()).unwrap(),
            );
            assert_eq!(
                resolve_db2_type(&syntax, Db2Nullability::NotNull)
                    .unwrap_err()
                    .code,
                Db2TypeErrorCode::InvalidLength
            );
        }
    }

    #[test]
    fn malformed_trailing_and_pending_families_fail_explicitly() {
        use Db2StringConstantErrorCode as Code;
        for source in [
            "'",
            "'abc",
            "'''",
            "'a'b'",
            "'a' 'b'",
            "'a';",
            "'a' --x",
            "'a' /*x*/",
            "BX",
            "X '31'",
            "BX/*x*/'31'",
        ] {
            rejected(source, Code::InvalidSpelling);
        }
        for source in [
            "X'F'",
            "BX'0'",
            "BX'GG'",
            "X'0Z'",
            "X'0 0'",
            "BX'00''00'",
            "BX'β'",
        ] {
            rejected(source, Code::InvalidHex);
        }
        for source in [
            "X'80'",
            "X'C0AF'",
            "X'EDA080'",
            "X'F4908080'",
            "X'C3'",
            "X'FF00'",
        ] {
            rejected(source, Code::InvalidUtf8InContext);
        }
        for source in [
            "N'a'",
            "G'a'",
            "UX'0041'",
            "U&'a'",
            "123",
            "-1",
            "DATE '2026-01-01'",
            "",
            " 'a'",
            "/*x*/'a'",
        ] {
            rejected(source, Code::UnsupportedForm);
        }
        for source in ["\"abc\"", "X\"31\"", "BX\"31\""] {
            rejected(source, Code::UnsupportedDelimiter);
        }
        rejected("'\0'", Code::InvalidLexerToken);
        for context in [
            Db2StringConstantContext::Ascii,
            Db2StringConstantContext::Ebcdic,
            Db2StringConstantContext::UnicodeUtf16,
        ] {
            let error = materialize_db2_string_constant(
                "'a'",
                full_span("'a'"),
                Db2StringConstantLimits::default(),
                context,
            )
            .unwrap_err();
            assert_eq!(error.code, Code::UnsupportedContext);
        }
    }

    #[test]
    fn relocated_original_utf8_crlf_spans_and_owned_results() {
        // Complete endpoints are independent fixed expectations, including a
        // multiline literal. Unrelated numeric and invalid source is not lexed.
        let proof = {
            let source = "/* é */\r\n1E3 \0 '元\r\na''b' + :hv".to_owned();
            let span = Db2SourceSpan {
                start_byte: 16,
                end_byte: 27,
                start: Db2SourceLocation { line: 2, column: 7 },
                end: Db2SourceLocation { line: 3, column: 6 },
            };
            let supplied_context = context(Db2MixedData::Yes);
            materialize_db2_string_constant(
                &source,
                span,
                Db2StringConstantLimits::default(),
                supplied_context,
            )
            .unwrap()
        };
        assert_eq!(
            proof.span(),
            Db2SourceSpan {
                start_byte: 16,
                end_byte: 27,
                start: Db2SourceLocation { line: 2, column: 7 },
                end: Db2SourceLocation { line: 3, column: 6 },
            }
        );
        assert_eq!(proof.value().bytes(), "元\r\na'b".as_bytes());
        for prefix in [
            "",
            "\r",
            "\r\n",
            "\n",
            "\n\r\n",
            "éβ\r\n",
            "-- x\r\n/*a*/ ",
            "1E3 ",
        ] {
            for literal in ["'é'", "X'00'", "bx'FF00'"] {
                let source = format!("{prefix}{literal} trailing");
                let span = Db2SourceSpan {
                    start_byte: prefix.len(),
                    end_byte: prefix.len() + literal.len(),
                    start: location_at(&source, prefix.len()),
                    end: location_at(&source, prefix.len() + literal.len()),
                };
                let proof = materialize_db2_string_constant(
                    &source,
                    span,
                    Db2StringConstantLimits::default(),
                    context(Db2MixedData::No),
                )
                .unwrap();
                assert_eq!(proof.span(), span);
                assert_eq!(proof.value(), materialize(literal).value());
            }
        }
    }

    #[test]
    fn forged_spans_are_rejected_before_selected_spelling() {
        use Db2StringConstantErrorCode as Code;
        let source = "é\r\n'a'";
        let valid = Db2SourceSpan {
            start_byte: 4,
            end_byte: 7,
            start: Db2SourceLocation { line: 2, column: 1 },
            end: Db2SourceLocation { line: 2, column: 4 },
        };
        assert!(
            materialize_db2_string_constant(
                source,
                valid,
                Db2StringConstantLimits::default(),
                context(Db2MixedData::No)
            )
            .is_ok()
        );
        for span in [
            Db2SourceSpan {
                start_byte: 1,
                ..valid
            },
            Db2SourceSpan {
                end_byte: 1,
                ..valid
            },
            Db2SourceSpan {
                start_byte: 8,
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
            Db2SourceSpan {
                start_byte: usize::MAX,
                end_byte: usize::MAX,
                ..valid
            },
        ] {
            let error = materialize_db2_string_constant(
                source,
                span,
                Db2StringConstantLimits::default(),
                context(Db2MixedData::No),
            )
            .unwrap_err();
            assert_eq!(error.code, Code::InvalidSourceSpan);
            assert_eq!(error.span, span);
        }
    }

    #[test]
    fn source_spelling_token_and_decoded_value_budgets_are_independent() {
        use Db2StringConstantErrorCode as Code;
        let source = "'a''β'"; // seven spelling bytes, four decoded bytes
        let limits = Db2StringConstantLimits {
            syntax: Db2SyntaxLimits {
                max_statement_bytes: 7,
                max_token_bytes: 7,
                max_tokens: 1,
                max_nesting: 1,
            },
            ast: Db2AstLimits {
                max_literal_bytes: 7,
                max_expression_nodes: 1,
                max_list_items: 1,
                max_expression_depth: 1,
                ..Db2AstLimits::default()
            },
            max_value_bytes: 4,
        };
        assert_eq!(
            materialize_db2_string_constant(
                source,
                full_span(source),
                limits,
                context(Db2MixedData::No)
            )
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
                materialize_db2_string_constant(
                    source,
                    full_span(source),
                    changed,
                    context(Db2MixedData::No)
                )
                .unwrap_err()
                .code,
                expected
            );
        }
        let source = "'a' surrounding";
        let mut limits = Db2StringConstantLimits::default();
        limits.syntax.max_statement_bytes = source.len();
        assert!(
            materialize_db2_string_constant(
                source,
                full_span("'a'"),
                limits,
                context(Db2MixedData::No)
            )
            .is_ok()
        );
        limits.syntax.max_statement_bytes -= 1;
        assert_eq!(
            materialize_db2_string_constant(
                source,
                full_span("'a'"),
                limits,
                context(Db2MixedData::No)
            )
            .unwrap_err()
            .code,
            Code::SourceTooLarge
        );
        assert_eq!(
            materialize_db2_string_constant(
                "BX'00'",
                full_span("BX'00'"),
                Db2StringConstantLimits {
                    max_value_bytes: 1,
                    ..Default::default()
                },
                context(Db2MixedData::No)
            )
            .unwrap()
            .value()
            .bytes(),
            &[0]
        );
        assert_eq!(
            materialize_db2_string_constant(
                "BX'0000'",
                full_span("BX'0000'"),
                Db2StringConstantLimits {
                    max_value_bytes: 1,
                    ..Default::default()
                },
                context(Db2MixedData::No)
            )
            .unwrap_err()
            .code,
            Code::ValueTooLarge
        );
    }

    #[test]
    fn publication_limits_use_body_bytes_and_hex_digits_not_natural_length() {
        use Db2StringConstantErrorCode as Code;
        let ordinary = format!("'{}'", "a".repeat(32704));
        assert_eq!(
            materialize(&ordinary).resolved_type().scalar(),
            &Db2ScalarType::VarChar { length: 32704 }
        );
        rejected(&format!("'{}'", "a".repeat(32705)), Code::BodyTooLarge);
        let unicode = format!("'{}'", "é".repeat(16352));
        assert_eq!(materialize(&unicode).value().bytes().len(), 32704);
        rejected(&format!("'{}a'", "é".repeat(16352)), Code::BodyTooLarge);
        let escaped = format!("'{}'", "''".repeat(16352));
        assert_eq!(materialize(&escaped).value().bytes(), vec![b'\''; 16352]);
        for prefix in ["X", "BX"] {
            let spelling = format!("{prefix}'{}'", "00".repeat(16352));
            let proof = materialize(&spelling);
            assert_eq!(proof.value().bytes(), vec![0; 16352]);
            assert_eq!(
                proof.resolved_type().scalar(),
                &if prefix == "X" {
                    Db2ScalarType::VarChar { length: 16352 }
                } else {
                    Db2ScalarType::VarBinary { length: 16352 }
                }
            );
            rejected(
                &format!("{prefix}'{}0'", "00".repeat(16352)),
                Code::BodyTooLarge,
            );
            rejected(
                &format!("{prefix}'{}'", "00".repeat(16353)),
                Code::BodyTooLarge,
            );
        }
    }

    #[test]
    fn zero_and_one_beyond_compiled_limits_fail_before_source_processing() {
        use Db2StringConstantErrorCode as Code;
        let defaults = Db2StringConstantLimits::default();
        let syntax_cases = [
            Db2SyntaxLimits {
                max_statement_bytes: 0,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_statement_bytes: 8 * 1024 * 1024 + 1,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_tokens: 0,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_tokens: 262145,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_token_bytes: 0,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_token_bytes: 1024 * 1024 + 1,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_nesting: 0,
                ..defaults.syntax
            },
            Db2SyntaxLimits {
                max_nesting: 1025,
                ..defaults.syntax
            },
        ];
        let ast_cases = [
            Db2AstLimits {
                max_identifier_bytes: 0,
                ..defaults.ast
            },
            Db2AstLimits {
                max_identifier_bytes: 1025,
                ..defaults.ast
            },
            Db2AstLimits {
                max_name_parts: 0,
                ..defaults.ast
            },
            Db2AstLimits {
                max_name_parts: 17,
                ..defaults.ast
            },
            Db2AstLimits {
                max_literal_bytes: 0,
                ..defaults.ast
            },
            Db2AstLimits {
                max_literal_bytes: 8 * 1024 * 1024 + 1,
                ..defaults.ast
            },
            Db2AstLimits {
                max_expression_nodes: 0,
                ..defaults.ast
            },
            Db2AstLimits {
                max_expression_nodes: 262145,
                ..defaults.ast
            },
            Db2AstLimits {
                max_list_items: 0,
                ..defaults.ast
            },
            Db2AstLimits {
                max_list_items: 65537,
                ..defaults.ast
            },
            Db2AstLimits {
                max_expression_depth: 0,
                ..defaults.ast
            },
            Db2AstLimits {
                max_expression_depth: 1025,
                ..defaults.ast
            },
        ];
        for limits in syntax_cases
            .into_iter()
            .map(|syntax| Db2StringConstantLimits { syntax, ..defaults })
            .chain(
                ast_cases
                    .into_iter()
                    .map(|ast| Db2StringConstantLimits { ast, ..defaults }),
            )
            .chain([
                Db2StringConstantLimits {
                    max_value_bytes: 0,
                    ..defaults
                },
                Db2StringConstantLimits {
                    max_value_bytes: 32705,
                    ..defaults
                },
            ])
        {
            assert_eq!(
                materialize_db2_string_constant(
                    "not a literal",
                    full_span("not a literal"),
                    limits,
                    context(Db2MixedData::No)
                )
                .unwrap_err()
                .code,
                Code::InvalidLimits
            );
        }
        let ceilings = Db2StringConstantLimits {
            syntax: Db2SyntaxLimits {
                max_statement_bytes: 8 * 1024 * 1024,
                max_tokens: 262144,
                max_token_bytes: 1024 * 1024,
                max_nesting: 1024,
            },
            ast: Db2AstLimits {
                max_identifier_bytes: 1024,
                max_name_parts: 16,
                max_literal_bytes: 8 * 1024 * 1024,
                max_expression_nodes: 262144,
                max_list_items: 65536,
                max_expression_depth: 1024,
            },
            max_value_bytes: 32704,
        };
        let source = format!("'a'{}", " ".repeat(8 * 1024 * 1024 - 3));
        assert!(
            materialize_db2_string_constant(
                &source,
                full_span("'a'"),
                ceilings,
                context(Db2MixedData::No)
            )
            .is_ok()
        );
        let beyond = format!("{source} ");
        assert_eq!(
            materialize_db2_string_constant(
                &beyond,
                full_span("'a'"),
                ceilings,
                context(Db2MixedData::No)
            )
            .unwrap_err()
            .code,
            Code::SourceTooLarge
        );
    }
}
