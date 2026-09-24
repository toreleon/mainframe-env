//! Memory transactions for provider archives, including container replay capacity.

use super::*;

impl MemoryStore {
    pub(super) fn archive_provider_deletion(
        &self,
        request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_deletion(&request, self.limits.max_blob_bytes, false)?;
        let mut state = self.lock()?;
        if state.provider_epoch != request.expected_epoch {
            return Err(StoreError::Conflict);
        }
        for candidate in &request.rows {
            let current = state
                .provider_state
                .get(&(candidate.row.namespace.clone(), candidate.row.key.clone()))
                .ok_or(StoreError::Conflict)?;
            if current != &candidate.row {
                return Err(StoreError::Conflict);
            }
            if let Some(proof) = &candidate.observation
                && state.observations.get(&(
                    request.target,
                    proof.observation.namespace.clone(),
                    proof.observation.key.clone(),
                )) != Some(&(proof.version, proof.observation.clone()))
            {
                return Err(StoreError::Conflict);
            }
            validate_memory_provider_dependency(&state, candidate)?;
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
        for row in &archive.rows {
            remove_memory_row(&mut staged, row)?;
            remove_memory_observation(&mut staged, request.target, row)?;
        }
        staged.provider_epoch = staged
            .provider_epoch
            .checked_add(
                u64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?,
            )
            .ok_or(StoreError::CapacityExceeded)?;
        insert_memory_archive(&mut staged, archive.clone(), self.limits)?;
        *state = staged;
        Ok(archive)
    }

    pub(super) fn archive_container_replay_with_capacity(
        &self,
        request: ProviderStateArchiveDeletionWithCapacity,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_container_replay_archive(&request, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        let deletion = request.deletion;
        if state.provider_epoch != deletion.expected_epoch
            || state.provider_state.get(&(
                request.capacity_source.namespace.clone(),
                request.capacity_source.key.clone(),
            )) != Some(&request.capacity_source)
            || request.outer_receipts.iter().any(|outer| {
                state
                    .provider_state
                    .get(&(outer.namespace.clone(), outer.key.clone()))
                    != Some(outer)
            })
        {
            return Err(StoreError::Conflict);
        }
        for candidate in &deletion.rows {
            if state
                .provider_state
                .get(&(candidate.row.namespace.clone(), candidate.row.key.clone()))
                != Some(&candidate.row)
            {
                return Err(StoreError::Conflict);
            }
            validate_memory_provider_dependency(&state, candidate)?;
        }
        let archive = build_archive(
            deletion.target,
            deletion.archived_tick,
            deletion.watermark_tick,
            deletion
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
        for row in &archive.rows {
            remove_memory_row(&mut staged, row)?;
        }
        Self::put_provider_state_locked(
            &mut staged,
            request.capacity_replacement.record,
            Some(request.capacity_source.version),
            self.limits,
        )?;
        staged.provider_epoch = staged
            .provider_epoch
            .checked_add(
                u64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?,
            )
            .ok_or(StoreError::CapacityExceeded)?;
        insert_memory_archive(&mut staged, archive.clone(), self.limits)?;
        *state = staged;
        Ok(archive)
    }
}
