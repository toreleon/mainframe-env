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

pub(super) fn dump_segments(
    machine: &ReferenceMachine,
    plan: &CicsEffectPlan,
) -> Result<Vec<u8>, MachineProblem> {
    let storage = |name| {
        plan.operands
            .iter()
            .find(|operand| operand.name == name)
            .and_then(|operand| match &operand.value {
                CicsOperandValue::Storage(slot) => Some(slot),
                _ => None,
            })
            .ok_or_else(|| invalid_plan("DUMP TRANSACTION segment list is incomplete"))
    };
    let count = read_integer_slot(machine, storage(CicsOperandName::DumpNumSegments)?)?;
    if !(0..=256).contains(&count) {
        return Err(invalid_plan("DUMP TRANSACTION NUMSEGMENTS is out of range"));
    }
    let count = usize::try_from(count).map_err(|_| invalid_plan("invalid segment count"))?;
    let pointers = read_slot(machine, storage(CicsOperandName::DumpSegmentList)?)?;
    let lengths = read_slot(machine, storage(CicsOperandName::DumpLengthList)?)?;
    let minimum = count
        .checked_mul(4)
        .ok_or_else(|| invalid_plan("segment list exceeds local bound"))?;
    if pointers.len() < minimum || lengths.len() < minimum {
        return Err(invalid_plan("DUMP TRANSACTION segment lists are too short"));
    }
    let mut encoded = Vec::new();
    for index in 0..count {
        let start = index * 4;
        let length = i32::from_be_bytes(
            lengths[start..start + 4]
                .try_into()
                .map_err(|_| invalid_plan("invalid segment length"))?,
        );
        if !(0..=16_777_215).contains(&length) {
            return Err(invalid_plan(
                "DUMP TRANSACTION segment length is out of range",
            ));
        }
        let length = usize::try_from(length).map_err(|_| invalid_plan("invalid segment length"))?;
        let (base, offset) = machine
            .decode_address(&pointers[start..start + 4])
            .ok()
            .flatten()
            .ok_or_else(|| invalid_plan("DUMP TRANSACTION segment address is invalid"))?;
        if machine.freed_allocations.contains(&base) {
            return Err(invalid_plan("DUMP TRANSACTION segment address is freed"));
        }
        let bytes = machine
            .bases
            .get(base)
            .and_then(|bytes| bytes.get(offset..offset.checked_add(length)?))
            .ok_or_else(|| invalid_plan("DUMP TRANSACTION segment exceeds storage"))?;
        encoded.extend_from_slice(
            &u32::try_from(length)
                .map_err(|_| invalid_plan("invalid segment length"))?
                .to_be_bytes(),
        );
        encoded.extend_from_slice(bytes);
        if encoded.len() > 1024 * 1024 {
            return Err(invalid_plan(
                "DUMP TRANSACTION segment payload exceeds local bound",
            ));
        }
    }
    Ok(encoded)
}

pub(super) fn argument_summary(arguments: &BTreeMap<String, BoundedPayload>) -> String {
    arguments
        .iter()
        .map(|(name, value)| {
            if matches!(name.as_str(), "MAP" | "MAPSET" | "TRANSID" | "PROGRAM") {
                format!(
                    "{name}={:?}/{}",
                    String::from_utf8_lossy(value.bytes()),
                    value.schema()
                )
            } else {
                format!("{name}=<{} bytes>/{}", value.bytes().len(), value.schema())
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}
