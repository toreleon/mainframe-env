//! Source-bounded conversion of the four architected DATESTRING forms.

use super::super::super::{CicsService, Run, decimal_payload};
use super::super::condition;
use super::{
    ClockInstant, absolute_milliseconds, civil_from_days, days_from_civil, instant_from_absolute,
};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::ConvertTime {
        return Err(HostProblem::Malformed);
    }
    let source = request
        .arguments
        .get("DATESTRING")
        .ok_or(HostProblem::Malformed)?;
    if source.schema() != "mainframe-env.cics.storage-value@1" || source.bytes().len() != 64 {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "DATESTRING" => true,
            "ABSTIME" | "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if !request.arguments.contains_key("ABSTIME") {
        return Err(HostProblem::Malformed);
    }
    let parsed = parse_date_string(source.bytes());
    let (response, value) = match parsed {
        Ok(instant) => (
            service.response(
                run,
                CicsDisposition::Complete,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            )?,
            absolute_milliseconds(instant)?,
        ),
        Err(response2) => (
            condition::respond(
                service,
                run,
                &request.condition_policy,
                HostProblem::Condition {
                    name: "INVREQ".into(),
                    response: 16,
                    response2,
                },
            )?,
            0,
        ),
    };
    let mut response = response;
    response
        .outputs
        .insert("ABSTIME".into(), decimal_payload(value)?);
    Ok(response)
}

fn parse_date_string(bytes: &[u8]) -> Result<ClockInstant, i32> {
    let text = std::str::from_utf8(bytes).map_err(|_| 1)?;
    if !text.is_ascii() {
        return Err(1);
    }
    let text = text.trim_end_matches([' ', '\0']);
    if text.is_empty() || text.contains('\0') {
        return Err(1);
    }
    if text.as_bytes().get(4) == Some(&b'-') {
        parse_rfc3339(text)
    } else if text.contains(',') {
        let (weekday, _) = text.split_once(',').ok_or(1)?;
        if weekday.len() == 3 {
            parse_rfc1123(text)
        } else {
            parse_rfc850(text)
        }
    } else {
        parse_asctime(text)
    }
}

fn parse_rfc3339(text: &str) -> Result<ClockInstant, i32> {
    let (date, time) = text.split_once('T').ok_or(1)?;
    let mut parts = date.split('-');
    let (year, month, day) = (
        year(parts.next().ok_or(1)?)?,
        month_number(parts.next().ok_or(1)?)?,
        day_number(parts.next().ok_or(1)?)?,
    );
    if parts.next().is_some() {
        return Err(1);
    }
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else {
        let split = time.rfind(['+', '-']).ok_or(9)?;
        let (clock, zone) = time.split_at(split);
        (clock, offset(zone, true)?)
    };
    let (whole, fraction) = clock
        .split_once('.')
        .map_or((clock, None), |(a, b)| (a, Some(b)));
    let (hour, minute, second) = hms(whole)?;
    let millisecond = match fraction {
        None => 0,
        Some(digits) if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) => {
            digits
                .bytes()
                .take(3)
                .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
                * 10u32.pow(3 - digits.len().min(3) as u32)
        }
        Some(_) => return Err(8),
    };
    checked_instant(
        ClockInstant {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
        },
        offset,
        None,
    )
}

fn parse_rfc1123(text: &str) -> Result<ClockInstant, i32> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let [weekday, day, month, year_text, time, zone] = words.as_slice() else {
        return Err(1);
    };
    let weekday = weekday.strip_suffix(',').ok_or(5)?;
    let year = year(year_text)?;
    let month = month_name(month)?;
    let day = day_number(day)?;
    let (hour, minute, second) = hms(time)?;
    let offset = if *zone == "GMT" {
        0
    } else {
        offset(zone, false)?
    };
    checked_instant(
        ClockInstant {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        },
        offset,
        Some(weekday),
    )
}

fn parse_rfc850(text: &str) -> Result<ClockInstant, i32> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let [weekday, date, time, zone] = words.as_slice() else {
        return Err(1);
    };
    let weekday = weekday.strip_suffix(',').ok_or(5)?;
    if *zone != "GMT" {
        return Err(7);
    }
    let mut parts = date.split('-');
    let day = day_number(parts.next().ok_or(1)?)?;
    let month = month_name(parts.next().ok_or(1)?)?;
    let short_year = fixed_number(parts.next().ok_or(1)?, 2).ok_or(1)?;
    if parts.next().is_some() {
        return Err(1);
    }
    let year = if short_year <= 69 {
        2000 + i64::from(short_year)
    } else {
        1900 + i64::from(short_year)
    };
    let (hour, minute, second) = hms(time)?;
    checked_instant(
        ClockInstant {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        },
        0,
        Some(weekday),
    )
}

fn parse_asctime(text: &str) -> Result<ClockInstant, i32> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let [weekday, month, day, time, year_text] = words.as_slice() else {
        return Err(1);
    };
    if weekday.len() != 3 {
        return Err(5);
    }
    let year = year(year_text)?;
    let month = month_name(month)?;
    let day = day_number(day)?;
    let (hour, minute, second) = hms(time)?;
    checked_instant(
        ClockInstant {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: 0,
        },
        0,
        Some(weekday),
    )
}

fn fixed_number(text: &str, width: usize) -> Option<u32> {
    (text.len() == width && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn year(text: &str) -> Result<i64, i32> {
    let value = fixed_number(text, 4).ok_or(1)?;
    if value < 1900 {
        Err(4)
    } else {
        Ok(i64::from(value))
    }
}

fn month_number(text: &str) -> Result<u32, i32> {
    let value = fixed_number(text, 2).ok_or(3)?;
    if (1..=12).contains(&value) {
        Ok(value)
    } else {
        Err(3)
    }
}

fn month_name(text: &str) -> Result<u32, i32> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .iter()
        .position(|month| *month == text)
        .map(|index| index as u32 + 1)
        .ok_or(3)
}

fn day_number(text: &str) -> Result<u32, i32> {
    if !(1..=2).contains(&text.len()) || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(6);
    }
    let value = text.parse::<u32>().map_err(|_| 6)?;
    if (1..=31).contains(&value) {
        Ok(value)
    } else {
        Err(6)
    }
}

fn hms(text: &str) -> Result<(u32, u32, u32), i32> {
    let mut parts = text.split(':');
    let (hour, minute, second) = (
        fixed_number(parts.next().ok_or(2)?, 2).ok_or(2)?,
        fixed_number(parts.next().ok_or(2)?, 2).ok_or(2)?,
        fixed_number(parts.next().ok_or(2)?, 2).ok_or(2)?,
    );
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return Err(2);
    }
    Ok((hour, minute, second))
}

fn offset(text: &str, colon: bool) -> Result<i32, i32> {
    let (sign, rest) = match text.as_bytes().first() {
        Some(b'+') => (1, &text[1..]),
        Some(b'-') => (-1, &text[1..]),
        _ => return Err(9),
    };
    let (hours, minutes) = if colon {
        let (hours, minutes) = rest.split_once(':').ok_or(9)?;
        (
            fixed_number(hours, 2).ok_or(9)?,
            fixed_number(minutes, 2).ok_or(9)?,
        )
    } else if rest.len() == 4 {
        (
            fixed_number(&rest[..2], 2).ok_or(9)?,
            fixed_number(&rest[2..], 2).ok_or(9)?,
        )
    } else {
        return Err(9);
    };
    if hours > 12 || minutes > 59 || hours == 12 && minutes > 0 {
        return Err(9);
    }
    Ok(sign * (hours * 60 + minutes) as i32)
}

fn checked_instant(
    instant: ClockInstant,
    offset_minutes: i32,
    weekday: Option<&str>,
) -> Result<ClockInstant, i32> {
    let ClockInstant {
        year, month, day, ..
    } = instant;
    let days = days_from_civil(year, month, day).ok_or(6)?;
    if civil_from_days(days) != (year, month, day) {
        return Err(6);
    }
    if let Some(weekday) = weekday {
        let index = (days + 4).rem_euclid(7) as usize;
        const SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const LONG: [&str; 7] = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        if weekday != SHORT[index] && weekday != LONG[index] {
            return Err(5);
        }
    }
    let absolute = absolute_milliseconds(instant).map_err(|_| 4)?;
    let normalized = absolute
        .checked_sub(i64::from(offset_minutes) * 60_000)
        .ok_or(4)?;
    instant_from_absolute(normalized).map_err(|_| 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str) -> [u8; 64] {
        let mut value = [b' '; 64];
        value[..text.len()].copy_from_slice(text.as_bytes());
        value
    }

    #[test]
    fn four_architected_formats_map_to_the_same_millisecond() {
        let expected = parse_date_string(&input("2003-04-01T10:01:02.498Z")).unwrap();
        for text in [
            "Tue, 01 Apr 2003 10:01:02 +0000",
            "Tuesday, 01-Apr-03 10:01:02 GMT",
            "Tue Apr  1 10:01:02 2003",
        ] {
            let mut actual = parse_date_string(&input(text)).unwrap();
            actual.millisecond = 498;
            assert_eq!(actual, expected, "{text}");
        }
        assert_eq!(
            parse_date_string(&input("2003-04-01T12:01:02.498+02:00")),
            Ok(expected)
        );
    }

    #[test]
    fn invalid_calendar_clock_zone_and_fraction_have_exact_conditions() {
        for (text, code) in [
            ("2003-13-01T10:01:02Z", 3),
            ("1899-12-31T10:01:02Z", 4),
            ("2003-02-30T10:01:02Z", 6),
            ("2003-04-01T24:01:02Z", 2),
            ("Mon, 01 Apr 2003 10:01:02 GMT", 5),
            ("Tuesday Apr 1 10:01:02 2003", 5),
            ("Tuesday, 01-Apr-03 10:01:02 UTC", 7),
            ("2003-04-01T10:01:02.aZ", 8),
            ("2003-04-01T10:01:02+13:00", 9),
        ] {
            assert_eq!(parse_date_string(&input(text)), Err(code), "{text}");
        }
    }
}
