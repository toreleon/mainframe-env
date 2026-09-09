//! Static category, PICTURE, extent, and address-mode consistency.

use super::{CobolLayoutAbi, is_abi_numeric_layout, picture_runs, validate_numeric_layout};
use crate::{CobolAddressMode, cobol_index_name_is_valid};

pub(super) fn validate_layout_category(layout: &CobolLayoutAbi<'_>) -> Result<(), &'static str> {
    if is_abi_numeric_layout(layout.category) {
        if layout.byte_length != 0 || !layout.object_class.is_empty() {
            return Err("numeric COBOL layout carries nonnumeric ABI metadata");
        }
        return validate_numeric_layout(layout, true);
    }
    if layout.category != "object_reference" && !layout.object_class.is_empty() {
        return Err("non-object COBOL layout carries an object class");
    }
    if !matches!(layout.category, "utf8") && layout.byte_length != 0 {
        return Err("non-UTF-8 COBOL layout carries BYTE-LENGTH metadata");
    }
    if !matches!(layout.category, "condition" | "rename")
        && (layout.digits != 0
            || layout.scale != 0
            || layout.signed
            || layout.sign_separate
            || layout.blank_when_zero)
    {
        return Err("nonnumeric COBOL layout carries numeric ABI metadata");
    }
    match layout.category {
        "alphabetic" => validate_character_picture(layout, b"A", None, 1),
        "alphanumeric" => {
            let runs = validated_picture_runs(layout.picture, b"AX9")?;
            let has = |symbol| runs.iter().any(|(candidate, _)| *candidate == symbol);
            if !has(b'X') && !(has(b'A') && has(b'9')) {
                return Err("alphanumeric PICTURE is purely alphabetic or numeric");
            }
            validate_picture_extent(layout, &runs, 1)
        }
        "alphanumeric_edited" => {
            let runs = validated_picture_runs(layout.picture, b"AX9B0/")?;
            if !runs
                .iter()
                .any(|(symbol, _)| matches!(*symbol, b'A' | b'X'))
                || !runs
                    .iter()
                    .any(|(symbol, _)| matches!(*symbol, b'B' | b'0' | b'/'))
            {
                return Err("alphanumeric-edited PICTURE lacks data or editing positions");
            }
            validate_picture_extent(layout, &runs, 1)
        }
        "dbcs" => validate_character_picture(layout, b"GB", Some(b'G'), 2),
        "national" => validate_character_picture(layout, b"N", Some(b'N'), 2),
        "utf8" => {
            let runs = validated_picture_runs(layout.picture, b"U")?;
            let characters = picture_run_extent(&runs)?;
            let expected = if layout.byte_length == 0 {
                characters
                    .checked_mul(4)
                    .ok_or("UTF-8 layout default extent overflows")?
            } else {
                layout.byte_length
            };
            if expected == layout.element_length {
                Ok(())
            } else {
                Err("UTF-8 layout extent disagrees with its BYTE-LENGTH ABI")
            }
        }
        "group" | "national_group" | "utf8_group" => {
            if layout.picture.is_empty() {
                Ok(())
            } else {
                Err("group COBOL layout unexpectedly carries a PICTURE")
            }
        }
        "index" | "pointer" | "function_pointer" | "object_reference" => {
            if !layout.picture.is_empty() || !matches!(layout.element_length, 4 | 8) {
                return Err("pointer-like COBOL layout metadata is inconsistent with its ABI");
            }
            if !layout.object_class.is_empty() && !cobol_index_name_is_valid(layout.object_class) {
                return Err("COBOL object-reference class name is noncanonical");
            }
            Ok(())
        }
        "pointer_32" => validate_fixed_opaque_layout(layout, 4),
        "procedure_pointer" => validate_fixed_opaque_layout(layout, 8),
        "condition" | "rename" => {
            if layout.byte_length == 0 && layout.object_class.is_empty() {
                Ok(())
            } else {
                Err("special COBOL layout carries unrelated ABI metadata")
            }
        }
        _ => Err("COBOL layout category has no static ABI validator"),
    }
}

pub(crate) fn validate_address_width(
    layout: &CobolLayoutAbi<'_>,
    address_mode: Option<CobolAddressMode>,
) -> Result<(), &'static str> {
    if matches!(
        layout.category,
        "index" | "pointer" | "function_pointer" | "object_reference"
    ) && address_mode.is_some_and(|mode| {
        layout.element_length
            != match mode {
                CobolAddressMode::Bits32 => 4,
                CobolAddressMode::Bits64 => 8,
            }
    }) {
        Err("pointer-like COBOL layout extent disagrees with address mode")
    } else {
        Ok(())
    }
}

fn validate_fixed_opaque_layout(
    layout: &CobolLayoutAbi<'_>,
    expected: u64,
) -> Result<(), &'static str> {
    if layout.picture.is_empty() && layout.element_length == expected {
        Ok(())
    } else {
        Err("opaque COBOL layout extent is inconsistent with its ABI")
    }
}

fn validate_character_picture(
    layout: &CobolLayoutAbi<'_>,
    allowed: &[u8],
    required: Option<u8>,
    width: u64,
) -> Result<(), &'static str> {
    let runs = validated_picture_runs(layout.picture, allowed)?;
    if required.is_some_and(|required| !runs.iter().any(|(symbol, _)| *symbol == required)) {
        return Err("character layout PICTURE lacks its category-defining symbol");
    }
    validate_picture_extent(layout, &runs, width)
}

fn validated_picture_runs(picture: &str, allowed: &[u8]) -> Result<Vec<(u8, u64)>, &'static str> {
    let runs = picture_runs(picture)?;
    if runs.is_empty() || runs.iter().any(|(symbol, _)| !allowed.contains(symbol)) {
        Err("character layout PICTURE is inconsistent with its category")
    } else {
        Ok(runs)
    }
}

fn validate_picture_extent(
    layout: &CobolLayoutAbi<'_>,
    runs: &[(u8, u64)],
    width: u64,
) -> Result<(), &'static str> {
    let expected = picture_run_extent(runs)?
        .checked_mul(width)
        .ok_or("character layout extent overflows")?;
    if expected == layout.element_length {
        Ok(())
    } else {
        Err("character layout PICTURE disagrees with its element extent")
    }
}

fn picture_run_extent(runs: &[(u8, u64)]) -> Result<u64, &'static str> {
    runs.iter().try_fold(0u64, |extent, (_, count)| {
        extent
            .checked_add(*count)
            .ok_or("character layout PICTURE extent overflows")
    })
}
