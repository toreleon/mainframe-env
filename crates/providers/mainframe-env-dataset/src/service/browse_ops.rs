//! Position and replace dataset browse cursors without changing their identity.

use super::{BrowseIdentity, Cursor, DatasetLimits, State, browse_identities, condition};
use mainframe_env_host_api::{DatasetName, DatasetResult, HostProblem, KeyRelation};

fn position(
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
        state.require_eof_browse(dataset, key, relation, !identities.is_empty())?;
    }
    Ok((identities, index))
}

fn identity_bytes(identities: &[BrowseIdentity]) -> usize {
    identities
        .iter()
        .map(|(logical, identity)| logical.len() + identity.len())
        .sum()
}

pub(super) fn start(
    state: &mut State,
    dataset: &DatasetName,
    key: &[u8],
    relation: KeyRelation,
    limits: DatasetLimits,
) -> Result<DatasetResult, HostProblem> {
    let (identities, index) = position(state, dataset, key, relation)?;
    let active_identities = state
        .cursors
        .values()
        .map(|cursor| cursor.identities.len())
        .sum::<usize>();
    let active_bytes = state
        .cursors
        .values()
        .map(|cursor| identity_bytes(&cursor.identities))
        .sum::<usize>();
    if state.cursors.len() >= limits.max_cursors
        || active_identities
            .checked_add(identities.len())
            .is_none_or(|total| total > limits.max_records)
        || active_bytes
            .checked_add(identity_bytes(&identities))
            .is_none_or(|total| total > limits.max_total_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let cursor = format!("cursor-{}", state.next_cursor);
    state.next_cursor = state
        .next_cursor
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    state.cursors.insert(
        cursor.clone(),
        Cursor {
            dataset: dataset.as_str().into(),
            identities,
            index: index as isize,
        },
    );
    Ok(DatasetResult::Browse {
        cursor,
        record: None,
        identity: None,
        key: None,
    })
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
