use super::super::super::{CicsService, Run};
use super::open;
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{CicsRequest, CicsResponse, HostProblem};

mod client;
mod server;

pub(super) struct SendInput {
    pub token: Option<[u8; 8]>,
    pub method: Option<String>,
    pub path: Option<String>,
    pub query: String,
    pub urimap: Option<String>,
    pub body: Vec<u8>,
    pub document_token: Option<[u8; 16]>,
    pub media_type: Option<String>,
    pub status: u16,
    pub reason: String,
    pub eventual: bool,
    pub close: bool,
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    let result = parse(service, run, request).and_then(|input| {
        if input.token.is_some() {
            client::invoke(service, run, request, retention_tick, input)
        } else {
            server::invoke(service, run, request, retention_tick, input)
        }
    });
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

pub(super) fn parse(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<SendInput, HostProblem> {
    const ALLOWED: &[&str] = &[
        "SESSTOKEN",
        "METHOD",
        "PATH",
        "PATHLENGTH",
        "URIMAP",
        "QUERYSTRING",
        "QUERYSTRLEN",
        "FROM",
        "FROMLENGTH",
        "DOCTOKEN",
        "MEDIATYPE",
        "STATUSCODE",
        "STATUSTEXT",
        "STATUSLEN",
        "ACTION",
        "CLOSESTATUS",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
    ];
    if request.mutation.is_none()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (area, length) in [
        ("FROM", "FROMLENGTH"),
        ("PATH", "PATHLENGTH"),
        ("QUERYSTRING", "QUERYSTRLEN"),
        ("STATUSTEXT", "STATUSLEN"),
    ] {
        if request.arguments.contains_key(area) != request.arguments.contains_key(length) {
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
    let client = token.is_some();
    if client != request.arguments.contains_key("METHOD")
        || request.arguments.contains_key("FROM") && request.arguments.contains_key("DOCTOKEN")
        || client
            && ["STATUSCODE", "STATUSTEXT", "STATUSLEN", "ACTION"]
                .iter()
                .any(|name| request.arguments.contains_key(*name))
        || !client
            && ["PATH", "PATHLENGTH", "QUERYSTRING", "QUERYSTRLEN", "URIMAP"]
                .iter()
                .any(|name| request.arguments.contains_key(*name))
        || client
            && request.arguments.contains_key("PATH")
            && request.arguments.contains_key("URIMAP")
    {
        return Err(HostProblem::Malformed);
    }
    let document_token: Option<[u8; 16]> = request
        .arguments
        .get("DOCTOKEN")
        .map(|value| value.bytes().try_into().map_err(|_| HostProblem::Malformed))
        .transpose()?;
    let body = if let Some(value) = request.arguments.get("FROM") {
        let length = decimal(request, "FROMLENGTH")?;
        if length <= 0 {
            return Err(condition("LENGERR", 22, 50));
        }
        let length = usize::try_from(length).map_err(|_| HostProblem::ResourceExhausted)?;
        if length > value.bytes().len() {
            return Err(HostProblem::Malformed);
        }
        if length > service.limits.max_web_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        value.bytes()[..length].to_vec()
    } else if let Some(token) = document_token {
        super::super::document_control::web_document_body(service, run, &token)?
    } else {
        Vec::new()
    };
    if !client && body.is_empty() {
        return Err(condition("INVREQ", 16, 123));
    }
    let method = request
        .arguments
        .get("METHOD")
        .map(|value| literal(value.bytes(), 8))
        .transpose()?;
    if let Some(method) = method.as_deref() {
        if !matches!(
            method,
            "GET" | "HEAD" | "PATCH" | "POST" | "PUT" | "TRACE" | "OPTIONS" | "DELETE"
        ) {
            return Err(condition("INVREQ", 16, 54));
        }
        if matches!(method, "GET" | "HEAD" | "TRACE" | "DELETE") && !body.is_empty() {
            return Err(condition("INVREQ", 16, 33));
        }
        if matches!(method, "PATCH" | "POST" | "PUT") && body.is_empty() {
            return Err(condition("INVREQ", 16, 34));
        }
    }
    let path = request
        .arguments
        .get("PATH")
        .map(|value| {
            let length = decimal(request, "PATHLENGTH")?;
            if length <= 0 {
                return Err(condition("LENGERR", 22, 5));
            }
            slice_text(value.bytes(), length, 4096)
        })
        .transpose()?;
    if path
        .as_ref()
        .is_some_and(|path| !path.starts_with('/') || path.contains(['?', '#']))
    {
        return Err(condition("INVREQ", 16, 49));
    }
    let query = request
        .arguments
        .get("QUERYSTRING")
        .map(|value| {
            let length = decimal(request, "QUERYSTRLEN")?;
            if length <= 0 {
                return Err(condition("LENGERR", 22, 8));
            }
            slice_text(value.bytes(), length, 4096)
        })
        .transpose()?
        .unwrap_or_default();
    if query
        .bytes()
        .any(|byte| !(0x21..=0x7e).contains(&byte) || byte == b'#')
    {
        return Err(condition("INVREQ", 16, 49));
    }
    let urimap = request
        .arguments
        .get("URIMAP")
        .map(|value| literal(value.bytes(), 8))
        .transpose()?;
    let media_type = request
        .arguments
        .get("MEDIATYPE")
        .map(|value| literal(value.bytes(), 56))
        .transpose()?;
    if media_type
        .as_ref()
        .is_some_and(|value| value.bytes().any(|byte| byte.is_ascii_whitespace()))
    {
        return Err(condition("INVREQ", 16, 32));
    }
    let status = match request.arguments.get("STATUSCODE") {
        Some(_) => u16::try_from(decimal(request, "STATUSCODE")?)
            .map_err(|_| condition("INVREQ", 16, 87))?,
        None => 200,
    };
    if !(100..=599).contains(&status) {
        return Err(condition("INVREQ", 16, 87));
    }
    let reason = request
        .arguments
        .get("STATUSTEXT")
        .map(|value| {
            let length = decimal(request, "STATUSLEN")?;
            if length <= 0 {
                return Err(HostProblem::Malformed);
            }
            slice_text(value.bytes(), length, 256)
        })
        .transpose()?
        .unwrap_or_else(|| default_reason(status).into());
    let eventual = match request.arguments.get("ACTION") {
        None => true,
        Some(value) if value.bytes() == b"EVENTUAL" => true,
        Some(value) if value.bytes() == b"IMMEDIATE" => false,
        Some(_) => return Err(condition("INVREQ", 16, 11)),
    };
    let close = match request.arguments.get("CLOSESTATUS") {
        None => false,
        Some(value) if value.bytes() == b"CLOSE" => true,
        Some(value) if value.bytes() == b"NOCLOSE" => false,
        Some(_) => return Err(condition("INVREQ", 16, 13)),
    };
    Ok(SendInput {
        token,
        method,
        path,
        query,
        urimap,
        body,
        document_token,
        media_type,
        status,
        reason,
        eventual,
        close,
    })
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

fn slice_text(bytes: &[u8], length: i64, maximum: usize) -> Result<String, HostProblem> {
    let length = usize::try_from(length).map_err(|_| HostProblem::ResourceExhausted)?;
    if length > bytes.len() || length > maximum {
        return Err(HostProblem::Malformed);
    }
    String::from_utf8(bytes[..length].to_vec()).map_err(|_| HostProblem::Malformed)
}

fn literal(bytes: &[u8], maximum: usize) -> Result<String, HostProblem> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| HostProblem::Malformed)?
        .trim();
    if text.is_empty() || text.len() > maximum {
        return Err(HostProblem::Malformed);
    }
    Ok(text.into())
}

fn default_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
