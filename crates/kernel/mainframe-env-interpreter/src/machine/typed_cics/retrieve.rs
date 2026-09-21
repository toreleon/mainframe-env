use super::*;

pub(super) fn allocation_arguments(
    machine: &ReferenceMachine,
    target: &CicsTarget,
    operation: CicsPlanOperation,
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let mut arguments = BTreeMap::from([(
        "SET.MAXLENGTH".into(),
        payload(
            "mainframe-env.cics.decimal@1",
            allocation_capacity(machine, target)?
                .to_string()
                .into_bytes(),
        )?,
    )]);
    if operation == CicsPlanOperation::Getmain {
        arguments.insert(
            "SET.LIMIT".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                machine
                    .invocation
                    .limits
                    .max_storage_bytes
                    .to_string()
                    .into_bytes(),
            )?,
        );
    }
    Ok(arguments)
}

pub(super) fn allocation_capacity(
    machine: &ReferenceMachine,
    target: &CicsTarget,
) -> Result<usize, MachineProblem> {
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    let allocated = (machine.static_base_count..machine.bases.len())
        .filter(|base| !machine.freed_allocations.contains(base))
        .count();
    if allocated >= machine.invocation.limits.max_frames as usize {
        return Ok(0);
    }
    machine.address_bytes_for(machine.bases.len(), 0, pointer.length)?;
    let used = machine
        .bases
        .iter()
        .enumerate()
        .filter(|(base, _)| !machine.freed_allocations.contains(base))
        .try_fold(0usize, |total, (_, storage)| {
            total.checked_add(storage.len())
        })
        .ok_or(MachineProblem::ResourceExhausted)?;
    usize::try_from(machine.invocation.limits.max_storage_bytes)
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .checked_sub(used)
        .ok_or(MachineProblem::ResourceExhausted)
}

pub(super) fn freemain_argument(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<(&'static str, Vec<u8>), MachineProblem> {
    let pointer = resolved_slot(machine, slot)?;
    let bytes = machine.read_reference(&pointer)?;
    let valid = machine
        .decode_address(&bytes)
        .ok()
        .flatten()
        .is_some_and(|(base, offset)| {
            base >= machine.static_base_count
                && offset == 0
                && !machine.freed_allocations.contains(&base)
        });
    Ok((
        if valid {
            "mainframe-env.cics.allocated-pointer@1"
        } else {
            "mainframe-env.cics.invalid-pointer@1"
        },
        bytes,
    ))
}

pub(super) fn release_pointer(
    machine: &mut ReferenceMachine,
    value: &BoundedPayload,
) -> Result<(), MachineProblem> {
    if value.schema() != "mainframe-env.cics.allocated-pointer@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let Some((base, offset)) = machine.decode_address(value.bytes())? else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    if base < machine.static_base_count || offset != 0 || machine.freed_allocations.contains(&base)
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    machine.freed_allocations.insert(base);
    Ok(())
}

pub(super) fn release_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    name: &str,
    value: &BoundedPayload,
) -> Result<bool, MachineProblem> {
    if name != "FREEMAIN.POINTER" {
        return Ok(false);
    }
    if operation != CicsOperation::Freemain {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    release_pointer(machine, value)?;
    Ok(true)
}

pub(super) fn write_set_output(
    machine: &mut ReferenceMachine,
    target: &CicsTarget,
    value: &BoundedPayload,
) -> Result<(), MachineProblem> {
    if value.schema() == "mainframe-env.cics.pointer-null@1" {
        if !value.bytes().is_empty() {
            return Err(MachineProblem::UnexpectedHostResult);
        }
        let CicsTarget::Resolved(slot) = target else {
            return Err(MachineProblem::UnexpectedHostResult);
        };
        let pointer = resolved_slot(machine, slot)?;
        return machine.write_reference(&pointer, &vec![0; pointer.length]);
    }
    if value.schema() != "mainframe-env.cics.payload@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let capacity = allocation_capacity(machine, target)?;
    if value.bytes().len() > capacity {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    let base = machine.bases.len();
    let address = machine.address_bytes_for(base, 0, pointer.length)?;
    machine.bases.push(value.bytes().to_vec());
    machine.write_reference(&pointer, &address)
}
