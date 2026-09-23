use super::*;

pub(super) fn monitor_pointer_bytes(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<Option<Vec<u8>>, MachineProblem> {
    let pointer = read_slot(machine, slot)?;
    let Some((base, offset)) = machine.decode_address(&pointer).ok().flatten() else {
        return Ok(None);
    };
    if machine.freed_allocations.contains(&base) {
        return Ok(None);
    }
    let bytes = machine
        .bases
        .get(base)
        .and_then(|bytes| bytes.get(offset..))
        .map(|bytes| bytes[..bytes.len().min(8192)].to_vec());
    Ok(bytes)
}
