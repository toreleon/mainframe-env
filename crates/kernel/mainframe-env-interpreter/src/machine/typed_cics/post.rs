//! Apply a due POST timer event to the interpreter-owned four-byte area.

use super::*;

pub(super) fn apply_event(
    machine: &mut ReferenceMachine,
    name: &str,
    value: &BoundedPayload,
) -> Result<bool, MachineProblem> {
    if name != "POST.EVENT" {
        return Ok(false);
    }
    if value.schema() != "mainframe-env.cics.post-event@1"
        || value.bytes().len() != 8
        || value.bytes()[4..] != [0x40, 0, 0x80, 0]
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let Some((base, offset)) = machine.decode_address(&value.bytes()[..4])? else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    if base < machine.static_base_count || offset != 0 || machine.freed_allocations.contains(&base)
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let area = machine
        .bases
        .get_mut(base)
        .filter(|area| area.len() == 4)
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    area.copy_from_slice(&value.bytes()[4..]);
    Ok(true)
}
