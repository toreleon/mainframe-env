//! One storage authority for COMP-1/COMP-2, including legacy IEEE layouts.
use super::{Decimal, LayoutCategory, LayoutMetadata, MachineProblem};
use dec::{Context, Rounding};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FloatProfile {
    Hex,
    Ieee,
}

// 384 decimal digits suffice for exact base-16 scaling across the entire HFP
// exponent range and all 34-digit runtime coefficients (no binary64 ingress).
pub(super) type Wide = dec::Decimal<128>;

fn context() -> Context<Wide> {
    let mut context = Context::default();
    context.set_rounding(Rounding::Down);
    context
}

fn width(layout: &LayoutMetadata) -> usize {
    match layout.category {
        LayoutCategory::FloatShort => 4,
        LayoutCategory::FloatLong => 8,
        _ => unreachable!("floating codec requires a floating layout"),
    }
}

pub(super) fn encode(layout: &LayoutMetadata, value: Decimal) -> Result<Vec<u8>, MachineProblem> {
    if layout.float_profile == FloatProfile::Ieee {
        let value = super::decimal_to_float(value)?;
        return match width(layout) {
            4 if value < f64::from(f32::MIN) || value > f64::from(f32::MAX) => {
                Err(MachineProblem::SizeError)
            }
            4 => Ok((value as f32).to_be_bytes().to_vec()),
            _ => Ok(value.to_be_bytes().to_vec()),
        };
    }
    // Even the largest i128 coefficient is below the minimum normalized HFP
    // magnitude at this scale. Avoid allocating a scale-sized decimal string.
    if value.scale > 117 {
        return Ok(vec![0; width(layout)]);
    }
    let value = context()
        .parse(format!("{}E-{}", value.coefficient, value.scale))
        .map_err(|_| MachineProblem::SizeError)?;
    encode_hex(width(layout), value)
}

pub(super) fn encode_hex(width: usize, mut value: Wide) -> Result<Vec<u8>, MachineProblem> {
    if !matches!(width, 4 | 8) || !value.is_finite() {
        return Err(MachineProblem::SizeError);
    }
    let negative = value.is_negative();
    let mut context = context();
    context.abs(&mut value);
    let mut output = vec![0; width];
    if value.is_zero() {
        return Ok(output);
    }
    let sixteen = Wide::from(16);
    let one = Wide::from(1);
    let sixteenth = context.parse("0.0625").expect("constant");
    let mut exponent = 64i32;
    while value >= one {
        context.div(&mut value, &sixteen);
        exponent += 1;
        if exponent > 127 {
            return Err(MachineProblem::SizeError);
        }
    }
    while value < sixteenth {
        context.mul(&mut value, &sixteen);
        exponent -= 1;
        if exponent < 0 {
            return Ok(output);
        }
    }
    let fraction_bits = (width - 1) * 8;
    let fraction_unit = Wide::from(1u64 << fraction_bits);
    // UNVERIFIED ASSUMPTIONS (#160), pending licensed differential: reject
    // magnitudes above the largest representable value even if truncation
    // would fit; truncate fraction toward zero; underflow is true zero.
    let mut maximum_fraction = Wide::from((1u64 << fraction_bits) - 1);
    context.div(&mut maximum_fraction, &fraction_unit);
    if exponent == 127 && value > maximum_fraction {
        return Err(MachineProblem::SizeError);
    }
    context.mul(&mut value, &fraction_unit);
    context.rescale(&mut value, &Wide::from(0));
    let fraction = u64::try_from(value).map_err(|_| MachineProblem::SizeError)?;
    output[0] = u8::try_from(exponent).map_err(|_| MachineProblem::SizeError)?
        | if negative { 0x80 } else { 0 };
    output[1..].copy_from_slice(&fraction.to_be_bytes()[9 - width..]);
    Ok(output)
}

pub(super) fn decode_hex(bytes: &[u8]) -> Result<Wide, MachineProblem> {
    if !matches!(bytes.len(), 4 | 8) {
        return Err(MachineProblem::DataException);
    }
    let fraction = bytes[1..]
        .iter()
        .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
    // HFP has neither NaNs nor infinities. Zero fractions (including signed
    // and unnormalized zero) decode as true zero; aliases may be unnormalized.
    let mut value = Wide::from(fraction);
    let mut context = context();
    let shift = 4 * (i32::from(bytes[0] & 0x7f) - 64) - ((bytes.len() - 1) * 8) as i32;
    let two = Wide::from(2);
    for _ in 0..shift.unsigned_abs() {
        if shift < 0 {
            context.div(&mut value, &two);
        } else {
            context.mul(&mut value, &two);
        }
    }
    if bytes[0] & 0x80 != 0 {
        context.minus(&mut value);
    }
    Ok(value)
}

pub(super) fn decode(layout: &LayoutMetadata, bytes: &[u8]) -> Result<Decimal, MachineProblem> {
    if bytes.len() != width(layout) {
        return Err(MachineProblem::DataException);
    }
    if layout.float_profile == FloatProfile::Ieee {
        let value = match width(layout) {
            4 => f64::from(f32::from_be_bytes(bytes.try_into().expect("checked width"))),
            _ => f64::from_be_bytes(bytes.try_into().expect("checked width")),
        };
        return super::decimal_from_float(value);
    }
    let mut value = decode_hex(bytes)?;
    // Retain the runtime's bounded 34-digit decimal arithmetic contract.
    let mut context = context();
    // If an exact fraction exceeds that precision, round magnitude upward by
    // one decimal ulp so truncating it back to HFP preserves the stored bits.
    context.set_rounding(Rounding::Up);
    context
        .set_precision(34)
        .expect("bounded decimal precision");
    context.plus(&mut value);
    context.reduce(&mut value);
    let exponent = value.exponent();
    let mut coefficient: i128 = value.coefficient().map_err(|_| MachineProblem::SizeError)?;
    let scale = if exponent > 0 {
        coefficient = coefficient
            .checked_mul(super::ten_power(exponent as u32)?)
            .ok_or(MachineProblem::SizeError)?;
        0
    } else {
        exponent.unsigned_abs()
    };
    super::decimal_checked(Decimal { coefficient, scale })
}
