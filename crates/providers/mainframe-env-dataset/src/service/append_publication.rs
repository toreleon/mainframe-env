//! Shared atomic dataset publication; append moves its prepared entry after commit.
use super::*;

impl DatasetService {
    pub(super) fn persist_with_indexes(
        &self,
        state: &mut State,
        dataset: &str,
        current: &Entry,
        next: &Entry,
        mutation: &mainframe_env_host_api::Mutation,
        request_digest: [u8; 32],
        result: &DatasetResult,
    ) -> Result<(), HostProblem> {
        self.persist_with_indexes_and_lock_deletes(
            state,
            dataset,
            current,
            next,
            mutation,
            request_digest,
            result,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn persist_with_indexes_and_lock_deletes(
        &self,
        state: &mut State,
        dataset: &str,
        current: &Entry,
        next: &Entry,
        mutation: &mainframe_env_host_api::Mutation,
        request_digest: [u8; 32],
        result: &DatasetResult,
        released_locks: &[mainframe_env_host_api::DatasetLockReceipt],
    ) -> Result<(), HostProblem> {
        self.persist_entry_with_indexes_and_lock_deletes(
            state,
            dataset,
            current.version,
            std::borrow::Cow::Borrowed(next),
            mutation,
            request_digest,
            result,
            released_locks,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn persist_entry_with_indexes_and_lock_deletes(
        &self,
        state: &mut State,
        dataset: &str,
        current_version: u64,
        next: std::borrow::Cow<'_, Entry>,
        mutation: &mainframe_env_host_api::Mutation,
        request_digest: [u8; 32],
        result: &DatasetResult,
        released_locks: &[mainframe_env_host_api::DatasetLockReceipt],
    ) -> Result<(), HostProblem> {
        let next_ref = next.as_ref();
        validate_entry_shape(next_ref, self.limits)?;
        validate_guaranteed_volume_capacity(state, dataset, next_ref, self.limits)?;
        if bytes(next_ref) > self.limits.max_total_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in state
            .alternate_indexes
            .iter()
            .filter(|(_, index)| index.base == dataset && index.upgrade)
        {
            validate_alternate_index(next_ref, index)?;
            let mut updated = index.clone();
            updated.identities = build_alternate_identities(next_ref, &updated)?;
            updated.version = updated
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            updated_indexes.push((name.clone(), index.version, updated));
        }
        let replay = resolved_replay(state, mutation, request_digest, result.clone())?;
        let mut writes = Vec::with_capacity(2 + updated_indexes.len());
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset".into(),
                key: dataset.into(),
                version: next_ref.version,
                payload: encode(next_ref).map_err(|_| HostProblem::InfrastructureFailure)?,
            },
            expected_version: Some(current_version),
        });
        for (name, expected, index) in &updated_indexes {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-aix".into(),
                    key: name.clone(),
                    version: index.version,
                    payload: encode_alternate_index(index)?,
                },
                expected_version: Some(*expected),
            });
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset-replay".into(),
                key: mutation.idempotency_key.as_str().into(),
                version: 2,
                payload: encode_replay(&replay)?,
            },
            expected_version: Some(1),
        });
        let mut mutations = writes
            .into_iter()
            .map(ProviderStateMutation::Put)
            .collect::<Vec<_>>();
        mutations.extend(
            released_locks
                .iter()
                .map(|lock| ProviderStateMutation::Delete {
                    namespace: "dataset-lock".into(),
                    key: lock.lock_id.clone(),
                    expected_version: lock.version,
                }),
        );
        let (replay_version, replay_payload) =
            self.commit_catalog_mutations(mutations, mutation, &replay)?;
        state.entries.insert(dataset.into(), next.into_owned());
        for (name, _, index) in updated_indexes {
            state.alternate_indexes.insert(name, index);
        }
        state
            .replay
            .record_mutation(mutation, replay_version, &replay_payload, replay);
        Ok(())
    }
}
