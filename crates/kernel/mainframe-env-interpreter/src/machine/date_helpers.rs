use super::*;

pub(super) fn split_yyyymmdd(value: i128) -> Result<(i32, u32, u32), MachineProblem> {
    let value = i64::try_from(value).map_err(|_| MachineProblem::DataException)?;
    let year = i32::try_from(value / 10_000).map_err(|_| MachineProblem::DataException)?;
    let month = u32::try_from((value / 100) % 100).map_err(|_| MachineProblem::DataException)?;
    let day = u32::try_from(value % 100).map_err(|_| MachineProblem::DataException)?;
    if !valid_date(year, month, day) {
        return Err(MachineProblem::DataException);
    }
    Ok((year, month, day))
}

pub(super) fn day_of_integer(value: i128) -> Result<Decimal, MachineProblem> {
    let value = i64::try_from(value).map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = cobol_date_of_integer(value)?;
    let ordinal = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
    Ok(Decimal {
        coefficient: i128::from(year) * 1_000 + i128::from(ordinal),
        scale: 0,
    })
}

pub(super) fn integer_of_day(value: i128) -> Result<Decimal, MachineProblem> {
    let year = i32::try_from(value / 1_000).map_err(|_| MachineProblem::DataException)?;
    let ordinal = i64::try_from(value % 1_000).map_err(|_| MachineProblem::DataException)?;
    let maximum = if valid_date(year, 2, 29) { 366 } else { 365 };
    if !(1..=maximum).contains(&ordinal) {
        return Err(MachineProblem::DataException);
    }
    let days = days_from_civil(year, 1, 1) + ordinal - 1;
    let (year, month, day) = civil_from_days(days);
    Ok(Decimal {
        coefficient: i128::from(cobol_integer_of_date(year, month, day)?),
        scale: 0,
    })
}

pub(super) fn current_year(current_date: &[u8]) -> Result<i128, MachineProblem> {
    let year = current_date
        .get(..4)
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|value| value.parse::<i128>().ok())
        .ok_or(MachineProblem::DataException)?;
    (1_601..=9_999)
        .contains(&year)
        .then_some(year)
        .ok_or(MachineProblem::DataException)
}

pub(super) fn windowed_year(
    value: i128,
    offset: i128,
    current_year: i128,
    trailing_digits: u32,
) -> Result<CobolValue, MachineProblem> {
    let divisor = 10i128
        .checked_pow(trailing_digits)
        .ok_or(MachineProblem::SizeError)?;
    if value < 0 {
        return Err(MachineProblem::DataException);
    }
    let short_year = value / divisor;
    if !(0..=99).contains(&short_year) {
        return Err(MachineProblem::DataException);
    }
    let ending_year = current_year
        .checked_add(offset)
        .filter(|year| (1_700..=9_999).contains(year))
        .ok_or(MachineProblem::DataException)?;
    let mut year = (ending_year / 100) * 100 + short_year;
    if year > ending_year {
        year -= 100;
    }
    Ok(integer_value(year * divisor + value % divisor))
}

pub(super) fn test_date_yyyymmdd(value: i128) -> i128 {
    if !(16_010_000..=99_999_999).contains(&value) {
        return 1;
    }
    let year = (value / 10_000) as i32;
    let month_day = value % 10_000;
    if !(100..=1_299).contains(&month_day) {
        return 2;
    }
    let month = (month_day / 100) as u32;
    let day = (month_day % 100) as u32;
    if !valid_date(year, month, day) {
        return 3;
    }
    0
}

pub(super) fn test_day_yyyyddd(value: i128) -> i128 {
    if !(1_601_000..=9_999_999).contains(&value) {
        return 1;
    }
    let year = (value / 1_000) as i32;
    let day = value % 1_000;
    let maximum = if valid_date(year, 2, 29) { 366 } else { 365 };
    if !(1..=maximum).contains(&day) {
        return 2;
    }
    0
}

pub(super) fn parse_hhmmss(bytes: &[u8]) -> Result<u32, MachineProblem> {
    if bytes.len() != 6 || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(MachineProblem::DataException);
    }
    let value = std::str::from_utf8(bytes)
        .map_err(|_| MachineProblem::DataException)?
        .parse::<u32>()
        .map_err(|_| MachineProblem::DataException)?;
    let hour = value / 10_000;
    let minute = value / 100 % 100;
    let second = value % 100;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(MachineProblem::DataException);
    }
    Ok(hour * 3_600 + minute * 60 + second)
}

pub(super) fn render_datetime_format(
    format: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Result<Vec<u8>, MachineProblem> {
    if !valid_date(year, month, day) || hour > 23 || minute > 59 || second > 59 {
        return Err(MachineProblem::DataException);
    }
    let mut output = format.to_string();
    for (token, value) in [
        ("YYYY", format!("{year:04}")),
        ("YY", format!("{:02}", year.rem_euclid(100))),
        ("MM", format!("{month:02}")),
        ("DD", format!("{day:02}")),
        ("hh", format!("{hour:02}")),
        ("mm", format!("{minute:02}")),
        ("ss", format!("{second:02}")),
    ] {
        output = output.replace(token, &value);
    }
    Ok(output.into_bytes())
}

pub(super) fn format_datetime(format: &str, current: &[u8]) -> Result<Vec<u8>, MachineProblem> {
    if current.len() != 21 {
        return Err(MachineProblem::DataException);
    }
    let (year, month, day) = split_yyyymmdd(
        std::str::from_utf8(&current[..8])
            .map_err(|_| MachineProblem::DataException)?
            .parse()
            .map_err(|_| MachineProblem::DataException)?,
    )?;
    let seconds = parse_hhmmss(&current[8..14])?;
    render_datetime_format(
        format,
        year,
        month,
        day,
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
    )
}

pub(super) fn formatted_date(format: &str, date: i128) -> Result<Vec<u8>, MachineProblem> {
    let date = i64::try_from(date).map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = cobol_date_of_integer(date)?;
    render_datetime_format(format, year, month, day, 0, 0, 0)
}

pub(super) fn formatted_datetime(
    format: &str,
    date: i128,
    time: Decimal,
    _offset: i128,
) -> Result<Vec<u8>, MachineProblem> {
    let date = i64::try_from(date).map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = cobol_date_of_integer(date)?;
    let seconds = decimal_f64(time)?;
    if !(0.0..86_400.0).contains(&seconds) {
        return Err(MachineProblem::DataException);
    }
    let seconds = seconds as u32;
    render_datetime_format(
        format,
        year,
        month,
        day,
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
    )
}

pub(super) fn formatted_time(
    format: &str,
    time: Decimal,
    _offset: i128,
) -> Result<Vec<u8>, MachineProblem> {
    let seconds = decimal_f64(time)?;
    if !(0.0..86_400.0).contains(&seconds) {
        return Err(MachineProblem::DataException);
    }
    let seconds = seconds as u32;
    render_datetime_format(
        format,
        1_600,
        1,
        1,
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
    )
}

pub(super) fn digits_only(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

pub(super) fn integer_of_formatted_date(
    format: &str,
    value: &str,
) -> Result<Decimal, MachineProblem> {
    if !format.contains("YYYY") || !format.contains("MM") || !format.contains("DD") {
        return Err(MachineProblem::DataException);
    }
    let digits = digits_only(value);
    if digits.len() != 8 {
        return Err(MachineProblem::DataException);
    }
    let date = digits
        .parse::<i128>()
        .map_err(|_| MachineProblem::DataException)?;
    let (year, month, day) = split_yyyymmdd(date)?;
    Ok(Decimal {
        coefficient: i128::from(cobol_integer_of_date(year, month, day)?),
        scale: 0,
    })
}

pub(super) fn test_formatted_datetime(format: &str, value: &str) -> usize {
    #[derive(Clone, Copy)]
    enum Field {
        Year,
        ShortYear,
        Month,
        Day,
        Hour,
        Minute,
        Second,
    }

    let format = format.as_bytes();
    let value = value.as_bytes();
    let mut format_at = 0usize;
    let mut value_at = 0usize;
    let mut fields = Vec::new();
    while format_at < format.len() {
        let field = [
            (b"YYYY".as_slice(), 4usize, Field::Year),
            (b"YY".as_slice(), 2, Field::ShortYear),
            (b"MM".as_slice(), 2, Field::Month),
            (b"DD".as_slice(), 2, Field::Day),
            (b"hh".as_slice(), 2, Field::Hour),
            (b"mm".as_slice(), 2, Field::Minute),
            (b"ss".as_slice(), 2, Field::Second),
        ]
        .into_iter()
        .find_map(|(token, width, field)| {
            format[format_at..]
                .starts_with(token)
                .then_some((token.len(), width, field))
        });
        if let Some((token_length, width, field)) = field {
            for offset in 0..width {
                if value
                    .get(value_at + offset)
                    .is_none_or(|byte| !byte.is_ascii_digit())
                {
                    return value_at + offset + 1;
                }
            }
            fields.push((field, value_at, width));
            format_at += token_length;
            value_at += width;
        } else {
            if value.get(value_at) != format.get(format_at) {
                return value_at + 1;
            }
            format_at += 1;
            value_at += 1;
        }
    }
    if value_at != value.len() {
        return value_at + 1;
    }
    let value_of = |wanted: fn(Field) -> bool| {
        fields
            .iter()
            .find(|(field, _, _)| wanted(*field))
            .and_then(|(_, start, width)| value.get(*start..start + width))
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| digits.parse::<u32>().ok())
    };
    let year = value_of(|field| matches!(field, Field::Year));
    let month = value_of(|field| matches!(field, Field::Month));
    let day_limit = match (year, month) {
        (Some(year), Some(month)) if (1_601..=9_999).contains(&year) => {
            days_in_month(year as i32, month).unwrap_or(31)
        }
        _ => 31,
    };
    fields
        .into_iter()
        .filter_map(|(field, start, width)| {
            let range = match field {
                Field::Year => Some((1_601, 9_999)),
                Field::ShortYear => None,
                Field::Month => Some((1, 12)),
                Field::Day => Some((1, day_limit)),
                Field::Hour => Some((0, 23)),
                Field::Minute | Field::Second => Some((0, 59)),
            }?;
            first_range_error(&value[start..start + width], start, range.0, range.1)
        })
        .min()
        .unwrap_or(0)
}

pub(super) fn first_range_error(
    digits: &[u8],
    start: usize,
    minimum: u32,
    maximum: u32,
) -> Option<usize> {
    let width = digits.len();
    for consumed in 1..=width {
        let prefix = std::str::from_utf8(&digits[..consumed])
            .ok()?
            .parse::<u32>()
            .ok()?;
        let factor = 10u32.checked_pow((width - consumed) as u32)?;
        let possible_minimum = prefix.checked_mul(factor)?;
        let possible_maximum = possible_minimum.checked_add(factor - 1)?;
        if possible_maximum < minimum || possible_minimum > maximum {
            return Some(start + consumed);
        }
    }
    None
}

pub(super) fn days_in_month(year: i32, month: u32) -> Option<u32> {
    (1..=12).contains(&month).then(|| match month {
        2 if valid_date(year, 2, 29) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    })
}

pub(super) fn seconds_from_formatted_time(
    format: &str,
    value: &str,
) -> Result<Decimal, MachineProblem> {
    if !format.contains("hh") {
        return Err(MachineProblem::DataException);
    }
    let digits = digits_only(value);
    let time = digits
        .get(digits.len().saturating_sub(6)..)
        .ok_or(MachineProblem::DataException)?;
    Ok(Decimal {
        coefficient: i128::from(parse_hhmmss(time.as_bytes())?),
        scale: 0,
    })
}

pub(super) fn accept_clock_value(
    format: AcceptClockFormat,
    value: &str,
) -> Result<Vec<u8>, MachineProblem> {
    match format {
        AcceptClockFormat::Time => {
            if value.len() != 9 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            Ok(value.as_bytes()[..8].to_vec())
        }
        _ => {
            if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(MachineProblem::UnexpectedHostResult);
            }
            let numeric = value
                .parse::<i128>()
                .map_err(|_| MachineProblem::UnexpectedHostResult)?;
            let (year, month, day) = split_yyyymmdd(numeric)?;
            let ordinal = days_from_civil(year, month, day) - days_from_civil(year, 1, 1) + 1;
            Ok(match format {
                AcceptClockFormat::DateYymmdd => value.as_bytes()[2..].to_vec(),
                AcceptClockFormat::DateYyyymmdd => value.as_bytes().to_vec(),
                AcceptClockFormat::DayYyddd => {
                    format!("{:02}{ordinal:03}", year.rem_euclid(100)).into_bytes()
                }
                AcceptClockFormat::DayYyyyddd => format!("{year:04}{ordinal:03}").into_bytes(),
                AcceptClockFormat::DayOfWeek => {
                    let weekday = (days_from_civil(year, month, day) + 3).rem_euclid(7) + 1;
                    weekday.to_string().into_bytes()
                }
                AcceptClockFormat::Time => unreachable!(),
            })
        }
    }
}

pub(super) fn cobol_integer_of_date(
    year: i32,
    month: u32,
    day: u32,
) -> Result<i64, MachineProblem> {
    if !valid_date(year, month, day) || !(1601..=9999).contains(&year) {
        return Err(MachineProblem::DataException);
    }
    let base = days_from_civil(1600, 12, 31);
    days_from_civil(year, month, day)
        .checked_sub(base)
        .ok_or(MachineProblem::DataException)
}

pub(super) fn cobol_date_of_integer(value: i64) -> Result<(i32, u32, u32), MachineProblem> {
    if value <= 0 {
        return Err(MachineProblem::DataException);
    }
    let base = days_from_civil(1600, 12, 31);
    let days = base
        .checked_add(value)
        .ok_or(MachineProblem::DataException)?;
    let date = civil_from_days(days);
    if date.0 > 9999 {
        return Err(MachineProblem::DataException);
    }
    Ok(date)
}

pub(super) fn valid_date(year: i32, month: u32, day: u32) -> bool {
    if year <= 0 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let length = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    day <= length
}

pub(super) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub(super) fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}
