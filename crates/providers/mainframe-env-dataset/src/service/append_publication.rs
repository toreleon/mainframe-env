//! Shared atomic publication and mutex-owned sequential append rollback.
use super::*;

struct PreparedEntryPublication {
    updated_indexes: Vec<(String, u64, AlternateIndex)>,
    replay: Replay,
    mutations: Vec<ProviderStateMutation>,
}

struct CommittedEntryPublication {
    updated_indexes: Vec<(String, u64, AlternateIndex)>,
    replay: Replay,
    replay_version: u64,
    replay_payload: Vec<u8>,
}

impl PreparedEntryPublication {
    fn commit(
        self,
        service: &DatasetService,
        mutation: &mainframe_env_host_api::Mutation,
    ) -> Result<CommittedEntryPublication, HostProblem> {
        let (replay_version, replay_payload) =
            service.commit_catalog_mutations(self.mutations, mutation, &self.replay)?;
        Ok(CommittedEntryPublication {
            updated_indexes: self.updated_indexes,
            replay: self.replay,
            replay_version,
            replay_payload,
        })
    }
}

impl CommittedEntryPublication {
    fn install(self, state: &mut State, mutation: &mainframe_env_host_api::Mutation) {
        for (name, _, index) in self.updated_indexes {
            state.alternate_indexes.insert(name, index);
        }
        state.replay.record_mutation(
            mutation,
            self.replay_version,
            &self.replay_payload,
            self.replay,
        );
    }
}

/// The caller holds the service mutex until this guard is dropped. Only the
/// version and appended suffix change; rollback keeps every original record
/// allocation and restores the complete logical entry on errors or unwind.
struct SequentialAppend<'a> {
    state: &'a mut State,
    dataset: &'a str,
    version: u64,
    records: usize,
    committed: bool,
}

impl<'a> SequentialAppend<'a> {
    fn begin(
        state: &'a mut State,
        dataset: &'a str,
        records: &[Vec<u8>],
    ) -> Result<Self, HostProblem> {
        let current = entry_text(state, dataset)?;
        if current.attributes.organization
            != mainframe_env_host_api::DatasetOrganization::Sequential
        {
            return Err(HostProblem::Unsupported);
        }
        let next_version = current
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let guard = Self {
            version: current.version,
            records: current.records.len(),
            state,
            dataset,
            committed: false,
        };
        let next = guard
            .state
            .entries
            .get_mut(dataset)
            .ok_or(HostProblem::NotFound)?;
        next.version = next_version;
        next.records.extend(records.iter().cloned());
        Ok(guard)
    }
}

impl Drop for SequentialAppend<'_> {
    fn drop(&mut self) {
        if !self.committed
            && let Some(current) = self.state.entries.get_mut(self.dataset)
        {
            current.records.truncate(self.records);
            current.version = self.version;
        }
    }
}

impl DatasetService {
    pub(super) fn append_records(
        &self,
        state: &mut State,
        request: &DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        match request {
            DatasetRequest::Append {
                dataset,
                member,
                records,
                expected_version,
                mutation,
            } => {
                validate_records(records, &entry(state, dataset)?.attributes, self.limits)?;
                authorize_data_mutation(
                    state,
                    dataset,
                    &mainframe_env_host_api::DatasetLockTarget::Dataset,
                    mutation,
                    true,
                )?;
                let current = entry(state, dataset)?;
                let current_version = current.version;
                if expected_version.is_some_and(|expected| expected != current_version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if member.is_none()
                    && current.attributes.organization
                        == mainframe_env_host_api::DatasetOrganization::Sequential
                {
                    return self.append_sequential_records(
                        state,
                        dataset.as_str(),
                        records,
                        mutation,
                        request,
                    );
                }
                let mut next = current.clone();
                let mut dependencies = state.dependencies.clone();
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                if let Some(member) = member {
                    if !partitioned(next.attributes.organization) {
                        return Err(HostProblem::Unsupported);
                    }
                    if next.attributes.organization
                        == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                    {
                        let target = resolved_member_name(&next, member).to_string();
                        let mut combined = member_records(&next, member, 0)?.to_vec();
                        combined.extend(records.clone());
                        write_pdse_generation(&mut next, member, combined, false)?;
                        dependencies.add_dependency(
                            &format!("{}({target})", dataset.as_str()),
                            dataset.as_str(),
                            dependency_limits(self.limits),
                        )?;
                    } else {
                        next.members
                            .entry(member.as_str().into())
                            .or_default()
                            .extend(records.clone());
                    }
                } else if matches!(
                    next.attributes.organization,
                    mainframe_env_host_api::DatasetOrganization::Sequential
                        | mainframe_env_host_api::DatasetOrganization::EntrySequenced
                ) {
                    next.records.extend(records.clone());
                } else {
                    return Err(HostProblem::Unsupported);
                }
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_entry_with_indexes_and_lock_deletes(
                    state,
                    dataset.as_str(),
                    current_version,
                    std::borrow::Cow::Owned(next),
                    mutation,
                    request_digest(request)?,
                    &result,
                    &[],
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            _ => Err(HostProblem::Unsupported),
        }
    }

    pub(super) fn append_sequential_records(
        &self,
        state: &mut State,
        dataset: &str,
        records: &[Vec<u8>],
        mutation: &mainframe_env_host_api::Mutation,
        request: &DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        let mut guard = SequentialAppend::begin(state, dataset, records)?;
        let next = entry_text(guard.state, dataset)?;
        let result = DatasetResult::Mutated {
            version: next.version,
        };
        let prepared = self.prepare_entry_publication(
            guard.state,
            dataset,
            guard.version,
            next,
            mutation,
            request_digest(request)?,
            &result,
            &[],
        )?;
        let committed = prepared.commit(self, mutation)?;
        // A successful physical commit or exact replay read-back is required
        // before keeping the suffix. No provider or callback runs after this.
        guard.committed = true;
        committed.install(guard.state, mutation);
        Ok(result)
    }

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
        let prepared = self.prepare_entry_publication(
            state,
            dataset,
            current_version,
            next_ref,
            mutation,
            request_digest,
            result,
            released_locks,
        )?;
        let committed = prepared.commit(self, mutation)?;
        state.entries.insert(dataset.into(), next.into_owned());
        committed.install(state, mutation);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_entry_publication(
        &self,
        state: &State,
        dataset: &str,
        current_version: u64,
        next_ref: &Entry,
        mutation: &mainframe_env_host_api::Mutation,
        request_digest: [u8; 32],
        result: &DatasetResult,
        released_locks: &[mainframe_env_host_api::DatasetLockReceipt],
    ) -> Result<PreparedEntryPublication, HostProblem> {
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
        Ok(PreparedEntryPublication {
            updated_indexes,
            replay,
            mutations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::IdempotencyKey;
    use mainframe_env_host_api::{DatasetAttributes, DatasetOrganization, Mutation, RecordFormat};
    use mainframe_env_store::MemoryStore;

    #[test]
    fn sequential_append_unwind_restores_entry_and_original_allocations() {
        let service = DatasetService::open(
            Arc::new(MemoryStore::new(Default::default())),
            DatasetLimits::default(),
        )
        .unwrap();
        let dataset = DatasetName::new("APPEND.UNWIND", 44).unwrap();
        service
            .invoke(DatasetRequest::Create {
                dataset: dataset.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: 4,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: Mutation {
                    idempotency_key: IdempotencyKey::new(
                        "append-unwind-create",
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                    sequence: 1,
                    transaction: None,
                },
            })
            .unwrap();
        let mut state = service.lock_state().unwrap();
        let current = state.entries.get_mut(dataset.as_str()).unwrap();
        current.records.push(b"AA11".to_vec());
        let original = current.clone();
        let allocation = current.records[0].as_ptr();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let guard = SequentialAppend::begin(
                &mut state,
                dataset.as_str(),
                &[b"BB22".to_vec(), b"CC33".to_vec()],
            )
            .unwrap();
            assert_eq!(guard.state.entries[dataset.as_str()].records.len(), 3);
            panic!("injected before publication");
        }));
        assert!(panicked.is_err());
        assert_eq!(state.entries[dataset.as_str()], original);
        assert_eq!(
            state.entries[dataset.as_str()].records[0].as_ptr(),
            allocation
        );
        // The lock is held across the unwind and inspected before release.
        drop(state);
        assert!(service.lock_state().is_ok());
    }
}
