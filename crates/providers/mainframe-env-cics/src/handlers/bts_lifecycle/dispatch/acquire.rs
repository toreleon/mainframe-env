//! ACQUIRE PROCESS and ACQUIRE ACTIVITYID over one fenced UOW lease.

use super::*;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let (process_type, process_name, activity_id, operation, definition) = match request.operation {
        CicsOperation::AcquireProcess => {
            let process_type = argument_name(request, "PROCESSTYPE", 8)?;
            let process_name = argument_name(request, "PROCESS", 36)?;
            let definition = authority
                .load_process_type(&process_type)?
                .filter(|definition| definition.enabled)
                .ok_or_else(|| condition("PROCESSERR", 108, 9))?;
            let process = authority
                .load_process(&process_type, &process_name)?
                .ok_or_else(|| condition("PROCESSERR", 108, 5))?;
            let root = process.root_id;
            (
                process_type,
                process_name,
                root,
                "ACQUIRE PROCESS",
                definition,
            )
        }
        CicsOperation::AcquireActivityId => {
            let id = argument_name(request, "ACTIVITYID", 52)?;
            if id.len() != 52 {
                return Err(condition("ACTIVITYERR", 109, 8));
            }
            let index = authority
                .load_activity_index(&id)?
                .filter(|index| index.parent_id.is_some())
                .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?;
            let definition = authority
                .load_process_type(&index.process_type)?
                .filter(|definition| definition.enabled)
                .ok_or_else(|| condition("PROCESSERR", 108, 9))?;
            (
                index.process_type,
                index.process_name,
                id,
                "ACQUIRE ACTIVITYID",
                definition,
            )
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if let Some(current) = authority.active_context(
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
    )? {
        if current.process_type == process_type && current.process_name == process_name {
            return Err(condition("INVREQ", 16, 22));
        }
    }
    service.authorize(
        run,
        "BTSREPO",
        &definition.repository_resource,
        AccessIntent::Update,
    )?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    authority
        .acquire_exact(
            run.invocation.run_unit_id.as_str(),
            run.invocation.execution_id.as_str(),
            run.invocation.principal.id().as_str(),
            &process_type,
            &process_name,
            &activity_id,
            operation,
            mutation.idempotency_key.as_str(),
            digest,
        )
        .map_err(|problem| match problem {
            HostProblem::NotFound if request.operation == CicsOperation::AcquireProcess => {
                condition("PROCESSERR", 108, 5)
            }
            HostProblem::NotFound => condition("ACTIVITYERR", 109, 8),
            other => other,
        })?;
    response(service, run, BTreeMap::new())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let required: &[&str] = match request.operation {
        CicsOperation::AcquireProcess => &["PROCESS", "PROCESSTYPE"],
        CicsOperation::AcquireActivityId => &["ACTIVITYID"],
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if required
        .iter()
        .any(|name| !request.arguments.contains_key(*name))
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        if name == "OPTION.NOHANDLE" {
            if value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty() {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(name.as_str(), "RESP" | "RESP2") {
            if value.schema() != "mainframe-env.cics.argument@1" {
                return Err(HostProblem::Malformed);
            }
        } else if required.contains(&name.as_str()) {
            if !matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            ) {
                return Err(HostProblem::Malformed);
            }
        } else {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}
