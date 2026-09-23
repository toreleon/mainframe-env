use super::{response, state};
use crate::service::{CicsService, Run};
use mainframe_env_host_api::{
    AccessIntent, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.keys().any(|name| {
        !matches!(
            name.as_str(),
            "TRACENUM"
                | "FROM"
                | "FROMLENGTH"
                | "RESOURCE"
                | "OPTION.EXCEPTION"
                | "OPTION.NOHANDLE"
                | "RESP"
                | "RESP2"
        )
    }) || request
        .arguments
        .get("OPTION.EXCEPTION")
        .is_some_and(|value| {
            value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
        })
    {
        return Err(HostProblem::Malformed);
    }
    let number = decimal(request, "TRACENUM")?;
    if !(0..=199).contains(&number) {
        return Err(condition("INVREQ", 16, 1));
    }
    let length = if request.arguments.contains_key("FROMLENGTH") {
        decimal(request, "FROMLENGTH")?
    } else {
        8
    };
    if !(0..=4000).contains(&length) {
        return Err(condition("LENGERR", 22, 4));
    }
    let from = request
        .arguments
        .get("FROM")
        .map(|value| value.bytes().to_vec())
        .unwrap_or_else(|| vec![0; 8]);
    let length = usize::try_from(length).map_err(|_| HostProblem::Malformed)?;
    if length > from.len() || length > service.limits.max_diagnostic_payload_bytes {
        return Err(condition("LENGERR", 22, 4));
    }
    let resource = match request.arguments.get("RESOURCE") {
        Some(value) if value.bytes().len() == 8 => {
            String::from_utf8(value.bytes().to_vec()).map_err(|_| HostProblem::Malformed)?
        }
        Some(_) => return Err(HostProblem::Malformed),
        None => String::new(),
    };
    let exception = request.arguments.contains_key("OPTION.EXCEPTION");
    service.authorize(run, "CICSDIAG", "CICS.DIAG.TRACE", AccessIntent::Update)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let _guard = service.lock()?;
    let current = state::load(service)?;
    if let Some(reply) = current.replay(mutation.idempotency_key.as_str(), digest)? {
        return response(service, run, reply);
    }
    let configuration = current.configuration;
    if !exception && !configuration.user_trace {
        return Err(condition("INVREQ", 16, 3));
    }
    if !exception && !(configuration.internal || configuration.auxiliary || configuration.system) {
        return Err(condition("INVREQ", 16, 2));
    }
    let mut destinations = Vec::new();
    if exception || configuration.internal {
        destinations.push("INTERNAL");
    }
    if configuration.auxiliary {
        destinations.push("AUXILIARY");
    }
    if configuration.system {
        destinations.push("SYSTEM");
    }
    if current.traces.len() + current.dumps.len() >= service.limits.max_diagnostic_entries {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut next = current.clone();
    let sequence = next.allocate_sequence()?;
    next.traces.push(state::CicsDiagnosticTraceRecord {
        sequence,
        kind: format!("TRACENUM:{}", destinations.join("+")),
        identifier: number.to_string(),
        resource,
        data: from[..length].to_vec(),
        exception,
        run_unit: run.invocation.run_unit_id.as_str().into(),
        principal: run.invocation.principal.id().as_str().into(),
    });
    let reply = state::DiagnosticReply::normal();
    next.record_replay(
        mutation.idempotency_key.as_str(),
        digest,
        reply.clone(),
        service.limits,
    )?;
    state::persist(service, current.version, &mut next)?;
    response(service, run, reply)
}

fn decimal(request: &CicsRequest, name: &str) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.decimal@1" {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
