use super::super::{CicsService, Run, argument_text, bounded, decimal_payload};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, ClockRequest, HostProblem,
    HostRequest, HostResult,
};

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::Asktime | CicsOperation::AsktimeEib => asktime(service, run, request),
        CicsOperation::FormatTime => format_time(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn asktime(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_asktime_request(request)?;
    let timestamp = match service.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
        HostResult::Clock(value) => value,
        _ => return Err(HostProblem::ProviderFailure),
    };
    let instant = parse_clock_timestamp(&timestamp)?;
    let (eib_date, eib_time) = eib_date_time(instant)?;
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    response
        .outputs
        .insert("EIBDATE".into(), decimal_payload(eib_date)?);
    response
        .outputs
        .insert("EIBTIME".into(), decimal_payload(eib_time)?);
    if request.operation == CicsOperation::Asktime {
        response.outputs.insert(
            "ABSTIME".into(),
            decimal_payload(absolute_milliseconds(instant)?)?,
        );
    }
    Ok(response)
}

fn validate_asktime_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = if request.operation == CicsOperation::Asktime {
        &["ABSTIME", "OPTION.NOHANDLE", "RESP", "RESP2"][..]
    } else {
        &["OPTION.NOHANDLE", "RESP", "RESP2"][..]
    };
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || if name == "OPTION.NOHANDLE" {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else {
                value.schema() != "mainframe-env.cics.argument@1"
            }
    }) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn format_time(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_format_time_request(request)?;
    let absolute = argument_text(request, "ABSTIME")?
        .trim()
        .parse::<i64>()
        .map_err(|_| invalid_absolute_time())?;
    let instant = instant_from_absolute(absolute).map_err(|problem| {
        if problem == HostProblem::Malformed {
            invalid_absolute_time()
        } else {
            problem
        }
    })?;
    let date_separator = separator(request, "DATESEP", b'/')?;
    let time_separator = separator(request, "TIMESEP", b':')?;
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for key in ["YYYYMMDD", "YYMMDD", "MMDDYY", "MMDDYYYY", "YYDDD"] {
        if request.arguments.contains_key(key) {
            let value = format_date(instant, key, date_separator)?;
            response.outputs.insert(key.into(), bounded(value)?);
        }
    }
    if request.arguments.contains_key("TIME") {
        response.outputs.insert(
            "TIME".into(),
            bounded(format_time_value(instant, time_separator))?,
        );
    }
    if request.arguments.contains_key("MILLISECONDS") {
        response.outputs.insert(
            "MILLISECONDS".into(),
            decimal_payload(i64::from(instant.millisecond))?,
        );
    }
    Ok(response)
}

fn validate_format_time_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const OUTPUTS: &[&str] = &[
        "MILLISECONDS",
        "MMDDYY",
        "MMDDYYYY",
        "TIME",
        "YYDDD",
        "YYMMDD",
        "YYYYMMDD",
    ];
    if !request.arguments.contains_key("ABSTIME") {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "ABSTIME" => matches!(
                value.schema(),
                "mainframe-env.cics.decimal@1" | "mainframe-env.cics.argument@1"
            ),
            "DATESEP" | "TIMESEP" => {
                matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                        | "mainframe-env.cics.argument@1"
                ) && value.bytes().len() == 1
            }
            "OPTION.DATESEP" | "OPTION.NOHANDLE" | "OPTION.TIMESEP" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            name if OUTPUTS.contains(&name) => value.schema() == "mainframe-env.cics.argument@1",
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn invalid_absolute_time() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 1,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClockInstant {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
}

fn parse_clock_timestamp(value: &str) -> Result<ClockInstant, HostProblem> {
    if value.len() != 17 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HostProblem::ProviderFailure);
    }
    let number = |range: std::ops::Range<usize>| {
        value[range]
            .parse::<u32>()
            .map_err(|_| HostProblem::ProviderFailure)
    };
    let instant = ClockInstant {
        year: i64::from(number(0..4)?),
        month: number(4..6)?,
        day: number(6..8)?,
        hour: number(8..10)?,
        minute: number(10..12)?,
        second: number(12..14)?,
        millisecond: number(14..17)?,
    };
    validate_instant(instant)?;
    Ok(instant)
}

fn validate_instant(instant: ClockInstant) -> Result<(), HostProblem> {
    let days = days_from_civil(instant.year, instant.month, instant.day)
        .ok_or(HostProblem::ProviderFailure)?;
    let (year, month, day) = civil_from_days(days);
    if (year, month, day) != (instant.year, instant.month, instant.day)
        || instant.hour > 23
        || instant.minute > 59
        || instant.second > 59
        || instant.millisecond > 999
    {
        Err(HostProblem::ProviderFailure)
    } else {
        Ok(())
    }
}

fn absolute_milliseconds(instant: ClockInstant) -> Result<i64, HostProblem> {
    let epoch = days_from_civil(1900, 1, 1).ok_or(HostProblem::ProviderFailure)?;
    let days = days_from_civil(instant.year, instant.month, instant.day)
        .ok_or(HostProblem::ProviderFailure)?
        .checked_sub(epoch)
        .ok_or(HostProblem::ResourceExhausted)?;
    days.checked_mul(86_400_000)
        .and_then(|value| value.checked_add(i64::from(instant.hour) * 3_600_000))
        .and_then(|value| value.checked_add(i64::from(instant.minute) * 60_000))
        .and_then(|value| value.checked_add(i64::from(instant.second) * 1_000))
        .and_then(|value| value.checked_add(i64::from(instant.millisecond)))
        .ok_or(HostProblem::ResourceExhausted)
}

fn eib_date_time(instant: ClockInstant) -> Result<(i64, i64), HostProblem> {
    let century = match instant.year {
        1900..=1999 => 0,
        2000..=2099 => 1,
        _ => return Err(HostProblem::ProviderFailure),
    };
    let january_first = days_from_civil(instant.year, 1, 1).ok_or(HostProblem::ProviderFailure)?;
    let current = days_from_civil(instant.year, instant.month, instant.day)
        .ok_or(HostProblem::ProviderFailure)?;
    let ordinal = current
        .checked_sub(january_first)
        .and_then(|value| value.checked_add(1))
        .ok_or(HostProblem::ProviderFailure)?;
    let year = instant.year % 100;
    let date = century * 100_000 + year * 1_000 + ordinal;
    let time = i64::from(instant.hour) * 10_000
        + i64::from(instant.minute) * 100
        + i64::from(instant.second);
    Ok((date, time))
}

fn instant_from_absolute(value: i64) -> Result<ClockInstant, HostProblem> {
    if value < 0 {
        return Err(HostProblem::Malformed);
    }
    let epoch = days_from_civil(1900, 1, 1).ok_or(HostProblem::ProviderFailure)?;
    let days = value / 86_400_000;
    let rest = value % 86_400_000;
    let (year, month, day) = civil_from_days(
        epoch
            .checked_add(days)
            .ok_or(HostProblem::ResourceExhausted)?,
    );
    Ok(ClockInstant {
        year,
        month,
        day,
        hour: u32::try_from(rest / 3_600_000).map_err(|_| HostProblem::Malformed)?,
        minute: u32::try_from((rest / 60_000) % 60).map_err(|_| HostProblem::Malformed)?,
        second: u32::try_from((rest / 1_000) % 60).map_err(|_| HostProblem::Malformed)?,
        millisecond: u32::try_from(rest % 1_000).map_err(|_| HostProblem::Malformed)?,
    })
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn separator(request: &CicsRequest, name: &str, default: u8) -> Result<Option<u8>, HostProblem> {
    if let Some(value) = request.arguments.get(name) {
        if value.bytes().len() != 1 {
            return Err(HostProblem::Malformed);
        }
        Ok(value.bytes().first().copied())
    } else if request.arguments.contains_key(&format!("OPTION.{name}")) {
        Ok(Some(default))
    } else {
        Ok(None)
    }
}

fn format_date(
    instant: ClockInstant,
    format: &str,
    separator: Option<u8>,
) -> Result<Vec<u8>, HostProblem> {
    let year = u32::try_from(instant.year).map_err(|_| HostProblem::Malformed)?;
    let parts = match format {
        "YYYYMMDD" => vec![
            format!("{year:04}"),
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
        ],
        "YYMMDD" => vec![
            format!("{:02}", year % 100),
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
        ],
        "MMDDYY" => vec![
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
            format!("{:02}", year % 100),
        ],
        "MMDDYYYY" => vec![
            format!("{:02}", instant.month),
            format!("{:02}", instant.day),
            format!("{year:04}"),
        ],
        "YYDDD" => {
            let jan1 = days_from_civil(instant.year, 1, 1).ok_or(HostProblem::Malformed)?;
            let current = days_from_civil(instant.year, instant.month, instant.day)
                .ok_or(HostProblem::Malformed)?;
            return Ok(format!("{:02}{:03}", year % 100, current - jan1 + 1).into_bytes());
        }
        _ => return Err(HostProblem::Malformed),
    };
    let joiner = separator.map_or_else(String::new, |value| char::from(value).to_string());
    Ok(parts.join(&joiner).into_bytes())
}

fn format_time_value(instant: ClockInstant, separator: Option<u8>) -> Vec<u8> {
    let parts = [
        format!("{:02}", instant.hour),
        format!("{:02}", instant.minute),
        format!("{:02}", instant.second),
    ];
    let joiner = separator.map_or_else(String::new, |value| char::from(value).to_string());
    parts.join(&joiner).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_time_roundtrips_across_leap_day() {
        let instant = ClockInstant {
            year: 2024,
            month: 2,
            day: 29,
            hour: 23,
            minute: 59,
            second: 58,
            millisecond: 321,
        };
        let absolute = absolute_milliseconds(instant).unwrap();
        assert_eq!(instant_from_absolute(absolute).unwrap(), instant);
    }

    #[test]
    fn impossible_clock_dates_fail_closed() {
        assert_eq!(
            parse_clock_timestamp("20260230123456789"),
            Err(HostProblem::ProviderFailure)
        );
    }

    #[test]
    fn eib_date_and_time_are_exact_packed_decimal_coefficients() {
        assert_eq!(
            eib_date_time(ClockInstant {
                year: 2026,
                month: 8,
                day: 30,
                hour: 12,
                minute: 34,
                second: 56,
                millisecond: 789,
            }),
            Ok((126_242, 123_456))
        );
        assert_eq!(
            eib_date_time(ClockInstant {
                year: 2100,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                millisecond: 0,
            }),
            Err(HostProblem::ProviderFailure)
        );
    }
}
