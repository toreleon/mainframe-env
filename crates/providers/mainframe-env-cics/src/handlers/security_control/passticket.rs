//! One-use PassTicket request through the installed RACF/SAF authority.

use super::super::super::{CicsService, Run, decimal_payload};
use super::authority::{CicsPassTicketFailure, CicsPassTicketRequest};
use super::verify::{condition, live_tick, text};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use sha2::{Digest, Sha256};

pub(super) fn issue(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let application = text(request, "ESMAPPNAME")?
        .ok_or(HostProblem::Malformed)?
        .trim_end()
        .to_ascii_uppercase();
    if application.is_empty()
        || application.len() > 8
        || !application.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(condition("INVREQ", 16, 247));
    }
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-passticket-effect@1\0");
    identity.update(run.invocation.execution_id.as_str().as_bytes());
    identity.update(run.invocation.run_unit_id.as_str().as_bytes());
    identity.update(mutation.idempotency_key.as_str().as_bytes());
    let idempotency_key = format!("CICS-PTKT:{:x}", identity.finalize());
    let binding_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let tick = live_tick(service, retention_tick)?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if tick >= run.invocation.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let issued = service
        .security_authority()?
        .issue_passticket(CicsPassTicketRequest {
            actor: run.invocation.principal.id(),
            application: &application,
            binding_digest,
            idempotency_key: &idempotency_key,
            correlation: &idempotency_key,
            tick,
        });
    let resolved_tick =
        live_tick(service, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    if run.invocation.cancellation_requested() || resolved_tick >= run.invocation.deadline_tick {
        return Err(HostProblem::UnknownOutcome);
    }
    let issued = issued.map_err(|_| HostProblem::UnknownOutcome)?;
    if issued.failure.is_none() == issued.ticket.is_none() {
        return Err(HostProblem::UnknownOutcome);
    }
    let mut response = if let Some(failure) = issued.failure {
        let problem = match failure {
            CicsPassTicketFailure::DefaultUser => condition("INVREQ", 16, 256),
            CicsPassTicketFailure::RegionDenied => condition("NOTAUTH", 70, 260),
            CicsPassTicketFailure::TargetDenied => condition("NOTAUTH", 70, 250),
            CicsPassTicketFailure::SecurityUnavailable => condition("INVREQ", 16, 251),
            CicsPassTicketFailure::Unsupported => condition("INVREQ", 16, 254),
        };
        super::super::condition(service, run, &request.condition_policy, problem)?
    } else {
        service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?
    };
    if let Some(ticket) = issued.ticket {
        if ticket.bytes().len() != 8 || ticket.schema() != "mainframe-env.cics.secret@1" {
            return Err(HostProblem::UnknownOutcome);
        }
        response.outputs.insert("PASSTICKET".into(), ticket);
    }
    for (name, code) in [
        ("ESMRESP", issued.esm_response),
        ("ESMREASON", issued.esm_reason),
    ] {
        if request.arguments.contains_key(name) {
            response.outputs.insert(name.into(), decimal_payload(code)?);
        }
    }
    Ok(response)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if !request.arguments.contains_key("ESMAPPNAME")
        || !request.arguments.contains_key("PASSTICKET")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "ESMAPPNAME" => !matches!(value.schema(), "mainframe-env.cics.storage-value@1"),
                "PASSTICKET" | "ESMRESP" | "ESMREASON" | "RESP" | "RESP2" => {
                    value.schema() != "mainframe-env.cics.argument@1"
                }
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
