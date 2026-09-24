//! Local BRXA BMS callback with checked DFHCOMMAREA pointer bounds.

use super::bridge_abi::BrxaBmsFrame;
use super::terminal_control::{
    map_names, receive, send, validate_receive_request, validate_send_request,
};
use crate::service::{CicsService, Run};
use mainframe_env_execution_api::{ArtifactRef, BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    CicsOperation, CicsRequest, CicsResponse, HostLimits, HostProblem, HostRequest, HostResult,
    ProgramLinkSelection, ProgramName, ProgramRequest,
};
use mainframe_env_ir::cics_application_registry_for_runtime_operation;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    let receive_map = request.operation == CicsOperation::ReceiveMap;
    if receive_map {
        validate_receive_request(request)?;
    } else {
        validate_send_request(request)?;
    }
    if receive_map && request.arguments.contains_key("FROM") {
        return Err(HostProblem::Unsupported);
    }
    if request.arguments.keys().any(|name| {
        matches!(
            name.as_str(),
            "MAPSET"
                | "OPTION.CURSOR"
                | "OPTION.DATAONLY"
                | "OPTION.MAPONLY"
                | "OPTION.ERASE"
                | "OPTION.FREEKB"
        )
    }) {
        return Err(HostProblem::Unsupported);
    }
    let binding = run
        .invocation
        .bindings
        .get("cics.bridge-request")
        .ok_or(HostProblem::InfrastructureFailure)?;
    if binding.schema() != "mainframe-env.cics.bridge-request@1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let request_id = std::str::from_utf8(binding.bytes()).map_err(|_| HostProblem::Malformed)?;
    let runtime = service.bridge_runtime(request_id)?;
    if runtime.run_unit != run.invocation.run_unit_id.as_str()
        || runtime.principal != run.invocation.principal.id().as_str()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let descriptor =
        cics_application_registry_for_runtime_operation(request.operation.runtime_name())
            .ok_or(HostProblem::InfrastructureFailure)?;
    let names = if request.arguments.contains_key("MAP") {
        Some(map_names(request)?)
    } else if receive_map {
        let state = service.lock()?;
        state
            .sessions
            .get(&run.session)
            .and_then(|session| session.mapset.clone().zip(session.map.clone()))
    } else {
        None
    };
    let mut from = if receive_map {
        Vec::new()
    } else {
        request
            .arguments
            .get("FROM")
            .map_or(Vec::new(), |value| value.bytes().to_vec())
    };
    if let Some(length) = request.arguments.get("LENGTH") {
        let length = std::str::from_utf8(length.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .trim()
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if length > from.len() {
            return Err(HostProblem::Malformed);
        }
        from.truncate(length);
    }
    let frame = BrxaBmsFrame::new(
        &runtime.bound_frame,
        runtime.commarea_address,
        runtime.commarea_capacity,
        descriptor.eibfn,
        names.as_ref().map(|names| names.0.as_str()),
        names.as_ref().map(|names| names.1.as_str()),
        &from,
        receive_map,
    )?;
    let limits = InvocationLimits::default();
    let link_request = HostRequest::Program(ProgramRequest::Link {
        program: ProgramName::new(&runtime.exit, HostLimits::default().max_name_bytes)
            .map_err(|_| HostProblem::Malformed)?,
        payload: BoundedPayload::new("mainframe-env.cics.brxa@1", frame.bytes().to_vec(), limits)
            .map_err(|_| HostProblem::ResourceExhausted)?,
        selection: Some(ProgramLinkSelection {
            artifact: ArtifactRef::new(&runtime.artifact, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            generation: 1,
            content_identity: runtime.artifact.clone(),
        }),
    });
    let returned = match service.nested(run, link_request)? {
        HostResult::Program(payload) => payload,
        _ => return Err(HostProblem::ProviderFailure),
    };
    let input = frame.validate_reply(returned.bytes(), runtime.commarea_address)?;
    if receive_map {
        let mut supplied = request.clone();
        // The bridge owns the terminal response; the existing BMS decoder
        // applies the target map's symbolic output contract.
        supplied.arguments.insert(
            "FROM".into(),
            BoundedPayload::new("mainframe-env.cics.argument@1", input, limits)
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        receive(service, run, &supplied)
    } else {
        send(service, run, request)
    }
}
