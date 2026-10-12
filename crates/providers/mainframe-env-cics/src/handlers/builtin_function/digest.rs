//! Source-bounded BIF DIGEST SHA-1 formats over a caller-owned record.

use super::*;
use base64::Engine;

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.mutation.is_some() {
        return Err(HostProblem::Malformed);
    }
    let record = request
        .arguments
        .get("RECORD")
        .ok_or(HostProblem::Malformed)?;
    if !matches!(
        record.schema(),
        "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    let length = decimal(request, "RECORDLEN")?.ok_or(HostProblem::Malformed)?;
    if length < 1 {
        return Err(length_error());
    }
    let length = usize::try_from(length).map_err(|_| length_error())?;
    if length > record.bytes().len() {
        return Err(length_error());
    }
    if request
        .arguments
        .get("RESULT")
        .is_none_or(|value| value.schema() != "mainframe-env.cics.argument@1")
    {
        return Err(HostProblem::Malformed);
    }
    let result_maximum = decimal(request, "RESULT.MAXLENGTH")?;
    let mut flags = Vec::new();
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "RECORD" => true,
            "RECORDLEN" | "RESULT.MAXLENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "DIGESTTYPE" => matches!(
                value.schema(),
                "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
            ),
            "RESULT" | "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "OPTION.DIGESTHEX" | "OPTION.DIGESTBINARY" | "OPTION.DIGESTBASE64"
                if value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty() =>
            {
                flags.push(name.as_str());
                true
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if flags.len() + usize::from(request.arguments.contains_key("DIGESTTYPE")) != 1 {
        return Err(HostProblem::Malformed);
    }
    let format = if let Some(value) = request.arguments.get("DIGESTTYPE") {
        std::str::from_utf8(value.bytes())
            .map_err(|_| invalid_type())?
            .trim()
            .to_ascii_uppercase()
    } else {
        match flags.first().copied() {
            Some("OPTION.DIGESTBINARY") => "BINARY".into(),
            Some("OPTION.DIGESTBASE64") => "BASE64".into(),
            Some("OPTION.DIGESTHEX") => "HEX".into(),
            _ => return Err(HostProblem::Malformed),
        }
    };
    let digest = ring::digest::digest(
        &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        &record.bytes()[..length],
    );
    let result = match format.as_str() {
        "HEX" => digest
            .as_ref()
            .iter()
            .flat_map(|byte| format!("{byte:02X}").into_bytes())
            .collect::<Vec<_>>(),
        "BINARY" => digest.as_ref().to_vec(),
        "BASE64" => base64::engine::general_purpose::STANDARD
            .encode(digest.as_ref())
            .into_bytes(),
        _ => return Err(invalid_type()),
    };
    if result_maximum.is_some_and(|maximum| maximum < result.len() as i64) {
        return Err(HostProblem::Malformed);
    }
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
    response.outputs.insert("RESULT".into(), bounded(result)?);
    Ok(response)
}

fn decimal(request: &CicsRequest, name: &str) -> Result<Option<i64>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            if value.schema() != "mainframe-env.cics.decimal@1" {
                return Err(HostProblem::Malformed);
            }
            std::str::from_utf8(value.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<i64>()
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

fn length_error() -> HostProblem {
    HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 2,
    }
}

fn invalid_type() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_format_widths_and_known_vector() {
        let digest = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, b"abc");
        let hex = digest
            .as_ref()
            .iter()
            .flat_map(|byte| format!("{byte:02X}").into_bytes())
            .collect::<Vec<_>>();
        assert_eq!(hex, b"A9993E364706816ABA3E25717850C26C9CD0D89D");
        assert_eq!(digest.as_ref().len(), 20);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .encode(digest.as_ref())
                .len(),
            28
        );
    }
}
