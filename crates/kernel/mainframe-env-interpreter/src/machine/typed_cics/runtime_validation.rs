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
                    CicsOperandName::Length | CicsOperandName::KeyLength | CicsOperandName::Item
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
