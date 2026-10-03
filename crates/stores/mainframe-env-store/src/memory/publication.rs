//! One touched-row mutation authority for legacy batches and audited publication.
use super::*;
use mainframe_env_store_api::AuditedProviderPublication;
#[cfg(test)]
mod tests;

impl MemoryStore {
    pub(super) fn publish_audited(
        &self,
        request: AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        crate::publication::validate(&request, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        super::root_terminal::guard_actor(&state, &request.intent.execution_id, None)?;
        for mutation in &request.mutations {
            match mutation {
                ProviderStateMutation::Put(write) => super::root_terminal::guard_provider(
                    &state,
                    &write.record.namespace,
                    &write.record.key,
                    Some(&write.record.payload),
                )?,
                ProviderStateMutation::Delete { namespace, key, .. } => {
                    super::root_terminal::guard_provider(&state, namespace, key, None)?
                }
                ProviderStateMutation::Move {
                    record, old_key, ..
                } => {
                    super::root_terminal::guard_provider(&state, &record.namespace, old_key, None)?;
                    super::root_terminal::guard_provider(
                        &state,
                        &record.namespace,
                        &record.key,
                        Some(&record.payload),
                    )?;
                }
            }
        }
        let retained = state
            .effects
            .get(&request.intent.key)
            .ok_or(StoreError::NotFound)?;
        let execution = state
            .executions
            .get(&request.intent.execution_id)
            .ok_or(StoreError::NotFound)?;
        let floor = logical_floor(&state)?;
        crate::publication::assert_fence(&request, retained, execution, floor)?;
        // Private attribution exists only after the real intent/execution fence.
        let document =
            super::root_terminal::writer_document(&state, execution, &request.mutations)?;
        if let Some(document) = &document {
            document.require_live(request.observed_tick, floor)?;
        }
        super::root_terminal::guard_writer_scopes(
            &state,
            document.as_ref(),
            &request.mutations,
            false,
        )?;
        Self::append_audited_locked(&mut state, request, self.limits)
    }
    pub(super) fn append_audited_locked(
        state: &mut State,
        request: AuditedProviderPublication,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        journal::journaled(state, |state, journal| {
            Self::apply_mutations_locked(state, journal, request.mutations, limits)?;
            journal.touch_logical_tick(state);
            state.logical_tick = request.observed_tick;
            journal.touch_next_audit_ordinal(state);
            journal.touch_provider_epoch(state);
            if let Some(ordinal) = state.next_audit_ordinal.checked_add(1) {
                journal.record_new_audit_key(crate::durable::audit_storage_key(
                    &request.audit.execution_id,
                    &format!("memory:{ordinal:020}"),
                ));
            }
            Self::append_audit_locked(state, request.audit, limits)
        })
    }
    pub(super) fn mutate_provider_rows(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        if mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        let mut state = self.lock()?;
        for mutation in &mutations {
            match mutation {
                ProviderStateMutation::Put(w) => super::root_terminal::guard_provider(
                    &state,
                    &w.record.namespace,
                    &w.record.key,
                    Some(&w.record.payload),
                )?,
                ProviderStateMutation::Delete { namespace, key, .. } => {
                    super::root_terminal::guard_provider(&state, namespace, key, None)?
                }
                ProviderStateMutation::Move {
                    record, old_key, ..
                } => {
                    super::root_terminal::guard_provider(&state, &record.namespace, old_key, None)?;
                    super::root_terminal::guard_provider(
                        &state,
                        &record.namespace,
                        &record.key,
                        Some(&record.payload),
                    )?;
                }
            }
        }
        let limits = self.limits;
        journal::journaled(&mut state, |state, journal| {
            Self::apply_mutations_locked(state, journal, mutations, limits)
        })
    }

    pub(super) fn apply_mutations_locked(
        state: &mut State,
        journal: &mut journal::Journal,
        mutations: Vec<ProviderStateMutation>,
        limits: StoreLimits,
    ) -> Result<(), StoreError> {
        let staging_limits = StoreLimits {
            max_provider_state: usize::MAX,
            max_total_blob_bytes: usize::MAX,
            ..limits
        };
        for mutation in mutations {
            match mutation {
                ProviderStateMutation::Put(write) => {
                    let key = (write.record.namespace.clone(), write.record.key.clone());
                    journal.touch_provider_state(state, &key);
                    journal.touch_blob_bytes(state);
                    journal.touch_provider_epoch(state);
                    Self::put_provider_state_locked(
                        state,
                        write.record,
                        write.expected_version,
                        staging_limits,
                    )?;
                }
                ProviderStateMutation::Delete {
                    namespace,
                    key,
                    expected_version,
                } => {
                    if namespace.is_empty() || key.is_empty() || expected_version == 0 {
                        return Err(StoreError::Conflict);
                    }
                    let map_key = (namespace, key);
                    let current = state
                        .provider_state
                        .get(&map_key)
                        .ok_or(StoreError::NotFound)?;
                    if current.version != expected_version {
                        return Err(StoreError::Conflict);
                    }
                    let bytes = current.payload.len();
                    journal.touch_provider_state(state, &map_key);
                    journal.touch_blob_bytes(state);
                    journal.touch_provider_epoch(state);
                    state.provider_state.remove(&map_key);
                    state.blob_bytes = state.blob_bytes.saturating_sub(bytes);
                    state.provider_epoch = state
                        .provider_epoch
                        .checked_add(1)
                        .ok_or(StoreError::CapacityExceeded)?;
                }
                ProviderStateMutation::Move {
                    record,
                    old_key,
                    expected_version,
                } => {
                    record.validate_move(&old_key, expected_version, limits.max_blob_bytes)?;
                    let old_map_key = (record.namespace.clone(), old_key);
                    let new_map_key = (record.namespace.clone(), record.key.clone());
                    let old = state
                        .provider_state
                        .get(&old_map_key)
                        .ok_or(StoreError::Conflict)?;
                    if old.version != expected_version
                        || state.provider_state.contains_key(&new_map_key)
                    {
                        return Err(StoreError::Conflict);
                    }
                    let old_bytes = old.payload.len();
                    journal.touch_provider_state(state, &old_map_key);
                    journal.touch_provider_state(state, &new_map_key);
                    journal.touch_blob_bytes(state);
                    Self::reserve_blob(state, old_bytes, record.payload.len(), staging_limits)?;
                    journal.touch_provider_epoch(state);
                    state.provider_state.remove(&old_map_key);
                    state.provider_state.insert(new_map_key, record);
                    state.provider_epoch = state
                        .provider_epoch
                        .checked_add(1)
                        .ok_or(StoreError::CapacityExceeded)?;
                }
            }
        }
        if state.provider_state.len() > limits.max_provider_state
            || state.blob_bytes > limits.max_total_blob_bytes
        {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(())
    }
}

/// The same physical floor for original-effect and audited row publications.
pub(super) fn logical_floor(state: &State) -> Result<u64, StoreError> {
    let mut floor = state.logical_tick;
    if let Some(legacy) = state
        .provider_state
        .get(&("jes-worker-meta".into(), "logical-clock".into()))
    {
        let tick = u64::from_be_bytes(
            legacy
                .payload
                .as_slice()
                .try_into()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        );
        if tick == 0 || tick > i64::MAX as u64 || legacy.version == 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        floor = floor.max(tick);
    }
    Ok(floor)
}
