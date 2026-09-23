use super::*;

pub(super) fn read_slot(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<Vec<u8>, MachineProblem> {
    let view = machine
        .views_by_id
        .get(&slot.storage)
        .ok_or(MachineProblem::UnknownStorage)?;
    let name_view = machine
        .views
        .get(&slot.qualified_layout_name)
        .ok_or(MachineProblem::UnknownStorage)?;
    if view != name_view {
        return Err(invalid_plan("resolved operand storage view changed"));
    }
    machine.read(&slot.qualified_layout_name)
}

pub(super) fn read_integer_slot(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<i128, MachineProblem> {
    let layout = machine
        .layouts
        .get(&slot.qualified_layout_name)
        .ok_or(MachineProblem::UnknownStorage)?;
    if !is_numeric(layout.category) {
        return Err(MachineProblem::DataException);
    }
    let value = decode_decimal(layout, &read_slot(machine, slot)?)?;
    if value.scale != 0 {
        return Err(MachineProblem::DataException);
    }
    Ok(value.coefficient)
}
