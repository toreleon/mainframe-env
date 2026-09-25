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
    if request
        .arguments
        .iter()
        .any(|(name, value)| match name.as_str() {
            "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
            "OPTION.ON" | "OPTION.OFF" | "OPTION.SYSTEM" | "OPTION.USER" | "OPTION.EI"
            | "OPTION.SINGLE" | "OPTION.NOHANDLE" => {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            }
            _ => true,
        })
    {
        return Err(HostProblem::Malformed);
    }
    let on = request.arguments.contains_key("OPTION.ON");
    let off = request.arguments.contains_key("OPTION.OFF");
    let targets = ["USER", "SYSTEM", "EI", "SINGLE"]
        .iter()
        .filter(|name| request.arguments.contains_key(&format!("OPTION.{name}")))
        .count();
    if on == off || targets == 0 {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 1,
        });
    }
    service.authorize(
        run,
        "CICSDIAG",
        "CICS.DIAG.TRACE.CONFIG",
        AccessIntent::Update,
    )?;
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
    let mut next = current.clone();
    if request.arguments.contains_key("OPTION.USER") {
        next.configuration.user_trace = on;
    }
    if request.arguments.contains_key("OPTION.SYSTEM") {
        next.configuration.system = on;
    }
    if request.arguments.contains_key("OPTION.EI") {
        next.configuration.internal = on;
    }
    if request.arguments.contains_key("OPTION.SINGLE") {
        next.single_trace = on;
    }
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
