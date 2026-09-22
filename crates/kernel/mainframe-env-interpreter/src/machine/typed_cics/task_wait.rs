use super::*;

const POSTED_ECB: [u8; 4] = [0x40, 0, 0, 0];

pub(super) fn arguments(
    machine: &ReferenceMachine,
    plan: &CicsEffectPlan,
) -> Result<Option<BTreeMap<String, BoundedPayload>>, MachineProblem> {
    if plan.operation != CicsPlanOperation::WaitEvent {
        return Ok(None);
    }
    let slot = plan
        .operands
        .iter()
        .find_map(|operand| {
            (operand.name == CicsOperandName::EventControlAddress).then_some(&operand.value)
        })
        .and_then(|value| match value {
            CicsOperandValue::Storage(slot) => Some(slot),
            _ => None,
        })
        .ok_or_else(|| invalid_plan("WAIT EVENT ECADDR storage is missing"))?;
    let pointer = resolved_slot(machine, slot)?;
    if pointer.length != 4 {
        return Err(invalid_plan(
            "WAIT EVENT ECADDR must be a four-byte pointer",
        ));
    }
    let bytes = machine.read_reference(&pointer)?;
    let invalid = |response2: u8| {
        Ok(Some(BTreeMap::from([(
            "ECADDR".into(),
            payload("mainframe-env.cics.invalid-event-list@1", vec![response2])?,
        )])))
    };
    if bytes.as_slice() == [0, 0, 0, 0] || bytes.as_slice() == [0xff, 0, 0, 0] {
        return invalid(2);
    }
    let Some((base, offset)) = machine.decode_address(&bytes).ok().flatten() else {
        return invalid(2);
    };
    if offset % 4 != 0 {
        return invalid(4);
    }
    let Some(event) = machine
        .bases
        .get(base)
        .and_then(|storage| storage.get(offset..offset.saturating_add(4)))
    else {
        return invalid(2);
    };
    Ok(Some(BTreeMap::from([
        (
            "ECADDR".into(),
            payload("mainframe-env.cics.event-list@1", bytes)?,
        ),
        (
            "EVENT.POSTED".into(),
            payload(
                "mainframe-env.cics.event-posted@1",
                vec![u8::from(event != [0, 0, 0, 0])],
            )?,
        ),
    ])))
}

pub(super) fn apply_posted_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    value: &BoundedPayload,
) -> Result<bool, MachineProblem> {
    if name != "EVENT.POSTED" {
        return Ok(false);
    }
    if operation != CicsOperation::WaitEvent
        || value.schema() != "mainframe-env.cics.event-index@1"
        || value.bytes() != b"0"
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let operation = machine
        .pc
        .checked_sub(1)
        .and_then(|pc| machine.operations.get(pc))
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let plan = plan(operation)?;
    if plan.operation != CicsPlanOperation::WaitEvent {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let slot = plan
        .operands
        .iter()
        .find_map(|operand| {
            (operand.name == CicsOperandName::EventControlAddress).then_some(&operand.value)
        })
        .and_then(|value| match value {
            CicsOperandValue::Storage(slot) => Some(slot),
            _ => None,
        })
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let pointer = resolved_slot(machine, slot)?;
    let pointer = machine.read_reference(&pointer)?;
    let Some((base, offset)) = machine.decode_address(&pointer)? else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    machine
        .bases
        .get_mut(base)
        .and_then(|storage| storage.get_mut(offset..offset.saturating_add(4)))
        .ok_or(MachineProblem::UnexpectedHostResult)?
        .copy_from_slice(&POSTED_ECB);
    Ok(true)
}
