use crate::EncodingProblem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sign {
    Positive,
    Negative,
    Unsigned,
}

/// Exact fixed-point value: coefficient × 10^-scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecimalValue {
    coefficient: i128,
    scale: u8,
}

impl DecimalValue {
    pub fn new(coefficient: i128, scale: u8) -> Result<Self, EncodingProblem> {
        if scale > 38 {
            return Err(EncodingProblem::DecimalOverflow);
        }
        Ok(Self { coefficient, scale })
    }

    #[must_use]
    pub const fn coefficient(self) -> i128 {
        self.coefficient
    }

    #[must_use]
    pub const fn scale(self) -> u8 {
        self.scale
    }
}

pub fn encode_packed(value: DecimalValue, digits: u8) -> Result<Vec<u8>, EncodingProblem> {
    if digits == 0 || digits > 38 {
        return Err(EncodingProblem::InvalidWidth);
    }
    let negative = value.coefficient < 0;
    let magnitude = value.coefficient.unsigned_abs();
    let text = magnitude.to_string();
    if text.len() > usize::from(digits) {
        return Err(EncodingProblem::DecimalOverflow);
    }
    let mut digits_text = String::with_capacity(usize::from(digits) + 1);
    for _ in text.len()..usize::from(digits) {
        digits_text.push('0');
    }
    digits_text.push_str(&text);
    let mut nibbles: Vec<u8> = digits_text.bytes().map(|byte| byte - b'0').collect();
    nibbles.push(if negative { 0x0d } else { 0x0c });
    if !nibbles.len().is_multiple_of(2) {
        nibbles.insert(0, 0);
    }
    Ok(nibbles
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (pair[0] << 4) | pair[1])
        .collect())
}

pub fn decode_packed(bytes: &[u8], digits: u8, scale: u8) -> Result<DecimalValue, EncodingProblem> {
    if digits == 0 || digits > 38 || scale > digits || bytes.is_empty() {
        return Err(EncodingProblem::InvalidWidth);
    }
    let expected = (usize::from(digits) + 2) / 2;
    if bytes.len() != expected {
        return Err(EncodingProblem::InvalidWidth);
    }
    let sign = bytes.last().copied().unwrap_or_default() & 0x0f;
    let negative = match sign {
        0x0b | 0x0d => true,
        0x0a | 0x0c | 0x0e | 0x0f => false,
        _ => return Err(EncodingProblem::InvalidDecimal),
    };
    let mut all_digits = Vec::with_capacity(bytes.len() * 2 - 1);
    for (index, byte) in bytes.iter().enumerate() {
        let high = byte >> 4;
        let low = byte & 0x0f;
        if high > 9 || (index + 1 != bytes.len() && low > 9) {
            return Err(EncodingProblem::InvalidDecimal);
        }
        all_digits.push(high);
        if index + 1 != bytes.len() {
            all_digits.push(low);
        }
    }
    let skip = all_digits.len().saturating_sub(usize::from(digits));
    if all_digits[..skip].iter().any(|digit| *digit != 0) {
        return Err(EncodingProblem::DecimalOverflow);
    }
    let mut coefficient = 0i128;
    for digit in &all_digits[skip..] {
        coefficient = coefficient
            .checked_mul(10)
            .and_then(|value| value.checked_add(i128::from(*digit)))
            .ok_or(EncodingProblem::DecimalOverflow)?;
    }
    if negative {
        coefficient = -coefficient;
    }
    DecimalValue::new(coefficient, scale)
}

pub fn encode_zoned(
    value: DecimalValue,
    digits: u8,
    signed: bool,
) -> Result<Vec<u8>, EncodingProblem> {
    if digits == 0 || digits > 38 {
        return Err(EncodingProblem::InvalidWidth);
    }
    let magnitude = value.coefficient.unsigned_abs().to_string();
    if magnitude.len() > usize::from(digits) {
        return Err(EncodingProblem::DecimalOverflow);
    }
    let mut output = vec![0xf0; usize::from(digits)];
    for (index, byte) in magnitude.bytes().rev().enumerate() {
        let target = output.len() - 1 - index;
        output[target] |= byte - b'0';
    }
    if signed {
        let last = output.last_mut().ok_or(EncodingProblem::InvalidWidth)?;
        *last = (*last & 0x0f) | if value.coefficient < 0 { 0xd0 } else { 0xc0 };
    }
    Ok(output)
}

pub fn decode_zoned(bytes: &[u8], scale: u8) -> Result<DecimalValue, EncodingProblem> {
    if bytes.is_empty() || bytes.len() > 38 || usize::from(scale) > bytes.len() {
        return Err(EncodingProblem::InvalidWidth);
    }
    let mut coefficient = 0i128;
    let mut negative = false;
    for (index, byte) in bytes.iter().enumerate() {
        let digit = byte & 0x0f;
        if digit > 9 {
            return Err(EncodingProblem::InvalidDecimal);
        }
        if index + 1 == bytes.len() {
            negative = match byte >> 4 {
                0x0d | 0x0b => true,
                0x0c | 0x0e | 0x0f => false,
                _ => return Err(EncodingProblem::InvalidDecimal),
            };
        } else if byte >> 4 != 0x0f {
            return Err(EncodingProblem::InvalidDecimal);
        }
        coefficient = coefficient * 10 + i128::from(digit);
    }
    if negative {
        coefficient = -coefficient;
    }
    DecimalValue::new(coefficient, scale)
}

pub fn encode_binary(value: i64, width: usize) -> Result<Vec<u8>, EncodingProblem> {
    let bytes = value.to_be_bytes();
    match width {
        2 => i16::try_from(value)
            .map(|number| number.to_be_bytes().to_vec())
            .map_err(|_| EncodingProblem::DecimalOverflow),
        4 => i32::try_from(value)
            .map(|number| number.to_be_bytes().to_vec())
            .map_err(|_| EncodingProblem::DecimalOverflow),
        8 => Ok(bytes.to_vec()),
        _ => Err(EncodingProblem::InvalidWidth),
    }
}

pub fn decode_binary(bytes: &[u8]) -> Result<i64, EncodingProblem> {
    match bytes.len() {
        2 => Ok(i64::from(i16::from_be_bytes([bytes[0], bytes[1]]))),
        4 => Ok(i64::from(i32::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        8 => Ok(i64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| EncodingProblem::InvalidWidth)?,
        )),
        _ => Err(EncodingProblem::InvalidWidth),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn packed_decimal_roundtrip() {
        let value = DecimalValue::new(-12345, 2).unwrap();
        let bytes = encode_packed(value, 5).unwrap();
        assert_eq!(decode_packed(&bytes, 5, 2).unwrap(), value);
    }

    #[test]
    fn zoned_decimal_roundtrip() {
        let value = DecimalValue::new(12345, 2).unwrap();
        let bytes = encode_zoned(value, 5, true).unwrap();
        assert_eq!(decode_zoned(&bytes, 2).unwrap(), value);
    }

    #[test]
    fn invalid_packed_digit_fails() {
        assert_eq!(
            decode_packed(&[0x1a, 0x2c], 3, 0),
            Err(EncodingProblem::InvalidDecimal)
        );
    }

    #[test]
    fn binary_width_and_range_are_checked() {
        assert!(encode_binary(i64::from(i16::MAX) + 1, 2).is_err());
        assert_eq!(decode_binary(&encode_binary(-42, 4).unwrap()).unwrap(), -42);
    }

    proptest! {
        #[test]
        fn all_i32_values_roundtrip(value in any::<i32>()) {
            let encoded = encode_binary(i64::from(value), 4).unwrap();
            prop_assert_eq!(decode_binary(&encoded).unwrap(), i64::from(value));
        }

        #[test]
        fn bounded_packed_values_roundtrip(value in -999_999i64..=999_999i64) {
            let decimal = DecimalValue::new(i128::from(value), 2).unwrap();
            let encoded = encode_packed(decimal, 6).unwrap();
            prop_assert_eq!(decode_packed(&encoded, 6, 2).unwrap(), decimal);
        }
    }
}
