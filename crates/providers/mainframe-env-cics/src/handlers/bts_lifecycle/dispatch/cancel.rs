//! CANCEL activity/process through atomic completion event publication.

use super::*;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let (process, activity_id, _) = super::state::subject(service, run, request)?;
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let definition = authority
        .load_process_type(&process.process_type)?
        .ok_or_else(|| condition("PROCESSERR", 108, 9))?;
    service
        .authorize(
            run,
            "BTSREPO",
            &definition.repository_resource,
            AccessIntent::Update,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => condition("NOTAUTH", 70, 101),
            other => other,
        })?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    authority.cancel_with_events(
        &process.process_type,
        &process.name,
        &activity_id,
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
        mutation.idempotency_key.as_str(),
        digest,
    )?;
    response(service, run, BTreeMap::new())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let selector = match request.operation {
        CicsOperation::CancelAcqActivity => Some("OPTION.ACQACTIVITY"),
        CicsOperation::CancelAcqProcess => Some("OPTION.ACQPROCESS"),
        CicsOperation::CancelActivity => None,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if selector.is_some_and(|name| !request.arguments.contains_key(name))
        || selector.is_none() && !request.arguments.contains_key("ACTIVITY")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        if name.starts_with("OPTION.") {
            if name != "OPTION.NOHANDLE" && selector != Some(name.as_str())
                || value.schema() != "mainframe-env.cics.option@1"
                || !value.bytes().is_empty()
            {
                return Err(HostProblem::Malformed);
            }
        } else if name == "ACTIVITY" && selector.is_none() {
            if !matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ) {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(name.as_str(), "RESP" | "RESP2") {
            if value.schema() != "mainframe-env.cics.argument@1" {
                return Err(HostProblem::Malformed);
            }
        } else {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}
