//! Five source-profile property calls on the existing selected owner/registry.
//! These volatile handle payloads have no cold restoration authority. Receipt
//! publication is durable; later BUFMH/MHBUF/delivery projection remains separate.
use super::*;
use mainframe_env_host_api::{MqHandle, MqHandleKind, MqHandleObservation};

pub(super) fn prepare(
    runtime: &mut SelectedRuntime,
    invocation: &Invocation,
    owner: MqHandleOwner,
    request: &MqPropertyRequest,
    authorizer: &dyn EnterpriseAuthorizer,
    candidate: &mut transition::Candidate,
) -> Result<(), HostProblem> {
    transition::require_connection(runtime, owner, request.connection())?;
    if let Some(handle) = request.handle() {
        runtime
            .handles
            .handles_mut()
            .validate_message_property(owner, request.connection(), handle.into())
            .map_err(|_| HostProblem::Malformed)?;
    }
    let intent = match request {
        MqPropertyRequest::Create { .. } | MqPropertyRequest::DeleteHandle { .. } => {
            AccessIntent::Execute
        }
        MqPropertyRequest::Set { .. } | MqPropertyRequest::Delete { .. } => AccessIntent::Update,
        MqPropertyRequest::Inquire { .. } => AccessIntent::Read,
    };
    // Existing MQUOW resource is the real current logical connection ownership,
    // not an application property-name principal or a new SAF namespace.
    authorization::authorize_property(authorizer, invocation, intent)?;
    candidate.property = Some(request.clone());
    Ok(())
}
pub(super) fn kernel_error(value: crate::MqHandleKernelProblem) -> HostProblem {
    match value {
        crate::MqHandleKernelProblem::Capacity => HostProblem::ResourceExhausted,
        crate::MqHandleKernelProblem::Handle(mainframe_env_host_api::MqHandleProblem::Capacity) => {
            HostProblem::ResourceExhausted
        }
        crate::MqHandleKernelProblem::UnsupportedWire => HostProblem::Unsupported,
        _ => HostProblem::Malformed,
    }
}
pub(super) fn replay(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    request: &MqPropertyRequest,
    reply: &mut MqMqiHostResult,
) -> Result<(), HostProblem> {
    let connection = request.connection();
    transition::require_connection(runtime, owner, connection)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let binding = runtime
        .connections
        .iter()
        .find(|v| v.connection == connection)
        .ok_or(HostProblem::UnknownOutcome)?;
    state
        .ownership
        .units
        .get(&binding.unit)
        .ok_or(HostProblem::UnknownOutcome)?
        .require_owner(logical, &binding.key, &runtime.control, binding.unit)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    let output = match &mut reply.result.outcome {
        MqMqiOutcome::ReviewedOutput { output, .. } => output,
        _ => return Err(HostProblem::UnknownOutcome),
    };
    if let (MqPropertyRequest::Create { .. }, MqMqiOutput::MessageHandle(message)) =
        (request, &mut *output)
    {
        let observation = MqHandleObservation::from(MqHandle::Message(*message));
        let resolved = runtime
            .handles
            .handles_mut()
            .resolve_observed_handle(owner, connection, observation, MqHandleKind::Message)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let MqHandle::Message(live) = resolved else {
            return Err(HostProblem::UnknownOutcome);
        };
        runtime
            .handles
            .handles_mut()
            .validate_message_property(owner, connection, live.into())
            .map_err(|_| HostProblem::UnknownOutcome)?;
        *message = live;
    } else if !matches!(request, MqPropertyRequest::DeleteHandle { .. }) {
        runtime
            .handles
            .handles_mut()
            .validate_message_property(
                owner,
                connection,
                request.handle().ok_or(HostProblem::UnknownOutcome)?.into(),
            )
            .map_err(|_| HostProblem::UnknownOutcome)?;
    }
    Ok(())
}
