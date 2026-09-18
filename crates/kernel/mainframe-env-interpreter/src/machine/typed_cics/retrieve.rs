use super::*;

pub(super) fn allocation_capacity(
    machine: &ReferenceMachine,
    target: &CicsTarget,
) -> Result<usize, MachineProblem> {
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    if machine
        .bases
        .len()
        .saturating_sub(machine.static_base_count)
        >= machine.invocation.limits.max_frames as usize
    {
        return Err(MachineProblem::ResourceExhausted);
    }
    machine.address_bytes_for(machine.bases.len(), 0, pointer.length)?;
    let used = machine
        .bases
        .iter()
        .try_fold(0usize, |total, storage| total.checked_add(storage.len()))
        .ok_or(MachineProblem::ResourceExhausted)?;
    usize::try_from(machine.invocation.limits.max_storage_bytes)
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .checked_sub(used)
        .ok_or(MachineProblem::ResourceExhausted)
}

pub(super) fn write_set_output(
    machine: &mut ReferenceMachine,
    target: &CicsTarget,
    value: &BoundedPayload,
) -> Result<(), MachineProblem> {
    if value.schema() != "mainframe-env.cics.payload@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let capacity = allocation_capacity(machine, target)?;
    if value.bytes().len() > capacity {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    let base = machine.bases.len();
    let address = machine.address_bytes_for(base, 0, pointer.length)?;
    machine.bases.push(value.bytes().to_vec());
    machine.write_reference(&pointer, &address)
}
