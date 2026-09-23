use super::*;

pub(super) fn validate_runtime_plan(
    machine: &ReferenceMachine,
    operation: &Operation,
    plan: &CicsEffectPlan,
) -> Result<(), MachineProblem> {
    if matches!(
        plan.operation,
        CicsPlanOperation::Getmain64 | CicsPlanOperation::Freemain64
    ) && machine
        .invocation
        .bindings
        .get("cics.amode64.caller")
        .is_none_or(|value| {
            value.schema() != "mainframe-env.cics.amode64-caller@1"
                || value.bytes() != b"non-le-amode64"
        })
    {
        return Err(invalid_plan(
            "GETMAIN64 requires the checked non-LE AMODE(64) caller ABI",
        ));
    }
    if matches!(
        plan.operation,
        CicsPlanOperation::Getmain64 | CicsPlanOperation::Freemain64
    ) && machine
        .invocation
        .bindings
        .get("cics.amode64.taskdatakey")
        .is_none_or(|value| {
            value.schema() != "mainframe-env.cics.taskdatakey@1"
                || !matches!(value.bytes(), b"USER" | b"CICS")
        })
    {
        return Err(invalid_plan("GETMAIN64 requires a checked TASKDATAKEY"));
    }
    for operand in &plan.operands {
        if let CicsOperandValue::Storage(slot) | CicsOperandValue::LengthOf(slot) = &operand.value {
            let slot_use = if matches!(operand.value, CicsOperandValue::Storage(_))
                && matches!(
                    operand.name,
                    CicsOperandName::Length
                        | CicsOperandName::ControlCursor
                        | CicsOperandName::KeyLength
                        | CicsOperandName::Item
                ) {
                if matches!(
                    plan.operation,
                    CicsPlanOperation::DocumentCreate
                        | CicsPlanOperation::DocumentInsert
                        | CicsPlanOperation::DocumentSet
                ) {
                    SlotUse::FullwordInput
                } else {
                    SlotUse::HalfwordInput
                }
            } else {
                names::input_slot_use(operand.name)
            };
            validate_machine_slot(machine, operation, slot, slot_use)?;
        }
    }
    validate_address_set_slots(machine, operation, plan)?;
    for output in &plan.outputs {
        validate_machine_slot(
            machine,
            operation,
            &output.target,
            names::output_slot_use(output.name),
        )?;
    }
    Ok(())
}

pub(in crate::machine) fn into_payload_schema(
    operation: CicsOperation,
    response: &CicsResponse,
) -> Option<&str> {
    (!matches!(
        operation,
        CicsOperation::ReadTransientData
            | CicsOperation::DocumentRetrieve
            | CicsOperation::SpoolRead
    ) || matches!(response.condition.as_str(), "NORMAL" | "LENGERR"))
    .then(|| response.payload.schema())
}

pub(super) fn validate_machine_slot(
    machine: &ReferenceMachine,
    operation: &Operation,
    slot: &CicsStorageSlot,
    slot_use: SlotUse,
) -> Result<(), MachineProblem> {
    let layout = machine
        .layouts
        .get(&slot.qualified_layout_name)
        .ok_or_else(|| invalid_plan("plan layout name is unknown"))?;
    if machine.storage_names_by_id.get(&slot.storage) != Some(&slot.qualified_layout_name) {
        return Err(invalid_plan("plan storage ID does not name its layout"));
    }
    let id_view = machine
        .views_by_id
        .get(&slot.storage)
        .ok_or_else(|| invalid_plan("plan storage slot is unknown"))?;
    let name_view = machine
        .views
        .get(&slot.qualified_layout_name)
        .ok_or_else(|| invalid_plan("plan layout has no storage view"))?;
    if layout.name != slot.qualified_layout_name || id_view != name_view {
        return Err(invalid_plan(
            "plan storage slot does not match its qualified layout name",
        ));
    }
    if matches!(
        slot_use,
        SlotUse::Output
            | SlotUse::CounterNumber
            | SlotUse::AbstimeOutput
            | SlotUse::FormatTextOutput(_)
            | SlotUse::MillisecondsOutput
            | SlotUse::NumericOutput
            | SlotUse::HalfwordOutput
            | SlotUse::FullwordOutput
            | SlotUse::PointerOutput
            | SlotUse::Pointer64Output
            | SlotUse::AddressOutput
            | SlotUse::AssignOutput(_)
            | SlotUse::SpoolTokenOutput
            | SlotUse::SpoolToFlengthOutput
    ) && matches!(
        layout.category,
        LayoutCategory::Condition | LayoutCategory::Rename
    ) {
        return Err(invalid_plan("plan output is not writable storage"));
    }
    if matches!(slot_use, SlotUse::NumericOutput) && !is_numeric(layout.category) {
        return Err(invalid_plan("RESP and RESP2 outputs must be numeric"));
    }
    if matches!(slot_use, SlotUse::HalfwordOutput)
        && (layout.category != LayoutCategory::Binary || layout.length != 2 || layout.scale != 0)
    {
        return Err(invalid_plan("LOAD LENGTH output must be halfword binary"));
    }
    if matches!(slot_use, SlotUse::FullwordOutput)
        && (layout.category != LayoutCategory::Binary || layout.length != 4 || layout.scale != 0)
    {
        return Err(invalid_plan("LOAD FLENGTH output must be fullword binary"));
    }
    if matches!(slot_use, SlotUse::HalfwordInput)
        && (layout.category != LayoutCategory::Binary || layout.length != 2)
    {
        return Err(invalid_plan(
            "LENGTH and KEYLENGTH inputs must be halfword binary",
        ));
    }
    if matches!(slot_use, SlotUse::FullwordInput)
        && (layout.category != LayoutCategory::Binary || layout.length != 4 || layout.scale != 0)
    {
        return Err(invalid_plan("fullword CICS input must be fullword binary"));
    }
    if matches!(slot_use, SlotUse::CounterNumber) {
        let (length, signed) = names::counter_shape(expected_operation(&operation.identity))
            .ok_or_else(|| invalid_plan("counter number belongs to another operation"))?;
        if layout.category != LayoutCategory::Binary
            || layout.length != length
            || layout.scale != 0
            || layout.signed != signed
        {
            return Err(invalid_plan(
                "counter number has the wrong binary width or sign",
            ));
        }
    }
    if matches!(slot_use, SlotUse::FullwordOutput)
        && (layout.category != LayoutCategory::Binary || layout.length != 4 || layout.scale != 0)
    {
        return Err(invalid_plan("TOKEN output must be fullword binary"));
    }
    if let SlotUse::AssignOutput(output) = slot_use {
        assign::validate_output(layout, output)?;
    }
    if matches!(slot_use, SlotUse::AbcodeInput)
        && (!matches!(layout.length, 1..=4)
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "ABEND ABCODE input must be a 1-4 character field",
        ));
    }
    if matches!(slot_use, SlotUse::ProgramNameInput)
        && (!matches!(layout.length, 1..=8)
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "HANDLE ABEND PROGRAM input must be a 1-8 character field",
        ));
    }
    spool_control::validate_slot(layout, slot_use)?;
    if matches!(slot_use, SlotUse::AbstimeInput | SlotUse::AbstimeOutput)
        && (layout.category != LayoutCategory::PackedDecimal
            || layout.length != 8
            || layout.digits != 15
            || layout.scale != 0
            || !layout.signed)
    {
        return Err(invalid_plan("CICS absolute time must be PIC S9(15) COMP-3"));
    }
    if matches!(slot_use, SlotUse::SeparatorInput)
        && (layout.length != 1
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "FORMATTIME separator input must be one character",
        ));
    }
    if let SlotUse::FormatTextOutput(expected) = slot_use
        && (layout.length != expected
            || !matches!(
                layout.category,
                LayoutCategory::Alphabetic | LayoutCategory::Alphanumeric
            ))
    {
        return Err(invalid_plan(
            "FORMATTIME character output has the wrong layout",
        ));
    }
    if matches!(slot_use, SlotUse::MillisecondsOutput)
        && (layout.category != LayoutCategory::Binary || layout.length != 4 || layout.scale != 0)
    {
        return Err(invalid_plan(
            "FORMATTIME MILLISECONDS output must be fullword binary",
        ));
    }
    if matches!(slot_use, SlotUse::PointerInput | SlotUse::PointerOutput)
        && !matches!(
            layout.category,
            LayoutCategory::Pointer | LayoutCategory::Pointer32
        )
    {
        return Err(invalid_plan(
            "ADDRESS SET pointer operands must use POINTER or POINTER-32",
        ));
    }
    storage64::validate_slot(slot_use, layout)?;
    if matches!(slot_use, SlotUse::AddressInput | SlotUse::AddressOutput)
        && matches!(
            layout.category,
            LayoutCategory::Condition | LayoutCategory::Rename
        )
    {
        return Err(invalid_plan("ADDRESS SET data area is not addressable"));
    }
    if matches!(slot_use, SlotUse::AddressOutput) && !layout.linkage {
        return Err(invalid_plan(
            "ADDRESS SET ADDRESS OF target must be linkage storage",
        ));
    }
    let exact_length = u64::try_from(id_view.length)
        .map_err(|_| invalid_plan("plan storage view length is invalid"))?;
    if !operation.storage.iter().any(|reference| {
        reference.storage == slot.storage
            && reference.offset == 0
            && reference.length == exact_length
    }) {
        return Err(invalid_plan(
            "operation does not declare the complete plan storage view",
        ));
    }
    Ok(())
}
