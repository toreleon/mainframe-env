//! Bounded storage assignment from the existing opaque natural literal proof.
//!
//! Source baseline: ibm-db2-for-zos-13-2026-08-13, SSEPEK_13.0.0/sqlref/src/tpc/:
//! - db2z_stringassignmentintro.html, 12061 bytes,
//!   a72f1dd0d80391b8ef20100313bfca498dc881b9881fbb49beb9177263e7f46e;
//! - db2z_constantsintro.html, 17915 bytes,
//!   bf0cb79eac0636348209b6919c4f4ee680f1c3d2ada9cab186dddfdd39a13fa8;
//! - db2z_charstrings.html, 17888 bytes,
//!   54c3f8ec1620479ffb002788e198c788050d8616e1ee4fbac431fb45bb59d835;
//! - db2z_binarystringsintro.html, 3946 bytes,
//!   69af1d1645cf137f58d372fe93589e1f299a76937c4e9d1284ad1aa6e2643036;
//! - db2z_assignmentandcomparison.html, 40605 bytes,
//!   2cc975449a25d1d825cbd8a6551f5ea9c6dfc6ed5dc9956643f0e1dc9948af8a.
//!
//! Language elements have no standalone statement row. Storage rules are at
//! assignment-topic lines 11–12 and 19–23; conversion context at 40–51. This
//! preparation kernel establishes neither default legality nor a durable target
//! policy identity. Retrieval, host normalization, SQLCA, cells and execution
//! remain separately owned obligations. Other ordinary families remain required
//! implementation; explicit unsupported errors do not defer their acceptance.

use super::super::{
    Db2AssignmentCompatibility, Db2ResolvedType, Db2ScalarType, MAX_BINARY_LENGTH,
    MAX_CHARACTER_LENGTH, MAX_VARBINARY_LENGTH, MAX_VARCHAR_LENGTH, classify_db2_assignment,
};
use super::{
    Db2StringConstant, Db2StringConstantEncoding, Db2StringConstantValue, MAX_STRING_BODY_BYTES,
};
use crate::Db2SourceSpan;
use std::fmt;

/// Caller-selected target interpretation. It is not inferred from the resolved
/// type (which has no CCSID), host encoding, source bytes or a catalog identity.
/// Only UnicodeUtf8Mixed character and Binary targets are implemented here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2StringStorageContext {
    /// Character CCSID 1208/MIXED, independent of installation MIXED DATA.
    UnicodeUtf8Mixed,
    /// Binary data, with no CCSID; never character FOR BIT DATA.
    Binary,
    UnicodeUtf16,
    Ascii,
    Ebcdic,
    BitData,
}

/// Explicit output budget, from 1 through 32704 bytes. A varying output may be
/// empty; the budget itself must be positive. No implicit budget is selected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Db2StringStorageLimits {
    pub max_output_bytes: usize,
}

/// Opaque owned source/target/value pairing, constructed only by storage
/// assignment. The target is the exact supplied resolved type, including its
/// nullability, and the preparation context is retained independently of it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2StoredStringConstant {
    source: Db2StringConstant,
    target_type: Db2ResolvedType,
    context: Db2StringStorageContext,
    value: Db2StringConstantValue,
    compatibility: Db2AssignmentCompatibility,
    padded_bytes: usize,
    truncated_bytes: usize,
}

impl Db2StoredStringConstant {
    #[must_use]
    pub const fn source(&self) -> &Db2StringConstant {
        &self.source
    }

    #[must_use]
    pub const fn resolved_type(&self) -> &Db2ResolvedType {
        &self.target_type
    }

    #[must_use]
    pub const fn span(&self) -> Db2SourceSpan {
        self.source.span()
    }

    #[must_use]
    pub const fn context(&self) -> Db2StringStorageContext {
        self.context
    }

    #[must_use]
    pub const fn value(&self) -> &Db2StringConstantValue {
        &self.value
    }

    #[must_use]
    pub const fn compatibility(&self) -> Db2AssignmentCompatibility {
        self.compatibility
    }

    /// Observed right-padding byte count, not a host or SQLCA status.
    #[must_use]
    pub const fn padded_bytes(&self) -> usize {
        self.padded_bytes
    }

    /// Observed excess ASCII blanks removed under character storage rules.
    /// This is not retrieval truncation or a SQLCA warning claim.
    #[must_use]
    pub const fn truncated_bytes(&self) -> usize {
        self.truncated_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2StringStorageErrorCode {
    InvalidLimits,
    IncompatibleTypes,
    UnsupportedNumericTarget,
    UnsupportedGraphicTarget,
    UnsupportedDatetimeTarget,
    TargetEncodingMismatch,
    UnsupportedCharacterConversion,
    UnsupportedBitData,
    InvalidTargetLength,
    NonBlankExcess,
    BinaryTooLong,
    OutputTooLarge,
}

/// Fixed-size located failure; no source payload or allocated message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2StringStorageError {
    pub code: Db2StringStorageErrorCode,
    pub span: Db2SourceSpan,
    pub message: &'static str,
}

impl fmt::Display for Db2StringStorageError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "{:?} at {}:{}: {}",
            self.code, self.span.start.line, self.span.start.column, self.message
        )
    }
}

impl std::error::Error for Db2StringStorageError {}

/// Assign an opaque literal proof to an existing validated declared target.
/// Compatibility is classified by the existing type authority before supported
/// conversions are selected. Context-dependent datetime conversion is pending,
/// rather than mislabeled fundamental incompatibility. Bounds, context and all
/// excess bytes are checked before allocating output or cloning owned proofs.
pub fn store_db2_string_constant(
    source: &Db2StringConstant,
    target: &Db2ResolvedType,
    context: Db2StringStorageContext,
    limits: Db2StringStorageLimits,
) -> Result<Db2StoredStringConstant, Db2StringStorageError> {
    use Db2StringStorageContext as Context;
    use Db2StringStorageErrorCode as Code;
    let failure = |code, message| Db2StringStorageError {
        code,
        span: source.span(),
        message,
    };
    if limits.max_output_bytes == 0 || limits.max_output_bytes > MAX_STRING_BODY_BYTES {
        return Err(failure(
            Code::InvalidLimits,
            "invalid storage output byte budget",
        ));
    }
    let compatibility = classify_db2_assignment(source.resolved_type(), target);
    if compatibility == Db2AssignmentCompatibility::Incompatible {
        return Err(failure(
            Code::IncompatibleTypes,
            "literal and target types are incompatible",
        ));
    }
    let (length, maximum, fixed, encoding) = match target.scalar() {
        Db2ScalarType::Character { length } => (
            *length,
            MAX_CHARACTER_LENGTH,
            true,
            Db2StringConstantEncoding::UnicodeUtf8Mixed,
        ),
        Db2ScalarType::VarChar { length } => (
            *length,
            MAX_VARCHAR_LENGTH,
            false,
            Db2StringConstantEncoding::UnicodeUtf8Mixed,
        ),
        Db2ScalarType::Binary { length } => (
            *length,
            MAX_BINARY_LENGTH,
            true,
            Db2StringConstantEncoding::Binary,
        ),
        Db2ScalarType::VarBinary { length } => (
            *length,
            MAX_VARBINARY_LENGTH,
            false,
            Db2StringConstantEncoding::Binary,
        ),
        Db2ScalarType::Graphic { .. } | Db2ScalarType::VarGraphic { .. } => {
            return Err(failure(
                Code::UnsupportedGraphicTarget,
                "graphic storage conversion is not implemented",
            ));
        }
        Db2ScalarType::Date | Db2ScalarType::Time | Db2ScalarType::Timestamp { .. } => {
            return Err(failure(
                Code::UnsupportedDatetimeTarget,
                "context-dependent datetime conversion is not implemented",
            ));
        }
        _ => {
            return Err(failure(
                Code::UnsupportedNumericTarget,
                "compatible numeric conversion is not implemented",
            ));
        }
    };
    // Natural empty VARCHAR/VARBINARY proofs have length zero, but cannot serve
    // as a declared target. Reuse the type owner's fixed/varying maxima.
    if length == 0 || length > maximum {
        return Err(failure(
            Code::InvalidTargetLength,
            "declared target length is outside its type bounds",
        ));
    }
    match (encoding, context) {
        (Db2StringConstantEncoding::UnicodeUtf8Mixed, Context::UnicodeUtf8Mixed)
        | (Db2StringConstantEncoding::Binary, Context::Binary) => {}
        (Db2StringConstantEncoding::UnicodeUtf8Mixed, Context::BitData) => {
            return Err(failure(
                Code::UnsupportedBitData,
                "character FOR BIT DATA storage is not implemented",
            ));
        }
        (
            Db2StringConstantEncoding::UnicodeUtf8Mixed,
            Context::UnicodeUtf16 | Context::Ascii | Context::Ebcdic,
        ) => {
            return Err(failure(
                Code::UnsupportedCharacterConversion,
                "target character encoding conversion is not implemented",
            ));
        }
        _ => {
            return Err(failure(
                Code::TargetEncodingMismatch,
                "target family and explicit encoding do not match",
            ));
        }
    }
    let capacity = length as usize;
    let bytes = source.value().bytes();
    let retained = bytes.len().min(capacity);
    let truncated_bytes = bytes.len() - retained;
    if truncated_bytes != 0 {
        if encoding == Db2StringConstantEncoding::Binary {
            return Err(failure(
                Code::BinaryTooLong,
                "binary storage value exceeds target length",
            ));
        }
        if !bytes[retained..].iter().all(|byte| *byte == b' ') {
            return Err(failure(
                Code::NonBlankExcess,
                "character storage excess contains a nonblank byte",
            ));
        }
    }
    let required = if fixed { capacity } else { retained };
    if required > limits.max_output_bytes {
        return Err(failure(
            Code::OutputTooLarge,
            "assigned storage value exceeds output byte budget",
        ));
    }
    let padded_bytes = required - retained;
    let value = match source.value() {
        Db2StringConstantValue::Character(text) => {
            // All removed bytes are ASCII blanks. This independently guards
            // the slice boundary; a UTF-8 continuation byte cannot be a blank.
            if !text.is_char_boundary(retained) {
                return Err(failure(
                    Code::NonBlankExcess,
                    "target length splits a UTF-8 character",
                ));
            }
            let mut value = String::with_capacity(required);
            value.push_str(&text[..retained]);
            value.extend(std::iter::repeat_n(' ', padded_bytes));
            Db2StringConstantValue::Character(value)
        }
        Db2StringConstantValue::Binary(bytes) => {
            let mut value = Vec::with_capacity(required);
            value.extend_from_slice(bytes);
            value.resize(required, 0);
            Db2StringConstantValue::Binary(value)
        }
    };
    Ok(Db2StoredStringConstant {
        source: source.clone(),
        target_type: target.clone(),
        context,
        value,
        compatibility,
        padded_bytes,
        truncated_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{
        Db2MixedData, Db2StringConstantContext, Db2StringConstantLimits,
        materialize_db2_string_constant,
    };
    use super::*;
    use crate::{
        Db2BuiltInDataType, Db2BuiltInType as Kind, Db2DataType, Db2Nullability, Db2SourceLocation,
        Db2TypeErrorCode, resolve_db2_type,
    };

    const UTF8: Db2StringStorageContext = Db2StringStorageContext::UnicodeUtf8Mixed;
    const BINARY: Db2StringStorageContext = Db2StringStorageContext::Binary;
    const LIMITS: Db2StringStorageLimits = Db2StringStorageLimits {
        max_output_bytes: 32704,
    };

    fn literal(spelling: &str) -> Db2StringConstant {
        let span = Db2SourceSpan {
            start_byte: 0,
            end_byte: spelling.len(),
            start: Db2SourceLocation::START,
            end: Db2SourceLocation {
                line: 1,
                column: spelling.chars().count() as u32 + 1,
            },
        };
        materialize_db2_string_constant(
            spelling,
            span,
            Db2StringConstantLimits::default(),
            Db2StringConstantContext::UnicodeUtf8 {
                mixed_data: Db2MixedData::No,
            },
        )
        .unwrap()
    }

    fn target(kind: Kind, arguments: &[u32], nullability: Db2Nullability) -> Db2ResolvedType {
        resolve_db2_type(
            &Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(kind, arguments.to_vec(), false, Default::default())
                    .unwrap(),
            ),
            nullability,
        )
        .unwrap()
    }

    fn stored(
        spelling: &str,
        kind: Kind,
        length: u32,
        context: Db2StringStorageContext,
    ) -> Db2StoredStringConstant {
        store_db2_string_constant(
            &literal(spelling),
            &target(kind, &[length], Db2Nullability::NotNull),
            context,
            LIMITS,
        )
        .unwrap()
    }

    fn rejected(
        spelling: &str,
        kind: Kind,
        arguments: &[u32],
        context: Db2StringStorageContext,
        code: Db2StringStorageErrorCode,
    ) {
        let source = literal(spelling);
        let before = source.clone();
        let target = target(kind, arguments, Db2Nullability::NotNull);
        let target_before = target.clone();
        let error = store_db2_string_constant(&source, &target, context, LIMITS).unwrap_err();
        assert_eq!(error.code, code, "{spelling} {kind:?} {context:?}");
        assert_eq!(error.span, source.span());
        assert!(!error.message.is_empty());
        assert_eq!(source, before);
        assert_eq!(target, target_before);
    }

    #[test]
    fn character_storage_fixed_and_varying_vectors_preserve_exact_bytes() {
        for (spelling, kind, length, expected, padded, truncated) in [
            ("'Ab'", Kind::Character, 5, "Ab   ", 3, 0),
            ("' A''b  '", Kind::Character, 6, " A'b  ", 0, 0),
            ("'A  '", Kind::Character, 2, "A ", 0, 1),
            ("'A  '", Kind::VarChar, 1, "A", 0, 2),
            ("' A b '", Kind::VarChar, 12, " A b ", 0, 0),
            ("''", Kind::Character, 1, " ", 1, 0),
            ("X''", Kind::VarChar, 1, "", 0, 0),
            ("'   '", Kind::VarChar, 1, " ", 0, 2),
            ("X'00612020'", Kind::Character, 3, "\0a ", 0, 1),
            ("X'0061'", Kind::Character, 4, "\0a  ", 2, 0),
            ("X'006120'", Kind::VarChar, 9, "\0a ", 0, 0),
        ] {
            let result = stored(spelling, kind, length, UTF8);
            assert_eq!(
                result.value(),
                &Db2StringConstantValue::Character(expected.into())
            );
            assert_eq!(result.value().bytes(), expected.as_bytes());
            assert_eq!(result.context(), UTF8);
            assert_eq!(result.padded_bytes(), padded);
            assert_eq!(result.truncated_bytes(), truncated);
            assert_eq!(result.source(), &literal(spelling));
            assert_eq!(result.source().encoding().ccsid(), Some(1208));
        }
    }

    #[test]
    fn character_excess_must_be_ascii_blanks_in_every_byte() {
        use Db2StringStorageErrorCode::NonBlankExcess;
        for kind in [Kind::Character, Kind::VarChar] {
            for spelling in [
                "'AB'",
                "'A B'",
                "'A\t'",
                "X'410A'",
                "'A\u{a0}'",
                "X'4100'",
                "X'41200020'",
            ] {
                rejected(spelling, kind, &[1], UTF8, NonBlankExcess);
            }
            rejected("'A　'", kind, &[1], UTF8, NonBlankExcess);
        }
    }

    #[test]
    fn utf8_storage_uses_byte_lengths_and_never_splits_characters() {
        for kind in [Kind::Character, Kind::VarChar] {
            assert_eq!(stored("'é  '", kind, 2, UTF8).value().bytes(), b"\xc3\xa9");
            assert_eq!(stored("'é '", kind, 3, UTF8).value().bytes(), b"\xc3\xa9 ");
            assert_eq!(
                stored("'界  '", kind, 3, UTF8).value().bytes(),
                b"\xe7\x95\x8c"
            );
            assert_eq!(
                stored("'😀 '", kind, 4, UTF8).value().bytes(),
                b"\xf0\x9f\x98\x80"
            );
            for (spelling, length) in [("'é '", 1), ("'界 '", 2), ("'😀 '", 3), ("'Aé  '", 2)]
            {
                rejected(
                    spelling,
                    kind,
                    &[length],
                    UTF8,
                    Db2StringStorageErrorCode::NonBlankExcess,
                );
            }
        }
        assert_eq!(
            stored("'é'", Kind::Character, 4, UTF8).value().bytes(),
            b"\xc3\xa9  "
        );
        assert_eq!(
            stored("'é'", Kind::VarChar, 4, UTF8).value().bytes(),
            b"\xc3\xa9"
        );
    }

    #[test]
    fn binary_storage_vectors_pad_only_fixed_targets_and_preserve_nuls() {
        for (spelling, kind, length, expected, padded) in [
            (
                "BX'fF00c1'",
                Kind::Binary,
                5,
                &b"\xff\x00\xc1\x00\x00"[..],
                2,
            ),
            ("BX'002000'", Kind::VarBinary, 5, &b"\x00\x20\x00"[..], 0),
            ("BX''", Kind::Binary, 1, &b"\x00"[..], 1),
            ("BX''", Kind::VarBinary, 1, &b""[..], 0),
            ("BX'0000'", Kind::Binary, 2, &b"\x00\x00"[..], 0),
        ] {
            let result = stored(spelling, kind, length, BINARY);
            assert_eq!(
                result.value(),
                &Db2StringConstantValue::Binary(expected.to_vec())
            );
            assert_eq!(result.value().bytes(), expected);
            assert_eq!(result.padded_bytes(), padded);
            assert_eq!(result.truncated_bytes(), 0);
            assert_eq!(result.context(), BINARY);
            assert_eq!(result.source().encoding().ccsid(), None);
        }
        for kind in [Kind::Binary, Kind::VarBinary] {
            for spelling in ["BX'0000'", "BX'4120'", "BX'4100'", "BX'FF00'"] {
                rejected(
                    spelling,
                    kind,
                    &[1],
                    BINARY,
                    Db2StringStorageErrorCode::BinaryTooLong,
                );
            }
        }
    }

    #[test]
    fn x_character_and_bx_binary_families_never_coerce_each_other() {
        use Db2StringStorageErrorCode::IncompatibleTypes;
        for kind in [Kind::Binary, Kind::VarBinary] {
            for spelling in ["'A'", "X'41'", "X'00'", "''"] {
                rejected(spelling, kind, &[1], BINARY, IncompatibleTypes);
            }
        }
        for kind in [
            Kind::Character,
            Kind::VarChar,
            Kind::Graphic,
            Kind::VarGraphic,
        ] {
            for spelling in ["BX'41'", "BX'00'", "BX''"] {
                rejected(spelling, kind, &[1], UTF8, IncompatibleTypes);
            }
        }
    }

    #[test]
    fn explicit_encoding_mismatch_and_pending_conversion_are_distinct() {
        use Db2StringStorageContext as Context;
        use Db2StringStorageErrorCode as Code;
        for kind in [Kind::Character, Kind::VarChar] {
            for spelling in ["'A'", "''", "X'00'"] {
                rejected(spelling, kind, &[1], BINARY, Code::TargetEncodingMismatch);
                rejected(
                    spelling,
                    kind,
                    &[1],
                    Context::BitData,
                    Code::UnsupportedBitData,
                );
                for context in [Context::UnicodeUtf16, Context::Ascii, Context::Ebcdic] {
                    rejected(
                        spelling,
                        kind,
                        &[1],
                        context,
                        Code::UnsupportedCharacterConversion,
                    );
                }
            }
        }
        for kind in [Kind::Binary, Kind::VarBinary] {
            for context in [
                UTF8,
                Context::UnicodeUtf16,
                Context::Ascii,
                Context::Ebcdic,
                Context::BitData,
            ] {
                rejected("BX''", kind, &[1], context, Code::TargetEncodingMismatch);
            }
        }
    }

    #[test]
    fn compatible_unimplemented_targets_do_not_claim_fundamental_incompatibility() {
        use Db2StringStorageErrorCode as Code;
        for (kind, arguments, code) in [
            (Kind::SmallInt, &[][..], Code::UnsupportedNumericTarget),
            (Kind::Integer, &[][..], Code::UnsupportedNumericTarget),
            (Kind::BigInt, &[][..], Code::UnsupportedNumericTarget),
            (Kind::Decimal, &[31, 31][..], Code::UnsupportedNumericTarget),
            (Kind::Float, &[53][..], Code::UnsupportedNumericTarget),
            (Kind::Real, &[][..], Code::UnsupportedNumericTarget),
            (Kind::Double, &[][..], Code::UnsupportedNumericTarget),
            (Kind::DecFloat, &[34][..], Code::UnsupportedNumericTarget),
            (Kind::Graphic, &[1][..], Code::UnsupportedGraphicTarget),
            (Kind::VarGraphic, &[1][..], Code::UnsupportedGraphicTarget),
            (Kind::Date, &[][..], Code::UnsupportedDatetimeTarget),
            (Kind::Time, &[][..], Code::UnsupportedDatetimeTarget),
            (Kind::Timestamp, &[12][..], Code::UnsupportedDatetimeTarget),
        ] {
            rejected("'1'", kind, arguments, UTF8, code);
            rejected("BX'31'", kind, arguments, BINARY, Code::IncompatibleTypes);
        }
    }

    #[test]
    fn target_type_nullability_and_source_proof_are_retained_exactly() {
        use crate::{Db2AssignmentNullability as Nulls, Db2ConversionKind as Conversion};
        for nullability in [Db2Nullability::NotNull, Db2Nullability::Nullable] {
            for (spelling, kind, context, conversion) in [
                ("'A'", Kind::Character, UTF8, Conversion::CharacterString),
                ("'A'", Kind::VarChar, UTF8, Conversion::Identity),
                ("BX'41'", Kind::Binary, BINARY, Conversion::BinaryString),
                ("BX'41'", Kind::VarBinary, BINARY, Conversion::Identity),
            ] {
                let source = literal(spelling);
                let target = target(kind, &[1], nullability);
                let before = target.clone();
                let result = store_db2_string_constant(&source, &target, context, LIMITS).unwrap();
                assert_eq!(result.resolved_type(), &before);
                assert_eq!(target, before);
                assert_eq!(result.source(), &source);
                assert_eq!(
                    result.source().resolved_type().nullability(),
                    Db2Nullability::NotNull
                );
                assert_eq!(result.resolved_type().nullability(), nullability);
                assert_eq!(
                    result.compatibility(),
                    Db2AssignmentCompatibility::Compatible {
                        conversion,
                        nullability: Nulls::Safe,
                    }
                );
            }
        }
    }

    #[test]
    fn natural_zero_length_types_cannot_be_used_as_declared_targets() {
        for (spelling, context) in [("''", UTF8), ("X''", UTF8), ("BX''", BINARY)] {
            let source = literal(spelling);
            let error = store_db2_string_constant(&source, source.resolved_type(), context, LIMITS)
                .unwrap_err();
            assert_eq!(error.code, Db2StringStorageErrorCode::InvalidTargetLength);
            assert_eq!(error.span, source.span());
        }
        for kind in [
            Kind::Character,
            Kind::VarChar,
            Kind::Binary,
            Kind::VarBinary,
        ] {
            let syntax = Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(kind, vec![0], false, Default::default()).unwrap(),
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
    fn fixed255_varying32704_limits_and_one_beyond_are_enforced() {
        for (spelling, kind, context, expected) in [
            ("''", Kind::Character, UTF8, vec![32; 255]),
            ("BX''", Kind::Binary, BINARY, vec![0; 255]),
        ] {
            let result = stored(spelling, kind, 255, context);
            assert_eq!(result.value().bytes(), expected);
            assert_eq!(result.padded_bytes(), 255);
            let syntax = Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(kind, vec![256], false, Default::default()).unwrap(),
            );
            assert_eq!(
                resolve_db2_type(&syntax, Db2Nullability::NotNull)
                    .unwrap_err()
                    .code,
                Db2TypeErrorCode::InvalidLength
            );
        }
        let exact_character = format!("'{}'", "A".repeat(255));
        let exact_binary = format!("BX'{}'", "FF".repeat(255));
        assert_eq!(
            stored(&exact_character, Kind::Character, 255, UTF8)
                .value()
                .bytes(),
            &[65; 255]
        );
        assert_eq!(
            stored(&exact_binary, Kind::Binary, 255, BINARY)
                .value()
                .bytes(),
            &[255; 255]
        );
        rejected(
            &format!("'{}'", "A".repeat(256)),
            Kind::Character,
            &[255],
            UTF8,
            Db2StringStorageErrorCode::NonBlankExcess,
        );
        let one_blank_excess = format!("'{} '", "A".repeat(255));
        let result = stored(&one_blank_excess, Kind::Character, 255, UTF8);
        assert_eq!(result.value().bytes(), &[65; 255]);
        assert_eq!(result.truncated_bytes(), 1);
        for byte in ["FF", "00"] {
            rejected(
                &format!("BX'{}'", byte.repeat(256)),
                Kind::Binary,
                &[255],
                BINARY,
                Db2StringStorageErrorCode::BinaryTooLong,
            );
        }
        let text = format!("'{}'", "A".repeat(32704));
        let result = stored(&text, Kind::VarChar, 32704, UTF8);
        assert_eq!(result.value().bytes(), &[65; 32704]);
        assert_eq!(result.truncated_bytes(), 0);
        rejected(
            &text,
            Kind::VarChar,
            &[32703],
            UTF8,
            Db2StringStorageErrorCode::NonBlankExcess,
        );
        let blanks = format!("'A{}'", " ".repeat(32703));
        let result = stored(&blanks, Kind::VarChar, 1, UTF8);
        assert_eq!(result.value().bytes(), b"A");
        assert_eq!(result.truncated_bytes(), 32703);
        // BX's existing 32704-hex-digit cap permits 16352 decoded bytes.
        let binary = format!("BX'{}'", "FF".repeat(16352));
        let result = stored(&binary, Kind::VarBinary, 32704, BINARY);
        assert_eq!(result.value().bytes(), &[255; 16352]);
        assert_eq!(result.padded_bytes(), 0);
        rejected(
            &binary,
            Kind::VarBinary,
            &[16351],
            BINARY,
            Db2StringStorageErrorCode::BinaryTooLong,
        );
        for kind in [Kind::VarChar, Kind::VarBinary] {
            let syntax = Db2DataType::BuiltIn(
                Db2BuiltInDataType::new(kind, vec![32705], false, Default::default()).unwrap(),
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
    fn explicit_output_budget_preflights_padding_and_actual_varying_length() {
        for (spelling, kind, length, context, exact) in [
            ("'A'", Kind::Character, 255, UTF8, 255),
            ("BX'41'", Kind::Binary, 255, BINARY, 255),
            ("'AB'", Kind::VarChar, 32704, UTF8, 2),
            ("BX'4100'", Kind::VarBinary, 32704, BINARY, 2),
            ("'A  '", Kind::VarChar, 1, UTF8, 1),
        ] {
            let source = literal(spelling);
            let target = target(kind, &[length], Db2Nullability::Nullable);
            let result = store_db2_string_constant(
                &source,
                &target,
                context,
                Db2StringStorageLimits {
                    max_output_bytes: exact,
                },
            )
            .unwrap();
            assert_eq!(result.value().bytes().len(), exact);
            if exact > 1 {
                let error = store_db2_string_constant(
                    &source,
                    &target,
                    context,
                    Db2StringStorageLimits {
                        max_output_bytes: exact - 1,
                    },
                )
                .unwrap_err();
                assert_eq!(error.code, Db2StringStorageErrorCode::OutputTooLarge);
                assert_eq!(error.span, source.span());
            }
            for budget in [0, 32705, usize::MAX] {
                let error = store_db2_string_constant(
                    &source,
                    &target,
                    context,
                    Db2StringStorageLimits {
                        max_output_bytes: budget,
                    },
                )
                .unwrap_err();
                assert_eq!(error.code, Db2StringStorageErrorCode::InvalidLimits);
                assert_eq!(error.span, source.span());
            }
        }
        let source = literal(&format!("'{}'", "A".repeat(32704)));
        let declared = target(Kind::VarChar, &[32704], Db2Nullability::NotNull);
        assert_eq!(
            store_db2_string_constant(&source, &declared, UTF8, LIMITS)
                .unwrap()
                .value()
                .bytes(),
            &[65; 32704]
        );
        assert_eq!(
            store_db2_string_constant(
                &source,
                &declared,
                UTF8,
                Db2StringStorageLimits {
                    max_output_bytes: 32703
                }
            )
            .unwrap_err()
            .code,
            Db2StringStorageErrorCode::OutputTooLarge
        );
        assert_eq!(
            stored("''", Kind::VarChar, 32704, UTF8).value().bytes(),
            b""
        );
        for (spelling, kind, context) in [
            ("''", Kind::VarChar, UTF8),
            ("BX''", Kind::VarBinary, BINARY),
        ] {
            let result = store_db2_string_constant(
                &literal(spelling),
                &target(kind, &[32704], Db2Nullability::NotNull),
                context,
                Db2StringStorageLimits {
                    max_output_bytes: 1,
                },
            )
            .unwrap();
            assert!(result.value().bytes().is_empty());
            assert_eq!(result.padded_bytes(), 0);
        }
    }

    #[test]
    fn owned_source_target_context_values_and_located_errors_survive_input_scope() {
        let (result, error) = {
            let source = String::from("-- é\r\n  '界\r\nA  ' trailing SQL");
            let span = Db2SourceSpan {
                start_byte: 9,
                end_byte: 19,
                start: Db2SourceLocation { line: 2, column: 3 },
                end: Db2SourceLocation { line: 3, column: 5 },
            };
            let proof = materialize_db2_string_constant(
                &source,
                span,
                Default::default(),
                Db2StringConstantContext::UnicodeUtf8 {
                    mixed_data: Db2MixedData::No,
                },
            )
            .unwrap();
            let declared = target(Kind::Character, &[6], Db2Nullability::Nullable);
            let context = UTF8;
            let result = store_db2_string_constant(&proof, &declared, context, LIMITS).unwrap();
            let error = store_db2_string_constant(
                &proof,
                &target(Kind::VarChar, &[5], Db2Nullability::NotNull),
                context,
                LIMITS,
            )
            .unwrap_err();
            (result.clone(), error)
        };
        assert_eq!(
            result.span(),
            Db2SourceSpan {
                start_byte: 9,
                end_byte: 19,
                start: Db2SourceLocation { line: 2, column: 3 },
                end: Db2SourceLocation { line: 3, column: 5 },
            }
        );
        assert_eq!(result.source().value().bytes(), b"\xe7\x95\x8c\r\nA  ");
        assert_eq!(
            result.source().resolved_type().scalar(),
            &Db2ScalarType::VarChar { length: 8 }
        );
        assert_eq!(
            result.source().resolved_type().nullability(),
            Db2Nullability::NotNull
        );
        assert_eq!(result.value().bytes(), b"\xe7\x95\x8c\r\nA");
        assert_eq!(
            result.resolved_type().scalar(),
            &Db2ScalarType::Character { length: 6 }
        );
        assert_eq!(
            result.resolved_type().nullability(),
            Db2Nullability::Nullable
        );
        assert_eq!(result.context(), UTF8);
        assert_eq!(result.truncated_bytes(), 2);
        assert_eq!(error.code, Db2StringStorageErrorCode::NonBlankExcess);
        assert_eq!(error.span, result.span());
        assert_eq!(
            error.to_string(),
            "NonBlankExcess at 2:3: character storage excess contains a nonblank byte"
        );
    }
}
