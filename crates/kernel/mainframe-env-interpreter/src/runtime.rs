use dec::{Context, Decimal, Decimal128, Rounding, Status};
use mainframe_env_execution_api::{Invocation, ResourceLimits};
use std::fmt;

/// The bounded decNumber backing size. `Decimal<12>` stores at most 36 digits;
/// the owned COBOL adapter admits at most the ARITH(EXTEND) 34-digit contract.
const DECIMAL_UNITS: usize = 12;
pub const MAX_COBOL_DECIMAL_DIGITS: usize = 34;
pub const MAX_LOCALE_BYTES: usize = 64;
pub const CURRENT_DATE_BYTES: usize = 21;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CobolArithmeticMode {
    Compatible,
    Extended,
}

impl CobolArithmeticMode {
    #[must_use]
    pub const fn precision(self) -> usize {
        match self {
            Self::Compatible => 18,
            Self::Extended => MAX_COBOL_DECIMAL_DIGITS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CobolRounding {
    AwayFromZero,
    NearestAwayFromZero,
    NearestEven,
    Prohibited,
    TowardGreater,
    TowardLesser,
    Truncation,
}

impl CobolRounding {
    const fn primitive(self) -> Rounding {
        match self {
            Self::AwayFromZero => Rounding::Up,
            Self::NearestAwayFromZero => Rounding::HalfUp,
            Self::NearestEven => Rounding::HalfEven,
            // Prohibited is checked by the owned adapter when discarded digits
            // are reported; truncation keeps the primitive result observable.
            Self::Prohibited | Self::Truncation => Rounding::Down,
            Self::TowardGreater => Rounding::Ceiling,
            Self::TowardLesser => Rounding::Floor,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CobolArithmeticFlags {
    pub conversion_syntax: bool,
    pub division_by_zero: bool,
    pub division_impossible: bool,
    pub division_undefined: bool,
    pub insufficient_storage: bool,
    pub inexact: bool,
    pub invalid_context: bool,
    pub invalid_operation: bool,
    pub overflow: bool,
    pub clamped: bool,
    pub rounded: bool,
    pub subnormal: bool,
    pub underflow: bool,
}

impl CobolArithmeticFlags {
    fn from_status(status: Status) -> Self {
        Self {
            conversion_syntax: status.conversion_syntax(),
            division_by_zero: status.division_by_zero(),
            division_impossible: status.division_impossible(),
            division_undefined: status.division_undefined(),
            insufficient_storage: status.insufficient_storage(),
            inexact: status.inexact(),
            invalid_context: status.invalid_context(),
            invalid_operation: status.invalid_operation(),
            overflow: status.overflow(),
            clamped: status.clamped(),
            rounded: status.rounded(),
            subnormal: status.subnormal(),
            underflow: status.underflow(),
        }
    }

    #[must_use]
    pub const fn size_error(self) -> bool {
        self.division_by_zero
            || self.division_impossible
            || self.division_undefined
            || self.insufficient_storage
            || self.invalid_context
            || self.invalid_operation
            || self.overflow
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CobolDecimal(Decimal<DECIMAL_UNITS>);

impl Eq for CobolDecimal {}

impl CobolDecimal {
    #[must_use]
    pub fn to_standard_string(self) -> String {
        self.0.to_standard_notation_string()
    }

    #[must_use]
    pub fn digits(self) -> u32 {
        self.0.digits()
    }

    #[must_use]
    pub fn exponent(self) -> i32 {
        self.0.exponent()
    }

    #[must_use]
    pub fn is_negative_zero(self) -> bool {
        self.0.is_zero() && self.0.is_negative()
    }
}

pub struct CobolArithmetic {
    context: Context<Decimal<DECIMAL_UNITS>>,
    rounding: CobolRounding,
}

impl CobolArithmetic {
    pub fn new(
        mode: CobolArithmeticMode,
        rounding: CobolRounding,
    ) -> Result<Self, RuntimeContractProblem> {
        let mut context = Context::<Decimal<DECIMAL_UNITS>>::default();
        context
            .set_precision(mode.precision())
            .map_err(|_| RuntimeContractProblem::InvalidArithmeticContext)?;
        context
            .set_max_exponent(9_999)
            .map_err(|_| RuntimeContractProblem::InvalidArithmeticContext)?;
        context
            .set_min_exponent(-9_999)
            .map_err(|_| RuntimeContractProblem::InvalidArithmeticContext)?;
        context.set_rounding(rounding.primitive());
        Ok(Self { context, rounding })
    }

    pub fn parse(
        &mut self,
        text: &str,
    ) -> Result<(CobolDecimal, CobolArithmeticFlags), RuntimeContractProblem> {
        self.context.clear_status();
        let value = self
            .context
            .parse(text)
            .map_err(|_| RuntimeContractProblem::InvalidDecimal)?;
        let flags = self.flags();
        self.finish(value, flags)
    }

    pub fn add(
        &mut self,
        left: CobolDecimal,
        right: CobolDecimal,
    ) -> Result<(CobolDecimal, CobolArithmeticFlags), RuntimeContractProblem> {
        self.context.clear_status();
        let mut value = left.0;
        self.context.add(&mut value, &right.0);
        let flags = self.flags();
        self.finish(value, flags)
    }

    pub fn subtract(
        &mut self,
        left: CobolDecimal,
        right: CobolDecimal,
    ) -> Result<(CobolDecimal, CobolArithmeticFlags), RuntimeContractProblem> {
        self.context.clear_status();
        let mut value = left.0;
        self.context.sub(&mut value, &right.0);
        let flags = self.flags();
        self.finish(value, flags)
    }

    pub fn multiply(
        &mut self,
        left: CobolDecimal,
        right: CobolDecimal,
    ) -> Result<(CobolDecimal, CobolArithmeticFlags), RuntimeContractProblem> {
        self.context.clear_status();
        let mut value = left.0;
        self.context.mul(&mut value, &right.0);
        let flags = self.flags();
        self.finish(value, flags)
    }

    pub fn divide(
        &mut self,
        left: CobolDecimal,
        right: CobolDecimal,
    ) -> Result<(CobolDecimal, CobolArithmeticFlags), RuntimeContractProblem> {
        self.context.clear_status();
        let mut value = left.0;
        self.context.div(&mut value, &right.0);
        let flags = self.flags();
        self.finish(value, flags)
    }

    fn flags(&self) -> CobolArithmeticFlags {
        CobolArithmeticFlags::from_status(self.context.status())
    }

    fn finish(
        &self,
        value: Decimal<DECIMAL_UNITS>,
        flags: CobolArithmeticFlags,
    ) -> Result<(CobolDecimal, CobolArithmeticFlags), RuntimeContractProblem> {
        if !value.is_finite() || value.digits() as usize > MAX_COBOL_DECIMAL_DIGITS {
            return Err(RuntimeContractProblem::DecimalOutOfRange);
        }
        if self.rounding == CobolRounding::Prohibited && flags.rounded {
            return Err(RuntimeContractProblem::RoundingProhibited);
        }
        Ok((CobolDecimal(value), flags))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BinaryFloating32(u32);

impl BinaryFloating32 {
    #[must_use]
    pub const fn from_be_bytes(bytes: [u8; 4]) -> Self {
        Self(u32::from_be_bytes(bytes))
    }
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BinaryFloating64(u64);

impl BinaryFloating64 {
    #[must_use]
    pub const fn from_be_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_be_bytes(bytes))
    }
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 8] {
        self.0.to_be_bytes()
    }
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecimalFloating128([u8; 16]);

impl DecimalFloating128 {
    #[must_use]
    pub fn from_primitive(value: Decimal128) -> Self {
        Self(value.canonical().to_be_bytes())
    }
    #[must_use]
    pub const fn from_be_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
    #[must_use]
    pub const fn to_be_bytes(self) -> [u8; 16] {
        self.0
    }
    #[must_use]
    pub fn primitive(self) -> Decimal128 {
        Decimal128::from_be_bytes(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEncoding {
    Display { ccsid: u16 },
    Dbcs { ccsid: u16 },
    NationalUtf16Be,
    Utf8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CobolRuntimeLimits {
    pub max_steps: u64,
    pub max_storage_bytes: u64,
    pub max_output_bytes: u64,
    pub max_frames: u32,
    pub max_effects: u64,
    pub max_table_elements: u64,
    pub max_sort_records: u64,
}

impl From<ResourceLimits> for CobolRuntimeLimits {
    fn from(limits: ResourceLimits) -> Self {
        Self {
            max_steps: limits.max_steps,
            max_storage_bytes: limits.max_storage_bytes,
            max_output_bytes: limits.max_output_bytes,
            max_frames: limits.max_frames,
            max_effects: limits.max_effects,
            max_table_elements: limits.max_storage_bytes,
            max_sort_records: limits.max_events,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CobolRuntimeEnvironment {
    pub display_ccsid: u16,
    pub locale: String,
    pub current_date: Option<[u8; CURRENT_DATE_BYTES]>,
    pub random_seed: Option<u64>,
    pub limits: CobolRuntimeLimits,
}

impl CobolRuntimeEnvironment {
    pub fn from_invocation(invocation: &Invocation) -> Result<Self, RuntimeContractProblem> {
        let display_ccsid = binding_text(invocation, "cobol.display-ccsid")
            .map(str::parse)
            .transpose()
            .map_err(|_| RuntimeContractProblem::InvalidCcsid)?
            .unwrap_or(37);
        if display_ccsid == 0 {
            return Err(RuntimeContractProblem::InvalidCcsid);
        }
        let locale = binding_text(invocation, "cobol.locale")
            .unwrap_or("C")
            .to_string();
        if locale.is_empty()
            || locale.len() > MAX_LOCALE_BYTES
            || !locale
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.@".contains(&byte))
        {
            return Err(RuntimeContractProblem::InvalidLocale);
        }
        let current_date = invocation
            .bindings
            .get("cobol.current-date")
            .map(|payload| {
                let bytes: [u8; CURRENT_DATE_BYTES] = payload
                    .bytes()
                    .try_into()
                    .map_err(|_| RuntimeContractProblem::InvalidClock)?;
                if !bytes[..16].iter().all(u8::is_ascii_digit)
                    || !matches!(bytes[16], b'+' | b'-')
                    || !bytes[17..].iter().all(u8::is_ascii_digit)
                {
                    return Err(RuntimeContractProblem::InvalidClock);
                }
                Ok(bytes)
            })
            .transpose()?;
        let random_seed = binding_text(invocation, "cobol.random-seed")
            .map(str::parse)
            .transpose()
            .map_err(|_| RuntimeContractProblem::InvalidRandomSeed)?;
        Ok(Self {
            display_ccsid,
            locale,
            current_date,
            random_seed,
            limits: invocation.limits.into(),
        })
    }
}

fn binding_text<'a>(invocation: &'a Invocation, name: &str) -> Option<&'a str> {
    invocation
        .bindings
        .get(name)
        .and_then(|payload| std::str::from_utf8(payload.bytes()).ok())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeContractProblem {
    InvalidArithmeticContext,
    InvalidDecimal,
    DecimalOutOfRange,
    RoundingProhibited,
    InvalidCcsid,
    InvalidLocale,
    InvalidClock,
    InvalidRandomSeed,
}

impl fmt::Display for RuntimeContractProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "COBOL runtime contract rejected input: {self:?}")
    }
}

impl std::error::Error for RuntimeContractProblem {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decnumber_spike_preserves_precision_rounding_and_status() {
        let mut arithmetic =
            CobolArithmetic::new(CobolArithmeticMode::Extended, CobolRounding::NearestEven)
                .unwrap();
        let (left, left_flags) = arithmetic
            .parse("9999999999999999999999999999999999")
            .unwrap();
        let (one, _) = arithmetic.parse("1").unwrap();
        assert!(!left_flags.inexact);
        let (overflowed_precision, flags) = arithmetic.add(left, one).unwrap();
        assert_eq!(
            overflowed_precision.to_standard_string(),
            "10000000000000000000000000000000000"
        );
        assert!(flags.rounded);

        let (one, _) = arithmetic.parse("1").unwrap();
        let (eight, _) = arithmetic.parse("8").unwrap();
        let (quotient, flags) = arithmetic.divide(one, eight).unwrap();
        assert_eq!(quotient.to_standard_string(), "0.125");
        assert!(!flags.inexact);

        let (zero, _) = arithmetic.parse("0").unwrap();
        assert_eq!(
            arithmetic.divide(one, zero),
            Err(RuntimeContractProblem::DecimalOutOfRange)
        );
    }

    #[test]
    fn prohibited_rounding_and_binary_special_bits_are_explicit() {
        let mut arithmetic =
            CobolArithmetic::new(CobolArithmeticMode::Compatible, CobolRounding::Prohibited)
                .unwrap();
        let (one, _) = arithmetic.parse("1").unwrap();
        let (three, _) = arithmetic.parse("3").unwrap();
        assert_eq!(
            arithmetic.divide(one, three),
            Err(RuntimeContractProblem::RoundingProhibited)
        );

        let negative_zero = BinaryFloating64::from_be_bytes((-0.0f64).to_bits().to_be_bytes());
        let nan = BinaryFloating32::from_be_bytes(f32::NAN.to_bits().to_be_bytes());
        assert_eq!(negative_zero.bits(), (-0.0f64).to_bits());
        assert!(f32::from_bits(nan.bits()).is_nan());
    }

    #[test]
    fn decimal_floating_canonical_bytes_preserve_special_values() {
        let negative_zero: Decimal128 = "-0".parse().unwrap();
        let stored = DecimalFloating128::from_primitive(negative_zero);
        assert!(stored.primitive().is_zero());
        assert!(stored.primitive().is_signed());
        let nan = DecimalFloating128::from_primitive(Decimal128::NAN);
        assert!(nan.primitive().is_nan());
    }
}
