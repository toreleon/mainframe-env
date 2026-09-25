//! Source-shaped public BTS lifecycle command dispatch.

use super::*;
use crate::service::{CicsService, Run};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};

mod acquire;
mod cancel;
mod check;
mod define;
mod run;
mod state;
mod transid;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::CheckAcqActivity
        | CicsOperation::CheckAcqProcess
        | CicsOperation::CheckActivity => check::invoke(service, run, request),
        CicsOperation::AcquireActivityId | CicsOperation::AcquireProcess => {
            acquire::invoke(service, run, request)
        }
        CicsOperation::DefineActivity | CicsOperation::DefineProcess => {
            define::invoke(service, run, request)
        }
        CicsOperation::DeleteActivity
        | CicsOperation::ResetAcqProcess
        | CicsOperation::ResetActivity
        | CicsOperation::ResumeAcqActivity
        | CicsOperation::ResumeAcqProcess
        | CicsOperation::ResumeActivity
        | CicsOperation::SuspendAcqActivity
        | CicsOperation::SuspendAcqProcess
        | CicsOperation::SuspendActivity => state::invoke(service, run, request),
        CicsOperation::CancelAcqActivity
        | CicsOperation::CancelAcqProcess
        | CicsOperation::CancelActivity => cancel::invoke(service, run, request),
        CicsOperation::RunAcqActivity
        | CicsOperation::RunAcqProcess
        | CicsOperation::RunActivity => run::invoke(service, run, request),
        CicsOperation::RunTransId => transid::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn acquisition(
    service: &CicsService,
    run: &Run,
    root: bool,
) -> Result<(BtsAcquisition, BtsProcess, String), HostProblem> {
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let missing = || condition("INVREQ", 16, if root { 15 } else { 24 });
    let acquisition = authority
        .load_acquisition(run.invocation.run_unit_id.as_str())?
        .filter(BtsAcquisition::is_held)
        .ok_or_else(missing)?;
    if acquisition.owner_execution != run.invocation.execution_id.as_str()
        || acquisition.owner_principal != run.invocation.principal.id().as_str()
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    let process_type = acquisition
        .process_type
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let process_name = acquisition
        .process_name
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let activity_id = acquisition
        .activity_id
        .as_deref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let process = authority
        .load_process(process_type, process_name)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    if (activity_id == process.root_id) != root
        || !process.visible_to(run.invocation.run_unit_id.as_str())
    {
        return Err(missing());
    }
    let activity_id = activity_id.to_string();
    Ok((acquisition, process, activity_id))
}

fn active_context(service: &CicsService, run: &Run) -> Result<BtsActivityContext, HostProblem> {
    BtsLifecycleStore::new(service.store.as_ref())
        .active_context(
            run.invocation.run_unit_id.as_str(),
            run.invocation.execution_id.as_str(),
            run.invocation.principal.id().as_str(),
        )?
        .ok_or_else(|| condition("INVREQ", 16, 4))
}

fn argument_name(request: &CicsRequest, name: &str, maximum: usize) -> Result<String, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if !matches!(
        value.schema(),
        "mainframe-env.cics.argument@1"
            | "mainframe-env.cics.literal@1"
            | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
    let text = if matches!(name, "PROCESS" | "ACTIVITYID") {
        text
    } else {
        text.trim_end_matches(' ')
    };
    validate_name(text, maximum, name == "PROCESS")?;
    Ok(text.into())
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn response(
    service: &CicsService,
    run: &Run,
    outputs: impl IntoIterator<Item = (String, (&'static str, Vec<u8>))>,
) -> Result<CicsResponse, HostProblem> {
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
    for (name, (schema, bytes)) in outputs {
        response.outputs.insert(
            name,
            BoundedPayload::new(schema, bytes, InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
}
