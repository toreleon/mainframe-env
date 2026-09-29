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

pub(super) fn packed_decimal(value: &str, length: usize) -> Vec<u8> {
    let negative = value.trim_start().starts_with('-');
    let mut nibbles = value
        .bytes()
        .filter(u8::is_ascii_digit)
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

pub(super) fn binary_integer(value: &str, length: usize) -> Vec<u8> {
    let parsed = value.parse::<i128>().unwrap_or(0).to_be_bytes();
    parsed[parsed.len().saturating_sub(length)..].to_vec()
}

pub(super) fn negative_overpunch(digit: u8) -> u8 {
    const NEGATIVE: &[u8; 10] = b"}JKLMNOPQR";
    NEGATIVE
        .get(usize::from(digit.saturating_sub(b'0')))
        .copied()
        .unwrap_or(digit)
}
