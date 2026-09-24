use super::{CICS_START_WORK_GENERATION, IntervalStartState, name_argument, replace_state};
use crate::service::{CicsService, Run, store_error};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{WorkRecord, WorkState};

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_id = name_argument(request, "REQID", 8)?;
    let selected_transaction = request
        .arguments
        .get("TRANSID")
        .map(|_| name_argument(request, "TRANSID", 4))
        .transpose()?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let record = service.lock()?.interval_records.get(&request_id).cloned();
    let Some(record) = record else {
        if let Some(response) = super::post::cancel_named(
            service,
            run,
            &request_id,
            selected_transaction.as_deref(),
            mutation.idempotency_key.as_str(),
            digest,
        )? {
            return Ok(response);
        }
        return super::delay::cancel_named(
            service,
            run,
            &request_id,
            selected_transaction.as_deref(),
            mutation.idempotency_key.as_str(),
            digest,
        );
    };
    let replay = record.state == IntervalStartState::Cancelled
        && record.consumer_effect_key.as_deref() == Some(mutation.idempotency_key.as_str())
        && record.consumer_request_digest == Some(digest);
    if record.state != IntervalStartState::Pending && !replay {
        return Err(not_found());
    }
    let authorized_transaction = selected_transaction
        .as_deref()
        .unwrap_or(&record.transaction);
    service
        .authorize(
            run,
            "TCICSTRN",
            &format!("CICS.{authorized_transaction}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 0,
            },
            other => other,
        })?;
    let work = interval_work(service, &request_id)?;
    {
        let mut state = service.lock()?;
        let current = state
            .interval_records
            .get(&request_id)
            .ok_or_else(not_found)?;
        if current.state == IntervalStartState::Cancelled {
            if current.consumer_effect_key.as_deref() != Some(mutation.idempotency_key.as_str())
                || current.consumer_request_digest != Some(digest)
            {
                return Err(not_found());
            }
        } else if current.state == IntervalStartState::Pending {
            replace_state(
                service.store.as_ref(),
                &mut state.interval_records,
                &request_id,
                IntervalStartState::Cancelled,
                Some(mutation.idempotency_key.as_str().into()),
                Some(digest),
                service.limits,
            )?;
        } else {
            return Err(not_found());
        }
    }
    service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?
        .request_cancellation(&work.work_id)
        .map_err(store_error)?;
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn interval_work(service: &CicsService, request_id: &str) -> Result<WorkRecord, HostProblem> {
    let work_id = format!("cics-start:{request_id}");
    let work = service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?
        .get_work(&work_id)
        .map_err(store_error)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    if work.work_id != work_id
        || work.required_generation != CICS_START_WORK_GENERATION
        || work.required_selector.as_str() != "cics:start"
        || work.artifact.as_str() != "artifact:none"
        || work.payload != request_id.as_bytes()
        || !matches!(
            work.state,
            WorkState::Queued | WorkState::Claimed | WorkState::Cancelled | WorkState::DeadLetter
        )
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(work)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &["OPTION.NOHANDLE", "REQID", "RESP", "RESP2", "TRANSID"];
    if !request.arguments.contains_key("REQID")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || if name == "OPTION.NOHANDLE" {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                } else {
                    !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1"
                            | "mainframe-env.cics.storage-value@1"
                            | "mainframe-env.cics.argument@1"
                    )
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn not_found() -> HostProblem {
    HostProblem::Condition {
        name: "NOTFND".into(),
        response: 13,
        response2: 0,
    }
}
