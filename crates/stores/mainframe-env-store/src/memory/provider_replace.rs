//! The existing validated row replacement returns its owned prior row to undo callers.
use super::*;

impl MemoryStore {
    pub(super) fn replace_provider_state_locked(
        state: &mut State,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
        limits: StoreLimits,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        record.validate_write(limits.max_blob_bytes)?;
        let key = (record.namespace.clone(), record.key.clone());
        let current = state.provider_state.get(&key);
        match (current, expected_version) {
            (None, None) if record.version == 1 => {}
            (Some(current), Some(expected))
                if current.version == expected && record.version == expected + 1 => {}
            _ => return Err(StoreError::Conflict),
        }
        let old = current.map_or(0, |item| item.payload.len());
        if old == 0 && state.provider_state.len() >= limits.max_provider_state {
            return Err(StoreError::CapacityExceeded);
        }
        let next_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        Self::reserve_blob(state, old, record.payload.len(), limits)?;
        let prior = state.provider_state.insert(key, record);
        state.provider_epoch = next_epoch;
        Ok(prior)
    }
}
