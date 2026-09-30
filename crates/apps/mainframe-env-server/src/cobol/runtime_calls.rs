use super::*;

pub(super) fn decode_cobol_call_values(
    payload: &BoundedPayload,
) -> Result<Vec<Vec<u8>>, HostProblem> {
    if !matches!(
        payload.schema(),
        "mainframe-env.cobol.call@1" | "mainframe-env.cobol.batch-main@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    let bytes = payload.bytes();
    let mut at = 0usize;
    let take = |at: &mut usize, amount: usize| -> Result<&[u8], HostProblem> {
        let end = at
            .checked_add(amount)
            .ok_or(HostProblem::ResourceExhausted)?;
        let value = bytes.get(*at..end).ok_or(HostProblem::Malformed)?;
        *at = end;
        Ok(value)
    };
    let count = usize::try_from(u32::from_be_bytes(
        take(&mut at, 4)?
            .try_into()
            .map_err(|_| HostProblem::Malformed)?,
    ))
    .map_err(|_| HostProblem::ResourceExhausted)?;
    if count > InvocationLimits::default().max_bindings {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let name_length = usize::try_from(u64::from_be_bytes(
            take(&mut at, 8)?
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let _name = take(&mut at, name_length)?;
        if take(&mut at, 1)? != [1] {
            return Err(HostProblem::Malformed);
        }
        let value_length = usize::try_from(u64::from_be_bytes(
            take(&mut at, 8)?
                .try_into()
                .map_err(|_| HostProblem::Malformed)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        values.push(take(&mut at, value_length)?.to_vec());
    }
    if at != bytes.len() {
        return Err(HostProblem::Malformed);
    }
    Ok(values)
}

pub(super) fn batch_main_call_arguments(
    parameter: Option<&str>,
) -> Result<BoundedPayload, HostProblem> {
    // The interpreter keeps DISPLAY storage in its native encoding (CP037 is applied only at
    // dataset/JSON/national boundaries), so the PARM text is passed natively; z/OS passes the
    // same text in EBCDIC to EBCDIC storage. JCL conversion already bounds it to 100 characters.
    let text = parameter.unwrap_or_default().as_bytes().to_vec();
    if text.len() > 100 {
        return Err(HostProblem::Malformed);
    }
    let mut area = u16::try_from(text.len())
        .map_err(|_| HostProblem::Malformed)?
        .to_be_bytes()
        .to_vec();
    area.extend_from_slice(&text);
    let name = b"PARM-AREA";
    let mut bytes = 1u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(&(name.len() as u64).to_be_bytes());
    bytes.extend_from_slice(name);
    bytes.push(1);
    bytes.extend_from_slice(&(area.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&area);
    BoundedPayload::new(
        "mainframe-env.cobol.batch-main@1",
        bytes,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

pub(super) fn execute_ceedays(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let mut values = decode_cobol_call_values(payload)?;
    if values.len() != 4 {
        return Err(HostProblem::Malformed);
    }
    let date = cee_vstring(&values[0]).ok_or(HostProblem::Malformed)?;
    let picture = cee_vstring(&values[1]).ok_or(HostProblem::Malformed)?;
    let parsed = parse_ceedays_date(date, picture);
    values[2].fill(0);
    values[3].fill(0);
    if let Some((year, month, day)) = parsed {
        let lilian = civil_day(year, month, day) - civil_day(1582, 10, 14);
        let lilian = i32::try_from(lilian).map_err(|_| HostProblem::ResourceExhausted)?;
        if values[2].len() != 4 {
            return Err(HostProblem::Malformed);
        }
        values[2].copy_from_slice(&lilian.to_be_bytes());
    } else {
        if values[3].len() < 8 {
            return Err(HostProblem::Malformed);
        }
        values[3][1] = 3;
        values[3][3] = 1;
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

pub(super) fn execute_mvswait(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let values = decode_cobol_call_values(payload)?;
    if values.len() != 1 || values[0].len() != 4 {
        return Err(HostProblem::Malformed);
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

pub(super) fn execute_cobdatft(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let mut values = decode_cobol_call_values(payload)?;
    if values.len() != 1 || values[0].len() < 80 {
        return Err(HostProblem::Malformed);
    }
    let record = &mut values[0];
    let input = record[1..21].to_vec();
    let valid = match (record[0], record[21]) {
        (b'1', b'1') if input.get(4) != Some(&b'-') => {
            record[22..26].copy_from_slice(&input[..4]);
            record[26] = b'-';
            record[27..29].copy_from_slice(&input[4..6]);
            record[29] = b'-';
            record[30..32].copy_from_slice(&input[6..8]);
            true
        }
        (b'2', b'2') => {
            record[22..26].copy_from_slice(&input[..4]);
            record[26..28].copy_from_slice(&input[5..7]);
            record[28..30].copy_from_slice(&input[8..10]);
            true
        }
        _ => false,
    };
    if !valid {
        record[42..55].copy_from_slice(b"INVALID INPUT");
    }
    encode_cobol_call_result(&values).map_err(|_| HostProblem::ProviderFailure)
}

pub(super) fn execute_cee3abd(payload: &BoundedPayload) -> Result<BoundedPayload, HostProblem> {
    let values = decode_cobol_call_values(payload)?;
    if values.len() > 2 || values.iter().any(|value| value.len() != 4) {
        return Err(HostProblem::Malformed);
    }
    let code = values.first().map_or(999, |value| {
        i32::from_be_bytes(value.as_slice().try_into().unwrap_or(999i32.to_be_bytes()))
    });
    BoundedPayload::new(
        "mainframe-env.program.abend@1",
        format!("U{:04}", code.unsigned_abs().min(9999)).into_bytes(),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)
}

pub(super) fn cee_vstring(value: &[u8]) -> Option<&[u8]> {
    let length = usize::try_from(i16::from_be_bytes(value.get(..2)?.try_into().ok()?)).ok()?;
    value.get(2..2usize.checked_add(length)?)
}

#[cfg(test)]
pub(super) fn valid_ceedays_date(value: &[u8], picture: &[u8]) -> bool {
    parse_ceedays_date(value, picture).is_some()
}

pub(super) fn parse_ceedays_date(value: &[u8], picture: &[u8]) -> Option<(u32, u32, u32)> {
    let value = trim_ascii(value);
    let picture = trim_ascii(picture);
    let (year, month, day) = match picture {
        b"YYYY-MM-DD"
            if value.len() == 10 && value.get(4) == Some(&b'-') && value.get(7) == Some(&b'-') =>
        {
            (&value[..4], &value[5..7], &value[8..])
        }
        b"YYYYMMDD" if value.len() == 8 => (&value[..4], &value[4..6], &value[6..]),
        _ => return None,
    };
    let number = |bytes: &[u8]| -> Option<u32> {
        bytes.iter().try_fold(0u32, |value, byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + u32::from(*byte - b'0'))
        })
    };
    let year = number(year)?;
    let month = number(month)?;
    let day = number(day)?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (year <= 9999 && (year, month, day) >= (1582, 10, 15) && day >= 1 && day <= days)
        .then_some((year, month, day))
}

pub(super) fn civil_day(year: u32, month: u32, day: u32) -> i64 {
    let mut year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era
}

pub(super) fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(|byte| matches!(*byte, 0 | b' ')) {
        value = &value[1..];
    }
    while value.last().is_some_and(|byte| matches!(*byte, 0 | b' ')) {
        value = &value[..value.len() - 1];
    }
    value
}
