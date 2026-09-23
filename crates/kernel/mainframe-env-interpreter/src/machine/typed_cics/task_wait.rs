use super::*;

const POSTED_ECB: [u8; 4] = [0x40, 0, 0, 0];
const MAX_WAIT_EVENTS: usize = 1_024;

pub(super) fn arguments(
    machine: &ReferenceMachine,
    plan: &CicsEffectPlan,
) -> Result<Option<BTreeMap<String, BoundedPayload>>, MachineProblem> {
    match plan.operation {
        CicsPlanOperation::WaitEvent => wait_event_arguments(machine, plan).map(Some),
        CicsPlanOperation::WaitExternal => wait_external_arguments(machine, plan).map(Some),
        _ => Ok(None),
    }
}

fn wait_event_arguments(
    machine: &ReferenceMachine,
    plan: &CicsEffectPlan,
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
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
        Ok(BTreeMap::from([(
            "ECADDR".into(),
            payload("mainframe-env.cics.invalid-event-list@1", vec![response2])?,
        )]))
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
    Ok(BTreeMap::from([
        (
            "ECADDR".into(),
            payload("mainframe-env.cics.event-list@1", bytes)?,
        ),
        (
            "EVENT.POSTED".into(),
            payload(
                "mainframe-env.cics.event-posted@1",
                vec![u8::from(event[0] & POSTED_ECB[0] != 0)],
            )?,
        ),
    ]))
}

fn wait_external_arguments(
    machine: &ReferenceMachine,
    plan: &CicsEffectPlan,
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let invalid = |response2: u8| {
        Ok(BTreeMap::from([(
            "ECBLIST".into(),
            payload("mainframe-env.cics.invalid-event-list@1", vec![response2])?,
        )]))
    };
    let count = match plan_operand(plan, CicsOperandName::NumEvents)? {
        CicsOperandValue::Integer(value) => i128::from(*value),
        CicsOperandValue::Storage(slot) => read_integer_slot(machine, slot)?,
        _ => return Err(invalid_plan("WAIT EXTERNAL NUMEVENTS value is invalid")),
    };
    let Ok(count) = usize::try_from(count) else {
        return invalid(3);
    };
    if count == 0 {
        return invalid(3);
    }
    if count > MAX_WAIT_EVENTS {
        return invalid(5);
    }
    let CicsOperandValue::Storage(slot) = plan_operand(plan, CicsOperandName::EcbList)? else {
        return Err(invalid_plan("WAIT EXTERNAL ECBLIST storage is missing"));
    };
    let pointer = resolved_slot(machine, slot)?;
    if pointer.length != 4 {
        return Err(invalid_plan(
            "WAIT EXTERNAL ECBLIST must be a four-byte pointer",
        ));
    }
    let pointer = machine.read_reference(&pointer)?;
    if is_null_pointer(&pointer) {
        return invalid(2);
    }
    let Some((base, offset)) = machine.decode_address(&pointer).ok().flatten() else {
        return invalid(5);
    };
    if offset % 4 != 0 {
        return invalid(5);
    }
    let Some(length) = count.checked_mul(4) else {
        return invalid(5);
    };
    let Some(list) = machine
        .bases
        .get(base)
        .and_then(|storage| storage.get(offset..offset.saturating_add(length)))
    else {
        return invalid(5);
    };
    let mut events = Vec::with_capacity(length);
    let mut selected = None;
    let mut seen = BTreeSet::new();
    for (index, address) in list.chunks_exact(4).enumerate() {
        if is_null_pointer(address) {
            continue;
        }
        let Some((event_base, event_offset)) = machine.decode_address(address).ok().flatten()
        else {
            return invalid(1);
        };
        if event_offset % 4 != 0 || !seen.insert(address.to_vec()) {
            return invalid(1);
        }
        let Some(event) = machine
            .bases
            .get(event_base)
            .and_then(|storage| storage.get(event_offset..event_offset.saturating_add(4)))
        else {
            return invalid(1);
        };
        if selected.is_none() && event != [0, 0, 0, 0] {
            selected = Some(index);
        }
        events.extend_from_slice(
            &u32::try_from(index)
                .expect("bounded event index")
                .to_be_bytes(),
        );
        events.extend_from_slice(address);
    }
    if events.is_empty() {
        return invalid(5);
    }
    let posted = selected
        .map(|index| {
            u32::try_from(index)
                .expect("bounded event index")
                .to_be_bytes()
                .to_vec()
        })
        .unwrap_or_default();
    Ok(BTreeMap::from([
        (
            "ECBLIST".into(),
            payload("mainframe-env.cics.external-event-list@1", events)?,
        ),
        (
            "EVENT.POSTED".into(),
            payload("mainframe-env.cics.event-posted-index@1", posted)?,
        ),
    ]))
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
    if value.schema() != "mainframe-env.cics.event-index@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let index = std::str::from_utf8(value.bytes())
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let ir_operation = machine
        .pc
        .checked_sub(1)
        .and_then(|pc| machine.operations.get(pc))
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let plan = plan(ir_operation)?;
    if names::host_operation(plan.operation) != operation {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    if operation == CicsOperation::WaitExternal {
        return apply_external_post(machine, &plan, index);
    }
    if operation != CicsOperation::WaitEvent || index != 0 {
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

fn apply_external_post(
    machine: &mut ReferenceMachine,
    plan: &CicsEffectPlan,
    index: usize,
) -> Result<bool, MachineProblem> {
    let count = match plan_operand(plan, CicsOperandName::NumEvents)? {
        CicsOperandValue::Integer(value) => usize::try_from(i128::from(*value)).ok(),
        CicsOperandValue::Storage(slot) => usize::try_from(read_integer_slot(machine, slot)?).ok(),
        _ => None,
    }
    .ok_or(MachineProblem::UnexpectedHostResult)?;
    if index >= count {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let CicsOperandValue::Storage(slot) = plan_operand(plan, CicsOperandName::EcbList)? else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = machine.read_reference(&resolved_slot(machine, slot)?)?;
    let Some((base, offset)) = machine.decode_address(&pointer)? else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let address_offset = offset
        .checked_add(
            index
                .checked_mul(4)
                .ok_or(MachineProblem::UnexpectedHostResult)?,
        )
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let address = machine
        .bases
        .get(base)
        .and_then(|storage| storage.get(address_offset..address_offset.saturating_add(4)))
        .ok_or(MachineProblem::UnexpectedHostResult)?
        .to_vec();
    let Some((event_base, event_offset)) = machine.decode_address(&address)? else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let event = machine
        .bases
        .get_mut(event_base)
        .and_then(|storage| storage.get_mut(event_offset..event_offset.saturating_add(4)))
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    if event == [0, 0, 0, 0] {
        event.copy_from_slice(&POSTED_ECB);
    }
    Ok(true)
}

fn plan_operand(
    plan: &CicsEffectPlan,
    name: CicsOperandName,
) -> Result<&CicsOperandValue, MachineProblem> {
    plan.operands
        .iter()
        .find(|operand| operand.name == name)
        .map(|operand| &operand.value)
        .ok_or_else(|| invalid_plan("required wait operand is missing"))
}

fn is_null_pointer(value: &[u8]) -> bool {
    value == [0, 0, 0, 0] || value == [0xff, 0, 0, 0]
}
