use super::*;

pub(super) fn allocation_arguments(
    machine: &ReferenceMachine,
    target: &CicsTarget,
    operation: CicsPlanOperation,
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let mut capacity = allocation_capacity(machine, target)?;
    if matches!(
        operation,
        CicsPlanOperation::ReceivePartn | CicsPlanOperation::IssueReceive
    ) {
        let live = machine
            .bases
            .iter()
            .enumerate()
            .filter(|(base, _)| !machine.freed_allocations.contains(base))
            .count();
        capacity = if live + machine.storage64.live_allocations() + 3
            > machine.invocation.limits.max_frames as usize
        {
            0
        } else {
            capacity.saturating_sub(PARTITION_RECEIVE_MARKER.len())
        };
    }
    let mut arguments = BTreeMap::from([(
        "SET.MAXLENGTH".into(),
        payload(
            "mainframe-env.cics.decimal@1",
            capacity.to_string().into_bytes(),
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
    if allocated + machine.storage64.live_allocations()
        >= machine.invocation.limits.max_frames as usize
    {
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
    let used = used
        .checked_add(
            usize::try_from(
                machine
                    .storage64
                    .charged_bytes()
                    .ok_or(MachineProblem::ResourceExhausted)?,
            )
            .map_err(|_| MachineProblem::ResourceExhausted)?,
        )
        .ok_or(MachineProblem::ResourceExhausted)?;
    usize::try_from(machine.invocation.limits.max_storage_bytes)
        .map_err(|_| MachineProblem::ResourceExhausted)?
        .checked_sub(used)
        .ok_or(MachineProblem::ResourceExhausted)
}

pub(super) fn allocation64_arguments(
    machine: &ReferenceMachine,
    target: &CicsTarget,
    location: Option<&BoundedPayload>,
) -> Result<BTreeMap<String, BoundedPayload>, MachineProblem> {
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    if pointer.length != 8 {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let used_base = machine
        .bases
        .iter()
        .enumerate()
        .filter(|(base, _)| !machine.freed_allocations.contains(base))
        .try_fold(0u64, |total, (_, storage)| {
            total.checked_add(storage.len() as u64)
        })
        .ok_or(MachineProblem::ResourceExhausted)?;
    let available = machine
        .invocation
        .limits
        .max_storage_bytes
        .saturating_sub(used_base)
        .saturating_sub(
            machine
                .storage64
                .charged_bytes()
                .ok_or(MachineProblem::ResourceExhausted)?,
        );
    let live_bases = (machine.static_base_count..machine.bases.len())
        .filter(|base| !machine.freed_allocations.contains(base))
        .count();
    let available = if live_bases + machine.storage64.live_allocations()
        >= machine.invocation.limits.max_frames as usize
    {
        0
    } else {
        available
    };
    let mut arguments = BTreeMap::from([
        (
            "SET64.MAXLENGTH".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                available.to_string().into_bytes(),
            )?,
        ),
        (
            "SET64.LIMIT".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                machine
                    .invocation
                    .limits
                    .max_storage_bytes
                    .to_string()
                    .into_bytes(),
            )?,
        ),
    ]);
    let location = match location.map(BoundedPayload::bytes) {
        None => crate::storage64::Storage64Location::AboveBar,
        Some(b"LOC24") => crate::storage64::Storage64Location::Loc24,
        Some(b"LOC31") => crate::storage64::Storage64Location::Loc31,
        Some(_) => return Err(MachineProblem::UnexpectedHostResult),
    };
    if let Some((limit, available)) = machine.storage64.location_capacity(location) {
        arguments.insert(
            "SET64.DSALIMIT".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                limit.to_string().into_bytes(),
            )?,
        );
        arguments.insert(
            "SET64.DSAAVAILABLE".into(),
            payload(
                "mainframe-env.cics.decimal@1",
                available.to_string().into_bytes(),
            )?,
        );
    }
    Ok(arguments)
}

pub(super) fn write_set64_output(
    machine: &mut ReferenceMachine,
    target: &CicsTarget,
    value: &BoundedPayload,
) -> Result<(), MachineProblem> {
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    if pointer.length != 8 {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    if value.schema() == "mainframe-env.cics.pointer64-null@1" && value.bytes().is_empty() {
        return machine.write_reference(&pointer, &[0; 8]);
    }
    if value.schema() != "mainframe-env.cics.storage64-allocation@1" || value.bytes().len() != 8 {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let bytes = value.bytes();
    let attributes = crate::storage64::Storage64Attributes {
        location: match bytes[0] {
            0 => crate::storage64::Storage64Location::AboveBar,
            1 => crate::storage64::Storage64Location::Loc24,
            2 => crate::storage64::Storage64Location::Loc31,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        },
        key: match bytes[1] {
            0 => crate::storage64::Storage64Key::User,
            1 => crate::storage64::Storage64Key::Cics,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        },
        shared: match bytes[2] {
            0 => false,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        },
        executable: match bytes[3] {
            0 => false,
            1 => true,
            _ => return Err(MachineProblem::UnexpectedHostResult),
        },
    };
    let length = u32::from_be_bytes(
        bytes[4..8]
            .try_into()
            .map_err(|_| MachineProblem::UnexpectedHostResult)?,
    );
    let charged = ((u64::from(length) + 15) & !15) + 16;
    let base_used = machine
        .bases
        .iter()
        .enumerate()
        .filter(|(base, _)| !machine.freed_allocations.contains(base))
        .try_fold(0u64, |total, (_, storage)| {
            total.checked_add(storage.len() as u64)
        })
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    if base_used
        .checked_add(
            machine
                .storage64
                .charged_bytes()
                .ok_or(MachineProblem::UnexpectedHostResult)?,
        )
        .and_then(|used| used.checked_add(charged))
        .is_none_or(|used| used > machine.invocation.limits.max_storage_bytes)
        || (machine.static_base_count..machine.bases.len())
            .filter(|base| !machine.freed_allocations.contains(base))
            .count()
            + machine.storage64.live_allocations()
            >= machine.invocation.limits.max_frames as usize
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let address = machine
        .storage64
        .allocate(
            machine.invocation.run_unit_id.as_str(),
            i64::from(length),
            attributes,
        )
        .map_err(|_| MachineProblem::UnexpectedHostResult)?;
    machine.write_reference(&pointer, &address.to_be_bytes())
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

pub(super) fn freemain_data_argument(
    machine: &ReferenceMachine,
    slot: &CicsStorageSlot,
) -> Result<(&'static str, Vec<u8>), MachineProblem> {
    let reference = resolved_slot(machine, slot)?;
    let Some(view) = machine.storage_view(&reference.layout.name).ok() else {
        return Ok(("mainframe-env.cics.invalid-pointer@1", vec![0; 8]));
    };
    if view.base < machine.static_base_count
        || view.offset != 0
        || machine.freed_allocations.contains(&view.base)
    {
        return Ok(("mainframe-env.cics.invalid-pointer@1", vec![0; 8]));
    }
    Ok((
        "mainframe-env.cics.allocated-pointer@1",
        machine.address_bytes_for(view.base, view.offset, 8)?,
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

pub(super) fn release_temporary_storage_set(machine: &mut ReferenceMachine) {
    let mut releases = Vec::new();
    for marker in machine.static_base_count..machine.bases.len().saturating_sub(2) {
        if machine.bases[marker].is_empty()
            && machine.bases[marker + 1].is_empty()
            && machine.freed_allocations.contains(&marker)
            && machine.freed_allocations.contains(&(marker + 1))
            && !machine.freed_allocations.contains(&(marker + 2))
        {
            releases.push(marker + 2);
        }
    }
    machine.freed_allocations.extend(releases);
}

const PARTITION_RECEIVE_MARKER: &[u8] = b"MEC-RECEIVE-PARTN";

pub(super) fn release_partition_receive_set(machine: &mut ReferenceMachine) {
    let mut releases = Vec::new();
    for marker in machine.static_base_count..machine.bases.len().saturating_sub(2) {
        if machine.bases[marker] == PARTITION_RECEIVE_MARKER
            && machine.bases[marker + 1].is_empty()
            && machine.freed_allocations.contains(&marker)
            && machine.freed_allocations.contains(&(marker + 1))
            && !machine.freed_allocations.contains(&(marker + 2))
        {
            releases.push(marker + 2);
        }
    }
    machine.freed_allocations.extend(releases);
}

pub(super) fn write_set_output(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
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
    if value.bytes().len() > capacity
        || matches!(
            operation,
            CicsOperation::ReceivePartn | CicsOperation::IssueReceive
        ) && (value
            .bytes()
            .len()
            .saturating_add(PARTITION_RECEIVE_MARKER.len())
            > capacity
            || machine
                .bases
                .iter()
                .enumerate()
                .filter(|(base, _)| !machine.freed_allocations.contains(base))
                .count()
                + machine.storage64.live_allocations()
                + 3
                > machine.invocation.limits.max_frames as usize)
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    let base = if operation == CicsOperation::ReadTemporaryStorage {
        let marker = machine.bases.len();
        machine.bases.extend([Vec::new(), Vec::new()]);
        machine.freed_allocations.extend([marker, marker + 1]);
        marker + 2
    } else if matches!(
        operation,
        CicsOperation::ReceivePartn | CicsOperation::IssueReceive
    ) {
        let marker = machine.bases.len();
        machine
            .bases
            .extend([PARTITION_RECEIVE_MARKER.to_vec(), Vec::new()]);
        machine.freed_allocations.extend([marker, marker + 1]);
        marker + 2
    } else {
        machine.bases.len()
    };
    let address = machine.address_bytes_for(base, 0, pointer.length)?;
    machine.bases.push(value.bytes().to_vec());
    machine.write_reference(&pointer, &address)
}

pub(super) fn prepare_load_allocation(
    machine: &mut ReferenceMachine,
    operation: CicsOperation,
    response: &CicsResponse,
    outputs: &BTreeMap<String, CicsTarget>,
) -> Result<Option<usize>, MachineProblem> {
    if operation != CicsOperation::Load {
        return Ok(None);
    }
    let pointer_targets = ["SET", "ENTRY"]
        .into_iter()
        .filter_map(|name| outputs.get(name))
        .collect::<Vec<_>>();
    let content = response.outputs.get("LOAD.CONTENT");
    if pointer_targets.is_empty() {
        return if content.is_some() {
            Err(MachineProblem::UnexpectedHostResult)
        } else {
            Ok(None)
        };
    }
    if response.response != 0 {
        return if content.is_some() {
            Err(MachineProblem::UnexpectedHostResult)
        } else {
            Ok(None)
        };
    }
    let content = content.ok_or(MachineProblem::UnexpectedHostResult)?;
    if content.schema() != "mainframe-env.cics.payload@1"
        || pointer_targets
            .iter()
            .map(|target| allocation_capacity(machine, target))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|capacity| content.bytes().len() > capacity)
    {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let base = machine.bases.len();
    machine.bases.push(content.bytes().to_vec());
    Ok(Some(base))
}

pub(super) fn write_load_pointer(
    machine: &mut ReferenceMachine,
    target: &CicsTarget,
    value: &BoundedPayload,
    base: usize,
) -> Result<(), MachineProblem> {
    if value.schema() != "mainframe-env.cics.load-offset@1" {
        return Err(MachineProblem::UnexpectedHostResult);
    }
    let offset = std::str::from_utf8(value.bytes())
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|offset| {
            machine
                .bases
                .get(base)
                .is_some_and(|storage| *offset <= storage.len())
        })
        .ok_or(MachineProblem::UnexpectedHostResult)?;
    let CicsTarget::Resolved(slot) = target else {
        return Err(MachineProblem::UnexpectedHostResult);
    };
    let pointer = resolved_slot(machine, slot)?;
    let address = machine.address_bytes_for(base, offset, pointer.length)?;
    machine.write_reference(&pointer, &address)
}
