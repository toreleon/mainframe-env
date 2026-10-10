//! Sole typed CICS post-reply application owner, before subsequent machine drive.
use super::*;

pub(in crate::machine) fn apply_reply(
    machine: &mut ReferenceMachine,
    pending: PendingKind,
    response: CicsResponse,
) -> Result<(), MachineProblem> {
    let PendingKind::Cics {
        operation,
        storage64_intent,
        argument_summary: _,
        container,
        into,
        outputs,
        response: response_target,
        response2: response2_target,
        address_set,
        no_handle,
    } = pending
    else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let responded = response_target.is_some() || no_handle;
    let load_base = super::write_response_state(
        machine,
        (operation, container.as_ref()),
        storage64_intent,
        response_target.as_ref(),
        response2_target.as_ref(),
        address_set.as_ref(),
        &outputs,
        &response,
    )?;
    let container_set_base = super::apply_payload_outputs(
        machine,
        operation,
        into.as_ref(),
        &outputs,
        &response,
        load_base,
    )?;
    super::finish(
        machine,
        operation,
        container.as_ref(),
        container_set_base,
        response,
        responded,
    )?;
    #[cfg(feature = "assurance-observers")]
    machine.observe_read_copyback_applied(operation);
    Ok(())
}
