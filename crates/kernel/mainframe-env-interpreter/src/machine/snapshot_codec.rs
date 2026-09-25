use super::*;

pub(super) fn encode_snapshot(snapshot: &MachineSnapshot) -> Option<Vec<u8>> {
    let mut bytes = encode_snapshot_prefix(snapshot)?;
    bytes.extend_from_slice(
        &u32::try_from(snapshot.loop_reentry.len())
            .ok()?
            .to_be_bytes(),
    );
    for node in &snapshot.loop_reentry {
        bytes.extend_from_slice(&u64::try_from(*node).ok()?.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.loop_counts.len())
            .ok()?
            .to_be_bytes(),
    );
    for (node, count) in &snapshot.loop_counts {
        bytes.extend_from_slice(&u64::try_from(*node).ok()?.to_be_bytes());
        bytes.extend_from_slice(&count.to_be_bytes());
    }
    push_bytes(&mut bytes, snapshot.last_file_status.as_bytes())?;
    bytes.extend_from_slice(
        &u32::try_from(snapshot.dataset_cursors.len())
            .ok()?
            .to_be_bytes(),
    );
    for (dataset, cursor) in &snapshot.dataset_cursors {
        push_bytes(&mut bytes, dataset.as_bytes())?;
        push_bytes(&mut bytes, cursor.as_bytes())?;
    }
    bytes.push(snapshot.condition_statuses);
    bytes.extend_from_slice(
        &u32::try_from(snapshot.dynamic_lengths.len())
            .ok()?
            .to_be_bytes(),
    );
    for (name, length) in &snapshot.dynamic_lengths {
        push_bytes(&mut bytes, name.as_bytes())?;
        bytes.extend_from_slice(&u64::try_from(*length).ok()?.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.implicit_values.len())
            .ok()?
            .to_be_bytes(),
    );
    for (name, value) in &snapshot.implicit_values {
        push_bytes(&mut bytes, name.as_bytes())?;
        match value {
            MachineSnapshotValue::Bytes(value) => {
                bytes.push(0);
                push_bytes(&mut bytes, value)?;
            }
            MachineSnapshotValue::Decimal { coefficient, scale } => {
                bytes.push(1);
                bytes.extend_from_slice(&coefficient.to_be_bytes());
                bytes.extend_from_slice(&scale.to_be_bytes());
            }
        }
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.search_results.len())
            .ok()?
            .to_be_bytes(),
    );
    for (node, found) in &snapshot.search_results {
        bytes.extend_from_slice(&u64::try_from(*node).ok()?.to_be_bytes());
        bytes.push(u8::from(*found));
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.sql_cursors.len())
            .ok()?
            .to_be_bytes(),
    );
    for (name, values) in &snapshot.sql_cursors {
        push_bytes(&mut bytes, name.as_bytes())?;
        push_string_list(&mut bytes, values)?;
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.sort_workspaces.len())
            .ok()?
            .to_be_bytes(),
    );
    for (name, (records, cursor)) in &snapshot.sort_workspaces {
        push_bytes(&mut bytes, name.as_bytes())?;
        bytes.extend_from_slice(&u64::try_from(*cursor).ok()?.to_be_bytes());
        bytes.extend_from_slice(&u32::try_from(records.len()).ok()?.to_be_bytes());
        for record in records {
            push_bytes(&mut bytes, record)?;
        }
    }
    match &snapshot.active_sort_procedure {
        Some((sort_pc, sort_file, phase, arguments)) => {
            bytes.push(1);
            bytes.extend_from_slice(&u64::try_from(*sort_pc).ok()?.to_be_bytes());
            push_bytes(&mut bytes, sort_file.as_bytes())?;
            bytes.push(*phase);
            push_string_list(&mut bytes, arguments)?;
        }
        None => bytes.push(0),
    }
    match &snapshot.sort_io {
        Some((sort_pc, sort_file, arguments, inputs, outputs, next_input, next_output)) => {
            bytes.push(1);
            bytes.extend_from_slice(&u64::try_from(*sort_pc).ok()?.to_be_bytes());
            push_bytes(&mut bytes, sort_file.as_bytes())?;
            push_string_list(&mut bytes, arguments)?;
            push_string_list(&mut bytes, inputs)?;
            push_string_list(&mut bytes, outputs)?;
            bytes.extend_from_slice(&u64::try_from(*next_input).ok()?.to_be_bytes());
            bytes.extend_from_slice(&u64::try_from(*next_output).ok()?.to_be_bytes());
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.linkage_addresses.len())
            .ok()?
            .to_be_bytes(),
    );
    for (name, view) in &snapshot.linkage_addresses {
        push_bytes(&mut bytes, name.as_bytes())?;
        match view {
            Some((base, offset, length)) => {
                bytes.push(1);
                bytes.extend_from_slice(&u64::try_from(*base).ok()?.to_be_bytes());
                bytes.extend_from_slice(&u64::try_from(*offset).ok()?.to_be_bytes());
                bytes.extend_from_slice(&u64::try_from(*length).ok()?.to_be_bytes());
            }
            None => bytes.push(0),
        }
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.freed_allocations.len())
            .ok()?
            .to_be_bytes(),
    );
    for base in &snapshot.freed_allocations {
        bytes.extend_from_slice(&u64::try_from(*base).ok()?.to_be_bytes());
    }
    match snapshot.random_state {
        Some(state) => {
            bytes.push(1);
            bytes.extend_from_slice(&state.to_be_bytes());
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&snapshot.storage64.next_id.to_be_bytes());
    bytes.extend_from_slice(&snapshot.storage64.next_loc24.to_be_bytes());
    bytes.extend_from_slice(&snapshot.storage64.next_loc31.to_be_bytes());
    bytes.extend_from_slice(
        &u32::try_from(snapshot.storage64.allocations.len())
            .ok()?
            .to_be_bytes(),
    );
    for allocation in &snapshot.storage64.allocations {
        bytes.extend_from_slice(&allocation.address.to_be_bytes());
        push_bytes(&mut bytes, allocation.owner.as_bytes())?;
        bytes.push(match allocation.attributes.location {
            Storage64Location::AboveBar => 0,
            Storage64Location::Loc24 => 1,
            Storage64Location::Loc31 => 2,
        });
        bytes.push(match allocation.attributes.key {
            Storage64Key::User => 0,
            Storage64Key::Cics => 1,
        });
        bytes.push(u8::from(allocation.attributes.shared));
        bytes.push(u8::from(allocation.attributes.executable));
        push_bytes(&mut bytes, &allocation.bytes)?;
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.storage64_area_bindings.len())
            .ok()?
            .to_be_bytes(),
    );
    for (name, address) in &snapshot.storage64_area_bindings {
        push_bytes(&mut bytes, name.as_bytes())?;
        bytes.extend_from_slice(&address.to_be_bytes());
    }
    Some(bytes)
}

pub(super) fn decode_storage64(
    input: &mut SnapshotInput<'_>,
    header_version: u32,
    max_frames: usize,
    remaining_storage: &mut usize,
) -> Result<(Storage64Snapshot, BTreeMap<String, u64>), MachineProblem> {
    let mut storage64 = Storage64Snapshot {
        next_id: 1,
        next_loc24: 0x0000_1000,
        next_loc31: 0x0100_0000,
        allocations: Vec::new(),
    };
    let mut storage64_area_bindings = BTreeMap::new();
    if header_version >= 11 {
        storage64.next_id = input.u32()?;
        storage64.next_loc24 = input.u64()?;
        storage64.next_loc31 = input.u64()?;
        let count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..count {
            let address = input.u64()?;
            let owner = snapshot_string(input, 4096)?;
            let location = match input.take(1)?.first() {
                Some(0) => Storage64Location::AboveBar,
                Some(1) => Storage64Location::Loc24,
                Some(2) => Storage64Location::Loc31,
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            let key = match input.take(1)?.first() {
                Some(0) => Storage64Key::User,
                Some(1) => Storage64Key::Cics,
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            let shared = match input.take(1)?.first() {
                Some(0) => false,
                Some(1) => true,
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            let executable = match input.take(1)?.first() {
                Some(0) => false,
                Some(1) => true,
                _ => return Err(MachineProblem::IncompatibleSnapshot),
            };
            let bytes = input.bytes(*remaining_storage)?;
            *remaining_storage = (*remaining_storage)
                .checked_sub(bytes.len())
                .ok_or(MachineProblem::IncompatibleSnapshot)?;
            storage64.allocations.push(Storage64Allocation {
                address,
                owner,
                attributes: Storage64Attributes {
                    location,
                    key,
                    shared,
                    executable,
                },
                bytes,
            });
        }
    }
    if header_version >= 12 {
        let count =
            usize::try_from(input.u32()?).map_err(|_| MachineProblem::IncompatibleSnapshot)?;
        if count > max_frames {
            return Err(MachineProblem::IncompatibleSnapshot);
        }
        for _ in 0..count {
            let name = snapshot_string(input, 4096)?;
            let address = input.u64()?;
            if storage64_area_bindings.insert(name, address).is_some() {
                return Err(MachineProblem::IncompatibleSnapshot);
            }
        }
    }
    Ok((storage64, storage64_area_bindings))
}
