//! Provider-state trait routing retains the existing memory authority.
use super::*;

impl ProviderStateStore for MemoryStore {
    crate::checked_read::checked_read_methods!();
    fn advance_logical_clock(&self, observed_floor: u64) -> Result<u64, StoreError> {
        if observed_floor > i64::MAX as u64 {
            return Err(StoreError::CapacityExceeded);
        }
        let mut state = self.lock()?;
        if let Some(legacy) = state
            .provider_state
            .get(&("jes-worker-meta".into(), "logical-clock".into()))
            .cloned()
        {
            let tick = u64::from_be_bytes(
                legacy
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| StoreError::IncompatibleVersion)?,
            );
            if tick == 0 || legacy.version == 0 {
                return Err(StoreError::IncompatibleVersion);
            }
            state.logical_tick = state.logical_tick.max(tick);
            state
                .provider_state
                .remove(&("jes-worker-meta".into(), "logical-clock".into()));
            state.blob_bytes = state.blob_bytes.saturating_sub(legacy.payload.len());
            state.provider_epoch = state
                .provider_epoch
                .checked_add(1)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        let tick = state.logical_tick.max(observed_floor.max(1));
        state.logical_tick = tick;
        Ok(tick)
    }

    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        Ok(self
            .lock()?
            .provider_state
            .get(&(namespace.to_string(), key.to_string()))
            .cloned())
    }

    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.legacy_provider_page(namespace, max)
    }

    fn list_provider_state_bounded(
        &self,
        namespace: &str,
        max: usize,
        max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.bounded_provider_page(namespace, max, max_bytes)
    }

    fn list_provider_state_prefix(
        &self,
        namespace_prefix: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if namespace_prefix.is_empty()
            || namespace_prefix.len() > mainframe_env_store_api::MAX_PROVIDER_NAMESPACE_BYTES
            || max == 0
            || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN
        {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .provider_state
            .iter()
            .filter(|((namespace, _), _)| namespace.starts_with(namespace_prefix))
            .take(max)
            .map(|(_, record)| record.clone())
            .collect())
    }

    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected_version: Option<u64>,
    ) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        root_terminal::guard_successor_identity(
            &state,
            &record.namespace,
            &record.key,
            Some(&record.payload),
        )?;
        root_terminal::guard_provider(
            &state,
            &record.namespace,
            &record.key,
            Some(&record.payload),
        )?;
        Self::put_provider_state_locked(&mut state, record, expected_version, self.limits)
    }

    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        let mut state = self.lock()?;
        root_terminal::guard_successor_identity(&state, namespace, key, None)?;
        root_terminal::guard_provider(&state, namespace, key, None)?;
        let map_key = (namespace.to_string(), key.to_string());
        let current = state
            .provider_state
            .get(&map_key)
            .ok_or(StoreError::NotFound)?;
        if current.version != expected_version {
            return Err(StoreError::Conflict);
        }
        let next_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let bytes = current.payload.len();
        state.provider_state.remove(&map_key);
        state.blob_bytes = state.blob_bytes.saturating_sub(bytes);
        state.provider_epoch = next_epoch;
        Ok(())
    }

    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        record.validate_move(old_key, expected_version, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        root_terminal::guard_successor_identity(&state, &record.namespace, old_key, None)?;
        root_terminal::guard_successor_identity(&state, &record.namespace, &record.key, None)?;
        root_terminal::guard_provider(&state, &record.namespace, old_key, None)?;
        root_terminal::guard_provider(
            &state,
            &record.namespace,
            &record.key,
            Some(&record.payload),
        )?;
        let old_map_key = (record.namespace.clone(), old_key.to_string());
        let new_map_key = (record.namespace.clone(), record.key.clone());
        let old = state
            .provider_state
            .get(&old_map_key)
            .ok_or(StoreError::Conflict)?;
        if old.version != expected_version || state.provider_state.contains_key(&new_map_key) {
            return Err(StoreError::Conflict);
        }
        let next_epoch = state
            .provider_epoch
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let old_bytes = old.payload.len();
        Self::reserve_blob(&mut state, old_bytes, record.payload.len(), self.limits)?;
        state.provider_state.remove(&old_map_key);
        state.provider_state.insert(new_map_key, record);
        state.provider_epoch = next_epoch;
        Ok(())
    }

    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.mutate_provider_states_atomic(
            writes.into_iter().map(ProviderStateMutation::Put).collect(),
        )
    }

    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.mutate_provider_rows(mutations)
    }

    fn archive_provider_state_replacement(
        &self,
        request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_replacement(&request, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        if state.provider_epoch != request.expected_epoch {
            return Err(StoreError::Conflict);
        }
        let current = state
            .provider_state
            .get(&(request.source.namespace.clone(), request.source.key.clone()))
            .ok_or(StoreError::Conflict)?;
        if current != &request.source {
            return Err(StoreError::Conflict);
        }
        root_terminal::guard_successor_identity(
            &state,
            &request.replacement.record.namespace,
            &request.replacement.record.key,
            Some(&request.replacement.record.payload),
        )?;
        for candidate in &request.rows {
            if let Some(proof) = &candidate.observation
                && state.observations.get(&(
                    request.target,
                    proof.observation.namespace.clone(),
                    proof.observation.key.clone(),
                )) != Some(&(proof.version, proof.observation.clone()))
            {
                return Err(StoreError::Conflict);
            }
        }
        let archive = build_archive(
            request.target,
            request.archived_tick,
            request.watermark_tick,
            request
                .rows
                .into_iter()
                .map(|candidate| ArchivedRetentionRow {
                    namespace: candidate.row.namespace,
                    key: candidate.row.key,
                    version: candidate.row.version,
                    payload: candidate.row.payload,
                    retention_tick: candidate.retention_tick,
                    owner_execution: candidate.owner_execution,
                })
                .collect(),
        )?;
        let mut staged = self.snapshot(&state);
        Self::put_provider_state_locked(
            &mut staged,
            request.replacement.record,
            request.replacement.expected_version,
            self.limits,
        )?;
        for row in &archive.rows {
            remove_memory_observation(&mut staged, request.target, row)?;
        }
        insert_memory_archive(&mut staged, archive.clone(), self.limits)?;
        *state = staged;
        Ok(archive)
    }

    fn provider_retention_archive_usage(
        &self,
        target: RetentionTarget,
    ) -> Result<(usize, u64), StoreError> {
        let state = self.lock()?;
        let mut rows = 0usize;
        let mut bytes = 0u64;
        for archive in state
            .archives
            .values()
            .filter(|archive| archive.target == target)
        {
            rows = rows
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)?;
            bytes = bytes
                .checked_add(crate::retention::archive_storage_bytes(&archive.rows)?)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        Ok((rows, bytes))
    }

    fn provider_retention_authority_usage(
        &self,
        target: RetentionTarget,
    ) -> Result<mainframe_env_store_api::RetentionAuthorityUsage, StoreError> {
        let state = self.lock()?;
        let mut archive_rows = 0usize;
        let mut archive_bytes = 0u64;
        for archive in state
            .archives
            .values()
            .filter(|archive| archive.target == target)
        {
            archive_rows = archive_rows
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)?;
            archive_bytes = archive_bytes
                .checked_add(crate::retention::archive_storage_bytes(&archive.rows)?)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        let mut observation_rows = 0usize;
        let mut observation_bytes = 0u64;
        for ((candidate, _, _), (_, observation)) in &state.observations {
            if *candidate == target {
                observation_rows = observation_rows
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?;
                observation_bytes = observation_bytes
                    .checked_add(crate::retention::observation::storage_bytes(observation)?)
                    .ok_or(StoreError::CapacityExceeded)?;
            }
        }
        Ok(mainframe_env_store_api::RetentionAuthorityUsage {
            archive_rows,
            shared_archive_rows: state
                .archives
                .values()
                .map(|archive| archive.rows.len())
                .sum(),
            archive_row_capacity: self.limits.max_retention_archive_rows,
            archive_bytes,
            shared_archive_bytes: state.archive_bytes,
            archive_byte_capacity: self.limits.max_retention_archive_bytes,
            observation_rows,
            shared_observation_rows: state.observations.len(),
            observation_row_capacity: self.limits.max_retention_archive_rows,
            observation_bytes,
            shared_observation_bytes: state.observation_bytes,
            observation_byte_capacity: self.limits.max_retention_archive_bytes,
        })
    }

    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        Ok(self.lock()?.provider_epoch)
    }

    fn archive_provider_state_deletion(
        &self,
        request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        self.archive_provider_deletion(request)
    }

    fn archive_provider_state_deletion_with_capacity(
        &self,
        request: ProviderStateArchiveDeletionWithCapacity,
    ) -> Result<RetentionArchive, StoreError> {
        self.archive_container_replay_with_capacity(request)
    }

    fn provider_retention_observations(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionObservation>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .lock()?
            .observations
            .iter()
            .filter(|((candidate, _, _), _)| *candidate == target)
            .map(|(_, (_, observation))| observation.clone())
            .take(max)
            .collect())
    }

    fn provider_retention_observation_page(
        &self,
        target: RetentionTarget,
        after: Option<&mainframe_env_store_api::ProviderStateIdentity>,
        max: usize,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        let state = self.lock()?;
        Ok(state
            .observations
            .iter()
            .filter(|((candidate, namespace, key), _)| {
                *candidate == target
                    && after.is_none_or(|after| {
                        (namespace.as_str(), key.as_str())
                            > (after.namespace.as_str(), after.key.as_str())
                    })
            })
            .map(|(_, entry)| entry.clone())
            .take(max)
            .collect())
    }

    fn delete_provider_retention_observation(
        &self,
        request: ProviderRetentionObservationDeletion,
    ) -> Result<(), StoreError> {
        let observation = request.observation;
        crate::retention::observation::validate(&observation)?;
        if request.expected_observation_version == 0 {
            return Err(StoreError::Conflict);
        }
        let mut state = self.lock()?;
        if state.provider_epoch != request.expected_epoch {
            return Err(StoreError::Conflict);
        }
        match request.source {
            ProviderRetentionObservationSource::Present(source) => {
                if state
                    .provider_state
                    .get(&(source.namespace.clone(), source.key.clone()))
                    != Some(&source)
                {
                    return Err(StoreError::Conflict);
                }
            }
            ProviderRetentionObservationSource::Absent(identity) => {
                if state
                    .provider_state
                    .contains_key(&(identity.namespace, identity.key))
                {
                    return Err(StoreError::Conflict);
                }
            }
        }
        let key = (
            observation.target,
            observation.namespace.clone(),
            observation.key.clone(),
        );
        if state.observations.get(&key)
            != Some(&(request.expected_observation_version, observation.clone()))
        {
            return Err(StoreError::Conflict);
        }
        state.observations.remove(&key);
        state.observation_bytes = state
            .observation_bytes
            .saturating_sub(crate::retention::observation::storage_bytes(&observation)?);
        Self::bump_retention_epoch(&mut state)?;
        Ok(())
    }

    fn record_provider_retention_observation(
        &self,
        source: ProviderStateRecord,
        expected_epoch: u64,
        observation: RetentionObservation,
    ) -> Result<RetentionReconciliationReceipt, StoreError> {
        crate::retention::observation::validate(&observation)?;
        let mut state = self.lock()?;
        if state.provider_epoch != expected_epoch
            || state
                .provider_state
                .get(&(source.namespace.clone(), source.key.clone()))
                != Some(&source)
        {
            return Err(StoreError::Conflict);
        }
        let map_key = (
            observation.target,
            observation.namespace.clone(),
            observation.key.clone(),
        );
        let old = state.observations.get(&map_key).cloned();
        if let Some((version, current)) = &old
            && current.source_version == observation.source_version
            && current.source_digest == observation.source_digest
            && current.owner_execution == observation.owner_execution
        {
            return Ok(RetentionReconciliationReceipt {
                target: current.target,
                namespace: current.namespace.clone(),
                key: current.key.clone(),
                source_version: current.source_version,
                observation_version: *version,
                reconciled_tick: current.observed_tick,
            });
        }
        let observation_version = old.as_ref().map_or(Ok(1), |(version, _)| {
            version.checked_add(1).ok_or(StoreError::CapacityExceeded)
        })?;
        if old.is_none() && state.observations.len() >= self.limits.max_retention_archive_rows {
            return Err(StoreError::CapacityExceeded);
        }
        let old_bytes = old
            .as_ref()
            .map(|(_, item)| crate::retention::observation::storage_bytes(item))
            .transpose()?
            .unwrap_or(0);
        let new_bytes = crate::retention::observation::storage_bytes(&observation)?;
        let total = state
            .observation_bytes
            .checked_sub(old_bytes)
            .and_then(|bytes| bytes.checked_add(new_bytes))
            .ok_or(StoreError::CapacityExceeded)?;
        if total > self.limits.max_retention_archive_bytes {
            return Err(StoreError::CapacityExceeded);
        }
        state.observation_bytes = total;
        state
            .observations
            .insert(map_key, (observation_version, observation.clone()));
        Self::bump_retention_epoch(&mut state)?;
        Ok(RetentionReconciliationReceipt {
            target: observation.target,
            namespace: observation.namespace,
            key: observation.key,
            source_version: observation.source_version,
            observation_version,
            reconciled_tick: observation.observed_tick,
        })
    }
}
