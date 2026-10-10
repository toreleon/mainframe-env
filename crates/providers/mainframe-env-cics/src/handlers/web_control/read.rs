use super::super::super::{CicsService, Run, bounded, decimal_payload};
use super::{model, open};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let result = invoke_inner(service, run, request);
    let decision = match &result {
        Ok(_) => AuditDecision::Success,
        Err(HostProblem::Unauthorized) => AuditDecision::Deny,
        Err(HostProblem::Cancelled) => AuditDecision::Cancelled,
        Err(HostProblem::TimedOut) => AuditDecision::TimedOut,
        Err(HostProblem::UnknownOutcome) => AuditDecision::UnknownOutcome,
        Err(HostProblem::InfrastructureFailure) => AuditDecision::InfrastructureFailure,
        Err(_) => AuditDecision::ProviderFailure,
    };
    open::audit_web_decision(service, run, request, decision)?;
    result
}

fn invoke_inner(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let selector = validate_request(request)?;
    let client = request.arguments.contains_key("SESSTOKEN");
    let name_length = decimal(request, "NAMELENGTH")?;
    if name_length <= 0 {
        return Err(condition("LENGERR", 22, if client { 35 } else { 1 }));
    }
    let supplied = request.arguments[selector].bytes();
    let name_length = usize::try_from(name_length).map_err(|_| HostProblem::ResourceExhausted)?;
    if name_length > supplied.len() || name_length > 128 {
        return Err(condition("INVREQ", 16, 144));
    }
    let name = &supplied[..name_length];
    let capacity = decimal(request, "VALUELENGTH")?;
    if capacity <= 0 {
        return Err(condition("LENGERR", 22, if client { 55 } else { 1 }));
    }
    let capacity = usize::try_from(capacity).map_err(|_| HostProblem::ResourceExhausted)?;
    if capacity > service.limits.max_web_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(declared) = request.arguments.get("VALUE.MAXLENGTH") {
        let declared = std::str::from_utf8(declared.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if capacity > declared {
            return Err(HostProblem::Malformed);
        }
    }
    let state = service.lock()?;
    let value = if client {
        let token: [u8; 8] = request.arguments["SESSTOKEN"]
            .bytes()
            .try_into()
            .map_err(|_| condition("NOTOPEN", 19, 27))?;
        state
            .web
            .sessions
            .get(&model::token_key(token))
            .filter(|session| {
                session.owner_execution == run.invocation.execution_id.as_str()
                    && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                    && session.transaction == run.transaction
            })
            .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
        if selector != "HTTPHEADER" {
            return Err(HostProblem::Malformed);
        }
        let response = state
            .web
            .client_responses
            .get(&model::token_key(token))
            .filter(|response| {
                response.received
                    && response.owner_execution == run.invocation.execution_id.as_str()
                    && response.owner_run_unit == run.invocation.run_unit_id.as_str()
                    && response.transaction == run.transaction
            })
            .ok_or_else(|| condition("INVREQ", 16, 43))?;
        if response.response.headers.is_empty() {
            return Err(condition("INVREQ", 16, 43));
        }
        response
            .response
            .headers
            .iter()
            .find(|(header, _)| header.as_bytes().eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_bytes().to_vec())
            .ok_or_else(|| condition("NOTFND", 13, 1))?
    } else {
        let inbound = state
            .web
            .inbound
            .get(run.invocation.run_unit_id.as_str())
            .ok_or_else(|| condition("INVREQ", 16, 1))?;
        if !inbound.http {
            return Err(condition("INVREQ", 16, 3));
        }
        match selector {
            "HTTPHEADER" => {
                if inbound.headers.is_empty() {
                    return Err(condition("INVREQ", 16, 43));
                }
                inbound
                    .headers
                    .iter()
                    .find(|(header, _)| header.as_bytes().eq_ignore_ascii_case(name))
                    .map(|(_, value)| value.as_bytes().to_vec())
                    .ok_or_else(|| condition("NOTFND", 13, 1))?
            }
            "QUERYPARM" => {
                let pairs = url_encoded_pairs(inbound.query.as_bytes())?;
                if pairs.is_empty() {
                    return Err(condition("INVREQ", 16, 13));
                }
                pairs
                    .into_iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value)
                    .ok_or_else(|| condition("NOTFND", 13, 1))?
            }
            "FORMFIELD" => {
                let bytes = if inbound.method == "GET" {
                    inbound.query.as_bytes()
                } else {
                    let form = inbound.headers.iter().any(|(header, value)| {
                        header.eq_ignore_ascii_case("Content-Type")
                            && value
                                .to_ascii_lowercase()
                                .starts_with("application/x-www-form-urlencoded")
                    });
                    if !form {
                        return Err(condition("INVREQ", 16, 153));
                    }
                    inbound.body.as_slice()
                };
                let pairs = url_encoded_pairs(bytes)?;
                if pairs.is_empty() {
                    return Err(condition("INVREQ", 16, 13));
                }
                pairs
                    .into_iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, value)| value)
                    .ok_or_else(|| condition("NOTFND", 13, 1))?
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        }
    };
    drop(state);
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
    response.outputs.insert(
        "VALUE".into(),
        bounded(value[..value.len().min(capacity)].to_vec())?,
    );
    response.outputs.insert(
        "VALUELENGTH".into(),
        decimal_payload(i64::try_from(value.len()).map_err(|_| HostProblem::ResourceExhausted)?)?,
    );
    if value.len() > capacity {
        response.condition = "LENGERR".into();
        response.response = 22;
        response.response2 = if client {
            52
        } else if selector == "HTTPHEADER" {
            2
        } else {
            5
        };
    }
    Ok(response)
}

fn validate_request(request: &CicsRequest) -> Result<&'static str, HostProblem> {
    const ALLOWED: &[&str] = &[
        "HTTPHEADER",
        "QUERYPARM",
        "FORMFIELD",
        "NAMELENGTH",
        "SESSTOKEN",
        "VALUE",
        "VALUELENGTH",
        "VALUE.MAXLENGTH",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
    ];
    if request.mutation.is_some()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || ["NAMELENGTH", "VALUE", "VALUELENGTH"]
            .iter()
            .any(|name| !request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    let selected = ["HTTPHEADER", "QUERYPARM", "FORMFIELD"]
        .into_iter()
        .filter(|name| request.arguments.contains_key(*name))
        .collect::<Vec<_>>();
    if selected.len() != 1
        || request.arguments.contains_key("SESSTOKEN") && selected[0] != "HTTPHEADER"
    {
        return Err(HostProblem::Malformed);
    }
    Ok(selected[0])
}

type UrlEncodedPairs = Vec<(Vec<u8>, Vec<u8>)>;

pub(super) fn url_encoded_pairs(bytes: &[u8]) -> Result<UrlEncodedPairs, HostProblem> {
    if bytes.len() > 4096 {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut pairs = Vec::new();
    if bytes.is_empty() {
        return Ok(pairs);
    }
    for pair in bytes.split(|byte| *byte == b'&') {
        let equals = pair
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or_else(|| condition("INVREQ", 16, 17))?;
        let (name, value) = pair.split_at(equals);
        let value = &value[1..];
        if name.is_empty() || pairs.len() >= 128 {
            return Err(condition("INVREQ", 16, 17));
        }
        pairs.push((unescape(name)?, unescape(value)?));
    }
    Ok(pairs)
}

fn unescape(bytes: &[u8]) -> Result<Vec<u8>, HostProblem> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' => {
                let high = bytes
                    .get(index + 1)
                    .and_then(|byte| (*byte as char).to_digit(16));
                let low = bytes
                    .get(index + 2)
                    .and_then(|byte| (*byte as char).to_digit(16));
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(condition("INVREQ", 16, 17));
                };
                decoded.push((high * 16 + low) as u8);
                index += 2;
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    Ok(decoded)
}

fn decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_escape_and_missing_equals_are_rejected() {
        assert_eq!(
            url_encoded_pairs(b"q=%G0"),
            Err(condition("INVREQ", 16, 17))
        );
        assert_eq!(url_encoded_pairs(b"q"), Err(condition("INVREQ", 16, 17)));
        assert_eq!(
            url_encoded_pairs(b"q=a+b"),
            Ok(vec![(b"q".to_vec(), b"a b".to_vec())])
        );
    }
}
