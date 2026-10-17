//! Position and replace dataset browse cursors without changing their identity.

use super::{
    BrowseIdentity, DatasetLimits, State, browse_identities, condition, entry, record_for_identity,
};
use mainframe_env_host_api::{
    DatasetName, DatasetOrganization, DatasetResult, HostProblem, KeyRelation,
};

pub(super) fn position(
    state: &State,
    dataset: &DatasetName,
    key: &[u8],
    relation: KeyRelation,
) -> Result<(Vec<BrowseIdentity>, usize), HostProblem> {
    let identities = browse_identities(state, dataset)?;
    let lower = identities.partition_point(|(logical, _)| logical.as_slice() < key);
    let upper = identities.partition_point(|(logical, _)| logical.as_slice() <= key);
    let index = match relation {
        KeyRelation::Equal if lower < upper => lower,
        KeyRelation::Greater => upper,
        KeyRelation::GreaterOrEqual => lower,
        KeyRelation::Less if lower > 0 => lower - 1,
        KeyRelation::LessOrEqual if upper > 0 => upper - 1,
        KeyRelation::Equal | KeyRelation::Less | KeyRelation::LessOrEqual => {
            return Err(condition("NOTFND", 13));
        }
    };
    if index >= identities.len() {
        // Sequential OPEN INPUT/I-O uses an unkeyed GTEQ browse. An existing
        // empty file still needs a cursor so its first READ can report EOF.
        if identities.is_empty()
            && key.is_empty()
            && relation == KeyRelation::GreaterOrEqual
            && entry(state, dataset)?.attributes.organization == DatasetOrganization::Sequential
        {
            return Ok((identities, index));
        }
        state.require_eof_browse(dataset, key, relation, !identities.is_empty())?;
    }
    Ok((identities, index))
}

pub(super) fn identity_bytes(identities: &[BrowseIdentity]) -> usize {
    identities
        .iter()
        .map(|(logical, identity)| logical.len() + identity.len())
        .sum()
}

pub(super) fn reset(
    state: &mut State,
    dataset: &DatasetName,
    cursor: &str,
    key: &[u8],
    relation: KeyRelation,
    limits: DatasetLimits,
) -> Result<DatasetResult, HostProblem> {
    if state
        .cursors
        .get(cursor)
        .is_none_or(|active| active.dataset != dataset.as_str())
    {
        return Err(condition("INVREQ", 16));
    }
    let (identities, index) = position(state, dataset, key, relation)?;
    let active_identities = state
        .cursors
        .iter()
        .filter(|(id, _)| id.as_str() != cursor)
        .map(|(_, active)| active.identities.len())
        .sum::<usize>();
    let active_bytes = state
        .cursors
        .iter()
        .filter(|(id, _)| id.as_str() != cursor)
        .map(|(_, active)| identity_bytes(&active.identities))
        .sum::<usize>();
    if active_identities
        .checked_add(identities.len())
        .is_none_or(|total| total > limits.max_records)
        || active_bytes
            .checked_add(identity_bytes(&identities))
            .is_none_or(|total| total > limits.max_total_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let active = state
        .cursors
        .get_mut(cursor)
        .ok_or_else(|| condition("INVREQ", 16))?;
    active.identities = identities;
    active.index = index as isize;
    Ok(DatasetResult::Browse {
        cursor: cursor.to_string(),
        record: None,
        identity: None,
        key: None,
    })
}

// The legacy gap traversal body is extracted without changing its advancement order.
pub(super) fn read_next(
    state: &mut State,
    dataset: &DatasetName,
    cursor: &str,
    reverse: bool,
) -> Result<DatasetResult, HostProblem> {
    let identity = {
        let state_cursor = state
            .cursors
            .get_mut(cursor)
            .ok_or_else(|| condition("INVREQ", 16))?;
        if state_cursor.dataset != dataset.as_str() {
            return Err(condition("INVREQ", 16));
        }
        if reverse {
            state_cursor.index -= 1;
        }
        let current = state_cursor.index;
        if !reverse {
            state_cursor.index += 1;
        }
        if current < 0 {
            None
        } else {
            state_cursor.identities.get(current as usize).cloned()
        }
    };
    let record = match identity.as_ref() {
        Some((_, identity)) => record_for_identity(state, dataset, identity)?.cloned(),
        None => None,
    };
    let logical_key = identity.as_ref().map(|(logical, _)| logical.clone());
    let base_identity = identity.map(|(_, identity)| identity);
    Ok(DatasetResult::Browse {
        cursor: cursor.to_owned(),
        record,
        identity: base_identity,
        key: logical_key,
    })
}

/// Observe the snapshot anchor and live body under the caller's existing state lock.
/// Neither successful observations nor refusals change the gap or snapshot vector.
pub(super) fn read_position(
    state: &State,
    dataset: &DatasetName,
    cursor: &str,
    expected_key: &[u8],
) -> Result<DatasetResult, HostProblem> {
    let active = state
        .cursors
        .get(cursor)
        .ok_or_else(|| condition("INVREQ", 16))?;
    if active.dataset != dataset.as_str() {
        return Err(condition("INVREQ", 16));
    }
    let width = if let Some(index) = state.alternate_indexes.get(dataset.as_str()) {
        index.key_length
    } else {
        let attributes = &entry(state, dataset)?.attributes;
        if attributes.organization != DatasetOrganization::KeySequenced {
            return Err(HostProblem::Unsupported);
        }
        attributes
            .key_length
            .ok_or(HostProblem::InfrastructureFailure)?
    };
    if expected_key.is_empty() || expected_key.len() != width as usize {
        return Err(HostProblem::Malformed);
    }
    let (logical, identity) = usize::try_from(active.index)
        .ok()
        .and_then(|index| active.identities.get(index))
        .filter(|(logical, _)| logical.as_slice() == expected_key)
        .ok_or_else(|| condition("NOTFND", 13))?;
    let record =
        record_for_identity(state, dataset, identity)?.ok_or_else(|| condition("NOTFND", 13))?;
    Ok(DatasetResult::Browse {
        cursor: cursor.into(),
        record: Some(record.clone()),
        identity: Some(identity.clone()),
        key: Some(logical.clone()),
    })
}

#[cfg(test)]
mod tests;
