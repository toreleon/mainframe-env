use super::super::super::{CicsService, Run, bounded, decimal_payload};
use super::{model, open};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};
use std::sync::atomic::Ordering;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let result = invoke_inner(service, run, request, retention_tick);
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
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    const ALLOWED: &[&str] = &[
        "SESSTOKEN",
        "INTO",
        "INTO.MAXLENGTH",
        "LENGTH",
        "MAXLENGTH",
        "STATUSCODE",
        "STATUSTEXT",
        "STATUSTEXT.MAXLENGTH",
        "STATUSLEN",
        "MEDIATYPE",
        "BODYCHARSET",
        "OPTION.NOTRUNCATE",
        "OPTION.NOCLICONVERT",
        "OPTION.NOSRVCONVERT",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || !["INTO", "LENGTH", "MAXLENGTH"]
            .iter()
            .all(|name| request.arguments.contains_key(*name))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("STATUSTEXT")
            != request.arguments.contains_key("STATUSLEN")
    {
        return Err(HostProblem::Malformed);
    }
    let maximum = decimal(request, "MAXLENGTH")?;
    if maximum <= 0 {
        return Err(condition("LENGERR", 22, 16));
    }
    let maximum = usize::try_from(maximum).map_err(|_| HostProblem::ResourceExhausted)?;
    if maximum > service.limits.max_web_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if let Some(declared) = request.arguments.get("INTO.MAXLENGTH") {
        let declared = std::str::from_utf8(declared.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if maximum > declared {
            return Err(HostProblem::Malformed);
        }
    }
    let token: Option<[u8; 8]> = request
        .arguments
        .get("SESSTOKEN")
        .map(|value| {
            value
                .bytes()
                .try_into()
                .map_err(|_| condition("NOTOPEN", 19, 27))
        })
        .transpose()?;
    if token.is_some() && request.arguments.contains_key("OPTION.NOSRVCONVERT")
        || token.is_none() && request.arguments.contains_key("OPTION.NOCLICONVERT")
        || token.is_none()
            && ["STATUSCODE", "STATUSTEXT", "STATUSLEN"]
                .iter()
                .any(|name| request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    let (
        body,
        cursor,
        media_type,
        status,
        reason,
        urimap,
        code_page,
        previous_client,
        previous_server,
    ) = {
        let state = service.lock()?;
        if let Some(token) = token {
            let key = model::token_key(token);
            let session = state
                .web
                .sessions
                .get(&key)
                .filter(|session| {
                    session.owner_execution == run.invocation.execution_id.as_str()
                        && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                        && session.transaction == run.transaction
                })
                .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
            let response = state
                .web
                .client_responses
                .get(&key)
                .filter(|response| {
                    response.owner_execution == run.invocation.execution_id.as_str()
                        && response.owner_run_unit == run.invocation.run_unit_id.as_str()
                        && response.transaction == run.transaction
                })
                .cloned()
                .ok_or_else(|| condition("INVREQ", 16, 41))?;
            let media = header(&response.response.headers, "Content-Type");
            (
                response.response.body.clone(),
                response.cursor,
                media,
                Some(response.response.status),
                response.response.reason.clone(),
                session.endpoint.urimap.clone(),
                Some(session.endpoint.code_page),
                Some(response),
                None,
            )
        } else {
            let key = run.invocation.run_unit_id.as_str();
            let inbound = state
                .web
                .inbound
                .get(key)
                .ok_or_else(|| condition("INVREQ", 16, 1))?;
            let previous = state.web.body_cursors.get(key).cloned();
            if previous.as_ref().is_some_and(|cursor| {
                cursor.owner_execution != run.invocation.execution_id.as_str()
                    || cursor.transaction != run.transaction
            }) {
                return Err(HostProblem::Unauthorized);
            }
            let media = header(&inbound.headers, "Content-Type");
            (
                inbound.body.clone(),
                previous.as_ref().map_or(0, |cursor| cursor.cursor),
                media,
                None,
                String::new(),
                inbound.urimap.clone(),
                None,
                None,
                previous,
            )
        }
    };
    if let Some(name) = urimap.as_deref() {
        service.authorize(run, "URIMAP", name, AccessIntent::Read)?;
    }
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if cursor > body.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let available = &body[cursor..];
    let (copied, consumed) = chunk(
        available,
        maximum,
        &media_type,
        code_page,
        request.arguments.contains_key("OPTION.NOCLICONVERT"),
        service.limits.max_web_bytes,
    )?;
    let remaining = available.len() > consumed;
    let notruncate = request.arguments.contains_key("OPTION.NOTRUNCATE");
    let next_cursor = if remaining && !notruncate {
        body.len()
    } else {
        cursor + consumed
    };
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        if remaining { "LENGERR" } else { "NORMAL" },
        if remaining { 22 } else { 0 },
        if remaining {
            if notruncate { 36 } else { 57 }
        } else {
            0
        },
        None,
        None,
        Vec::new(),
    )?;
    response
        .outputs
        .insert("INTO".into(), bounded(copied.clone())?);
    response
        .outputs
        .insert("LENGTH".into(), decimal_payload(copied.len() as i64)?);
    if let Some(status) = status
        && request.arguments.contains_key("STATUSCODE")
    {
        response
            .outputs
            .insert("STATUSCODE".into(), decimal_payload(status.into())?);
    }
    if request.arguments.contains_key("STATUSTEXT") {
        let capacity = decimal(request, "STATUSLEN")?;
        if capacity <= 0 {
            return Err(condition("LENGERR", 22, 59));
        }
        let capacity = usize::try_from(capacity).map_err(|_| HostProblem::ResourceExhausted)?;
        if let Some(declared) = request.arguments.get("STATUSTEXT.MAXLENGTH") {
            let declared = std::str::from_utf8(declared.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<usize>()
                .map_err(|_| HostProblem::Malformed)?;
            if capacity > declared {
                return Err(HostProblem::Malformed);
            }
        }
        let value = reason.as_bytes();
        response.outputs.insert(
            "STATUSTEXT".into(),
            bounded(value[..value.len().min(capacity)].to_vec())?,
        );
        response
            .outputs
            .insert("STATUSLEN".into(), decimal_payload(value.len() as i64)?);
        if value.len() > capacity && response.response == 0 {
            response.condition = "LENGERR".into();
            response.response = 22;
            response.response2 = 58;
        }
    }
    if request.arguments.contains_key("MEDIATYPE") {
        response.outputs.insert(
            "MEDIATYPE".into(),
            bounded(
                media_type
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .as_bytes()
                    .iter()
                    .take(56)
                    .copied()
                    .collect(),
            )?,
        );
    }
    if request.arguments.contains_key("BODYCHARSET") {
        let charset = media_type
            .split(';')
            .skip(1)
            .find_map(|part| part.trim().strip_prefix("charset="))
            .unwrap_or("");
        response.outputs.insert(
            "BODYCHARSET".into(),
            bounded(charset.as_bytes().iter().take(40).copied().collect())?,
        );
    }
    let mut state = service.lock()?;
    let key = token.map_or_else(
        || run.invocation.run_unit_id.as_str().to_string(),
        model::token_key,
    );
    if token.is_some()
        && !state.web.sessions.get(&key).is_some_and(|session| {
            session.owner_execution == run.invocation.execution_id.as_str()
                && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                && session.transaction == run.transaction
        })
    {
        return Err(HostProblem::UnknownOutcome);
    }
    if token.is_none()
        && !state
            .web
            .inbound
            .get(&key)
            .is_some_and(|inbound| inbound.body == body)
    {
        return Err(HostProblem::UnknownOutcome);
    }
    let mut next_client = None;
    let mut next_server = None;
    let (namespace, version, payload, previous_bytes) = if let Some(mut previous) = previous_client
    {
        if state.web.client_responses.get(&key) != Some(&previous) {
            return Err(HostProblem::UnknownOutcome);
        }
        let previous_bytes = model::encode_client_response(&previous)?.len();
        previous.cursor = next_cursor;
        previous.received = true;
        previous.version += 1;
        let payload = model::encode_client_response(&previous)?;
        next_client = Some(previous.clone());
        (
            model::CLIENT_RESPONSE_NAMESPACE,
            previous.version,
            payload,
            previous_bytes,
        )
    } else {
        if state.web.body_cursors.get(&key) != previous_server.as_ref() {
            return Err(HostProblem::UnknownOutcome);
        }
        let previous_bytes = previous_server
            .as_ref()
            .map(model::encode_body_cursor)
            .transpose()?
            .map_or(0, |value| value.len());
        let cursor = model::WebServerBodyCursor {
            owner_execution: run.invocation.execution_id.as_str().into(),
            owner_run_unit: key.clone(),
            transaction: run.transaction.clone(),
            cursor: next_cursor,
            version: previous_server
                .as_ref()
                .map_or(1, |prior| prior.version + 1),
        };
        let payload = model::encode_body_cursor(&cursor)?;
        next_server = Some(cursor.clone());
        (
            model::BODY_CURSOR_NAMESPACE,
            cursor.version,
            payload,
            previous_bytes,
        )
    };
    let total = state
        .web
        .bytes
        .checked_sub(previous_bytes)
        .and_then(|bytes| bytes.checked_add(payload.len()))
        .filter(|total| *total <= service.limits.max_web_bytes)
        .ok_or(HostProblem::ResourceExhausted)?;
    let writes = vec![
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: namespace.into(),
                key: key.clone(),
                version,
                payload,
            },
            expected_version: (version > 1).then_some(version - 1),
        }),
        ProviderStateMutation::Put(open::replay_write(run, request, retention_tick, &response)?),
    ];
    if service.store.mutate_provider_states_atomic(writes).is_err() {
        return Err(HostProblem::UnknownOutcome);
    }
    state.web.bytes = total;
    if let Some(client) = next_client {
        state.web.client_responses.insert(key.clone(), client);
    }
    if let Some(server) = next_server {
        state.web.body_cursors.insert(key, server);
    }
    drop(state);
    if run.invocation.cancellation_requested()
        || open::expired_after_dispatch(service, run)?
        || service
            .replay_unknown_after_persist
            .swap(false, Ordering::SeqCst)
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(response)
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

fn header(headers: &[(String, String)], name: &str) -> String {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map_or_else(String::new, |(_, value)| value.clone())
}

pub(super) fn chunk(
    source: &[u8],
    maximum: usize,
    media_type: &str,
    code_page: Option<u16>,
    no_convert: bool,
    limit: usize,
) -> Result<(Vec<u8>, usize), HostProblem> {
    let text_media = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .starts_with("text/");
    if no_convert || !text_media || code_page.is_none() {
        let count = source.len().min(maximum);
        return Ok((source[..count].to_vec(), count));
    }
    let charset = media_type
        .split(';')
        .skip(1)
        .find_map(|part| {
            let part = part.trim();
            part.get(..8)
                .filter(|prefix| prefix.eq_ignore_ascii_case("charset="))
                .map(|_| part[8..].trim_matches('"'))
        })
        .unwrap_or("ISO-8859-1");
    let utf8 = charset.eq_ignore_ascii_case("UTF-8");
    let text = if utf8 {
        std::str::from_utf8(source)
            .map_err(|_| condition("INVREQ", 16, 15))?
            .to_string()
    } else {
        source
            .iter()
            .map(|byte| char::from(*byte))
            .collect::<String>()
    };
    let encoded = match code_page {
        Some(37) => CodePage::Cp037
            .encode(&text, limit)
            .map_err(|_| condition("INVREQ", 16, 15))?,
        Some(819) => text
            .chars()
            .map(|character| {
                u8::try_from(u32::from(character)).map_err(|_| condition("INVREQ", 16, 15))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(condition("INVREQ", 16, 15)),
    };
    let count = encoded.len().min(maximum);
    let consumed = if utf8 {
        text.chars().take(count).map(char::len_utf8).sum()
    } else {
        count
    };
    Ok((encoded[..count].to_vec(), consumed))
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
    fn client_text_default_converts_and_no_convert_keeps_wire_bytes() {
        let (converted, consumed) = chunk(b"ABC", 2, "text/plain", Some(37), false, 64).unwrap();
        assert_eq!(converted, CodePage::Cp037.encode("AB", 64).unwrap());
        assert_eq!(consumed, 2);
        let (raw, consumed) = chunk(b"ABC", 2, "text/plain", Some(37), true, 64).unwrap();
        assert_eq!(raw, b"AB");
        assert_eq!(consumed, 2);
    }
}
