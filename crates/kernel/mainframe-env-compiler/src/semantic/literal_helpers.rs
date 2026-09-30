use super::*;

pub(super) fn keyword_index(source: &str, keyword: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'-')
            {
                index += 1;
            }
            if source[start..index].eq_ignore_ascii_case(keyword) {
                return Some(start);
            }
        } else {
            index += 1;
        }
    }
    None
}

pub(super) fn quoted_or_word(tail: &str, words: &[String]) -> String {
    if let Some(quote) = tail.chars().next().filter(|ch| matches!(ch, '\'' | '"')) {
        return tail[quote.len_utf8()..]
            .split(quote)
            .next()
            .unwrap_or_default()
            .to_string();
    }
    find_after_owned(words, "VALUE").unwrap_or_default()
}

pub(super) fn packed_decimal(value: &[u8], negative: bool, length: usize) -> Vec<u8> {
    let mut nibbles = value
        .iter()
        .copied()
        .map(|byte| byte - b'0')
        .collect::<Vec<_>>();
    let digits = length.saturating_mul(2).saturating_sub(1);
    if nibbles.len() > digits {
        nibbles = nibbles[nibbles.len() - digits..].to_vec();
    }
    while nibbles.len() < digits {
        nibbles.insert(0, 0);
    }
    nibbles.push(if negative { 0x0d } else { 0x0c });
    nibbles
        .chunks(2)
        .map(|pair| (pair[0] << 4) | pair.get(1).copied().unwrap_or(0))
        .collect()
}

pub(super) fn binary_integer(digits: &[u8], negative: bool, length: usize) -> Vec<u8> {
    let parsed = std::str::from_utf8(digits)
        .ok()
        .and_then(|digits| digits.parse::<i128>().ok())
        .unwrap_or(0);
    let parsed = if negative { -parsed } else { parsed }.to_be_bytes();
    parsed[parsed.len().saturating_sub(length)..].to_vec()
}

pub(super) fn negative_overpunch(digit: u8) -> u8 {
    const NEGATIVE: &[u8; 10] = b"}JKLMNOPQR";
    NEGATIVE
        .get(usize::from(digit.saturating_sub(b'0')))
        .copied()
        .unwrap_or(digit)
}

pub(super) fn numeric_value_digits(
    value: &str,
    scale: usize,
) -> Result<(Vec<u8>, bool), SemanticProblem> {
    let value = value.trim();
    let value = if matches!(
        value.to_ascii_uppercase().as_str(),
        "ZERO" | "ZEROS" | "ZEROES"
    ) {
        "0"
    } else {
        value
    };
    let negative = value.starts_with('-');
    let unsigned = value.strip_prefix(['-', '+']).unwrap_or(value);
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if (integer.is_empty() && fraction.is_empty())
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() > scale
    {
        return Err(SemanticProblem::InvalidDeclaration(format!(
            "numeric VALUE {value} does not fit the PICTURE scale"
        )));
    }
    let mut digits = integer.as_bytes().to_vec();
    digits.extend_from_slice(fraction.as_bytes());
    digits.extend(std::iter::repeat_n(b'0', scale - fraction.len()));
    Ok((digits, negative))
}

pub(super) fn hexadecimal_literal(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() < 3 || !matches!(bytes[0], b'X' | b'x') || !matches!(bytes[1], b'\'' | b'"') {
        return None;
    }
    let quote = bytes[1];
    let end = bytes[2..].iter().position(|byte| *byte == quote)? + 2;
    let digits = &bytes[2..end];
    if digits.is_empty()
        || !digits.len().is_multiple_of(2)
        || !digits.iter().all(u8::is_ascii_hexdigit)
    {
        return None;
    }
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}
