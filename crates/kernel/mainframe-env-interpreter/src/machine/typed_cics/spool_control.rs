use super::*;

pub(super) fn validate_slot(
    layout: &LayoutMetadata,
    slot_use: SlotUse,
) -> Result<(), MachineProblem> {
    if matches!(slot_use, SlotUse::SpoolTokenInput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS spool TOKEN input must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolTokenOutput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS SPOOLOPEN TOKEN output must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolUserIdInput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS spool USERID input must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolNodeInput)
        && (layout.length != 8
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "CICS spool NODE input must be an 8-character field",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolRecordLengthInput)
        && (layout.length != 2 || layout.category != LayoutCategory::Binary)
    {
        return Err(invalid_plan(
            "CICS spool RECORDLENGTH input must be halfword binary",
        ));
    }
    if matches!(slot_use, SlotUse::SpoolOutDescrInput)
        && !matches!(
            layout.category,
            LayoutCategory::Pointer | LayoutCategory::Pointer32
        )
    {
        return Err(invalid_plan("CICS spool OUTDESCR input must be a pointer"));
    }
    if matches!(slot_use, SlotUse::SpoolClassInput)
        && (layout.length != 1
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan("CICS spool CLASS input must be one character"));
    }
    Ok(())
}

pub(super) fn out_descriptor_argument(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> (&'static str, Vec<u8>) {
    match read_spool_out_descriptor(machine, slot) {
        Ok(bytes) => ("mainframe-env.cics.outdescr@1", bytes),
        Err(reason) => ("mainframe-env.cics.outdescr-invalid@1", vec![reason]),
    }
}

fn read_spool_out_descriptor(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<Vec<u8>, u8> {
    let pointer = read_slot(machine, slot).map_err(|_| 52)?;
    let width = pointer.len();
    let (base, offset) = machine.decode_address(&pointer).ok().flatten().ok_or(52)?;
    if machine.freed_allocations.contains(&base) {
        return Err(52);
    }
    let field = machine
        .bases
        .get(base)
        .and_then(|bytes| bytes.get(offset..offset.checked_add(width)?))
        .ok_or(52)?;
    let (base, offset) = machine.decode_address(field).ok().flatten().ok_or(52)?;
    if machine.freed_allocations.contains(&base) {
        return Err(52);
    }
    let bytes = machine.bases.get(base).ok_or(52)?;
    let length = bytes
        .get(offset..offset.checked_add(4).ok_or(52)?)
        .ok_or(52)?;
    let length = u32::from_be_bytes(length.try_into().map_err(|_| 52)?) as usize;
    if length > 4096 {
        return Err(44);
    }
    bytes
        .get(offset.checked_add(4).ok_or(52)?..offset.checked_add(4 + length).ok_or(52)?)
        .map(<[u8]>::to_vec)
        .ok_or(52)
}
