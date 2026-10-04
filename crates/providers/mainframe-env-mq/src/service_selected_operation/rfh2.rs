//! Finite RFH2 composition on the actual selected logical connection/unit.
use super::*;

pub(super) fn require_profile(
    runtime: &SelectedRuntime,
    request: &MqMqiRfh2Request,
) -> Result<(), HostProblem> {
    require_binding_profile(&runtime.connections, request)
}
pub(super) fn require_binding_profile(
    bindings: &[transition::ConnectionBinding],
    request: &MqMqiRfh2Request,
) -> Result<(), HostProblem> {
    let binding = bindings
        .iter()
        .find(|v| v.connection == request.connection())
        .ok_or(HostProblem::Malformed)?;
    if binding.rfh2_profile != Some(request.profile()) {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}
pub(super) fn prepare(
    runtime: &mut SelectedRuntime,
    invocation: &Invocation,
    owner: MqHandleOwner,
    request: &MqMqiRfh2Request,
    authorizer: &dyn EnterpriseAuthorizer,
    candidate: &mut transition::Candidate,
) -> Result<(), HostProblem> {
    transition::require_connection(runtime, owner, request.connection())?;
    require_profile(runtime, request)?;
    runtime
        .handles
        .handles_mut()
        .validate_message_property(owner, request.connection(), request.handle().into())
        .map_err(|_| HostProblem::Malformed)?;
    let intent = match request {
        MqMqiRfh2Request::BufferToHandle { .. } => AccessIntent::Update,
        MqMqiRfh2Request::HandleToBuffer { options, .. } if options.deletes() => {
            AccessIntent::Update
        }
        _ => AccessIntent::Read,
    };
    authorization::authorize_property(authorizer, invocation, intent)?;
    candidate.rfh2 = Some(request.clone());
    Ok(())
}
pub(super) fn replay(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    request: &MqMqiRfh2Request,
) -> Result<(), HostProblem> {
    require_profile(runtime, request).map_err(|_| HostProblem::UnknownOutcome)?;
    transition::require_connection(runtime, owner, request.connection())
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let binding = runtime
        .connections
        .iter()
        .find(|v| v.connection == request.connection())
        .ok_or(HostProblem::UnknownOutcome)?;
    state
        .ownership
        .units
        .get(&binding.unit)
        .ok_or(HostProblem::UnknownOutcome)?
        .require_owner(logical, &binding.key, &runtime.control, binding.unit)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    runtime
        .handles
        .handles_mut()
        .validate_message_property(owner, request.connection(), request.handle().into())
        .map_err(|_| HostProblem::UnknownOutcome)?;
    Ok(())
}
