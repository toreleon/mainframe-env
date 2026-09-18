//! Deterministic interval-control time normalization.
//!
//! These values describe CICS time, not a worker or a second clock authority.
//! Admission supplies the local time of day and the shared durable clock once;
//! the resulting deadline is retained by the work owner across restart.

const DAY_MILLIS: u64 = 86_400_000;
const SIX_HOURS_MILLIS: u64 = 21_600_000;

/// Interpretation of an interval-control expiration value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsIntervalMode {
    /// INTERVAL/AFTER: elapsed time from command execution.
    Relative,
    /// TIME/AT: time measured from the preceding local midnight.
    Absolute,
}

/// A source-defined invalid interval-control operand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsIntervalError {
    /// No component was supplied, or the combined time is negative.
    InvalidRequest,
    /// HOURS, or the hours part of an interval, is outside 0 through 99.
    HoursOutOfRange,
    /// MINUTES exceeds the bound for the supplied combination of components.
    MinutesOutOfRange,
    /// SECONDS exceeds the bound for the supplied combination of components.
    SecondsOutOfRange,
}

impl CicsIntervalError {
    /// START's INVREQ RESP2 for this operand failure.
    #[must_use]
    pub const fn start_response2(self) -> i32 {
        match self {
            Self::InvalidRequest => 0,
            Self::HoursOutOfRange => 4,
            Self::MinutesOutOfRange => 5,
            Self::SecondsOutOfRange => 6,
        }
    }

    /// DELAY's INVREQ RESP2 for packed INTERVAL operand failures.
    #[must_use]
    pub const fn delay_response2(self) -> i32 {
        self.start_response2()
    }
}

/// A validated CICS interval-control expiration value.
///
/// Component presence matters: MINUTES(62) is valid, while HOURS(0)
/// MINUTES(62) is invalid. Constructors retain that distinction while checking
/// bounds, then normalize the valid value to whole seconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsIntervalTime {
    mode: CicsIntervalMode,
    seconds: u32,
}

impl CicsIntervalTime {
    /// The default INTERVAL(0), distinct from the absolute TIME(0).
    #[must_use]
    pub const fn immediate() -> Self {
        Self {
            mode: CicsIntervalMode::Relative,
            seconds: 0,
        }
    }

    /// Validate a decoded HHMMSS numeric value for INTERVAL or TIME.
    ///
    /// Packed-decimal decoding remains the language/host binding's authority;
    /// this constructor does not interpret an ASCII string as packed storage.
    pub fn from_hhmmss(mode: CicsIntervalMode, hhmmss: i64) -> Result<Self, CicsIntervalError> {
        if hhmmss < 0 {
            return Err(CicsIntervalError::InvalidRequest);
        }
        Self::from_components(
            mode,
            Some(hhmmss / 10_000),
            Some((hhmmss / 100) % 100),
            Some(hhmmss % 100),
        )
    }

    /// Validate explicit HOURS, MINUTES and SECONDS for AFTER or AT.
    pub fn from_components(
        mode: CicsIntervalMode,
        hours: Option<i64>,
        minutes: Option<i64>,
        seconds: Option<i64>,
    ) -> Result<Self, CicsIntervalError> {
        if hours.is_none() && minutes.is_none() && seconds.is_none() {
            return Err(CicsIntervalError::InvalidRequest);
        }
        let hour = checked_component(hours, 99, CicsIntervalError::HoursOutOfRange)?;
        let minute_limit = if hours.is_some() || seconds.is_some() {
            59
        } else {
            5_999
        };
        let second_limit = if hours.is_some() || minutes.is_some() {
            59
        } else {
            359_999
        };
        let minute =
            checked_component(minutes, minute_limit, CicsIntervalError::MinutesOutOfRange)?;
        let second =
            checked_component(seconds, second_limit, CicsIntervalError::SecondsOutOfRange)?;
        // All three checked ranges fit 99:59:59; arithmetic cannot wrap.
        Ok(Self {
            mode,
            seconds: hour * 3_600 + minute * 60 + second,
        })
    }

    /// The normalized seconds, including explicit hours beyond the current day.
    #[must_use]
    pub const fn seconds(self) -> u32 {
        self.seconds
    }

    /// Whether this value is relative to execution or to local midnight.
    #[must_use]
    pub const fn mode(self) -> CicsIntervalMode {
        self.mode
    }

    /// Resolve an elapsed delay from one validated local clock observation.
    ///
    /// An absolute time in the preceding six hours expires immediately, even
    /// across midnight. Explicit hours greater than 23 refer to future days
    /// and are never reduced modulo a day. `None` rejects an invalid clock
    /// observation; a host clock error is not an IBM operand condition.
    #[must_use]
    pub fn delay_millis(self, local_millis_since_midnight: u64) -> Option<u64> {
        if local_millis_since_midnight >= DAY_MILLIS {
            return None;
        }
        let target = u64::from(self.seconds) * 1_000;
        match self.mode {
            CicsIntervalMode::Relative => Some(target),
            CicsIntervalMode::Absolute if target >= DAY_MILLIS => {
                Some(target - local_millis_since_midnight)
            }
            CicsIntervalMode::Absolute => {
                let forward = (target + DAY_MILLIS - local_millis_since_midnight) % DAY_MILLIS;
                Some(if forward >= DAY_MILLIS - SIX_HOURS_MILLIS {
                    0
                } else {
                    forward
                })
            }
        }
    }

    /// Translate the resolved delay to a deadline on the shared durable clock.
    ///
    /// The caller stores the returned tick once. Replay must reuse that tick,
    /// not calculate a new delay from a later observation. `None` also rejects
    /// overflow and the invalid zero durable-clock tick.
    #[must_use]
    pub fn deadline_tick(self, logical_now: u64, local_millis_since_midnight: u64) -> Option<u64> {
        if logical_now == 0 {
            return None;
        }
        logical_now.checked_add(self.delay_millis(local_millis_since_midnight)?)
    }
}

fn checked_component(
    value: Option<i64>,
    maximum: i64,
    error: CicsIntervalError,
) -> Result<u32, CicsIntervalError> {
    let value = value.unwrap_or(0);
    if !(0..=maximum).contains(&value) {
        return Err(error);
    }
    u32::try_from(value).map_err(|_| error)
}

#[cfg(test)]
mod tests {
    use super::{CicsIntervalError as Error, CicsIntervalMode as Mode, CicsIntervalTime as Time};

    const HOUR: u64 = 3_600_000;

    #[test]
    fn hhmmss_is_not_a_decimal_count_of_seconds() {
        assert_eq!(
            Time::from_hhmmss(Mode::Relative, 10000).unwrap().seconds(),
            3600
        );
        assert_eq!(
            Time::from_hhmmss(Mode::Relative, 401000).unwrap().seconds(),
            144600
        );
        assert_eq!(
            Time::from_hhmmss(Mode::Relative, 995959).unwrap().seconds(),
            359999
        );
        for (input, expected) in [
            (-1, Error::InvalidRequest),
            (i64::MIN, Error::InvalidRequest),
            (1000000, Error::HoursOutOfRange),
            (i64::MAX, Error::HoursOutOfRange),
            (6000, Error::MinutesOutOfRange),
            (60, Error::SecondsOutOfRange),
        ] {
            assert_eq!(Time::from_hhmmss(Mode::Relative, input), Err(expected));
        }
    }

    #[test]
    fn single_units_have_the_source_defined_extended_ranges() {
        for (hours, minutes, seconds, expected) in [
            (Some(1), None, None, 3600),
            (None, Some(62), None, 3720),
            (None, None, Some(3723), 3723),
            (Some(99), None, None, 356400),
            (None, Some(5999), None, 359940),
            (None, None, Some(359999), 359999),
            (Some(1), None, Some(3), 3603),
        ] {
            for mode in [Mode::Relative, Mode::Absolute] {
                let time = Time::from_components(mode, hours, minutes, seconds).unwrap();
                assert_eq!(time.seconds(), expected);
                assert_eq!(time.mode(), mode);
            }
        }
    }

    #[test]
    fn explicitly_zero_components_still_narrow_other_unit_bounds() {
        for (hours, minutes, seconds, expected) in [
            (None, None, None, Error::InvalidRequest),
            (Some(-1), None, None, Error::HoursOutOfRange),
            (Some(100), None, None, Error::HoursOutOfRange),
            (None, Some(-1), None, Error::MinutesOutOfRange),
            (None, Some(6000), None, Error::MinutesOutOfRange),
            (Some(0), Some(60), None, Error::MinutesOutOfRange),
            (None, Some(60), Some(0), Error::MinutesOutOfRange),
            (None, None, Some(-1), Error::SecondsOutOfRange),
            (None, None, Some(360000), Error::SecondsOutOfRange),
            (Some(0), None, Some(60), Error::SecondsOutOfRange),
            (None, Some(0), Some(60), Error::SecondsOutOfRange),
        ] {
            assert_eq!(
                Time::from_components(Mode::Relative, hours, minutes, seconds),
                Err(expected)
            );
        }
        assert_eq!(Error::HoursOutOfRange.start_response2(), 4);
        assert_eq!(Error::MinutesOutOfRange.start_response2(), 5);
        assert_eq!(Error::SecondsOutOfRange.start_response2(), 6);
    }

    #[test]
    fn absolute_expiration_matches_the_published_start_examples() {
        for (current_hour, requested, expected_delay) in [
            (5, 123000, 7 * HOUR + HOUR / 2),
            (7, 123000, 5 * HOUR + HOUR / 2),
            (5, 20000, 0),
            (7, 20000, 0),
            (5, 3000, 0),
            (7, 3000, 17 * HOUR + HOUR / 2),
            (2, 230000, 0),
            (2, 250000, 23 * HOUR),
            (2, 490000, 47 * HOUR),
        ] {
            let time = Time::from_hhmmss(Mode::Absolute, requested).unwrap();
            assert_eq!(time.delay_millis(current_hour * HOUR), Some(expected_delay));
        }
    }

    #[test]
    fn absolute_lookback_preserves_millisecond_and_midnight_boundaries() {
        let midnight = Time::from_hhmmss(Mode::Absolute, 0).unwrap();
        assert_eq!(midnight.delay_millis(0), Some(0));
        assert_eq!(midnight.delay_millis(6 * HOUR), Some(0));
        assert_eq!(midnight.delay_millis(6 * HOUR + 1), Some(18 * HOUR - 1));
        let twenty = Time::from_hhmmss(Mode::Absolute, 200000).unwrap();
        assert_eq!(twenty.delay_millis(2 * HOUR), Some(0));
        assert_eq!(twenty.delay_millis(2 * HOUR + 1), Some(18 * HOUR - 1));
        assert_eq!(midnight.delay_millis(24 * HOUR - 1), Some(1));
        assert_eq!(Time::immediate().delay_millis(7 * HOUR), Some(0));
        assert_eq!(midnight.delay_millis(7 * HOUR), Some(17 * HOUR));
    }

    #[test]
    fn durable_deadline_translation_is_checked_and_keeps_clock_domains_separate() {
        let time = Time::from_hhmmss(Mode::Absolute, 80000).unwrap();
        assert_eq!(time.deadline_tick(1234, 7 * HOUR), Some(1234 + HOUR));
        assert_eq!(time.deadline_tick(0, 7 * HOUR), None);
        assert_eq!(time.deadline_tick(1234, 24 * HOUR), None);
        assert_eq!(time.deadline_tick(u64::MAX, 7 * HOUR), None);
        let relative = Time::from_components(Mode::Relative, None, None, Some(1)).unwrap();
        assert_eq!(relative.deadline_tick(u64::MAX - 1000, 0), Some(u64::MAX));
        assert_eq!(relative.deadline_tick(u64::MAX - 999, 0), None);
    }
}
