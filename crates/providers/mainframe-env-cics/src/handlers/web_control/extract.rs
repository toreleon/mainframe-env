use super::super::super::{CicsService, Run, bounded, decimal_payload};
use super::{model, open};
use mainframe_env_execution_api::AuditDecision;
use mainframe_env_host_api::{CicsDisposition, CicsRequest, CicsResponse, HostProblem};
use std::net::{Ipv4Addr, Ipv6Addr};

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
    validate_request(request)?;
    let state = service.lock()?;
    let (scheme, host, port, method, version, path, query, urimap, realm, http) =
        if let Some(value) = request.arguments.get("SESSTOKEN") {
            let token: [u8; 8] = value
                .bytes()
                .try_into()
                .map_err(|_| condition("INVREQ", 16, 144))?;
            let session = state
                .web
                .sessions
                .get(&model::token_key(token))
                .filter(|session| {
                    session.owner_execution == run.invocation.execution_id.as_str()
                        && session.owner_run_unit == run.invocation.run_unit_id.as_str()
                        && session.transaction == run.transaction
                })
                .ok_or_else(|| condition("NOTOPEN", 19, 27))?;
            if session.server_closed {
                return Err(condition("INVREQ", 16, 41));
            }
            (
                session.endpoint.scheme.clone(),
                session.endpoint.host.clone(),
                session.endpoint.port,
                String::new(),
                session.http_version,
                session.endpoint.default_path.clone(),
                String::new(),
                session.endpoint.urimap.clone(),
                String::new(),
                true,
            )
        } else {
            let inbound = state
                .web
                .inbound
                .get(run.invocation.run_unit_id.as_str())
                .ok_or_else(|| condition("INVREQ", 16, 1))?;
            (
                inbound.scheme.clone(),
                inbound.host.clone(),
                inbound.port,
                inbound.method.clone(),
                inbound.version,
                inbound.path.clone(),
                inbound.query.clone(),
                inbound.urimap.clone(),
                String::new(),
                inbound.http,
            )
        };
    drop(state);
    if !http
        && ["HTTPMETHOD", "HTTPVERSION", "PATH"]
            .iter()
            .any(|name| request.arguments.contains_key(*name))
    {
        return Err(condition("INVREQ", 16, 3));
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
    for (name, value) in [
        ("SCHEME", if scheme == "HTTPS" { 2 } else { 1 }),
        ("HOSTTYPE", host_type(&host)),
        ("PORTNUMBER", i64::from(port)),
        ("REQUESTTYPE", i64::from(http)),
    ] {
        if request.arguments.contains_key(name) {
            response
                .outputs
                .insert(name.into(), decimal_payload(value)?);
        }
    }
    let version = format!("{}.{}", version.major, version.minor);
    for (area, length, bytes, invalid_length, truncated) in [
        ("HOST", "HOSTLENGTH", host.as_bytes(), 21, 29),
        ("HTTPMETHOD", "METHODLENGTH", method.as_bytes(), 4, 4),
        ("HTTPVERSION", "VERSIONLEN", version.as_bytes(), 7, 6),
        ("PATH", "PATHLENGTH", path.as_bytes(), 5, 30),
        ("QUERYSTRING", "QUERYSTRLEN", query.as_bytes(), 8, 8),
        ("REALM", "REALMLEN", realm.as_bytes(), 141, 141),
    ] {
        if !request.arguments.contains_key(area) {
            continue;
        }
        let capacity = decimal(request, length)?;
        if capacity <= 0 {
            return Err(condition("LENGERR", 22, invalid_length));
        }
        let capacity = usize::try_from(capacity).map_err(|_| HostProblem::ResourceExhausted)?;
        if capacity > service.limits.max_web_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        if let Some(declared) = request.arguments.get(&format!("{area}.MAXLENGTH")) {
            let declared = std::str::from_utf8(declared.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<usize>()
                .map_err(|_| HostProblem::Malformed)?;
            if capacity > declared {
                return Err(HostProblem::Malformed);
            }
        }
        let copied = bytes.len().min(capacity);
        response
            .outputs
            .insert(area.into(), bounded(bytes[..copied].to_vec())?);
        response.outputs.insert(
            length.into(),
            decimal_payload(
                i64::try_from(bytes.len()).map_err(|_| HostProblem::ResourceExhausted)?,
            )?,
        );
        if copied < bytes.len() && response.response == 0 {
            response.condition = "LENGERR".into();
            response.response = 22;
            response.response2 = truncated;
        }
    }
    if request.arguments.contains_key("URIMAP") {
        response.outputs.insert(
            "URIMAP".into(),
            bounded(urimap.unwrap_or_default().into_bytes())?,
        );
    }
    Ok(response)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "SESSTOKEN",
        "SCHEME",
        "HOST",
        "HOSTLENGTH",
        "HOSTTYPE",
        "HTTPMETHOD",
        "METHODLENGTH",
        "HTTPVERSION",
        "VERSIONLEN",
        "PATH",
        "PATHLENGTH",
        "PORTNUMBER",
        "QUERYSTRING",
        "QUERYSTRLEN",
        "REQUESTTYPE",
        "URIMAP",
        "REALM",
        "REALMLEN",
        "RESP",
        "RESP2",
        "OPTION.NOHANDLE",
        "HOST.MAXLENGTH",
        "HTTPMETHOD.MAXLENGTH",
        "HTTPVERSION.MAXLENGTH",
        "PATH.MAXLENGTH",
        "QUERYSTRING.MAXLENGTH",
        "REALM.MAXLENGTH",
    ];
    if request.mutation.is_some()
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || !request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "SESSTOKEN" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Malformed);
    }
    let client = request.arguments.contains_key("SESSTOKEN");
    if client
        && [
            "HTTPMETHOD",
            "METHODLENGTH",
            "QUERYSTRING",
            "QUERYSTRLEN",
            "REQUESTTYPE",
        ]
        .iter()
        .any(|name| request.arguments.contains_key(*name))
        || !client
            && ["REALM", "REALMLEN"]
                .iter()
                .any(|name| request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    for (area, length) in [
        ("HOST", "HOSTLENGTH"),
        ("HTTPMETHOD", "METHODLENGTH"),
        ("HTTPVERSION", "VERSIONLEN"),
        ("PATH", "PATHLENGTH"),
        ("QUERYSTRING", "QUERYSTRLEN"),
        ("REALM", "REALMLEN"),
    ] {
        if request.arguments.contains_key(area) != request.arguments.contains_key(length) {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
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

fn host_type(host: &str) -> i64 {
    if host.parse::<Ipv4Addr>().is_ok() {
        2
    } else if host.parse::<Ipv6Addr>().is_ok() {
        3
    } else {
        1
    }
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
