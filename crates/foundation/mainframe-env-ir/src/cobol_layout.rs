//! Executable COBOL layout-dialect validation.

use crate::{Attribute, Operation, OperationId};

mod category;

pub(crate) use category::validate_address_width;

const MAX_PICTURE_SYMBOLS: u64 = 1_000_000;
/// Maximum admitted backing occurrences for an `OCCURS ... UNBOUNDED` layout.
pub const COBOL_MAX_UNBOUNDED_OCCURRENCES: usize = 4_096;
/// Maximum admitted backing bytes for one `OCCURS ... UNBOUNDED` layout.
pub const COBOL_MAX_UNBOUNDED_STORAGE_BYTES: usize = 16 * 1_024 * 1_024;
/// Maximum number of index names declared by one COBOL table.
pub const COBOL_MAX_INDEX_NAMES: usize = 12;
/// Maximum number of ordered keys declared by one COBOL table.
pub const COBOL_MAX_TABLE_KEYS: usize = 12;
/// Maximum aggregate static byte extent of one COBOL table's ordered keys.
pub const COBOL_MAX_TABLE_KEY_BYTES: u64 = 256;
const COBOL_MAX_DYNAMIC_LENGTH: u64 = 999_999_999;

#[derive(Clone, Copy)]
pub(crate) struct CobolLayoutAbi<'a> {
    pub(crate) id: OperationId,
    pub(crate) name: &'a str,
    pub(crate) simple_name: &'a str,
    pub(crate) parent: &'a str,
    pub(crate) category: &'a str,
    pub(crate) offset: u64,
    picture: &'a str,
    pub(crate) digits: u64,
    pub(crate) scale: u64,
    pub(crate) signed: bool,
    sign_separate: bool,
    blank_when_zero: bool,
    pub(crate) length: u64,
    pub(crate) element_length: u64,
    byte_length: u64,
    pub(crate) occurs_clause: bool,
    pub(crate) unbounded: bool,
    pub(crate) depending_on: &'a str,
    pub(crate) keys: &'a str,
    pub(crate) dynamic: bool,
    pub(crate) dynamic_limit: u64,
    pub(crate) alias_of: &'a str,
    pub(crate) rename_through: &'a str,
    pub(crate) condition_values: &'a str,
    object_class: &'a str,
}

pub(crate) fn validate_definition(
    definition: &Operation,
) -> Result<CobolLayoutAbi<'_>, &'static str> {
    let name = text_attribute(definition, "name")
        .filter(|name| !name.is_empty())
        .ok_or("COBOL layout name is missing, empty, or has the wrong type")?;
    let simple_name = text_attribute(definition, "simple_name")
        .filter(|name| !name.is_empty())
        .ok_or("COBOL layout simple name is missing, empty, or has the wrong type")?;
    if !simple_name_matches(name, simple_name) {
        return Err("COBOL layout simple name disagrees with its qualified name");
    }
    let parent = text_attribute(definition, "parent")
        .ok_or("COBOL layout parent is missing or has the wrong type")?;
    let expected_parent = name.rsplit_once('.').map_or("", |(parent, _)| parent);
    if !parent.eq_ignore_ascii_case(expected_parent) {
        return Err("COBOL layout parent disagrees with its qualified name");
    }
    let category = text_attribute(definition, "category")
        .ok_or("COBOL layout category is missing or has the wrong type")?;
    if !is_known_layout(category) {
        return Err("COBOL layout category is unsupported");
    }
    let picture = text_attribute(definition, "picture")
        .ok_or("COBOL layout picture is missing or has the wrong type")?;
    let digits = nonnegative_integer_attribute(definition, "digits")
        .ok_or("COBOL layout digits are missing or invalid")?;
    let scale = nonnegative_integer_attribute(definition, "scale")
        .filter(|scale| u32::try_from(*scale).is_ok())
        .ok_or("COBOL layout scale is missing or outside the runtime ABI")?;
    let signed = required_boolean_marker(definition, "signed")?;
    let sign_separate = required_boolean_marker(definition, "sign_separate")?;
    if sign_separate && !signed {
        return Err("COBOL layout separate sign requires signed numeric storage");
    }
    let section = text_attribute(definition, "section")
        .ok_or("COBOL layout section is missing or has the wrong type")?;
    if !matches!(section, "file" | "working" | "local" | "linkage") {
        return Err("COBOL layout section is unsupported");
    }
    let condition_values = text_attribute(definition, "condition_values")
        .ok_or("required COBOL layout text metadata is missing or has the wrong type")?;
    for attribute in [
        "depending_on",
        "indexes",
        "keys",
        "alias_of",
        "rename_through",
        "object_class",
    ] {
        if definition
            .attributes
            .get(attribute)
            .is_some_and(|value| !matches!(value, Attribute::Text(_)))
        {
            return Err("optional COBOL layout text metadata has the wrong type");
        }
    }
    let depending_on = text_attribute(definition, "depending_on").unwrap_or("");
    let indexes = text_attribute(definition, "indexes").unwrap_or("");
    let keys = text_attribute(definition, "keys").unwrap_or("");
    let alias_of = text_attribute(definition, "alias_of").unwrap_or("");
    let rename_through = text_attribute(definition, "rename_through").unwrap_or("");
    let object_class = text_attribute(definition, "object_class").unwrap_or("");
    let index_count = validate_index_names(indexes)?;
    let key_count = validate_keys(keys)?;
    if index_count > COBOL_MAX_INDEX_NAMES || key_count > COBOL_MAX_TABLE_KEYS {
        return Err("COBOL layout table metadata exceeds its key or index limit");
    }
    if !depending_on.is_empty() {
        normalize_layout_reference(depending_on)?;
    }
    if !rename_through.is_empty() {
        normalize_layout_reference(rename_through)?;
    }
    let offset = runtime_usize_attribute(definition, "offset")?;
    let length = runtime_usize_attribute(definition, "length")?;
    let element_length = runtime_usize_attribute(definition, "element_length")?;
    let byte_length = optional_runtime_usize_attribute(definition, "byte_length")?.unwrap_or(0);
    let occurs = runtime_usize_attribute(definition, "occurs")?;
    let occurs_min = optional_runtime_usize_attribute(definition, "occurs_min")?.unwrap_or(1);
    let justified_right = optional_boolean_marker(definition, "justified_right")?.unwrap_or(false);
    let blank_when_zero = optional_boolean_marker(definition, "blank_when_zero")?.unwrap_or(false);
    let occurs_clause = optional_boolean_marker(definition, "occurs_clause")?.unwrap_or(false);
    let unbounded = optional_boolean_marker(definition, "unbounded")?.unwrap_or(false);
    let dynamic = optional_boolean_marker(definition, "dynamic")?.unwrap_or(false);
    let dynamic_limit = optional_runtime_usize_attribute(definition, "dynamic_limit")?.unwrap_or(0);
    if justified_right
        && !matches!(
            category,
            "alphabetic" | "alphanumeric" | "dbcs" | "national"
        )
    {
        return Err("COBOL layout JUSTIFIED marker is incompatible with its category");
    }
    if blank_when_zero
        && (!matches!(category, "numeric_edited" | "national" | "national_edited")
            || signed
            || picture.contains('*'))
    {
        return Err("COBOL layout BLANK WHEN ZERO marker is incompatible with its ABI");
    }
    if occurs == 0 || occurs_min > occurs {
        return Err("COBOL layout occurrence bounds are invalid");
    }
    if !occurs_clause
        && (occurs != 1
            || occurs_min != 1
            || unbounded
            || !depending_on.is_empty()
            || !indexes.is_empty()
            || !keys.is_empty())
    {
        return Err("COBOL layout occurrence metadata contradicts its OCCURS marker");
    }
    if occurs_min != occurs && depending_on.is_empty() {
        return Err("variable COBOL layout occurrence metadata has no DEPENDING ON binding");
    }
    if !depending_on.is_empty() && !unbounded && occurs_min >= occurs {
        return Err("variable COBOL layout occurrence range is not increasing");
    }
    if unbounded {
        let max_by_storage = u64::try_from(COBOL_MAX_UNBOUNDED_STORAGE_BYTES)
            .expect("COBOL unbounded storage cap fits u64")
            .checked_div(element_length)
            .filter(|capacity| *capacity >= occurs_min)
            .ok_or("unbounded COBOL layout has no representable bounded capacity")?;
        let expected_occurs = u64::try_from(COBOL_MAX_UNBOUNDED_OCCURRENCES)
            .expect("COBOL occurrence cap fits u64")
            .min(max_by_storage);
        if !occurs_clause
            || dynamic
            || depending_on.is_empty()
            || !alias_of.is_empty()
            || occurs != expected_occurs
        {
            return Err("unbounded COBOL layout metadata is inconsistent with its bounded ABI");
        }
    }
    if dynamic {
        let valid_element = matches!(
            (category, picture, element_length),
            ("alphanumeric", "X" | "x", 1) | ("utf8", "U" | "u", 1..)
        );
        if unbounded
            || !valid_element
            || length != 0
            || occurs != 1
            || occurs_min != 1
            || occurs_clause
            || !depending_on.is_empty()
            || !indexes.is_empty()
            || !keys.is_empty()
            || !alias_of.is_empty()
            || byte_length != 0
            || (section == "file" && !parent.is_empty())
            || digits != 0
            || scale != 0
            || signed
            || sign_separate
            || !(1..=COBOL_MAX_DYNAMIC_LENGTH).contains(&dynamic_limit)
        {
            return Err("dynamic COBOL layout metadata is inconsistent with its ABI");
        }
    } else {
        if dynamic_limit != 0 {
            return Err("static COBOL layout has a nonzero dynamic extent limit");
        }
        let expected_length = element_length
            .checked_mul(occurs)
            .ok_or("COBOL layout occurrence extent overflows")?;
        if length != expected_length {
            return Err("COBOL layout length does not match its occurrence extent");
        }
    }
    if category == "condition" {
        if !picture.is_empty()
            || digits != 0
            || scale != 0
            || signed
            || sign_separate
            || length != 0
            || element_length != 0
            || occurs != 1
            || occurs_clause
            || unbounded
            || dynamic
            || !depending_on.is_empty()
            || !indexes.is_empty()
            || !keys.is_empty()
            || !rename_through.is_empty()
            || condition_values.is_empty()
        {
            return Err("COBOL condition layout metadata is inconsistent with its ABI");
        }
    } else if !condition_values.is_empty() {
        return Err("non-condition COBOL layout carries condition values");
    }
    if category == "rename"
        && (!picture.is_empty()
            || digits != 0
            || scale != 0
            || signed
            || sign_separate
            || length == 0
            || element_length != length
            || occurs != 1
            || occurs_clause
            || unbounded
            || dynamic
            || !depending_on.is_empty()
            || !indexes.is_empty()
            || !keys.is_empty()
            || alias_of.is_empty())
    {
        return Err("COBOL RENAMES layout metadata is inconsistent with its ABI");
    }
    if category != "rename" && !rename_through.is_empty() {
        return Err("non-RENAMES COBOL layout carries a range endpoint");
    }
    let layout = CobolLayoutAbi {
        id: definition.id,
        name,
        simple_name,
        parent,
        category,
        offset,
        picture,
        digits,
        scale,
        signed,
        sign_separate,
        blank_when_zero,
        length,
        element_length,
        byte_length,
        occurs_clause,
        unbounded,
        depending_on,
        keys,
        dynamic,
        dynamic_limit,
        alias_of,
        rename_through,
        condition_values,
        object_class,
    };
    category::validate_layout_category(&layout)?;
    Ok(layout)
}

/// Validate the canonical true-value list for the currently executable subset
/// of a level-88 condition association.
///
/// Per IBM Enterprise COBOL 6.5, VALUE clause format 2
/// (`SS6SG3_6.5/lr/ref/rlddeva2.html`), condition-names may be attached to an
/// alphanumeric group as well as to its subordinate items. Each group value
/// must be an alphanumeric literal or figurative constant no longer than the
/// group's total elementary size, and the condition test follows the group
/// comparison rules (`SS6SG3_6.5/lr/ref/rlpdsgrp.html`), which compare the
/// group as an alphanumeric item of the same byte length.
/// This executable subset therefore accepts "group" alongside "alphanumeric"
/// and reuses the same alphanumeric literal/figurative-constant/length
/// validation; "national_group" and "utf8_group" stay outside the subset
/// because their DBCS/national/UTF-8 byte semantics are not implemented
/// here.
pub fn validate_cobol_condition_values(
    category: &str,
    digits: u64,
    scale: u64,
    signed: bool,
    element_length: u64,
    values: &[&str],
) -> Result<(), &'static str> {
    if values.is_empty() || values.iter().any(|value| matches!(*value, "IS" | "ARE")) {
        return Err("COBOL condition values are empty or noncanonical");
    }
    let numeric = matches!(category, "numeric_display" | "packed_decimal" | "binary");
    let alphanumeric = matches!(category, "alphanumeric" | "group");
    if !numeric && !alphanumeric {
        return Err("COBOL condition variable category is outside the executable subset");
    }
    let mut index = 0usize;
    while index < values.len() {
        if numeric {
            let start = condition_number(values[index])?;
            validate_condition_number_fit(start, digits, scale, signed)?;
            if values
                .get(index + 1)
                .is_some_and(|value| matches!(*value, "THRU" | "THROUGH"))
            {
                let end = values
                    .get(index + 2)
                    .copied()
                    .ok_or("COBOL condition range has no end")
                    .and_then(condition_number)?;
                validate_condition_number_fit(end, digits, scale, signed)?;
                if condition_number_cmp(start, end) != std::cmp::Ordering::Less {
                    return Err("COBOL condition range is not strictly increasing");
                }
                index += 3;
            } else {
                index += 1;
            }
        } else {
            validate_alphanumeric_condition_value(values[index], element_length)?;
            if values
                .get(index + 1)
                .is_some_and(|value| matches!(*value, "THRU" | "THROUGH"))
            {
                return Err("alphanumeric condition ranges are outside the executable subset");
            }
            index += 1;
        }
    }
    Ok(())
}

/// Validate the bounded literal shape retained for a level-78 constant.
pub fn validate_cobol_level78_value(values: &[&str]) -> Result<(), &'static str> {
    if values.len() != 1
        || (!is_condition_figurative(values[0])
            && condition_number(values[0]).is_err()
            && quoted_condition_text(values[0]).is_none())
    {
        Err("COBOL level-78 constant value is outside the executable subset")
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct ConditionNumber<'a> {
    negative: bool,
    integer: &'a str,
    fraction: &'a str,
}

fn condition_number(value: &str) -> Result<ConditionNumber<'_>, &'static str> {
    if matches!(value, "ZERO" | "ZEROS" | "ZEROES") {
        return Ok(ConditionNumber {
            negative: false,
            integer: "0",
            fraction: "",
        });
    }
    if quoted_condition_text(value).is_some() {
        return Err("quoted alphanumeric literal is not numeric condition data");
    }
    let (negative, unsigned) = value.strip_prefix('-').map_or_else(
        || (false, value.strip_prefix('+').unwrap_or(value)),
        |value| (true, value),
    );
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || (integer.is_empty() && fraction.is_empty())
        || (!integer.is_empty() && !integer.bytes().all(|byte| byte.is_ascii_digit()))
        || (!fraction.is_empty() && !fraction.bytes().all(|byte| byte.is_ascii_digit()))
        || (unsigned.contains('.') && fraction.is_empty())
    {
        return Err("COBOL condition numeric literal is malformed");
    }
    Ok(ConditionNumber {
        negative,
        integer,
        fraction,
    })
}

fn validate_condition_number_fit(
    value: ConditionNumber<'_>,
    digits: u64,
    scale: u64,
    signed: bool,
) -> Result<(), &'static str> {
    if value.negative && !signed && !condition_number_zero(value) {
        return Err("negative COBOL condition value does not fit unsigned storage");
    }
    let integer = value.integer.trim_start_matches('0');
    let integer_capacity = digits
        .checked_sub(scale)
        .ok_or("COBOL condition numeric ABI has scale beyond digits")?;
    if u64::try_from(integer.len()).is_err()
        || u64::try_from(integer.len()).unwrap_or(u64::MAX) > integer_capacity
    {
        return Err("COBOL condition integer value exceeds its PICTURE");
    }
    let scale = usize::try_from(scale).map_err(|_| "COBOL condition scale is too large")?;
    if value
        .fraction
        .as_bytes()
        .get(scale..)
        .is_some_and(|remainder| remainder.iter().any(|digit| *digit != b'0'))
    {
        return Err("COBOL condition fraction loses nonzero digits");
    }
    Ok(())
}

fn condition_number_cmp(
    left: ConditionNumber<'_>,
    right: ConditionNumber<'_>,
) -> std::cmp::Ordering {
    let left_zero = condition_number_zero(left);
    let right_zero = condition_number_zero(right);
    if left_zero && right_zero {
        return std::cmp::Ordering::Equal;
    }
    if left.negative != right.negative {
        return if left.negative {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        };
    }
    let magnitude = condition_number_magnitude_cmp(left, right);
    if left.negative {
        magnitude.reverse()
    } else {
        magnitude
    }
}

fn condition_number_magnitude_cmp(
    left: ConditionNumber<'_>,
    right: ConditionNumber<'_>,
) -> std::cmp::Ordering {
    let left_integer = left.integer.trim_start_matches('0');
    let right_integer = right.integer.trim_start_matches('0');
    left_integer
        .len()
        .cmp(&right_integer.len())
        .then_with(|| left_integer.cmp(right_integer))
        .then_with(|| {
            let length = left.fraction.len().max(right.fraction.len());
            (0..length)
                .map(|index| {
                    left.fraction
                        .as_bytes()
                        .get(index)
                        .copied()
                        .unwrap_or(b'0')
                        .cmp(
                            &right
                                .fraction
                                .as_bytes()
                                .get(index)
                                .copied()
                                .unwrap_or(b'0'),
                        )
                })
                .find(|ordering| *ordering != std::cmp::Ordering::Equal)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn condition_number_zero(value: ConditionNumber<'_>) -> bool {
    value
        .integer
        .bytes()
        .chain(value.fraction.bytes())
        .all(|digit| digit == b'0')
}

fn validate_alphanumeric_condition_value(
    value: &str,
    element_length: u64,
) -> Result<(), &'static str> {
    if is_condition_figurative(value) {
        return Ok(());
    }
    // Per IBM Enterprise COBOL 6.5 (`SS6SG3_6.5/lr/ref/rllitahx.html`), a
    // hexadecimal-notation literal (`X'..'`) is an alphanumeric literal and
    // is valid wherever one is. Its decoded byte length, not its hex-digit
    // character count, is what must fit the conditional variable.
    if let Some(hex) = hexadecimal_condition_bytes(value) {
        return if !hex.is_empty()
            && u64::try_from(hex.len()).is_ok_and(|length| length <= element_length)
        {
            Ok(())
        } else {
            Err("COBOL alphanumeric condition value exceeds its storage")
        };
    }
    let text = quoted_condition_text(value)
        .ok_or("COBOL alphanumeric condition value is not a quoted literal")?;
    if !text.is_empty() && u64::try_from(text.len()).is_ok_and(|length| length <= element_length) {
        Ok(())
    } else {
        Err("COBOL alphanumeric condition value exceeds its storage")
    }
}

fn quoted_condition_text(value: &str) -> Option<&str> {
    let delimiter = value.as_bytes().first().copied()?;
    if !matches!(delimiter, b'\'' | b'"') || value.as_bytes().last().copied() != Some(delimiter) {
        return None;
    }
    let text = value.get(1..value.len().checked_sub(1)?)?;
    (!text.as_bytes().contains(&delimiter)).then_some(text)
}

/// Decode an `X'hexadecimal-digits'` (or `X"..."`) alphanumeric literal, per
/// `SS6SG3_6.5/lr/ref/rllitahx.html`. Returns `None` when `value` is not
/// hexadecimal notation.
fn hexadecimal_condition_bytes(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    if bytes.len() < 3 || !matches!(bytes[0], b'X' | b'x') || !matches!(bytes[1], b'\'' | b'"') {
        return None;
    }
    let quote = bytes[1];
    if bytes.last().copied() != Some(quote) {
        return None;
    }
    let digits = &bytes[2..bytes.len() - 1];
    if digits.is_empty()
        || !digits.len().is_multiple_of(2)
        || !digits.iter().all(u8::is_ascii_hexdigit)
    {
        return None;
    }
    digits
        .chunks(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            u8::try_from((high << 4) | low).ok()
        })
        .collect()
}

fn is_condition_figurative(value: &str) -> bool {
    matches!(
        value,
        "SPACE"
            | "SPACES"
            | "ZERO"
            | "ZEROS"
            | "ZEROES"
            | "LOW-VALUE"
            | "LOW-VALUES"
            | "HIGH-VALUE"
            | "HIGH-VALUES"
    )
}

pub(crate) fn validate_typed_numeric(layout: &CobolLayoutAbi<'_>) -> Result<(), &'static str> {
    validate_numeric_layout(layout, false)
}

fn validate_numeric_layout(
    layout: &CobolLayoutAbi<'_>,
    allow_national_display: bool,
) -> Result<(), &'static str> {
    match layout.category {
        "float_short" => {
            if layout.element_length != 4
                || !layout.picture.is_empty()
                || layout.digits != 0
                || layout.scale != 0
                || layout.signed
                || layout.sign_separate
            {
                return Err("short-float layout metadata is inconsistent with its ABI");
            }
        }
        "float_long" => {
            if layout.element_length != 8
                || !layout.picture.is_empty()
                || layout.digits != 0
                || layout.scale != 0
                || layout.signed
                || layout.sign_separate
            {
                return Err("long-float layout metadata is inconsistent with its ABI");
            }
        }
        category => {
            let shape = numeric_picture_shape(layout.picture)?;
            if shape.digits != layout.digits
                || shape.scale != layout.scale
                || shape.signed != layout.signed
            {
                return Err("numeric layout metadata disagrees with its PICTURE");
            }
            let expected_length = match category {
                "numeric_display" if !shape.edited && !layout.blank_when_zero => {
                    compatible_display_length(
                        shape
                            .storage
                            .max(1)
                            .checked_add(u64::from(layout.sign_separate))
                            .ok_or("numeric display layout extent overflows")?,
                        layout.element_length,
                        allow_national_display,
                    )?
                }
                "numeric_edited" if shape.edited || layout.blank_when_zero => {
                    let display = shape
                        .storage
                        .max(1)
                        .checked_add(u64::from(layout.sign_separate))
                        .ok_or("numeric edited layout extent overflows")?;
                    compatible_display_length(
                        display,
                        layout.element_length,
                        allow_national_display && layout.blank_when_zero && !shape.edited,
                    )?
                }
                "national_edited" if shape.edited => shape
                    .storage
                    .max(1)
                    .checked_add(u64::from(layout.sign_separate))
                    .and_then(|display| display.checked_mul(2))
                    .ok_or("national edited layout extent overflows")?,
                "packed_decimal"
                    if !shape.edited
                        && !layout.sign_separate
                        && (1..=31).contains(&layout.digits) =>
                {
                    layout
                        .digits
                        .checked_add(2)
                        .map(|digits| digits / 2)
                        .ok_or("packed-decimal layout extent overflows")?
                }
                "binary" if !shape.edited && !layout.sign_separate => match layout.digits {
                    1..=4 => 2,
                    5..=9 => 4,
                    10..=18 => 8,
                    _ => return Err("binary layout digit capacity is unsupported"),
                },
                _ => return Err("numeric layout category and PICTURE are inconsistent"),
            };
            if expected_length != layout.element_length {
                return Err("numeric layout representation does not match its element extent");
            }
        }
    }
    Ok(())
}

fn compatible_display_length(
    display: u64,
    actual: u64,
    allow_double_width: bool,
) -> Result<u64, &'static str> {
    if actual == display
        || (allow_double_width
            && display
                .checked_mul(2)
                .is_some_and(|national| actual == national))
    {
        Ok(actual)
    } else {
        Ok(display)
    }
}

#[derive(Clone, Copy, Default)]
struct NumericPictureShape {
    storage: u64,
    digits: u64,
    scale: u64,
    signed: bool,
    edited: bool,
}

fn numeric_picture_shape(picture: &str) -> Result<NumericPictureShape, &'static str> {
    if picture.is_empty() {
        return Err("numeric layout PICTURE is missing");
    }
    let runs = picture_runs(picture)?;
    let symbols: Vec<u8> = runs
        .iter()
        .flat_map(|(symbol, count)| std::iter::repeat_n(*symbol, *count as usize))
        .collect();
    let floating_currency_prefix = cobol_floating_currency_prefix(&symbols);
    let mut currency_symbols_seen = 0u64;
    let mut position = 0usize;
    let mut shape = NumericPictureShape::default();
    let mut fractional = false;
    for (symbol, count) in runs {
        let in_floating_prefix = position < floating_currency_prefix;
        position += count as usize;
        match symbol {
            b'9' => add_picture_digits(&mut shape, count, fractional)?,
            b'Z' | b'*' => {
                shape.edited = true;
                add_picture_digits(&mut shape, count, fractional)?;
            }
            b'S' => shape.signed = true,
            b'V' => fractional = true,
            b'P' => {
                shape.digits = shape
                    .digits
                    .checked_add(count)
                    .ok_or("numeric PICTURE digit count overflows")?;
                if fractional {
                    shape.scale = shape
                        .scale
                        .checked_add(count)
                        .ok_or("numeric PICTURE scale overflows")?;
                }
            }
            b'+' | b'-' => {
                shape.signed = true;
                shape.edited = true;
                shape.storage = shape
                    .storage
                    .checked_add(count)
                    .ok_or("numeric PICTURE extent overflows")?;
                if count > 1 {
                    shape.digits = shape
                        .digits
                        .checked_add(count)
                        .ok_or("numeric PICTURE digit count overflows")?;
                    if fractional {
                        shape.scale = shape
                            .scale
                            .checked_add(count)
                            .ok_or("numeric PICTURE scale overflows")?;
                    }
                }
            }
            b'.' => {
                shape.edited = true;
                fractional = true;
                shape.storage = shape
                    .storage
                    .checked_add(count)
                    .ok_or("numeric PICTURE extent overflows")?;
            }
            b'$' => {
                shape.edited = true;
                shape.storage = shape
                    .storage
                    .checked_add(count)
                    .ok_or("numeric PICTURE extent overflows")?;
                if in_floating_prefix {
                    let digit_slots = count - u64::from(currency_symbols_seen == 0);
                    shape.digits = shape
                        .digits
                        .checked_add(digit_slots)
                        .ok_or("numeric PICTURE digit count overflows")?;
                    if fractional {
                        shape.scale = shape
                            .scale
                            .checked_add(digit_slots)
                            .ok_or("numeric PICTURE scale overflows")?;
                    }
                    currency_symbols_seen += count;
                }
            }
            b',' | b'/' | b'B' | b'0' | b'C' | b'R' | b'D' | b'E' => {
                shape.edited = true;
                shape.storage = shape
                    .storage
                    .checked_add(count)
                    .ok_or("numeric PICTURE extent overflows")?;
            }
            _ => return Err("numeric layout PICTURE contains an unsupported symbol"),
        }
    }
    if shape.storage == 0 && shape.digits == 0 {
        return Err("numeric layout PICTURE has no data positions");
    }
    Ok(shape)
}

/// Expanded prefix in which repeated currency symbols use floating insertion.
#[must_use]
pub fn cobol_floating_currency_prefix(symbols: &[u8]) -> usize {
    let prefix = symbols
        .iter()
        .take_while(|&&symbol| matches!(symbol, b'$' | b','))
        .count();
    if symbols[..prefix]
        .iter()
        .filter(|&&symbol| symbol == b'$')
        .count()
        >= 2
    {
        prefix
    } else {
        0
    }
}

fn add_picture_digits(
    shape: &mut NumericPictureShape,
    count: u64,
    fractional: bool,
) -> Result<(), &'static str> {
    shape.storage = shape
        .storage
        .checked_add(count)
        .ok_or("numeric PICTURE extent overflows")?;
    shape.digits = shape
        .digits
        .checked_add(count)
        .ok_or("numeric PICTURE digit count overflows")?;
    if fractional {
        shape.scale = shape
            .scale
            .checked_add(count)
            .ok_or("numeric PICTURE scale overflows")?;
    }
    Ok(())
}

fn picture_runs(picture: &str) -> Result<Vec<(u8, u64)>, &'static str> {
    let bytes = picture.as_bytes();
    let mut runs = Vec::<(u8, u64)>::new();
    let mut expanded = 0u64;
    let mut index = 0usize;
    while index < bytes.len() {
        let symbol = bytes[index].to_ascii_uppercase();
        index += 1;
        let count = if bytes.get(index) == Some(&b'(') {
            let close = bytes[index + 1..]
                .iter()
                .position(|byte| *byte == b')')
                .map(|relative| relative + index + 1)
                .ok_or("numeric layout PICTURE repetition is unterminated")?;
            let count = std::str::from_utf8(&bytes[index + 1..close])
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| (1..=MAX_PICTURE_SYMBOLS).contains(value))
                .ok_or("numeric layout PICTURE repetition is invalid")?;
            index = close + 1;
            count
        } else {
            1
        };
        expanded = expanded
            .checked_add(count)
            .filter(|total| *total <= MAX_PICTURE_SYMBOLS)
            .ok_or("numeric layout PICTURE exceeds its expanded-symbol limit")?;
        if let Some((prior, total)) = runs.last_mut()
            && *prior == symbol
        {
            *total = total
                .checked_add(count)
                .ok_or("numeric layout PICTURE repetition overflows")?;
        } else {
            runs.push((symbol, count));
        }
    }
    Ok(runs)
}

fn validate_keys(keys: &str) -> Result<usize, &'static str> {
    let count = validate_name_list(keys, "COBOL layout key metadata is noncanonical")?;
    for key in keys.split('').filter(|key| !key.is_empty()) {
        let Some((direction, name)) = key.split_once(':') else {
            return Err("COBOL layout key metadata is malformed");
        };
        if !matches!(direction, "A" | "D") || name.is_empty() {
            return Err("COBOL layout key metadata is noncanonical");
        }
        normalize_layout_reference(name)?;
    }
    Ok(count)
}

fn validate_index_names(indexes: &str) -> Result<usize, &'static str> {
    let count = validate_name_list(indexes, "COBOL layout index metadata is noncanonical")?;
    if indexes
        .split('')
        .filter(|name| !name.is_empty())
        .any(|name| !cobol_index_name_is_valid(name))
    {
        return Err("COBOL layout index metadata contains an invalid index name");
    }
    Ok(count)
}

fn validate_name_list(value: &str, problem: &'static str) -> Result<usize, &'static str> {
    if value.is_empty() {
        return Ok(0);
    }
    let mut seen = std::collections::BTreeSet::new();
    if value
        .split('')
        .any(|name| name.is_empty() || !seen.insert(name.to_ascii_uppercase()))
    {
        return Err(problem);
    }
    Ok(seen.len())
}

/// Whether a name is canonical for the executable COBOL index-name ABI.
#[must_use]
pub fn cobol_index_name_is_valid(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 30
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(|byte| *byte != b'-')
        && bytes.iter().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
        && bytes.iter().any(u8::is_ascii_alphabetic)
        && !cobol_source_word_is_undefinable(name)
}

/// Whether the pinned COBOL baseline forbids a word as a user-defined name.
#[must_use]
pub fn cobol_source_word_is_undefinable(word: &str) -> bool {
    crate::cobol_reserved_words::is_cobol_undefinable_word(word)
}

pub(crate) fn normalize_layout_reference(reference: &str) -> Result<(String, bool), &'static str> {
    let upper = reference.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return Err("COBOL layout reference is empty");
    }
    if upper.contains('.') {
        return Ok((upper, true));
    }
    let words = upper.split_whitespace().collect::<Vec<_>>();
    let Some(simple) = words.first() else {
        return Err("COBOL layout reference is empty");
    };
    if words.len() == 1 {
        return Ok(((*simple).to_string(), false));
    }
    if words.len().is_multiple_of(2)
        || !words
            .iter()
            .skip(1)
            .step_by(2)
            .all(|word| matches!(*word, "OF" | "IN"))
    {
        return Err("COBOL layout reference qualification is malformed");
    }
    let mut components = words.iter().skip(2).step_by(2).copied().collect::<Vec<_>>();
    components.reverse();
    components.push(simple);
    Ok((components.join("."), true))
}

/// Match a COBOL reference using its relative qualification hierarchy.
#[must_use]
pub fn cobol_layout_reference_matches(
    candidate_qualified: &str,
    candidate_simple: &str,
    reference: &str,
) -> bool {
    let Ok((normalized, explicitly_qualified)) = normalize_layout_reference(reference) else {
        return false;
    };
    if !explicitly_qualified {
        return candidate_simple.eq_ignore_ascii_case(&normalized);
    }
    let required = normalized.split('.').collect::<Vec<_>>();
    let Some(simple) = required.last() else {
        return false;
    };
    if !candidate_simple.eq_ignore_ascii_case(simple) {
        return false;
    }
    let candidate = candidate_qualified
        .split('.')
        .map(str::to_ascii_uppercase)
        .collect::<Vec<_>>();
    let ancestors = &candidate[..candidate.len().saturating_sub(1)];
    let mut at = 0usize;
    for qualifier in &required[..required.len().saturating_sub(1)] {
        let Some(found) = ancestors[at..]
            .iter()
            .position(|ancestor| ancestor.eq_ignore_ascii_case(qualifier))
        else {
            return false;
        };
        at += found + 1;
    }
    true
}

fn simple_name_matches(qualified: &str, simple: &str) -> bool {
    let terminal = qualified.rsplit('.').next().unwrap_or(qualified);
    let terminal = terminal.to_ascii_uppercase();
    let simple = simple.to_ascii_uppercase();
    if simple == "FILLER" {
        return terminal.strip_prefix("FILLER#").is_some_and(|suffix| {
            suffix.len() == 5 && suffix.bytes().all(|byte| byte.is_ascii_digit())
        });
    }
    if terminal == simple {
        return true;
    }
    terminal
        .strip_prefix(&simple)
        .and_then(|suffix| suffix.strip_prefix("#ALTERNATE"))
        .is_some_and(|suffix| suffix.len() == 5 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn alternate_primary_name(layout: &CobolLayoutAbi<'_>) -> Option<String> {
    let (parent, terminal) = layout.name.rsplit_once('.').unwrap_or(("", layout.name));
    let marker = terminal.to_ascii_uppercase().rfind("#ALTERNATE")?;
    let primary = &terminal[..marker];
    Some(if parent.is_empty() {
        primary.to_string()
    } else {
        format!("{parent}.{primary}")
    })
}

fn runtime_usize_attribute(operation: &Operation, name: &str) -> Result<u64, &'static str> {
    nonnegative_integer_attribute(operation, name)
        .filter(|value| usize::try_from(*value).is_ok())
        .ok_or("required COBOL layout size metadata is missing or outside the runtime ABI")
}

fn optional_runtime_usize_attribute(
    operation: &Operation,
    name: &str,
) -> Result<Option<u64>, &'static str> {
    match operation.attributes.get(name) {
        None => Ok(None),
        Some(Attribute::Integer(value)) if *value >= 0 && usize::try_from(*value).is_ok() => {
            Ok(Some(*value as u64))
        }
        Some(_) => Err("optional COBOL layout size metadata is invalid"),
    }
}

fn required_boolean_marker(operation: &Operation, name: &str) -> Result<bool, &'static str> {
    optional_boolean_marker(operation, name)?
        .ok_or("required COBOL layout boolean metadata is missing")
}

fn optional_boolean_marker(
    operation: &Operation,
    name: &str,
) -> Result<Option<bool>, &'static str> {
    match operation.attributes.get(name) {
        None => Ok(None),
        Some(Attribute::Integer(0)) => Ok(Some(false)),
        Some(Attribute::Integer(1)) => Ok(Some(true)),
        Some(_) => Err("COBOL layout boolean metadata is not canonical 0 or 1"),
    }
}

fn nonnegative_integer_attribute(operation: &Operation, name: &str) -> Option<u64> {
    match operation.attributes.get(name) {
        Some(Attribute::Integer(value)) => u64::try_from(*value).ok(),
        _ => None,
    }
}

fn text_attribute<'a>(operation: &'a Operation, name: &str) -> Option<&'a str> {
    match operation.attributes.get(name) {
        Some(Attribute::Text(value)) => Some(value),
        _ => None,
    }
}

fn is_abi_numeric_layout(category: &str) -> bool {
    is_numeric_layout(category) || category == "national_edited"
}

pub(crate) fn is_numeric_layout(category: &str) -> bool {
    matches!(
        category,
        "numeric_display"
            | "numeric_edited"
            | "packed_decimal"
            | "binary"
            | "float_short"
            | "float_long"
    )
}

/// Whether a layout category can describe an OCCURS table key.
#[must_use]
pub fn cobol_table_key_category_is_eligible(category: &str) -> bool {
    matches!(
        category,
        "alphabetic"
            | "alphanumeric"
            | "alphanumeric_edited"
            | "binary"
            | "dbcs"
            | "float_long"
            | "float_short"
            | "group"
            | "national"
            | "national_edited"
            | "national_group"
            | "numeric_display"
            | "numeric_edited"
            | "packed_decimal"
            | "utf8"
            | "utf8_group"
    )
}

fn is_known_layout(category: &str) -> bool {
    matches!(
        category,
        "alphabetic"
            | "alphanumeric"
            | "alphanumeric_edited"
            | "binary"
            | "condition"
            | "dbcs"
            | "float_long"
            | "float_short"
            | "function_pointer"
            | "group"
            | "index"
            | "national"
            | "national_edited"
            | "national_group"
            | "numeric_display"
            | "numeric_edited"
            | "object_reference"
            | "packed_decimal"
            | "pointer"
            | "pointer_32"
            | "procedure_pointer"
            | "rename"
            | "utf8"
            | "utf8_group"
    )
}

#[cfg(test)]
mod tests {
    use super::numeric_picture_shape;

    #[test]
    fn floating_insertion_picture_matches_compiler_metadata() {
        for (picture, storage, digits, scale, signed) in [
            ("$$$,$$9.99", 10, 7, 2, false),
            ("$$$.99", 6, 4, 2, false),
            ("$$V99", 4, 3, 2, false),
            ("$$$$", 4, 3, 0, false),
            ("$9", 2, 1, 0, false),
            ("+++,++9.99", 10, 8, 2, true),
            ("----9", 5, 5, 0, true),
        ] {
            let shape = numeric_picture_shape(picture).unwrap();
            assert_eq!(
                (shape.storage, shape.digits, shape.scale, shape.signed),
                (storage, digits, scale, signed),
                "{picture}"
            );
        }
    }
}
