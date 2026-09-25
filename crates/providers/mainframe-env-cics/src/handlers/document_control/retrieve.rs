use super::*;
use mainframe_env_encoding::CodePage;

#[derive(Clone, Copy)]
enum ClientCharacterSet {
    Utf8,
    Latin1,
    Cp037,
}

impl ClientCharacterSet {
    fn ccsid(self) -> u16 {
        match self {
            Self::Utf8 => 1208,
            Self::Latin1 => 819,
            Self::Cp037 => 37,
        }
    }
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let requested_maximum = request
        .arguments
        .get("MAXLENGTH")
        .map(|value| signed_length(value.bytes()))
        .transpose()?
        .map(|value| {
            usize::try_from(value).map_err(|_| HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 1,
            })
        })
        .transpose()?;
    let destination_maximum = request
        .arguments
        .get("INTO.MAXLENGTH")
        .ok_or(HostProblem::Malformed)
        .and_then(|value| {
            usize::try_from(signed_length(value.bytes())?).map_err(|_| HostProblem::Malformed)
        })?;
    let maximum = requested_maximum
        .unwrap_or(destination_maximum)
        .min(destination_maximum)
        .min(service.limits.max_screen_bytes);
    let document = {
        let state = service.lock()?;
        document_for_token(
            &state.documents,
            run,
            request.arguments["DOCTOKEN"].bytes(),
            1,
        )?
        .clone()
    };
    let data_only = request.arguments.contains_key("OPTION.DATAONLY");
    let content = if let Some(value) = request.arguments.get("CHARACTERSET") {
        let character_set = character_set(value.bytes())?;
        let mut converted = document;
        for segment in &mut converted.segments {
            if !segment.binary {
                segment.bytes = convert(segment, character_set, service.limits.max_document_bytes)?;
                segment.host_code_page = character_set.ccsid();
            }
        }
        if data_only {
            converted.content_bytes()
        } else {
            transport::encode_retrieved(
                &converted,
                CicsLimits {
                    max_screen_bytes: service.limits.max_document_bytes,
                    ..service.limits
                },
            )?
        }
    } else if data_only {
        document.content_bytes()
    } else {
        transport::encode_retrieved(&document, service.limits)?
    };
    let required = content.len();
    let truncated = maximum < required || maximum == 0;
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        if truncated { "LENGERR" } else { "NORMAL" },
        if truncated { 22 } else { 0 },
        if truncated { 2 } else { 0 },
        None,
        None,
        content[..maximum.min(required)].to_vec(),
    )?;
    response.outputs.insert(
        "LENGTH".into(),
        decimal_payload(i64::try_from(required).map_err(|_| HostProblem::ResourceExhausted)?)?,
    );
    Ok(response)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if !["DOCTOKEN", "INTO", "LENGTH", "INTO.MAXLENGTH"]
        .into_iter()
        .all(|name| request.arguments.contains_key(name))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "DOCTOKEN" => value.schema() == "mainframe-env.cics.storage-value@1",
            "INTO" | "LENGTH" | "RESP" | "RESP2" => {
                value.schema() == "mainframe-env.cics.argument@1"
            }
            "MAXLENGTH" | "INTO.MAXLENGTH" => value.schema() == "mainframe-env.cics.decimal@1",
            "CHARACTERSET" => matches!(
                value.schema(),
                "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
            ),
            "OPTION.NOHANDLE" | "OPTION.DATAONLY" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn signed_length(bytes: &[u8]) -> Result<i64, HostProblem> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or(HostProblem::Malformed)
}

fn character_set(bytes: &[u8]) -> Result<ClientCharacterSet, HostProblem> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| not_found_character_set())?
        .trim()
        .to_ascii_uppercase();
    match value.as_str() {
        "UTF-8" | "UTF8" | "1208" => Ok(ClientCharacterSet::Utf8),
        "ISO-8859-1" | "LATIN-1" | "819" => Ok(ClientCharacterSet::Latin1),
        "IBM-037" | "CP037" | "037" | "37" => Ok(ClientCharacterSet::Cp037),
        _ => Err(not_found_character_set()),
    }
}

fn convert(
    segment: &DocumentSegment,
    target: ClientCharacterSet,
    maximum: usize,
) -> Result<Vec<u8>, HostProblem> {
    let text = match segment.host_code_page {
        37 => CodePage::Cp037
            .decode(&segment.bytes, maximum)
            .map_err(|_| conversion_error(12))?,
        819 => segment.bytes.iter().map(|byte| char::from(*byte)).collect(),
        1208 => std::str::from_utf8(&segment.bytes)
            .map_err(|_| conversion_error(12))?
            .to_string(),
        _ => return Err(conversion_error(11)),
    };
    let bytes = match target {
        ClientCharacterSet::Utf8 => text.into_bytes(),
        ClientCharacterSet::Latin1 => text
            .chars()
            .map(|character| u8::try_from(u32::from(character)).map_err(|_| conversion_error(12)))
            .collect::<Result<Vec<_>, _>>()?,
        ClientCharacterSet::Cp037 => CodePage::Cp037
            .encode(&text, maximum)
            .map_err(|_| conversion_error(12))?,
    };
    if bytes.len() > maximum {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(bytes)
}

fn not_found_character_set() -> HostProblem {
    HostProblem::Condition {
        name: "NOTFND".into(),
        response: 13,
        response2: 7,
    }
}

fn conversion_error(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2,
    }
}
