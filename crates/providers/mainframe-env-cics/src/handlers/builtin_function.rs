//! Source-bounded CICS built-in functions over caller-owned data areas.

use super::super::{CicsService, Run, bounded};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::BifDeedit => deedit_request(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn deedit_request(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let field = request
        .arguments
        .get("FIELD")
        .ok_or(HostProblem::Malformed)?;
    if field.schema() != "mainframe-env.cics.storage-value@1" || request.mutation.is_some() {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "FIELD" => true,
            "LENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    let length = if let Some(value) = request.arguments.get("LENGTH") {
        let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
        let number = text.parse::<i64>().map_err(|_| HostProblem::Malformed)?;
        if number < 1 || usize::try_from(number).is_err() {
            return Err(length_error());
        }
        usize::try_from(number).map_err(|_| length_error())?
    } else {
        field.bytes().len()
    };
    if length == 0 || length > field.bytes().len() {
        return Err(length_error());
    }
    let edited = deedit(field.bytes(), length);
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
    response.outputs.insert("FIELD".into(), bounded(edited)?);
    Ok(response)
}

fn length_error() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 0,
    }
}

fn deedit(field: &[u8], length: usize) -> Vec<u8> {
    let mut result = field.to_vec();
    if length == 1 {
        return result;
    }
    let selected = &field[..length];
    let (end, negative) = if selected.ends_with(b"CR") || selected.ends_with(&[0xC3, 0xD9]) {
        (length - 2, true)
    } else if selected
        .last()
        .is_some_and(|byte| matches!(byte, b'-' | 0x60))
    {
        (length - 1, true)
    } else {
        (length, false)
    };
    let ebcdic = selected.iter().any(|byte| matches!(byte, 0xF0..=0xF9))
        || selected[end.saturating_sub(1)..end]
            .first()
            .is_some_and(|byte| matches!(byte >> 4, 0xA..=0xF) && (byte & 0x0F) <= 9);
    let zero = if ebcdic { 0xF0 } else { b'0' };
    let mut digits = Vec::with_capacity(length);
    for (index, byte) in selected[..end].iter().copied().enumerate() {
        let ascii_overpunch = (index + 1 == end)
            .then(|| ascii_zoned_digit(byte))
            .flatten();
        let preserved_zone = index + 1 == end
            && ((matches!(byte >> 4, 0xA..=0xF) && (byte & 0x0F) <= 9)
                || ascii_overpunch.is_some());
        let digit = if byte.is_ascii_digit() {
            Some(byte - b'0')
        } else if let Some(digit) = ascii_overpunch {
            Some(digit)
        } else if (0xF0..=0xF9).contains(&byte) || preserved_zone {
            Some(byte & 0x0F)
        } else {
            None
        };
        if let Some(digit) = digit {
            digits.push((digit, preserved_zone.then_some(byte)));
        }
    }
    result[..length].fill(zero);
    for (index, (digit, zone)) in digits.iter().rev().take(length).enumerate() {
        let slot = length - index - 1;
        result[slot] = zone.unwrap_or(zero + digit);
    }
    if negative {
        let last = digits.last().map_or(0, |(digit, _)| *digit);
        result[length - 1] = 0xD0 | last;
    }
    result
}

fn ascii_zoned_digit(byte: u8) -> Option<u8> {
    // COBOL source literals arrive as host text; preserve CP037 display glyphs
    // for the documented A-F terminal zones as well as raw EBCDIC bytes.
    match byte {
        b'^' | b'{' | b'}' | b'\\' => Some(0),
        b'~' | b'A' | b'J' => Some(1),
        b's'..=b'z' => Some(byte - b's' + 2),
        b'B'..=b'I' => Some(byte - b'A' + 1),
        b'K'..=b'R' => Some(byte - b'J' + 1),
        b'S'..=b'Z' => Some(byte - b'S' + 2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::deedit;

    #[test]
    fn deedit_removes_editing_right_aligns_and_preserves_suffix() {
        assert_eq!(deedit(b"$25.68   ", 9), b"000002568");
        assert_eq!(deedit(b"14-6704/B", 9), b"00146704B");
        assert_eq!(deedit(b"8.7R", 4), b"087R");
        assert_eq!(deedit(b"$25.68XX", 6), b"002568XX");
        assert_eq!(deedit(b"-", 1), b"-");
    }

    #[test]
    fn deedit_preserves_zoned_tail_and_marks_negative() {
        assert_eq!(deedit(&[0xF1, 0xF2, 0xC3], 3), vec![0xF1, 0xF2, 0xC3]);
        assert_eq!(deedit(&[0xF1, 0xF2, 0x60], 3), vec![0xF0, 0xF1, 0xD2]);
        assert_eq!(
            deedit(&[0xF4, 0xF5, 0xC3, 0xD9], 4),
            vec![0xF0, 0xF0, 0xF4, 0xD5]
        );
    }
}
