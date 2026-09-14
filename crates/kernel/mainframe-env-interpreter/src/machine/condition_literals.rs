//! Level-88 condition-name matching and value literal decoding: `IF`
//! evaluation (also used by JSON GENERATE/PARSE suppression), and the bytes
//! `SET ... TO TRUE` stores.
//!
//! Per IBM Enterprise COBOL 6.5 (`SS6SG3_6.5/lr/ref/rllitahx.html`), a
//! hexadecimal-notation literal (`X'..'`) is an alphanumeric literal and is
//! valid wherever one is. Its decoded
//! bytes, not the text of its hex digits, are what a condition-name test
//! compares and what `SET ... TO TRUE` stores; the actual group or item
//! bytes may not be valid UTF-8 text, so the comparison and the stored
//! value both use raw bytes rather than the lossy UTF-8 text used for a
//! quoted literal.

use super::{
    LayoutMetadata, MachineProblem, decimal_aligned, decimal_text, decode_decimal, hex_to_char,
    is_numeric, normalize,
};

pub(super) fn condition_matches(
    actual: &[u8],
    values: &[String],
    layout: &LayoutMetadata,
) -> Result<bool, MachineProblem> {
    if is_numeric(layout.category) && actual.len() == layout.length {
        let actual = decode_decimal(layout, actual)?;
        let mut index = 0usize;
        while index < values.len() {
            let start = values[index].trim_matches(['\'', '"']);
            let Some(start) = decimal_text(start) else {
                break;
            };
            if values
                .get(index + 1)
                .is_some_and(|value| value == "THRU" || value == "THROUGH")
            {
                let end = values
                    .get(index + 2)
                    .and_then(|value| decimal_text(value.trim_matches(['\'', '"'])))
                    .ok_or(MachineProblem::InvalidOperation)?;
                let (actual_start, start) = decimal_aligned(actual, start)?;
                let (actual_end, end) = decimal_aligned(actual, end)?;
                if actual_start.coefficient >= start.coefficient
                    && actual_end.coefficient <= end.coefficient
                {
                    return Ok(true);
                }
                index += 3;
            } else {
                let (actual, expected) = decimal_aligned(actual, start)?;
                if actual.coefficient == expected.coefficient {
                    return Ok(true);
                }
                index += 1;
            }
        }
        if index == values.len() {
            return Ok(false);
        }
    }
    let actual_text = String::from_utf8_lossy(actual).trim().to_string();
    let mut index = 0usize;
    while index < values.len() {
        let normalized = normalize(&values[index]);
        let figurative = match normalized.as_str() {
            "SPACE" | "SPACES" => Some(b' '),
            "ZERO" | "ZEROS" | "ZEROES" => Some(b'0'),
            "LOW-VALUE" | "LOW-VALUES" => Some(0),
            "HIGH-VALUE" | "HIGH-VALUES" => Some(0xff),
            _ => None,
        };
        if figurative.is_some_and(|byte| actual.iter().all(|actual| *actual == byte)) {
            return Ok(true);
        }
        if let Some(matched) = hex_literal_matches(&values[index], actual) {
            if matched {
                return Ok(true);
            }
            index += 1;
            continue;
        }
        let start = values[index].trim_matches(['\'', '"']).to_string();
        if values
            .get(index + 1)
            .is_some_and(|value| value == "THRU" || value == "THROUGH")
        {
            let end = values
                .get(index + 2)
                .ok_or(MachineProblem::InvalidOperation)?
                .trim_matches(['\'', '"']);
            let matched = match (
                decimal_text(&actual_text),
                decimal_text(&start),
                decimal_text(end),
            ) {
                (Some(actual), Some(start), Some(end)) => {
                    let (actual, start) = decimal_aligned(actual, start)?;
                    let (actual, end) = decimal_aligned(actual, end)?;
                    actual.coefficient >= start.coefficient && actual.coefficient <= end.coefficient
                }
                _ => actual_text.as_str() >= start.as_str() && actual_text.as_str() <= end,
            };
            if matched {
                return Ok(true);
            }
            index += 3;
        } else {
            if actual_text == start {
                return Ok(true);
            }
            index += 1;
        }
    }
    Ok(false)
}

/// The bytes a `SET condition-name TO TRUE` statement stores for `literal`:
/// its hexadecimal-notation bytes, or its quoted text if it is not
/// hexadecimal notation.
pub(super) fn condition_true_value_bytes(literal: &str) -> Vec<u8> {
    hex_literal_condition_bytes(literal)
        .unwrap_or_else(|| literal.trim_matches(['\'', '"']).as_bytes().to_vec())
}

/// `Some(bytes == actual)` when `value` is `X'..'`/`X".."` hexadecimal
/// notation; `None` when it is not, so the caller falls back to its other
/// condition-value forms.
fn hex_literal_matches(value: &str, actual: &[u8]) -> Option<bool> {
    Some(hex_literal_condition_bytes(value)? == actual)
}

fn hex_literal_condition_bytes(value: &str) -> Option<Vec<u8>> {
    let bytes = value.as_bytes();
    if bytes.len() < 3 || !matches!(bytes[0], b'X' | b'x') || !matches!(bytes[1], b'\'' | b'"') {
        return None;
    }
    let quote = bytes[1];
    if bytes.last().copied() != Some(quote) {
        return None;
    }
    hex_to_char(&bytes[2..bytes.len() - 1]).ok()
}
