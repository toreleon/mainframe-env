use super::*;

pub(super) fn encode_snapshot(snapshot: &MachineSnapshot) -> Option<Vec<u8>> {
    let mut bytes = b"MECP0011".to_vec();
    bytes.extend_from_slice(&snapshot.schema_version.to_be_bytes());
    bytes.extend_from_slice(&u64::try_from(snapshot.program_counter).ok()?.to_be_bytes());
    bytes.extend_from_slice(&snapshot.effect_sequence.to_be_bytes());
    bytes.extend_from_slice(&snapshot.executed_steps.to_be_bytes());
    push_bytes(&mut bytes, &snapshot.output)?;
    bytes.extend_from_slice(
        &u32::try_from(snapshot.base_storage.len())
            .ok()?
            .to_be_bytes(),
    );
    for storage in &snapshot.base_storage {
        push_bytes(&mut bytes, storage)?;
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.perform_stack.len())
            .ok()?
            .to_be_bytes(),
    );
    for target in &snapshot.perform_stack {
        bytes.extend_from_slice(&u64::try_from(*target).ok()?.to_be_bytes());
    }
    bytes.extend_from_slice(
        &u32::try_from(snapshot.altered_targets.len())
            .ok()?
            .to_be_bytes(),
    );
    for (from, to) in &snapshot.altered_targets {
        push_bytes(&mut bytes, from.as_bytes())?;
        push_bytes(&mut bytes, to.as_bytes())?;
    }
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
    Some(bytes)
}
