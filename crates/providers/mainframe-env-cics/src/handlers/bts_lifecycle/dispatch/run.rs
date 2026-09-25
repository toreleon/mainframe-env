//! RUN ACQACTIVITY, ACQPROCESS, and named child activity.

use super::*;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    if service.work_store.is_none() || service.replay_clock.is_none() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let synchronous = request.arguments.contains_key("OPTION.SYNCHRONOUS");
    let (process, activity_id, _) = super::state::subject(service, run, request)?;
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let definition = authority
        .load_process_type(&process.process_type)?
        .ok_or_else(|| condition("PROCESSERR", 108, 9))?;
    if !definition.enabled {
        return Err(condition("INVREQ", 16, 12));
    }
    let activity = process
        .activities
        .get(&activity_id)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let transaction = authority
        .load_transaction(&activity.transid)?
        .ok_or_else(|| condition("TRANSIDERR", 28, 0))?;
    if !transaction.enabled {
        return Err(condition("INVREQ", 16, 28));
    }
    if synchronous && transaction.remote {
        return Err(condition("INVREQ", 16, 32));
    }
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
    service
        .authorize(
            run,
            "TCICSTRN",
            &format!("CICS.{}", activity.transid),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => condition("NOTAUTH", 70, 101),
            other => other,
        })?;
    let input_event = request
        .arguments
        .get("INPUTEVENT")
        .map(|_| argument_name(request, "INPUTEVENT", 16))
        .transpose()
        .map_err(|_| condition("EVENTERR", 111, 7))?;
    let facility_token = request
        .arguments
        .get("FACILITYTOKN")
        .map(|value| <[u8; 8]>::try_from(value.bytes()).map_err(|_| HostProblem::Malformed))
        .transpose()?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let statement = request
        .arguments
        .get("BTS.RUN.ID")
        .ok_or(HostProblem::Malformed)?;
    if statement.schema() != "mainframe-env.cics.bts-run-id@1" {
        return Err(HostProblem::Malformed);
    }
    let statement_id =
        std::str::from_utf8(statement.bytes()).map_err(|_| HostProblem::Malformed)?;
    let position = statement_id
        .strip_prefix(&format!("{}:", run.invocation.run_unit_id.as_str()))
        .ok_or(HostProblem::Malformed)?;
    if statement_id.len() > 256
        || position.is_empty()
        || !position.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(HostProblem::Malformed);
    }
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let mut shape = request.clone();
    shape.mutation = None;
    let shape_digest = canonical_request_digest(&HostRequest::Cics(shape))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let tick = service
        .replay_clock
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?
        .now_tick()?;
    if tick == 0 {
        return Err(HostProblem::UnknownOutcome);
    }
    let record = authority.start_run(
        &process.process_type,
        &process.name,
        &activity_id,
        input_event.as_deref(),
        synchronous,
        facility_token,
        run.invocation.run_unit_id.as_str(),
        run.invocation.execution_id.as_str(),
        run.invocation.principal.id().as_str(),
        statement_id,
        mutation.idempotency_key.as_str(),
        digest,
        shape_digest,
        tick,
        run.invocation.priority,
    )?;
    if matches!(record.state, BtsRunState::Pending | BtsRunState::Attached) {
        service
            .enqueue_bts_run_work(&record)
            .map_err(|_| HostProblem::UnknownOutcome)?;
    }
    if synchronous && record.state != BtsRunState::Finished {
        return service.response(
            run,
            CicsDisposition::Suspended,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    if synchronous && record.completion == Some(BtsCompletion::Abend) {
        return Err(if activity_id == process.root_id {
            condition("PROCESSERR", 108, 27)
        } else {
            condition("ACTIVITYERR", 109, 27)
        });
    }
    response(service, run, BTreeMap::new())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let selector = match request.operation {
        CicsOperation::RunAcqActivity => Some("OPTION.ACQACTIVITY"),
        CicsOperation::RunAcqProcess => Some("OPTION.ACQPROCESS"),
        CicsOperation::RunActivity => None,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let synchronous = request.arguments.contains_key("OPTION.SYNCHRONOUS");
    let asynchronous = request.arguments.contains_key("OPTION.ASYNCHRONOUS");
    if synchronous == asynchronous
        || selector.is_some_and(|name| !request.arguments.contains_key(name))
        || selector.is_none() && !request.arguments.contains_key("ACTIVITY")
        || synchronous && request.arguments.contains_key("FACILITYTOKN")
        || !request.arguments.contains_key("BTS.RUN.ID")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        if name.starts_with("OPTION.") {
            if name != "OPTION.NOHANDLE"
                && name != "OPTION.SYNCHRONOUS"
                && name != "OPTION.ASYNCHRONOUS"
                && selector != Some(name.as_str())
                || value.schema() != "mainframe-env.cics.option@1"
                || !value.bytes().is_empty()
            {
                return Err(HostProblem::Malformed);
            }
        } else if name == "BTS.RUN.ID" {
            if value.schema() != "mainframe-env.cics.bts-run-id@1" {
                return Err(HostProblem::Malformed);
            }
        } else if matches!(name.as_str(), "INPUTEVENT" | "FACILITYTOKN")
            || name == "ACTIVITY" && selector.is_none()
        {
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
