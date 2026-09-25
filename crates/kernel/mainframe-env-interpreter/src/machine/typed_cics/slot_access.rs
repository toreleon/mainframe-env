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

pub(super) fn plan_slots(plan: &CicsEffectPlan) -> Result<Vec<CicsStorageSlot>, MachineProblem> {
    let mut slots = BTreeMap::<StorageId, CicsStorageSlot>::new();
    for operand in &plan.operands {
        if let CicsOperandValue::Storage(slot) | CicsOperandValue::LengthOf(slot) = &operand.value {
            insert_slot(&mut slots, slot)?;
        }
    }
    for output in &plan.outputs {
        insert_slot(&mut slots, &output.target)?;
    }
    match &plan.condition {
        CicsCondition::Default | CicsCondition::NoHandle => {}
        CicsCondition::Respond {
            response,
            response2,
        } => {
            insert_slot(&mut slots, response)?;
            if let Some(response2) = response2 {
                insert_slot(&mut slots, response2)?;
            }
        }
    }
    Ok(slots.into_values().collect())
}

fn insert_slot(
    slots: &mut BTreeMap<StorageId, CicsStorageSlot>,
    slot: &CicsStorageSlot,
) -> Result<(), MachineProblem> {
    if slots
        .insert(slot.storage, slot.clone())
        .is_some_and(|existing| existing.qualified_layout_name != slot.qualified_layout_name)
    {
        return Err(invalid_plan(
            "one storage slot names more than one qualified layout",
        ));
    }
    Ok(())
}
