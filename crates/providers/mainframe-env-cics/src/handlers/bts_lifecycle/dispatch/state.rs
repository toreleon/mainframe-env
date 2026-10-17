//! SUSPEND, RESUME, RESET, and DELETE lifecycle transitions.

use super::super::removal::BtsRemoval;
use super::*;
use mainframe_env_host_api::{HostRequest, canonical_request_digest};
use std::collections::BTreeMap;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let (process, activity_id, parent_id) = subject(service, run, request)?;
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
    let run_unit = run.invocation.run_unit_id.as_str();
    let owner_execution = run.invocation.execution_id.as_str();
    let owner_principal = run.invocation.principal.id().as_str();
    let replay_key = mutation.idempotency_key.as_str();
    if matches!(
        request.operation,
        CicsOperation::ResetAcqProcess | CicsOperation::ResetActivity
    ) {
        authority.remove_subtree(
            &process.process_type,
            &process.name,
            crate::service::handlers::bts_lifecycle::BtsReplayContext {
                run_unit,
                owner_execution,
                owner_principal,
                replay_key,
                request_digest: digest,
            },
            &BtsRemoval::Reset { activity_id },
        )?;
    } else if request.operation == CicsOperation::DeleteActivity {
        authority.remove_subtree(
            &process.process_type,
            &process.name,
            crate::service::handlers::bts_lifecycle::BtsReplayContext {
                run_unit,
                owner_execution,
                owner_principal,
                replay_key,
                request_digest: digest,
            },
            &BtsRemoval::Delete {
                parent_id: parent_id.ok_or(HostProblem::InfrastructureFailure)?,
                child_name: argument_name(request, "ACTIVITY", 16)?,
            },
        )?;
    } else {
        let suspended = matches!(
            request.operation,
            CicsOperation::SuspendAcqActivity
                | CicsOperation::SuspendAcqProcess
                | CicsOperation::SuspendActivity
        );
        if suspended {
            authority.mutate_process(
                &process.process_type,
                &process.name,
                crate::service::handlers::bts_lifecycle::BtsReplayContext {
                    run_unit,
                    owner_execution,
                    owner_principal,
                    replay_key,
                    request_digest: digest,
                },
                |process| {
                    process.set_suspended(&activity_id, true)?;
                    Ok(BtsReply::normal())
                },
            )?;
        } else {
            let tick = service
                .replay_clock
                .as_ref()
                .map(|clock| clock.now_tick())
                .transpose()?;
            let (_, released) = authority.resume_deferred(
                &process.process_type,
                &process.name,
                &activity_id,
                run_unit,
                owner_execution,
                owner_principal,
                replay_key,
                digest,
                tick,
                run.invocation.priority,
            )?;
            if let Some(record) = released
                && matches!(record.state, BtsRunState::Pending | BtsRunState::Attached)
            {
                service
                    .enqueue_bts_run_work(&record)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
            }
        }
    }
    response(service, run, BTreeMap::new())
}

pub(super) fn subject(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<(BtsProcess, String, Option<String>), HostProblem> {
    match request.operation {
        CicsOperation::ResetAcqProcess
        | CicsOperation::CancelAcqProcess
        | CicsOperation::RunAcqProcess
        | CicsOperation::ResumeAcqProcess
        | CicsOperation::SuspendAcqProcess => {
            let (_, process, id) = acquisition(service, run, true)?;
            Ok((process, id, None))
        }
        CicsOperation::CancelAcqActivity
        | CicsOperation::RunAcqActivity
        | CicsOperation::ResumeAcqActivity
        | CicsOperation::SuspendAcqActivity => {
            let (_, process, id) = acquisition(service, run, false)?;
            Ok((process, id, None))
        }
        CicsOperation::DeleteActivity
        | CicsOperation::CancelActivity
        | CicsOperation::RunActivity
        | CicsOperation::ResetActivity
        | CicsOperation::ResumeActivity
        | CicsOperation::SuspendActivity => {
            let context = active_context(service, run)?;
            let name = argument_name(request, "ACTIVITY", 16)?;
            let process = BtsLifecycleStore::new(service.store.as_ref())
                .load_process(&context.process_type, &context.process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let child = process
                .child(&context.activity_id, &name)
                .filter(|child| {
                    child
                        .pending_uow
                        .as_deref()
                        .is_none_or(|uow| uow == context.run_unit)
                })
                .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?;
            let id = child.id.clone();
            Ok((process, id, Some(context.activity_id)))
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let selector = match request.operation {
        CicsOperation::ResetAcqProcess
        | CicsOperation::ResumeAcqProcess
        | CicsOperation::SuspendAcqProcess => Some("OPTION.ACQPROCESS"),
        CicsOperation::ResumeAcqActivity | CicsOperation::SuspendAcqActivity => {
            Some("OPTION.ACQACTIVITY")
        }
        CicsOperation::DeleteActivity
        | CicsOperation::ResetActivity
        | CicsOperation::ResumeActivity
        | CicsOperation::SuspendActivity => None,
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
