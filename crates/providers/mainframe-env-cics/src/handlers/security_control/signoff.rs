//! Terminal SIGNOFF with audited denial and durable default-user restoration.

use super::super::super::{CicsService, Run};
use super::terminal_state::{self, TerminalIdentity};
use super::verify::{condition, live_tick};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use sha2::{Digest, Sha256};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-signoff-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let key = format!("CICS-SIGNOFF:{:x}", identity.finalize());
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let terminal = terminal_state::current(service, run, 2);
    let allowed = terminal
        .as_ref()
        .is_ok_and(|session| session.terminal_identity.user.is_some());
    let tick = live_tick(service, retention_tick)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let audited = service.security_authority()?.audit_signoff(
        run.invocation.principal.id(),
        &run.session,
        digest,
        &key,
        tick,
        allowed,
    );
    let resolved_tick =
        live_tick(service, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    if run.invocation.cancellation_requested() || resolved_tick >= run.invocation.deadline_tick {
        return Err(HostProblem::UnknownOutcome);
    }
    audited.map_err(|_| HostProblem::UnknownOutcome)?;
    let terminal = terminal?;
    if terminal.terminal_identity.effect_key.as_deref() == Some(key.as_str()) {
        return Err(HostProblem::UnknownOutcome);
    }
    if !allowed {
        return super::super::condition(
            service,
            run,
            &request.condition_policy,
            condition("INVREQ", 16, 1),
        );
    }
    let response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    terminal_state::persist(
        service,
        run,
        &terminal,
        TerminalIdentity {
            user: None,
            group: None,
            language: None,
            effect_key: Some(key),
            request_digest: Some(digest),
        },
    )?;
    Ok(response)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
