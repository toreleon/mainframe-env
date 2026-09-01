use crate::codec::{Entry, MemberGeneration, decode, encode, encode_definition_digest_v2};
use crate::dependency::{DependencyGraph, DependencyLimits};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, DatasetMemberGenerationSnapshot, DatasetMemberSnapshot, DatasetName,
    DatasetRelativeRecordSnapshot, DatasetRequest, DatasetResult, DatasetSnapshot, EffectRequest,
    EffectResult, HostProblem, HostProvider, HostRequest, HostResult, MemberName,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatasetLimits {
    pub max_datasets: usize,
    pub max_members: usize,
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_total_bytes: usize,
    pub max_cursors: usize,
    pub max_idempotency: usize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetSeedObject {
    pub source_id: String,
    pub dataset: DatasetName,
    pub attributes: mainframe_env_host_api::DatasetAttributes,
    pub record_length: u32,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SeedInstallReceipt {
    pub package: String,
    pub generation: String,
    pub seed_objects: usize,
    pub datasets: usize,
    pub records: usize,
    pub bytes: usize,
    pub identity: String,
    pub replayed: bool,
}
impl Default for DatasetLimits {
    fn default() -> Self {
        Self {
            max_datasets: 4096,
            max_members: 4096,
            max_records: 65536,
            max_record_bytes: 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_cursors: 4096,
            max_idempotency: 65536,
        }
    }
}

impl DatasetService {
    fn apply_concurrency(
        &self,
        state: &mut State,
        request: &DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        match request {
            DatasetRequest::AcquireLock {
                dataset,
                target,
                owner,
                mode,
                now_tick,
                lease_ticks,
                transaction,
                mutation,
            } => {
                let entry = entry(state, dataset)?;
                if entry.vsam.access_mode == mainframe_env_host_api::VsamAccessMode::NonRls {
                    return Err(HostProblem::UnsupportedCapability {
                        capability: "rls".into(),
                        detail: "AcquireLock requires dataset RLS or TVS mode".into(),
                    });
                }
                if entry.vsam.access_mode == mainframe_env_host_api::VsamAccessMode::Tvs {
                    let transaction = transaction.as_ref().ok_or(HostProblem::Malformed)?;
                    require_active_tvs(state, transaction, owner.as_str())?;
                } else if transaction.is_some() {
                    return Err(HostProblem::Malformed);
                }
                validate_lock_target(entry, target)?;
                let expires_at = now_tick
                    .checked_add(*lease_ticks)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let resource = lock_resource(dataset, target);
                if state
                    .locks
                    .values()
                    .filter(|lock| lock.owner == *owner && lock.expires_at > *now_tick)
                    .map(|lock| lock_resource(&lock.dataset, &lock.target))
                    .max()
                    .is_some_and(|held| resource < held)
                {
                    return Err(condition("LOCKORDER", 16));
                }
                for lock in state
                    .locks
                    .values()
                    .filter(|lock| lock.expires_at > *now_tick && lock.owner != *owner)
                {
                    if lock_conflicts(dataset, entry, target, *mode, lock) {
                        return Err(condition("LOCKED", 16));
                    }
                }
                let lock_id = lock_id(mutation.idempotency_key.as_str(), &resource);
                let receipt = mainframe_env_host_api::DatasetLockReceipt {
                    lock_id: lock_id.clone(),
                    dataset: dataset.clone(),
                    target: target.clone(),
                    owner: owner.clone(),
                    mode: *mode,
                    expires_at,
                    transaction: transaction.clone(),
                    version: 1,
                };
                let result = DatasetResult::Locks {
                    locks: vec![receipt.clone()],
                };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let mut mutations = state
                    .locks
                    .values()
                    .filter(|lock| lock.expires_at <= *now_tick)
                    .map(|lock| ProviderStateMutation::Delete {
                        namespace: "dataset-lock".into(),
                        key: lock.lock_id.clone(),
                        expected_version: lock.version,
                    })
                    .collect::<Vec<_>>();
                mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset-lock".into(),
                        key: lock_id.clone(),
                        version: 1,
                        payload: encode_lock(&receipt)?,
                    },
                    expected_version: None,
                }));
                mutations.push(replay_mutation(mutation, &replay)?);
                self.commit_catalog_mutations(mutations, mutation, &replay)?;
                state.locks.retain(|_, lock| lock.expires_at > *now_tick);
                state.locks.insert(lock_id, receipt);
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::ReleaseLock {
                dataset,
                lock_id,
                owner,
                mutation,
            } => {
                let lock = state
                    .locks
                    .get(lock_id)
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                if lock.dataset != *dataset || lock.owner != *owner {
                    return Err(HostProblem::Unauthorized);
                }
                if lock.transaction.is_some() {
                    return Err(condition("INVREQ", 16));
                }
                let result = DatasetResult::Mutated {
                    version: lock.version.saturating_add(1),
                };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_mutations(
                    vec![
                        ProviderStateMutation::Delete {
                            namespace: "dataset-lock".into(),
                            key: lock_id.clone(),
                            expected_version: lock.version,
                        },
                        replay_mutation(mutation, &replay)?,
                    ],
                    mutation,
                    &replay,
                )?;
                state.locks.remove(lock_id);
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::BeginTvs {
                transaction,
                owner,
                mutation,
            } => {
                if state.tvs_units.contains_key(transaction)
                    || state.tvs_units.len() >= self.limits.max_idempotency
                {
                    return Err(condition("DUPREC", 14));
                }
                let unit = TvsUnitOfWork {
                    owner: owner.as_str().into(),
                    state: mainframe_env_host_api::TvsUnitOfWorkState::Active,
                    operations: Vec::new(),
                    version: 1,
                };
                let result = DatasetResult::Tvs(tvs_receipt(transaction, &unit)?);
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_mutations(
                    vec![
                        ProviderStateMutation::Put(ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-tvs".into(),
                                key: transaction.clone(),
                                version: 1,
                                payload: encode_tvs(&unit)?,
                            },
                            expected_version: None,
                        }),
                        replay_mutation(mutation, &replay)?,
                    ],
                    mutation,
                    &replay,
                )?;
                state.tvs_units.insert(transaction.clone(), unit);
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::StageTvs {
                transaction,
                owner,
                operation,
                mutation,
            } => {
                let current = require_active_tvs(state, transaction, owner.as_str())?.clone();
                if current.operations.len() >= self.limits.max_records {
                    return Err(HostProblem::ResourceExhausted);
                }
                let dataset = tvs_operation_dataset(operation);
                let entry = entry(state, dataset)?;
                if entry.vsam.access_mode != mainframe_env_host_api::VsamAccessMode::Tvs {
                    return Err(HostProblem::UnsupportedCapability {
                        capability: "tvs".into(),
                        detail: "StageTvs target is not defined for TVS".into(),
                    });
                }
                let mut candidate_operations = current.operations.clone();
                candidate_operations.push(operation.clone());
                project_tvs_entries(state, &candidate_operations, self.limits)?;
                let identity = tvs_operation_identity(entry, operation)?;
                let target = mainframe_env_host_api::DatasetLockTarget::Record(identity);
                for lock in state.locks.values().filter(|lock| {
                    lock.expires_at > 0 && lock.transaction.as_deref() != Some(transaction.as_str())
                }) {
                    if lock_conflicts(
                        dataset,
                        entry,
                        &target,
                        mainframe_env_host_api::DatasetLockMode::Exclusive,
                        lock,
                    ) {
                        return Err(condition("LOCKED", 16));
                    }
                }
                let resource = lock_resource(dataset, &target);
                if state
                    .locks
                    .values()
                    .filter(|lock| lock.transaction.as_deref() == Some(transaction.as_str()))
                    .map(|lock| lock_resource(&lock.dataset, &lock.target))
                    .max()
                    .is_some_and(|held| resource < held)
                {
                    return Err(condition("LOCKORDER", 16));
                }
                let lock_id = lock_id(transaction, &resource);
                let new_lock = (!state.locks.contains_key(&lock_id)).then(|| {
                    mainframe_env_host_api::DatasetLockReceipt {
                        lock_id: lock_id.clone(),
                        dataset: dataset.clone(),
                        target,
                        owner: owner.clone(),
                        mode: mainframe_env_host_api::DatasetLockMode::Exclusive,
                        expires_at: u64::MAX,
                        transaction: Some(transaction.clone()),
                        version: 1,
                    }
                });
                let mut next = current.clone();
                next.operations.push(operation.clone());
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Tvs(tvs_receipt(transaction, &next)?);
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let mut mutations = vec![ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset-tvs".into(),
                        key: transaction.clone(),
                        version: next.version,
                        payload: encode_tvs(&next)?,
                    },
                    expected_version: Some(current.version),
                })];
                if let Some(lock) = &new_lock {
                    mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-lock".into(),
                            key: lock.lock_id.clone(),
                            version: 1,
                            payload: encode_lock(lock)?,
                        },
                        expected_version: None,
                    }));
                }
                mutations.push(replay_mutation(mutation, &replay)?);
                self.commit_catalog_mutations(mutations, mutation, &replay)?;
                state.tvs_units.insert(transaction.clone(), next);
                if let Some(lock) = new_lock {
                    state.locks.insert(lock_id, lock);
                }
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::CompleteTvs {
                transaction,
                owner,
                commit,
                mutation,
            } => self.complete_tvs(state, request, transaction, owner, *commit, mutation),
            DatasetRequest::ReconcileTvs {
                transaction,
                owner,
                committed,
                mutation,
            } => {
                let current = state
                    .tvs_units
                    .get(transaction)
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                if current.owner != owner.as_str() {
                    return Err(HostProblem::Unauthorized);
                }
                if current.state != mainframe_env_host_api::TvsUnitOfWorkState::Unknown {
                    return Err(condition("INVREQ", 16));
                }
                if *committed {
                    let mut retry = current.clone();
                    retry.state = mainframe_env_host_api::TvsUnitOfWorkState::Active;
                    state.tvs_units.insert(transaction.clone(), retry);
                    return self.complete_tvs(state, request, transaction, owner, true, mutation);
                }
                let mut next = current.clone();
                next.state = mainframe_env_host_api::TvsUnitOfWorkState::RolledBack;
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                self.finish_tvs_state(state, request, transaction, mutation, &current, next)
            }
            _ => Err(HostProblem::Malformed),
        }
    }

    fn complete_tvs(
        &self,
        state: &mut State,
        request: &DatasetRequest,
        transaction: &str,
        owner: &mainframe_env_execution_api::PrincipalId,
        commit: bool,
        mutation: &mainframe_env_host_api::Mutation,
    ) -> Result<DatasetResult, HostProblem> {
        let current = require_active_tvs(state, transaction, owner.as_str())?.clone();
        let mut next = current.clone();
        next.state = if commit {
            mainframe_env_host_api::TvsUnitOfWorkState::Committed
        } else {
            mainframe_env_host_api::TvsUnitOfWorkState::RolledBack
        };
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        if !commit {
            return self.finish_tvs_state(state, request, transaction, mutation, &current, next);
        }
        let projected = project_tvs_entries(state, &current.operations, self.limits)?;
        let result = DatasetResult::Tvs(tvs_receipt(transaction, &next)?);
        let replay = Replay {
            request_digest: request_digest(request)?,
            result: Some(result.clone()),
        };
        let mut mutations = Vec::new();
        let mut updated_entries = Vec::new();
        for (name, mut entry) in projected {
            let current_entry = state
                .entries
                .get(&name)
                .ok_or(HostProblem::InfrastructureFailure)?;
            entry.version = current_entry
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            validate_entry_shape(&entry, self.limits)?;
            mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: name.clone(),
                    version: entry.version,
                    payload: encode(&entry).map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: Some(current_entry.version),
            }));
            updated_entries.push((name, entry));
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in &state.alternate_indexes {
            if let Some((_, entry)) = updated_entries
                .iter()
                .find(|(dataset, _)| dataset == &index.base)
                && index.upgrade
            {
                validate_alternate_index(entry, index)?;
                let mut updated = index.clone();
                updated.identities = build_alternate_identities(entry, &updated)?;
                updated.version = updated
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset-aix".into(),
                        key: name.clone(),
                        version: updated.version,
                        payload: encode_alternate_index(&updated)?,
                    },
                    expected_version: Some(index.version),
                }));
                updated_indexes.push((name.clone(), updated));
            }
        }
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset-tvs".into(),
                key: transaction.into(),
                version: next.version,
                payload: encode_tvs(&next)?,
            },
            expected_version: Some(current.version),
        }));
        for lock in state
            .locks
            .values()
            .filter(|lock| lock.transaction.as_deref() == Some(transaction))
        {
            mutations.push(ProviderStateMutation::Delete {
                namespace: "dataset-lock".into(),
                key: lock.lock_id.clone(),
                expected_version: lock.version,
            });
        }
        mutations.push(replay_mutation(mutation, &replay)?);
        if let Err(problem) = self.commit_catalog_mutations(mutations, mutation, &replay) {
            if problem == HostProblem::UnknownOutcome {
                let mut unknown = current.clone();
                unknown.state = mainframe_env_host_api::TvsUnitOfWorkState::Unknown;
                unknown.version = next.version;
                if self
                    .store
                    .put_provider_state(
                        ProviderStateRecord {
                            namespace: "dataset-tvs".into(),
                            key: transaction.into(),
                            version: unknown.version,
                            payload: encode_tvs(&unknown)?,
                        },
                        Some(current.version),
                    )
                    .is_ok()
                {
                    state.tvs_units.insert(transaction.into(), unknown);
                }
            }
            return Err(problem);
        }
        for (name, entry) in updated_entries {
            state.entries.insert(name, entry);
        }
        for (name, index) in updated_indexes {
            state.alternate_indexes.insert(name, index);
        }
        state.tvs_units.insert(transaction.into(), next);
        state
            .locks
            .retain(|_, lock| lock.transaction.as_deref() != Some(transaction));
        state
            .replay
            .insert(mutation.idempotency_key.as_str().into(), replay);
        Ok(result)
    }

    fn finish_tvs_state(
        &self,
        state: &mut State,
        request: &DatasetRequest,
        transaction: &str,
        mutation: &mainframe_env_host_api::Mutation,
        current: &TvsUnitOfWork,
        next: TvsUnitOfWork,
    ) -> Result<DatasetResult, HostProblem> {
        let result = DatasetResult::Tvs(tvs_receipt(transaction, &next)?);
        let replay = Replay {
            request_digest: request_digest(request)?,
            result: Some(result.clone()),
        };
        let mut mutations = vec![ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset-tvs".into(),
                key: transaction.into(),
                version: next.version,
                payload: encode_tvs(&next)?,
            },
            expected_version: Some(current.version),
        })];
        for lock in state
            .locks
            .values()
            .filter(|lock| lock.transaction.as_deref() == Some(transaction))
        {
            mutations.push(ProviderStateMutation::Delete {
                namespace: "dataset-lock".into(),
                key: lock.lock_id.clone(),
                expected_version: lock.version,
            });
        }
        mutations.push(replay_mutation(mutation, &replay)?);
        self.commit_catalog_mutations(mutations, mutation, &replay)?;
        state.tvs_units.insert(transaction.into(), next);
        state
            .locks
            .retain(|_, lock| lock.transaction.as_deref() != Some(transaction));
        state
            .replay
            .insert(mutation.idempotency_key.as_str().into(), replay);
        Ok(result)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Replay {
    request_digest: [u8; 32],
    result: Option<DatasetResult>,
}
type BrowseIdentity = (Vec<u8>, Vec<u8>);
type SequentialRecord = (Vec<u8>, Vec<u8>);
#[derive(Clone, Debug)]
struct Cursor {
    dataset: String,
    identities: Vec<BrowseIdentity>,
    index: isize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct AlternateIndex {
    base: String,
    parent: String,
    is_path: bool,
    key_offset: u32,
    key_length: u32,
    allow_duplicates: bool,
    upgrade: bool,
    identities: Vec<BrowseIdentity>,
    version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct GenerationGroup {
    limit: u32,
    scratch: bool,
    empty: bool,
    next_generation: u32,
    generations: Vec<String>,
    retired: Vec<String>,
    version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct CatalogRecord {
    kind: mainframe_env_host_api::CatalogKind,
    connected: bool,
    version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct CatalogAlias {
    target: String,
    version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct TvsUnitOfWork {
    owner: String,
    state: mainframe_env_host_api::TvsUnitOfWorkState,
    operations: Vec<mainframe_env_host_api::TvsRecordOperation>,
    version: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct SeedGeneration {
    package: String,
    generation: String,
    objects: Vec<DatasetSeedObject>,
    entries: BTreeMap<String, Entry>,
    identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SeedSelection {
    generation: String,
    version: u64,
}
struct State {
    entries: BTreeMap<String, Entry>,
    alternate_indexes: BTreeMap<String, AlternateIndex>,
    generation_groups: BTreeMap<String, GenerationGroup>,
    catalogs: BTreeMap<String, CatalogRecord>,
    catalog_aliases: BTreeMap<String, CatalogAlias>,
    locks: BTreeMap<String, mainframe_env_host_api::DatasetLockReceipt>,
    tvs_units: BTreeMap<String, TvsUnitOfWork>,
    seed_generations: BTreeMap<(String, String), SeedGeneration>,
    seed_selections: BTreeMap<String, SeedSelection>,
    cursors: BTreeMap<String, Cursor>,
    next_cursor: u64,
    replay: BTreeMap<String, Replay>,
    dependencies: DependencyGraph,
}
pub struct DatasetService {
    store: Arc<dyn ProviderStateStore>,
    limits: DatasetLimits,
    state: Mutex<State>,
}
impl DatasetService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: DatasetLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let mut entries = BTreeMap::new();
        for row in store
            .list_provider_state("dataset", limits.max_datasets)
            .map_err(store_error)?
        {
            let mut entry = decode(
                &row.payload,
                limits.max_records,
                limits.max_record_bytes,
                limits.max_members,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?;
            if entry.version != row.version {
                return Err(HostProblem::InfrastructureFailure);
            }
            let definition = entry.definition();
            definition
                .validate(
                    mainframe_env_host_api::HostLimits {
                        max_record_bytes: limits.max_record_bytes,
                        max_records: limits.max_records,
                        ..Default::default()
                    },
                    dataset_capabilities(),
                )
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            validate_provider_definition(&definition)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            validate_entry_shape(&entry, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
            if entry.attributes.organization
                == mainframe_env_host_api::DatasetOrganization::KeySequenced
            {
                validate_keyed_entry(&mut entry).map_err(|_| HostProblem::InfrastructureFailure)?;
            }
            entries.insert(row.key, entry);
        }
        let mut replay = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-replay", limits.max_idempotency)
            .map_err(store_error)?
        {
            replay.insert(
                row.key,
                decode_replay(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?,
            );
        }
        let mut alternate_indexes = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-aix", limits.max_datasets)
            .map_err(store_error)?
        {
            let mut index = decode_alternate_index(&row.payload, row.version)?;
            let base = entries
                .get(&index.base)
                .ok_or(HostProblem::InfrastructureFailure)?;
            validate_alternate_index(base, &index)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let rebuilt = build_alternate_identities(base, &index)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if index.identities.is_empty() {
                index.identities = rebuilt.clone();
            }
            validate_index_identities(base, &index)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if index.upgrade && index.identities != rebuilt {
                return Err(HostProblem::InfrastructureFailure);
            }
            DatasetName::new(&row.key, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
            alternate_indexes.insert(row.key, index);
        }
        for index in alternate_indexes.values() {
            if index.is_path {
                let parent = alternate_indexes
                    .get(&index.parent)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if parent.is_path || parent.base != index.base {
                    return Err(HostProblem::InfrastructureFailure);
                }
            } else if index.parent != index.base {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        if entries
            .len()
            .checked_add(alternate_indexes.len())
            .is_none_or(|total| total > limits.max_datasets)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut catalogs = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-catalog", limits.max_datasets)
            .map_err(store_error)?
        {
            DatasetName::new(&row.key, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
            let catalog = decode_catalog(&row.payload, row.version)?;
            if catalogs.insert(row.key, catalog).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        if catalogs
            .values()
            .filter(|catalog| catalog.kind == mainframe_env_host_api::CatalogKind::Master)
            .count()
            > 1
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut catalog_aliases = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-catalog-alias", limits.max_datasets)
            .map_err(store_error)?
        {
            DatasetName::new(&row.key, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
            let alias = decode_catalog_alias(&row.payload, row.version)?;
            DatasetName::new(&alias.target, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
            if catalog_aliases.insert(row.key, alias).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut locks = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-lock", limits.max_idempotency)
            .map_err(store_error)?
        {
            let lock = decode_lock(&row.payload, row.version)?;
            if row.key != lock.lock_id || locks.insert(row.key, lock).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut tvs_units = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-tvs", limits.max_idempotency)
            .map_err(store_error)?
        {
            let unit = decode_tvs(&row.payload, row.version, limits)?;
            if tvs_units.insert(row.key, unit).is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut generation_groups = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-gdg", limits.max_datasets)
            .map_err(store_error)?
        {
            let group = decode_generation_group(&row.payload, row.version, limits)?;
            for generation in &group.generations {
                if !entries.contains_key(generation) {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
            for retired in &group.retired {
                entries.remove(retired);
            }
            generation_groups.insert(row.key, group);
        }
        for alias in catalog_aliases.values() {
            if !entries.contains_key(&alias.target)
                && !alternate_indexes.contains_key(&alias.target)
                && !generation_groups.contains_key(&alias.target)
                && !catalogs.contains_key(&alias.target)
                && !catalog_aliases.contains_key(&alias.target)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        for lock in locks.values() {
            let entry = entries
                .get(lock.dataset.as_str())
                .ok_or(HostProblem::InfrastructureFailure)?;
            if entry.vsam.access_mode == mainframe_env_host_api::VsamAccessMode::NonRls {
                return Err(HostProblem::InfrastructureFailure);
            }
            if lock.transaction.is_none() {
                validate_lock_target(entry, &lock.target)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            }
        }
        for (transaction, unit) in &tvs_units {
            mainframe_env_execution_api::PrincipalId::new(&unit.owner, InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if transaction.is_empty()
                || matches!(
                    unit.state,
                    mainframe_env_host_api::TvsUnitOfWorkState::Active
                        | mainframe_env_host_api::TvsUnitOfWorkState::Unknown
                ) && unit.operations.iter().any(|operation| {
                    let dataset = tvs_operation_dataset(operation);
                    !entries.get(dataset.as_str()).is_some_and(|entry| {
                        entry.vsam.access_mode == mainframe_env_host_api::VsamAccessMode::Tvs
                    })
                })
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        for lock in locks.values() {
            if let Some(transaction) = &lock.transaction
                && !tvs_units.get(transaction).is_some_and(|unit| {
                    unit.owner == lock.owner.as_str()
                        && matches!(
                            unit.state,
                            mainframe_env_host_api::TvsUnitOfWorkState::Active
                                | mainframe_env_host_api::TvsUnitOfWorkState::Unknown
                        )
                })
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        if entries
            .len()
            .checked_add(alternate_indexes.len())
            .and_then(|total| total.checked_add(generation_groups.len()))
            .is_none_or(|total| total > limits.max_datasets)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut seed_generations = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-seed-generation", limits.max_idempotency)
            .map_err(store_error)?
        {
            let generation = decode_seed_generation(&row.payload, limits)?;
            if row.key != seed_generation_key(&generation.package, &generation.generation) {
                return Err(HostProblem::InfrastructureFailure);
            }
            if seed_generations
                .insert(
                    (generation.package.clone(), generation.generation.clone()),
                    generation,
                )
                .is_some()
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        let mut seed_selections = BTreeMap::new();
        for row in store
            .list_provider_state("dataset-seed-selection", limits.max_datasets)
            .map_err(store_error)?
        {
            let selection = decode_seed_selection(&row.payload, row.version)?;
            if !seed_generations.contains_key(&(row.key.clone(), selection.generation.clone())) {
                return Err(HostProblem::InfrastructureFailure);
            }
            seed_selections.insert(row.key, selection);
        }
        validate_loaded_guaranteed_volume_capacity(&entries, limits)?;
        let graph_limits = dependency_limits(limits);
        let mut dependencies = DependencyGraph::default();
        for name in entries.keys() {
            dependencies.add_node(name, graph_limits)?;
        }
        for (dataset, entry) in &entries {
            if let Some(catalog) = &entry.catalog.catalog {
                if !catalogs
                    .get(catalog.as_str())
                    .is_some_and(|catalog| catalog.connected)
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
                dependencies.add_dependency(dataset, catalog.as_str(), graph_limits)?;
            }
            if entry.attributes.organization
                == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
            {
                let dataset = DatasetName::new(dataset, 128)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                for member in entry.member_generations.keys() {
                    let member = MemberName::new(member, 8)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    dependencies.add_dependency(
                        &member_node(&dataset, &member),
                        dataset.as_str(),
                        graph_limits,
                    )?;
                }
                for (alias, target) in &entry.member_aliases {
                    let alias = MemberName::new(alias, 8)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    let target = MemberName::new(target, 8)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    dependencies.add_dependency(
                        &member_node(&dataset, &alias),
                        &member_node(&dataset, &target),
                        graph_limits,
                    )?;
                }
            }
        }
        for (name, index) in &alternate_indexes {
            dependencies.add_dependency(name, &index.parent, graph_limits)?;
        }
        for (base, group) in &generation_groups {
            dependencies.add_node(base, graph_limits)?;
            for generation in &group.generations {
                dependencies.add_dependency(generation, base, graph_limits)?;
            }
        }
        for name in catalogs.keys() {
            dependencies.add_node(name, graph_limits)?;
        }
        if let Some((master, _)) = catalogs
            .iter()
            .find(|(_, catalog)| catalog.kind == mainframe_env_host_api::CatalogKind::Master)
        {
            for (user, _) in catalogs
                .iter()
                .filter(|(_, catalog)| catalog.kind == mainframe_env_host_api::CatalogKind::User)
            {
                dependencies.add_dependency(user, master, graph_limits)?;
            }
        }
        for (alias, target) in &catalog_aliases {
            dependencies.add_dependency(alias, &target.target, graph_limits)?;
        }
        Ok(Arc::new(Self {
            store,
            limits,
            state: Mutex::new(State {
                entries,
                alternate_indexes,
                generation_groups,
                catalogs,
                catalog_aliases,
                locks,
                tvs_units,
                seed_generations,
                seed_selections,
                cursors: BTreeMap::new(),
                next_cursor: 1,
                replay,
                dependencies,
            }),
        }))
    }

    pub fn install_seed_generation(
        &self,
        package: &str,
        generation: &str,
        objects: Vec<DatasetSeedObject>,
    ) -> Result<SeedInstallReceipt, HostProblem> {
        let planned = build_seed_generation(package, generation, objects, self.limits)?;
        self.select_seed_generation(planned, true)
    }

    pub fn rollback_seed_generation(
        &self,
        package: &str,
        generation: &str,
    ) -> Result<SeedInstallReceipt, HostProblem> {
        validate_seed_label(package)?;
        validate_seed_label(generation)?;
        let planned = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .seed_generations
            .get(&(package.to_ascii_uppercase(), generation.to_string()))
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        self.select_seed_generation(planned, false)
    }

    pub fn selected_seed_generation(&self, package: &str) -> Result<Option<String>, HostProblem> {
        validate_seed_label(package)?;
        Ok(self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .seed_selections
            .get(&package.to_ascii_uppercase())
            .map(|selection| selection.generation.clone()))
    }

    fn select_seed_generation(
        &self,
        planned: SeedGeneration,
        retain_new: bool,
    ) -> Result<SeedInstallReceipt, HostProblem> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let package = planned.package.clone();
        let generation = planned.generation.clone();
        let key = (package.clone(), generation.clone());
        if let Some(existing) = state.seed_generations.get(&key)
            && existing != &planned
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let current_selection = state.seed_selections.get(&package).cloned();
        if current_selection
            .as_ref()
            .is_some_and(|selection| selection.generation == generation)
        {
            return Ok(seed_receipt(&planned, true));
        }
        let previous_entries = current_selection
            .as_ref()
            .and_then(|selection| {
                state
                    .seed_generations
                    .get(&(package.clone(), selection.generation.clone()))
            })
            .map(|generation| generation.entries.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let next_entries = planned.entries.keys().cloned().collect::<Vec<_>>();
        if !previous_entries.is_empty() && previous_entries != next_entries {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut installed = BTreeMap::new();
        let mut writes = Vec::new();
        for (name, snapshot) in &planned.entries {
            let current = state.entries.get(name);
            if current_selection.is_none()
                && current.is_some_and(|entry| {
                    entry.attributes != snapshot.attributes
                        || !entry.records.is_empty()
                        || !entry.members.is_empty()
                        || !entry.relative_records.is_empty()
                        || !entry.member_generations.is_empty()
                        || !entry.member_aliases.is_empty()
                })
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let mut entry = snapshot.clone();
            entry.version = current.map_or(1, |entry| entry.version.saturating_add(1));
            validate_entry_shape(&entry, self.limits)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: name.clone(),
                    version: entry.version,
                    payload: encode(&entry).map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: current.map(|entry| entry.version),
            });
            installed.insert(name.clone(), entry);
        }
        if state
            .entries
            .len()
            .checked_add(
                installed
                    .keys()
                    .filter(|name| !state.entries.contains_key(*name))
                    .count(),
            )
            .and_then(|total| total.checked_add(state.alternate_indexes.len()))
            .and_then(|total| total.checked_add(state.generation_groups.len()))
            .is_none_or(|total| total > self.limits.max_datasets)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in &state.alternate_indexes {
            let Some(entry) = installed.get(&index.base) else {
                continue;
            };
            validate_alternate_index(entry, index)?;
            let mut updated = index.clone();
            updated.version = updated
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-aix".into(),
                    key: name.clone(),
                    version: updated.version,
                    payload: encode_alternate_index(&updated)?,
                },
                expected_version: Some(index.version),
            });
            updated_indexes.push((name.clone(), updated));
        }
        if retain_new && !state.seed_generations.contains_key(&key) {
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "dataset-seed-generation".into(),
                    key: seed_generation_key(&package, &generation),
                    version: 1,
                    payload: encode_seed_generation(&planned)?,
                },
                expected_version: None,
            });
        }
        let next_selection = SeedSelection {
            generation: generation.clone(),
            version: current_selection
                .as_ref()
                .map_or(1, |selection| selection.version.saturating_add(1)),
        };
        let mut dependencies = state.dependencies.clone();
        for name in &previous_entries {
            dependencies.remove_node(name);
        }
        for name in installed.keys() {
            dependencies.add_node(name, dependency_limits(self.limits))?;
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset-seed-selection".into(),
                key: package.clone(),
                version: next_selection.version,
                payload: encode_seed_selection(&next_selection)?,
            },
            expected_version: current_selection
                .as_ref()
                .map(|selection| selection.version),
        });
        if let Err(error) = self.store.put_provider_states_atomic(writes) {
            if matches!(
                error,
                StoreError::CapacityExceeded | StoreError::PayloadTooLarge
            ) {
                return Err(HostProblem::ResourceExhausted);
            }
            if error == StoreError::Conflict {
                return Err(HostProblem::IdempotencyConflict);
            }
            let selected = self
                .store
                .get_provider_state("dataset-seed-selection", &package)
                .map_err(store_error)?
                .ok_or(HostProblem::UnknownOutcome)?;
            if decode_seed_selection(&selected.payload, selected.version)? != next_selection {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        state.entries.extend(installed);
        state.dependencies = dependencies;
        for (name, index) in updated_indexes {
            state.alternate_indexes.insert(name, index);
        }
        if retain_new {
            state.seed_generations.insert(key, planned.clone());
        }
        state.seed_selections.insert(package, next_selection);
        Ok(seed_receipt(&planned, false))
    }

    pub fn invoke(&self, request: DatasetRequest) -> Result<DatasetResult, HostProblem> {
        HostRequest::Dataset(request.clone()).validate(mainframe_env_host_api::HostLimits {
            max_record_bytes: self.limits.max_record_bytes,
            max_records: self.limits.max_records,
            ..Default::default()
        })?;
        let mutation = mutation(&request);
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let digest = request_digest(&request)?;
        if let Some(key) = mutation.map(|value| value.idempotency_key.as_str())
            && let Some(replay) = state.replay.get(key)
        {
            if replay.request_digest != digest {
                return Err(HostProblem::IdempotencyConflict);
            }
            if let Some(result) = &replay.result {
                return Ok(result.clone());
            }
            if !atomic_dataset_request(&request) {
                return Err(HostProblem::UnknownOutcome);
            }
            self.store
                .delete_provider_state("dataset-replay", key, 1)
                .map_err(store_error)?;
            state.replay.remove(key);
        }
        if let Some(meta) = mutation {
            if state.replay.len() >= self.limits.max_idempotency {
                return Err(HostProblem::ResourceExhausted);
            }
            let replay = Replay {
                request_digest: digest,
                result: None,
            };
            self.store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "dataset-replay".into(),
                        key: meta.idempotency_key.as_str().into(),
                        version: 1,
                        payload: encode_replay(&replay)?,
                    },
                    None,
                )
                .map_err(store_error)?;
            state
                .replay
                .insert(meta.idempotency_key.as_str().into(), replay);
        }
        let result = match self.apply(&mut state, &request) {
            Ok(result) => result,
            Err(problem) => {
                if problem == HostProblem::UnknownOutcome {
                    return Err(problem);
                }
                if let Some(meta) = mutation {
                    self.store
                        .delete_provider_state("dataset-replay", meta.idempotency_key.as_str(), 1)
                        .map_err(store_error)?;
                    state.replay.remove(meta.idempotency_key.as_str());
                }
                return Err(problem);
            }
        };
        if let Some(meta) = mutation {
            if state
                .replay
                .get(meta.idempotency_key.as_str())
                .and_then(|replay| replay.result.as_ref())
                == Some(&result)
            {
                return Ok(result);
            }
            let replay = Replay {
                request_digest: digest,
                result: Some(result.clone()),
            };
            self.store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "dataset-replay".into(),
                        key: meta.idempotency_key.as_str().into(),
                        version: 2,
                        payload: encode_replay(&replay)?,
                    },
                    Some(1),
                )
                .map_err(|_| HostProblem::UnknownOutcome)?;
            state
                .replay
                .insert(meta.idempotency_key.as_str().into(), replay);
        }
        Ok(result)
    }

    fn invoke_for_principal(
        &self,
        principal: &mainframe_env_execution_api::PrincipalId,
        request: DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        if let Some(owner) = request_owner(&request)
            && owner != principal
        {
            return Err(HostProblem::Unauthorized);
        }
        if let DatasetRequest::Define { definition, .. } | DatasetRequest::Alter { definition, .. } =
            &request
            && definition
                .catalog
                .owner
                .as_deref()
                .is_some_and(|owner| owner != principal.as_str())
        {
            return Err(HostProblem::Unauthorized);
        }
        if let DatasetRequest::Restore { snapshot, .. } = &request
            && snapshot
                .definition
                .catalog
                .owner
                .as_deref()
                .is_some_and(|owner| owner != principal.as_str())
        {
            return Err(HostProblem::Unauthorized);
        }
        if let Some(dataset) = owned_mutation_dataset(&request) {
            let state = self
                .state
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if state
                .entries
                .get(dataset.as_str())
                .and_then(|entry| entry.catalog.owner.as_deref())
                .is_some_and(|owner| owner != principal.as_str())
            {
                return Err(HostProblem::Unauthorized);
            }
        }
        if let DatasetRequest::CompleteTvs { transaction, .. }
        | DatasetRequest::ReconcileTvs { transaction, .. } = &request
        {
            let state = self
                .state
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if state.tvs_units.get(transaction).is_some_and(|unit| {
                unit.operations.iter().any(|operation| {
                    state
                        .entries
                        .get(tvs_operation_dataset(operation).as_str())
                        .and_then(|entry| entry.catalog.owner.as_deref())
                        .is_some_and(|owner| owner != principal.as_str())
                })
            }) {
                return Err(HostProblem::Unauthorized);
            }
        }
        if let Some((dataset, mutation)) = rls_request_context(&request) {
            let state = self
                .state
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let current = entry(&state, dataset)?;
            if current.vsam.access_mode == mainframe_env_host_api::VsamAccessMode::Rls {
                let lock_id = mutation
                    .transaction
                    .as_deref()
                    .ok_or_else(|| condition("LOCKED", 16))?;
                if state
                    .locks
                    .get(lock_id)
                    .is_none_or(|lock| lock.owner != *principal)
                {
                    return Err(HostProblem::Unauthorized);
                }
            }
        }
        self.invoke(request)
    }

    fn apply(
        &self,
        state: &mut State,
        request: &DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        match request {
            DatasetRequest::Capabilities => Ok(DatasetResult::Capabilities {
                capabilities: dataset_capabilities(),
            }),
            DatasetRequest::Describe { dataset } => {
                let entry = entry(state, dataset)?;
                let geometry = dataset_geometry(entry)?;
                let mut allocation = allocation_geometry(entry)?;
                let volumes = abstract_volume_descriptions(state)?;
                for extent in &mut allocation.extents {
                    extent.volume_start = volumes
                        .iter()
                        .find(|volume| volume.volume_id == extent.volume_id)
                        .and_then(|volume| {
                            volume.extents.iter().find(|candidate| {
                                candidate.dataset == *dataset
                                    && candidate.dataset_extent_ordinal == extent.ordinal
                            })
                        })
                        .map(|extent| extent.volume_start)
                        .ok_or(HostProblem::InfrastructureFailure)?;
                }
                Ok(DatasetResult::Description(Box::new(
                    mainframe_env_host_api::DatasetDescription {
                        definition: entry.definition(),
                        version: entry.version,
                        allocated_bytes: allocation.allocated_bytes,
                        used_bytes: u64::try_from(bytes(entry))
                            .map_err(|_| HostProblem::ResourceExhausted)?,
                        control_intervals: geometry.control_intervals,
                        control_areas: geometry.control_areas,
                        high_used_rba: geometry.high_used_rba,
                        max_rba: if entry.sms.extended_addressable {
                            u64::MAX
                        } else {
                            u64::from(u32::MAX)
                        },
                        extents: allocation.extents,
                        buffer_bytes: buffer_bytes(entry)?,
                        abstract_placement: abstract_placement(entry),
                    },
                )))
            }
            DatasetRequest::Diagnose { dataset } => {
                let entry = entry(state, dataset)?;
                let mut diagnostics = unavailable_capability_diagnostics();
                if entry.lifecycle.state
                    == mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired
                {
                    diagnostics.push(mainframe_env_host_api::DatasetDiagnostic {
                        code: "RECOVERY_REQUIRED".into(),
                        field: Some("lifecycle.state".into()),
                        detail: "dataset requires VERIFY or RECOVER before update".into(),
                    });
                }
                if u64::try_from(bytes(entry)).map_err(|_| HostProblem::ResourceExhausted)?
                    > allocated_bytes(entry)?
                {
                    diagnostics.push(mainframe_env_host_api::DatasetDiagnostic {
                        code: "ALLOCATION_EXCEEDED".into(),
                        field: Some("allocation".into()),
                        detail: "used bytes exceed the deterministic allocation".into(),
                    });
                }
                Ok(DatasetResult::Diagnostics { diagnostics })
            }
            DatasetRequest::ResolveCatalog { name } => {
                Ok(DatasetResult::Catalog(resolve_catalog(state, name)?))
            }
            DatasetRequest::ListCatalog {
                pattern,
                start,
                max_items,
            } => {
                let mut catalog =
                    BTreeMap::<String, mainframe_env_host_api::CatalogListEntry>::new();
                for (name, entry) in &state.entries {
                    catalog.insert(
                        name.clone(),
                        mainframe_env_host_api::CatalogListEntry {
                            name: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            kind: entry.catalog.entry_kind,
                            related: entry.catalog.catalog.clone(),
                            version: entry.version,
                        },
                    );
                }
                for (name, index) in &state.alternate_indexes {
                    catalog.insert(
                        name.clone(),
                        mainframe_env_host_api::CatalogListEntry {
                            name: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            kind: if index.is_path {
                                mainframe_env_host_api::CatalogEntryKind::Path
                            } else {
                                mainframe_env_host_api::CatalogEntryKind::AlternateIndex
                            },
                            related: Some(
                                DatasetName::new(&index.parent, 128)
                                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                            ),
                            version: index.version,
                        },
                    );
                }
                for (name, group) in &state.generation_groups {
                    catalog.insert(
                        name.clone(),
                        mainframe_env_host_api::CatalogListEntry {
                            name: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            kind: mainframe_env_host_api::CatalogEntryKind::GenerationDataGroup,
                            related: None,
                            version: group.version,
                        },
                    );
                }
                for (name, record) in &state.catalogs {
                    catalog.insert(
                        name.clone(),
                        mainframe_env_host_api::CatalogListEntry {
                            name: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            kind: match record.kind {
                                mainframe_env_host_api::CatalogKind::Master => {
                                    mainframe_env_host_api::CatalogEntryKind::MasterCatalog
                                }
                                mainframe_env_host_api::CatalogKind::User => {
                                    mainframe_env_host_api::CatalogEntryKind::UserCatalog
                                }
                            },
                            related: None,
                            version: record.version,
                        },
                    );
                }
                for (name, alias) in &state.catalog_aliases {
                    catalog.insert(
                        name.clone(),
                        mainframe_env_host_api::CatalogListEntry {
                            name: DatasetName::new(name, 128)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                            kind: mainframe_env_host_api::CatalogEntryKind::Alias,
                            related: Some(
                                DatasetName::new(&alias.target, 128)
                                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                            ),
                            version: alias.version,
                        },
                    );
                }
                let mut entries = Vec::new();
                let mut more = false;
                for (name, entry) in catalog.into_iter().filter(|(name, _)| {
                    wildcard(pattern, name)
                        && start
                            .as_ref()
                            .is_none_or(|start| name.as_str() >= start.as_str())
                }) {
                    let _ = name;
                    if entries.len() >= *max_items as usize {
                        more = true;
                        break;
                    }
                    entries.push(entry);
                }
                Ok(DatasetResult::CatalogEntries { entries, more })
            }
            DatasetRequest::ListVolumes { start, max_items } => {
                let mut selected = Vec::new();
                let mut more = false;
                for volume in abstract_volume_descriptions(state)?
                    .into_iter()
                    .filter(|volume| {
                        start
                            .as_ref()
                            .is_none_or(|start| volume.volume_id.as_str() > start.as_str())
                    })
                {
                    if selected.len() == *max_items as usize {
                        more = true;
                        break;
                    }
                    selected.push(volume);
                }
                Ok(DatasetResult::Volumes {
                    volumes: selected,
                    more,
                })
            }
            DatasetRequest::ListLocks {
                dataset,
                now_tick,
                max_items,
            } => {
                let locks = state
                    .locks
                    .values()
                    .filter(|lock| lock.dataset == *dataset && lock.expires_at > *now_tick)
                    .take(*max_items as usize)
                    .cloned()
                    .collect();
                Ok(DatasetResult::Locks { locks })
            }
            DatasetRequest::TvsStatus { transaction, owner } => {
                let unit = state
                    .tvs_units
                    .get(transaction)
                    .ok_or(HostProblem::NotFound)?;
                if unit.owner != owner.as_str() {
                    return Err(HostProblem::Unauthorized);
                }
                Ok(DatasetResult::Tvs(tvs_receipt(transaction, unit)?))
            }
            DatasetRequest::List {
                pattern,
                start,
                max_items,
            } => {
                let mut names = Vec::new();
                let mut more = false;
                let source_names = state
                    .entries
                    .keys()
                    .chain(state.alternate_indexes.keys())
                    .chain(state.generation_groups.keys())
                    .chain(state.catalogs.keys())
                    .chain(state.catalog_aliases.keys())
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                for name in source_names
                    .iter()
                    .filter(|name| wildcard(pattern, name))
                    .filter(|name| {
                        start
                            .as_ref()
                            .is_none_or(|start| name.as_str() >= start.as_str())
                    })
                {
                    if names.len() >= *max_items as usize {
                        more = true;
                        break;
                    }
                    names.push(
                        DatasetName::new(name, 128)
                            .map_err(|_| HostProblem::InfrastructureFailure)?,
                    );
                }
                Ok(DatasetResult::Listed { names, more })
            }
            DatasetRequest::Attributes { dataset } => {
                if let Some(index) = state.alternate_indexes.get(dataset.as_str()) {
                    let mut attributes = entry_text(state, &index.base)?.attributes.clone();
                    attributes.key_offset = Some(index.key_offset);
                    attributes.key_length = Some(index.key_length);
                    return Ok(DatasetResult::Attributes {
                        attributes,
                        version: index.version,
                    });
                }
                let entry = entry(state, dataset)?;
                Ok(DatasetResult::Attributes {
                    attributes: entry.attributes.clone(),
                    version: entry.version,
                })
            }
            DatasetRequest::ListMembers {
                dataset,
                start,
                max_items,
            } => {
                let entry = entry(state, dataset)?;
                if !partitioned(entry.attributes.organization) {
                    return Err(HostProblem::Unsupported);
                }
                let mut names = Vec::new();
                let mut more = false;
                let directory = if entry.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    entry
                        .member_generations
                        .keys()
                        .chain(entry.member_aliases.keys())
                        .cloned()
                        .collect::<BTreeSet<_>>()
                } else {
                    entry.members.keys().cloned().collect::<BTreeSet<_>>()
                };
                for name in directory.iter().filter(|name| {
                    start
                        .as_ref()
                        .is_none_or(|start| name.as_str() >= start.as_str())
                }) {
                    if names.len() >= *max_items as usize {
                        more = true;
                        break;
                    }
                    names.push(
                        MemberName::new(name, 8).map_err(|_| HostProblem::InfrastructureFailure)?,
                    );
                }
                Ok(DatasetResult::Members { names, more })
            }
            DatasetRequest::ReadMemberGeneration {
                dataset,
                member,
                relative,
                max_records,
            } => {
                let entry = entry(state, dataset)?;
                let generation = pdse_generation(entry, member, *relative)?;
                let records = generation
                    .records
                    .iter()
                    .take(*max_records as usize)
                    .cloned()
                    .collect::<Vec<_>>();
                let identities = (0..records.len())
                    .map(|position| {
                        let mut identity = generation.generation.to_be_bytes().to_vec();
                        identity.extend_from_slice(
                            &u64::try_from(position)
                                .map_err(|_| HostProblem::ResourceExhausted)?
                                .to_be_bytes(),
                        );
                        Ok(identity)
                    })
                    .collect::<Result<Vec<_>, HostProblem>>()?;
                Ok(DatasetResult::MemberGeneration {
                    records,
                    identities,
                    generation: generation.generation,
                    program_object: generation.program_object,
                    version: entry.version,
                })
            }
            DatasetRequest::Read {
                dataset,
                member,
                key,
                max_records,
            } => {
                if member.is_some() && state.alternate_indexes.contains_key(dataset.as_str()) {
                    return Err(HostProblem::Unsupported);
                }
                let (records, identities, version) = if let Some(index) =
                    state.alternate_indexes.get(dataset.as_str())
                {
                    let base = entry_text(state, &index.base)?;
                    let ordered = alternate_identities(base, index)?;
                    let selected = ordered
                        .into_iter()
                        .filter(|(alternate, _)| key.as_ref().is_none_or(|key| alternate == key))
                        .filter(|(_, identity)| keyed_record(base, identity).is_some())
                        .take(*max_records as usize)
                        .collect::<Vec<_>>();
                    if key.is_some() && selected.is_empty() {
                        return Err(condition("NOTFND", 13));
                    }
                    let identities = selected
                        .iter()
                        .map(|(_, identity)| identity.clone())
                        .collect::<Vec<_>>();
                    let records = identities
                        .iter()
                        .map(|identity| keyed_record(base, identity).cloned())
                        .collect::<Option<Vec<_>>>()
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    (records, identities, index.version)
                } else {
                    let entry = entry(state, dataset)?;
                    if relative(entry.attributes.organization) {
                        if member.is_some() || key.is_some() {
                            return Err(HostProblem::Unsupported);
                        }
                        let selected = entry
                            .relative_records
                            .iter()
                            .take(*max_records as usize)
                            .map(|(record_number, record)| {
                                (record.clone(), record_number.to_be_bytes().to_vec())
                            })
                            .collect::<Vec<_>>();
                        return Ok(DatasetResult::Records {
                            records: selected.iter().map(|(record, _)| record.clone()).collect(),
                            identities: selected
                                .into_iter()
                                .map(|(_, identity)| identity)
                                .collect(),
                            version: entry.version,
                        });
                    }
                    let records = if let Some(member) = member {
                        member_records(entry, member, 0)?.to_vec()
                    } else if let Some(key) = key {
                        let record = keyed_record(entry, key)
                            .cloned()
                            .ok_or_else(|| condition("NOTFND", 13))?;
                        vec![record]
                    } else {
                        ordered_records(entry)?
                            .into_iter()
                            .take(*max_records as usize)
                            .cloned()
                            .collect()
                    };
                    let identities = records
                        .iter()
                        .enumerate()
                        .map(|(position, record)| record_identity(entry, record, position))
                        .collect::<Result<Vec<_>, _>>()?;
                    (records, identities, entry.version)
                };
                Ok(DatasetResult::Records {
                    records,
                    identities,
                    version,
                })
            }
            DatasetRequest::ReadGeneric {
                dataset,
                key_prefix,
                max_records,
            } => {
                let (records, identities, version) =
                    if let Some(index) = state.alternate_indexes.get(dataset.as_str()) {
                        let base = entry_text(state, &index.base)?;
                        let selected = alternate_identities(base, index)?
                            .into_iter()
                            .filter(|(alternate, identity)| {
                                alternate.starts_with(key_prefix)
                                    && keyed_record(base, identity).is_some()
                            })
                            .take(*max_records as usize)
                            .collect::<Vec<_>>();
                        let identities = selected
                            .iter()
                            .map(|(_, identity)| identity.clone())
                            .collect::<Vec<_>>();
                        let records = identities
                            .iter()
                            .map(|identity| keyed_record(base, identity).cloned())
                            .collect::<Option<Vec<_>>>()
                            .ok_or(HostProblem::InfrastructureFailure)?;
                        (records, identities, index.version)
                    } else {
                        let base = entry(state, dataset)?;
                        require_keyed(base)?;
                        let selected = ordered_records(base)?
                            .into_iter()
                            .filter_map(|record| {
                                let identity = primary_key(base, record).ok()?;
                                identity
                                    .starts_with(key_prefix)
                                    .then(|| (record.clone(), identity))
                            })
                            .take(*max_records as usize)
                            .collect::<Vec<_>>();
                        (
                            selected.iter().map(|(record, _)| record.clone()).collect(),
                            selected.into_iter().map(|(_, identity)| identity).collect(),
                            base.version,
                        )
                    };
                if records.is_empty() {
                    return Err(condition("NOTFND", 13));
                }
                Ok(DatasetResult::Records {
                    records,
                    identities,
                    version,
                })
            }
            DatasetRequest::ReadConcatenation {
                datasets,
                member,
                max_records,
            } => {
                let mut records = Vec::new();
                let mut identities = Vec::new();
                let mut version = 0u64;
                if let Some(member) = member {
                    for dataset in datasets {
                        let entry = entry(state, dataset)?;
                        if !partitioned(entry.attributes.organization) {
                            return Err(HostProblem::Unsupported);
                        }
                        version = version.max(entry.version);
                        if let Ok(found) = member_records(entry, member, 0) {
                            records.extend(found.iter().take(*max_records as usize).cloned());
                            identities.extend((0..records.len()).map(|position| {
                                u64::try_from(position)
                                    .unwrap_or(u64::MAX)
                                    .to_be_bytes()
                                    .to_vec()
                            }));
                            break;
                        }
                    }
                    if records.is_empty() {
                        return Err(condition("NOTFND", 13));
                    }
                } else {
                    for dataset in datasets {
                        let entry = entry(state, dataset)?;
                        if partitioned(entry.attributes.organization) {
                            return Err(HostProblem::Unsupported);
                        }
                        version = version.max(entry.version);
                        for record in ordered_records(entry)? {
                            if records.len() >= *max_records as usize {
                                break;
                            }
                            identities.push(
                                u64::try_from(identities.len())
                                    .map_err(|_| HostProblem::ResourceExhausted)?
                                    .to_be_bytes()
                                    .to_vec(),
                            );
                            records.push(record.clone());
                        }
                    }
                }
                Ok(DatasetResult::Records {
                    records,
                    identities,
                    version,
                })
            }
            DatasetRequest::ReadRelative {
                dataset,
                record_number,
            } => {
                let entry = entry(state, dataset)?;
                if !relative(entry.attributes.organization) {
                    return Err(HostProblem::Unsupported);
                }
                let record = entry
                    .relative_records
                    .get(record_number)
                    .cloned()
                    .ok_or_else(|| condition("NOTFND", 13))?;
                Ok(DatasetResult::Records {
                    records: vec![record],
                    identities: vec![record_number.to_be_bytes().to_vec()],
                    version: entry.version,
                })
            }
            DatasetRequest::ReadRba {
                dataset,
                rba,
                max_bytes,
            } => {
                let entry = entry(state, dataset)?;
                match entry.attributes.organization {
                    mainframe_env_host_api::DatasetOrganization::Linear => {
                        let content = linear_content(entry)?;
                        let start =
                            usize::try_from(*rba).map_err(|_| HostProblem::ResourceExhausted)?;
                        if start >= content.len() {
                            return Err(condition("NOTFND", 13));
                        }
                        let end = start
                            .checked_add(*max_bytes as usize)
                            .map_or(content.len(), |end| end.min(content.len()));
                        let data = content[start..end].to_vec();
                        Ok(DatasetResult::Rba {
                            data,
                            record: false,
                            rba: *rba,
                            next_rba: u64::try_from(end)
                                .map_err(|_| HostProblem::ResourceExhausted)?,
                            version: entry.version,
                        })
                    }
                    mainframe_env_host_api::DatasetOrganization::EntrySequenced => {
                        let (record, next_rba) = esds_record_at_rba(entry, *rba)?;
                        if record.len() > *max_bytes as usize {
                            return Err(condition("LENGERR", 22));
                        }
                        Ok(DatasetResult::Rba {
                            data: record.clone(),
                            record: true,
                            rba: *rba,
                            next_rba,
                            version: entry.version,
                        })
                    }
                    _ => Err(HostProblem::Unsupported),
                }
            }
            DatasetRequest::ReadSequential {
                dataset,
                member,
                start,
                reverse,
                max_records,
            } => {
                let entry = entry(state, dataset)?;
                let ordered = sequential_records(entry, member.as_ref())?;
                let selected =
                    select_sequential(&ordered, *start, *reverse, *max_records as usize)?;
                Ok(DatasetResult::Records {
                    records: selected.iter().map(|(_, record)| record.clone()).collect(),
                    identities: selected.into_iter().map(|(identity, _)| identity).collect(),
                    version: entry.version,
                })
            }
            DatasetRequest::Snapshot {
                dataset,
                max_records,
                max_members,
            } => {
                let entry = entry(state, dataset)?;
                let mut snapshot = DatasetSnapshot {
                    definition: entry.definition(),
                    records: Vec::new(),
                    relative_records: Vec::new(),
                    members: Vec::new(),
                    linear_data: Vec::new(),
                };
                match entry.attributes.organization {
                    mainframe_env_host_api::DatasetOrganization::Sequential
                    | mainframe_env_host_api::DatasetOrganization::KeySequenced
                    | mainframe_env_host_api::DatasetOrganization::EntrySequenced => {
                        snapshot.records = entry.records.clone();
                    }
                    mainframe_env_host_api::DatasetOrganization::Relative
                    | mainframe_env_host_api::DatasetOrganization::VariableRelative => {
                        snapshot.relative_records = entry
                            .relative_records
                            .iter()
                            .map(|(record_number, record)| DatasetRelativeRecordSnapshot {
                                record_number: *record_number,
                                record: record.clone(),
                            })
                            .collect();
                    }
                    mainframe_env_host_api::DatasetOrganization::Partitioned => {
                        snapshot.members = entry
                            .members
                            .iter()
                            .map(|(name, records)| {
                                Ok(DatasetMemberSnapshot {
                                    name: MemberName::new(name, 8)
                                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                                    records: records.clone(),
                                    generations: Vec::new(),
                                    alias_of: None,
                                })
                            })
                            .collect::<Result<_, HostProblem>>()?;
                    }
                    mainframe_env_host_api::DatasetOrganization::PartitionedExtended => {
                        snapshot.members = entry
                            .member_generations
                            .iter()
                            .map(|(name, generations)| {
                                Ok(DatasetMemberSnapshot {
                                    name: MemberName::new(name, 8)
                                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                                    records: Vec::new(),
                                    generations: generations
                                        .iter()
                                        .map(|generation| DatasetMemberGenerationSnapshot {
                                            generation: generation.generation,
                                            program_object: generation.program_object,
                                            records: generation.records.clone(),
                                        })
                                        .collect(),
                                    alias_of: None,
                                })
                            })
                            .chain(entry.member_aliases.iter().map(|(alias, target)| {
                                Ok(DatasetMemberSnapshot {
                                    name: MemberName::new(alias, 8)
                                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                                    records: Vec::new(),
                                    generations: Vec::new(),
                                    alias_of: Some(
                                        MemberName::new(target, 8)
                                            .map_err(|_| HostProblem::InfrastructureFailure)?,
                                    ),
                                })
                            }))
                            .collect::<Result<_, HostProblem>>()?;
                    }
                    mainframe_env_host_api::DatasetOrganization::Linear => {
                        snapshot.linear_data = linear_content(entry)?;
                    }
                }
                snapshot
                    .members
                    .sort_by(|left, right| left.name.as_str().cmp(right.name.as_str()));
                let record_count = snapshot_record_count(&snapshot)?;
                if record_count > *max_records as usize
                    || snapshot.members.len() > *max_members as usize
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                Ok(DatasetResult::Snapshot {
                    snapshot: Box::new(snapshot),
                    version: entry.version,
                })
            }
            DatasetRequest::Create {
                dataset,
                attributes,
                ..
            } => {
                attributes.validate(mainframe_env_host_api::HostLimits {
                    max_record_bytes: self.limits.max_record_bytes,
                    ..Default::default()
                })?;
                if state
                    .entries
                    .len()
                    .checked_add(state.alternate_indexes.len())
                    .and_then(|total| total.checked_add(state.generation_groups.len()))
                    .is_none_or(|total| total >= self.limits.max_datasets)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if state.entries.contains_key(dataset.as_str())
                    || state.alternate_indexes.contains_key(dataset.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let created = Entry::from_definition(
                    mainframe_env_host_api::DatasetDefinition::compatibility(attributes.clone()),
                    1,
                );
                let mut dependencies = state.dependencies.clone();
                dependencies.add_node(dataset.as_str(), dependency_limits(self.limits))?;
                self.persist(dataset.as_str(), &created, None)?;
                state.entries.insert(dataset.as_str().into(), created);
                state.dependencies = dependencies;
                Ok(DatasetResult::Created { version: 1 })
            }
            DatasetRequest::Define {
                dataset,
                definition,
                mutation,
            } => {
                let limits = mainframe_env_host_api::HostLimits {
                    max_record_bytes: self.limits.max_record_bytes,
                    max_records: self.limits.max_records,
                    ..Default::default()
                };
                definition.validate(limits, dataset_capabilities())?;
                validate_provider_definition(definition)?;
                if definition.lifecycle.migration_level != 0
                    || definition.lifecycle.backup_generation != 0
                    || !matches!(
                        definition.lifecycle.state,
                        mainframe_env_host_api::DatasetLifecycleState::Allocated
                            | mainframe_env_host_api::DatasetLifecycleState::Cataloged
                    )
                {
                    return Err(condition("INVREQ", 16));
                }
                if let Some(catalog) = &definition.catalog.catalog
                    && !state
                        .catalogs
                        .get(catalog.as_str())
                        .is_some_and(|catalog| catalog.connected)
                {
                    return Err(condition("CATLGERR", 16));
                }
                if state
                    .entries
                    .len()
                    .checked_add(state.alternate_indexes.len())
                    .and_then(|total| total.checked_add(state.generation_groups.len()))
                    .is_none_or(|total| total >= self.limits.max_datasets)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if state.entries.contains_key(dataset.as_str())
                    || state.alternate_indexes.contains_key(dataset.as_str())
                    || state.generation_groups.contains_key(dataset.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let created = Entry::from_definition(definition.as_ref().clone(), 1);
                validate_entry_shape(&created, self.limits)?;
                validate_guaranteed_volume_capacity(
                    state,
                    dataset.as_str(),
                    &created,
                    self.limits,
                )?;
                let mut dependencies = state.dependencies.clone();
                if let Some(catalog) = &definition.catalog.catalog {
                    dependencies.add_dependency(
                        dataset.as_str(),
                        catalog.as_str(),
                        dependency_limits(self.limits),
                    )?;
                } else {
                    dependencies.add_node(dataset.as_str(), dependency_limits(self.limits))?;
                }
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_writes(
                    vec![
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset".into(),
                                key: dataset.as_str().into(),
                                version: 1,
                                payload: encode(&created)
                                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                            },
                            expected_version: None,
                        },
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-replay".into(),
                                key: mutation.idempotency_key.as_str().into(),
                                version: 2,
                                payload: encode_replay(&replay)?,
                            },
                            expected_version: Some(1),
                        },
                    ],
                    mutation,
                    &replay,
                )?;
                state.entries.insert(dataset.as_str().into(), created);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::Alter {
                dataset,
                definition,
                expected_version,
                mutation,
            } => {
                definition.validate(
                    mainframe_env_host_api::HostLimits {
                        max_record_bytes: self.limits.max_record_bytes,
                        max_records: self.limits.max_records,
                        ..Default::default()
                    },
                    dataset_capabilities(),
                )?;
                validate_provider_definition(definition)?;
                if let Some(catalog) = &definition.catalog.catalog
                    && !state
                        .catalogs
                        .get(catalog.as_str())
                        .is_some_and(|catalog| catalog.connected)
                {
                    return Err(condition("CATLGERR", 16));
                }
                let current = entry(state, dataset)?.clone();
                ensure_update_allowed(&current)?;
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if definition.lifecycle != current.lifecycle {
                    return Err(condition("INVREQ", 16));
                }
                let mut dependencies = state.dependencies.clone();
                dependencies.set_direct_dependencies(
                    dataset.as_str(),
                    definition
                        .catalog
                        .catalog
                        .iter()
                        .map(|catalog| catalog.as_str().to_string()),
                    dependency_limits(self.limits),
                )?;
                let mut next = current.clone();
                next.replace_definition(definition.as_ref().clone());
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                validate_guaranteed_volume_capacity(state, dataset.as_str(), &next, self.limits)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::SetLifecycle {
                dataset,
                state: next_state,
                expected_version,
                mutation,
            } => {
                if matches!(
                    next_state,
                    mainframe_env_host_api::DatasetLifecycleState::Migrated
                        | mainframe_env_host_api::DatasetLifecycleState::RecallPending
                ) {
                    return Err(HostProblem::UnsupportedCapability {
                        capability: "migration-recall".into(),
                        detail:
                            "lifecycle transition requires provider capability migration-recall"
                                .into(),
                    });
                }
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if !lifecycle_transition_allowed(current.lifecycle.state, *next_state) {
                    return Err(condition("INVREQ", 16));
                }
                let released_locks =
                    if *next_state == mainframe_env_host_api::DatasetLifecycleState::Closed {
                        let locks = state
                            .locks
                            .values()
                            .filter(|lock| lock.dataset == *dataset)
                            .cloned()
                            .collect::<Vec<_>>();
                        if locks.iter().any(|lock| lock.transaction.is_some()) {
                            return Err(condition("LOCKED", 16));
                        }
                        locks
                    } else {
                        Vec::new()
                    };
                let mut next = current.clone();
                next.lifecycle.state = *next_state;
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                validate_guaranteed_volume_capacity(state, dataset.as_str(), &next, self.limits)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes_and_lock_deletes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                    &released_locks,
                )?;
                for lock in released_locks {
                    state.locks.remove(&lock.lock_id);
                }
                Ok(result)
            }
            DatasetRequest::RecordBackup {
                dataset,
                expected_version,
                mutation,
            } => {
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                next.lifecycle.backup_generation = next
                    .lifecycle
                    .backup_generation
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_writes(
                    vec![
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset".into(),
                                key: dataset.as_str().into(),
                                version: next.version,
                                payload: encode(&next)
                                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                            },
                            expected_version: Some(current.version),
                        },
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-replay".into(),
                                key: mutation.idempotency_key.as_str().into(),
                                version: 2,
                                payload: encode_replay(&replay)?,
                            },
                            expected_version: Some(1),
                        },
                    ],
                    mutation,
                    &replay,
                )?;
                state.entries.insert(dataset.as_str().into(), next);
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::Restore {
                dataset,
                snapshot,
                expected_version,
                mutation,
            } => {
                let definition = &snapshot.definition;
                definition.validate(
                    mainframe_env_host_api::HostLimits {
                        max_record_bytes: self.limits.max_record_bytes,
                        max_records: self.limits.max_records,
                        ..Default::default()
                    },
                    dataset_capabilities(),
                )?;
                validate_provider_definition(definition)?;
                if matches!(
                    definition.lifecycle.state,
                    mainframe_env_host_api::DatasetLifecycleState::Migrated
                        | mainframe_env_host_api::DatasetLifecycleState::RecallPending
                ) || definition.lifecycle.migration_level != 0
                {
                    return Err(HostProblem::UnsupportedCapability {
                        capability: "migration-recall".into(),
                        detail: "snapshot requires provider migration-recall capability".into(),
                    });
                }
                if let Some(catalog) = &definition.catalog.catalog
                    && !state
                        .catalogs
                        .get(catalog.as_str())
                        .is_some_and(|catalog| catalog.connected)
                {
                    return Err(condition("CATLGERR", 16));
                }
                let current = state.entries.get(dataset.as_str()).cloned();
                if expected_version.is_some_and(|expected| {
                    current
                        .as_ref()
                        .is_none_or(|entry| entry.version != expected)
                }) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if current.as_ref().is_some_and(|current| {
                    current.attributes.organization != definition.attributes.organization
                }) {
                    return Err(HostProblem::Unsupported);
                }
                if current.is_none()
                    && (state_name_in_use(state, dataset.as_str())
                        || state_object_count(state) >= self.limits.max_datasets)
                {
                    return Err(condition("DUPREC", 14));
                }
                let version = current.as_ref().map_or(Ok(1), |entry| {
                    entry
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)
                })?;
                let mut next = Entry::from_definition(definition.clone(), version);
                restore_snapshot_content(&mut next, snapshot, self.limits)?;
                if next.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::KeySequenced
                {
                    validate_keyed_entry(&mut next)?;
                }
                validate_entry_shape(&next, self.limits)?;
                validate_guaranteed_volume_capacity(state, dataset.as_str(), &next, self.limits)?;
                let mut dependencies = state.dependencies.clone();
                if let Some(current) = &current
                    && current.attributes.organization
                        == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    for member in current
                        .member_generations
                        .keys()
                        .chain(current.member_aliases.keys())
                    {
                        let member = MemberName::new(member, 8)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        dependencies.remove_node(&member_node(dataset, &member));
                    }
                }
                dependencies.set_direct_dependencies(
                    dataset.as_str(),
                    definition
                        .catalog
                        .catalog
                        .iter()
                        .map(|catalog| catalog.as_str().to_string()),
                    dependency_limits(self.limits),
                )?;
                if next.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    for member in next.member_generations.keys() {
                        let member = MemberName::new(member, 8)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        dependencies.add_dependency(
                            &member_node(dataset, &member),
                            dataset.as_str(),
                            dependency_limits(self.limits),
                        )?;
                    }
                    for (alias, target) in &next.member_aliases {
                        let alias = MemberName::new(alias, 8)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        let target = MemberName::new(target, 8)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        dependencies.add_dependency(
                            &member_node(dataset, &alias),
                            &member_node(dataset, &target),
                            dependency_limits(self.limits),
                        )?;
                    }
                }
                let result = DatasetResult::Mutated { version };
                if let Some(current) = current {
                    self.persist_with_indexes(
                        state,
                        dataset.as_str(),
                        &current,
                        &next,
                        mutation,
                        request_digest(request)?,
                        &result,
                    )?;
                } else {
                    let replay = Replay {
                        request_digest: request_digest(request)?,
                        result: Some(result.clone()),
                    };
                    self.commit_catalog_writes(
                        vec![
                            ProviderStateWrite {
                                record: ProviderStateRecord {
                                    namespace: "dataset".into(),
                                    key: dataset.as_str().into(),
                                    version,
                                    payload: encode(&next)
                                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                                },
                                expected_version: None,
                            },
                            ProviderStateWrite {
                                record: ProviderStateRecord {
                                    namespace: "dataset-replay".into(),
                                    key: mutation.idempotency_key.as_str().into(),
                                    version: 2,
                                    payload: encode_replay(&replay)?,
                                },
                                expected_version: Some(1),
                            },
                        ],
                        mutation,
                        &replay,
                    )?;
                    state.entries.insert(dataset.as_str().into(), next);
                    state
                        .replay
                        .insert(mutation.idempotency_key.as_str().into(), replay);
                }
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::DefineCatalog {
                catalog,
                kind,
                mutation,
            } => {
                if state_name_in_use(state, catalog.as_str())
                    || state_object_count(state) >= self.limits.max_datasets
                {
                    return Err(condition("DUPREC", 14));
                }
                if *kind == mainframe_env_host_api::CatalogKind::Master
                    && state
                        .catalogs
                        .values()
                        .any(|catalog| catalog.kind == mainframe_env_host_api::CatalogKind::Master)
                {
                    return Err(condition("DUPREC", 14));
                }
                let record = CatalogRecord {
                    kind: *kind,
                    connected: true,
                    version: 1,
                };
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let mut dependencies = state.dependencies.clone();
                if *kind == mainframe_env_host_api::CatalogKind::User {
                    if let Some((master, _)) = state.catalogs.iter().find(|(_, catalog)| {
                        catalog.kind == mainframe_env_host_api::CatalogKind::Master
                    }) {
                        dependencies.add_dependency(
                            catalog.as_str(),
                            master,
                            dependency_limits(self.limits),
                        )?;
                    } else {
                        dependencies.add_node(catalog.as_str(), dependency_limits(self.limits))?;
                    }
                } else {
                    dependencies.add_node(catalog.as_str(), dependency_limits(self.limits))?;
                    for (user, _) in state.catalogs.iter().filter(|(_, catalog)| {
                        catalog.kind == mainframe_env_host_api::CatalogKind::User
                    }) {
                        dependencies.add_dependency(
                            user,
                            catalog.as_str(),
                            dependency_limits(self.limits),
                        )?;
                    }
                }
                self.commit_catalog_writes(
                    vec![
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-catalog".into(),
                                key: catalog.as_str().into(),
                                version: 1,
                                payload: encode_catalog(&record),
                            },
                            expected_version: None,
                        },
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-replay".into(),
                                key: mutation.idempotency_key.as_str().into(),
                                version: 2,
                                payload: encode_replay(&replay)?,
                            },
                            expected_version: Some(1),
                        },
                    ],
                    mutation,
                    &replay,
                )?;
                state.catalogs.insert(catalog.as_str().into(), record);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::SetCatalogConnection {
                catalog,
                connected,
                expected_version,
                mutation,
            } => {
                let current = state
                    .catalogs
                    .get(catalog.as_str())
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if current.kind == mainframe_env_host_api::CatalogKind::Master && !connected {
                    return Err(condition("INVREQ", 16));
                }
                let mut next = current.clone();
                next.connected = *connected;
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_writes(
                    vec![
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-catalog".into(),
                                key: catalog.as_str().into(),
                                version: next.version,
                                payload: encode_catalog(&next),
                            },
                            expected_version: Some(current.version),
                        },
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-replay".into(),
                                key: mutation.idempotency_key.as_str().into(),
                                version: 2,
                                payload: encode_replay(&replay)?,
                            },
                            expected_version: Some(1),
                        },
                    ],
                    mutation,
                    &replay,
                )?;
                state.catalogs.insert(catalog.as_str().into(), next);
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::DefineAlias {
                alias,
                target,
                mutation,
            } => {
                if state_name_in_use(state, alias.as_str())
                    || state_object_count(state) >= self.limits.max_datasets
                {
                    return Err(condition("DUPREC", 14));
                }
                if !state_name_in_use(state, target.as_str()) {
                    return Err(HostProblem::NotFound);
                }
                let record = CatalogAlias {
                    target: target.as_str().into(),
                    version: 1,
                };
                let mut dependencies = state.dependencies.clone();
                dependencies.add_dependency(
                    alias.as_str(),
                    target.as_str(),
                    dependency_limits(self.limits),
                )?;
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_writes(
                    vec![
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-catalog-alias".into(),
                                key: alias.as_str().into(),
                                version: 1,
                                payload: encode_catalog_alias(&record)?,
                            },
                            expected_version: None,
                        },
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-replay".into(),
                                key: mutation.idempotency_key.as_str().into(),
                                version: 2,
                                payload: encode_replay(&replay)?,
                            },
                            expected_version: Some(1),
                        },
                    ],
                    mutation,
                    &replay,
                )?;
                state.catalog_aliases.insert(alias.as_str().into(), record);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::DefineMemberAlias {
                dataset,
                alias,
                target,
                expected_version,
                mutation,
            } => {
                let current = entry(state, dataset)?.clone();
                ensure_update_allowed(&current)?;
                if current.attributes.organization
                    != mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    return Err(HostProblem::Unsupported);
                }
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if current.member_generations.contains_key(alias.as_str())
                    || current.member_aliases.contains_key(alias.as_str())
                    || !current.member_generations.contains_key(target.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let mut next = current.clone();
                next.member_aliases
                    .insert(alias.as_str().into(), target.as_str().into());
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let mut dependencies = state.dependencies.clone();
                dependencies.add_dependency(
                    &member_node(dataset, alias),
                    &member_node(dataset, target),
                    dependency_limits(self.limits),
                )?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::WriteMemberGeneration {
                dataset,
                member,
                records,
                program_object,
                expected_version,
                mutation,
            } => {
                let current = entry(state, dataset)?.clone();
                ensure_update_allowed(&current)?;
                if current.attributes.organization
                    != mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    return Err(HostProblem::Unsupported);
                }
                validate_records(records, &current.attributes, self.limits)?;
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                let target = resolved_member_name(&next, member).to_string();
                write_pdse_generation(&mut next, member, records.clone(), *program_object)?;
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let mut dependencies = state.dependencies.clone();
                dependencies.add_dependency(
                    &format!("{}({target})", dataset.as_str()),
                    dataset.as_str(),
                    dependency_limits(self.limits),
                )?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::DeleteMemberGeneration {
                dataset,
                member,
                generation,
                expected_version,
                mutation,
            } => {
                let current = entry(state, dataset)?.clone();
                ensure_update_allowed(&current)?;
                if current.attributes.organization
                    != mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    return Err(HostProblem::Unsupported);
                }
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let member_name = resolved_member_name(&current, member).to_string();
                let mut next = current.clone();
                let generations = next
                    .member_generations
                    .get_mut(&member_name)
                    .ok_or(HostProblem::NotFound)?;
                let position = generations
                    .iter()
                    .position(|candidate| candidate.generation == *generation)
                    .ok_or(HostProblem::NotFound)?;
                generations.remove(position);
                let mut dependencies = state.dependencies.clone();
                if generations.is_empty() {
                    next.member_generations.remove(&member_name);
                    let removed_aliases = next
                        .member_aliases
                        .iter()
                        .filter(|(_, target)| target.as_str() == member_name)
                        .map(|(alias, _)| alias.clone())
                        .collect::<Vec<_>>();
                    next.member_aliases
                        .retain(|_, target| target != &member_name);
                    let target = MemberName::new(&member_name, 8)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    dependencies.remove_node(&member_node(dataset, &target));
                    for alias in removed_aliases {
                        let alias = MemberName::new(alias, 8)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        dependencies.remove_node(&member_node(dataset, &alias));
                    }
                }
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::AcquireLock { .. }
            | DatasetRequest::ReleaseLock { .. }
            | DatasetRequest::BeginTvs { .. }
            | DatasetRequest::StageTvs { .. }
            | DatasetRequest::CompleteTvs { .. }
            | DatasetRequest::ReconcileTvs { .. } => self.apply_concurrency(state, request),
            DatasetRequest::Write {
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
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                let mut dependencies = state.dependencies.clone();
                next.version += 1;
                if let Some(member) = member {
                    if !partitioned(next.attributes.organization) {
                        return Err(HostProblem::Unsupported);
                    }
                    if next.attributes.organization
                        == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                    {
                        let target = resolved_member_name(&next, member).to_string();
                        write_pdse_generation(&mut next, member, records.clone(), false)?;
                        dependencies.add_dependency(
                            &format!("{}({target})", dataset.as_str()),
                            dataset.as_str(),
                            dependency_limits(self.limits),
                        )?;
                    } else {
                        next.members.insert(member.as_str().into(), records.clone());
                    }
                } else if next.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::KeySequenced
                {
                    for record in records {
                        let key = primary_key(&next, record)?;
                        if keyed_record(&next, &key).is_some() {
                            return Err(condition("DUPREC", 14));
                        }
                        next.records.push(record.clone());
                    }
                    sort_keyed_records(&mut next)?;
                } else if next.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::EntrySequenced
                {
                    next.records.extend(records.clone());
                } else if relative(next.attributes.organization) {
                    return Err(HostProblem::Unsupported);
                } else {
                    next.records = records.clone();
                }
                if bytes(&next) > self.limits.max_total_bytes {
                    return Err(HostProblem::ResourceExhausted);
                }
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
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
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
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
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::Truncate {
                dataset,
                expected_version,
                mutation,
            } => {
                authorize_data_mutation(
                    state,
                    dataset,
                    &mainframe_env_host_api::DatasetLockTarget::Dataset,
                    mutation,
                    true,
                )?;
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                next.records.clear();
                next.members.clear();
                next.relative_records.clear();
                next.member_generations.clear();
                next.member_aliases.clear();
                let mut dependencies = state.dependencies.clone();
                for member in current
                    .member_generations
                    .keys()
                    .chain(current.member_aliases.keys())
                {
                    let member = MemberName::new(member, 8)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    dependencies.remove_node(&member_node(dataset, &member));
                }
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                state.dependencies = dependencies;
                Ok(result)
            }
            DatasetRequest::RewriteRecord {
                dataset,
                key,
                record,
                expected_version,
                mutation,
            } => {
                authorize_data_mutation(
                    state,
                    dataset,
                    &mainframe_env_host_api::DatasetLockTarget::Record(key.clone()),
                    mutation,
                    false,
                )?;
                let current = entry(state, dataset)?.clone();
                require_keyed(&current)?;
                validate_records(
                    std::slice::from_ref(record),
                    &current.attributes,
                    self.limits,
                )?;
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if primary_key(&current, record)? != *key {
                    return Err(condition("INVREQ", 16));
                }
                let mut next = current.clone();
                let position = next
                    .records
                    .iter()
                    .position(|candidate| {
                        primary_key(&current, candidate).ok().as_ref() == Some(key)
                    })
                    .ok_or_else(|| condition("NOTFND", 13))?;
                next.records[position] = record.clone();
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                sort_keyed_records(&mut next)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                Ok(result)
            }
            DatasetRequest::DeleteRecord {
                dataset,
                key,
                expected_version,
                mutation,
            } => {
                authorize_data_mutation(
                    state,
                    dataset,
                    &mainframe_env_host_api::DatasetLockTarget::Record(key.clone()),
                    mutation,
                    false,
                )?;
                let current = entry(state, dataset)?.clone();
                require_keyed(&current)?;
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                let position = next
                    .records
                    .iter()
                    .position(|candidate| {
                        primary_key(&current, candidate).ok().as_ref() == Some(key)
                    })
                    .ok_or_else(|| condition("NOTFND", 13))?;
                next.records.remove(position);
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                Ok(result)
            }
            DatasetRequest::WriteRelative {
                dataset,
                record_number,
                record,
                expected_version,
                mutation,
            } => {
                authorize_data_mutation(
                    state,
                    dataset,
                    &mainframe_env_host_api::DatasetLockTarget::Record(
                        record_number.to_be_bytes().to_vec(),
                    ),
                    mutation,
                    false,
                )?;
                let current = entry(state, dataset)?.clone();
                if !relative(current.attributes.organization) {
                    return Err(HostProblem::Unsupported);
                }
                validate_records(
                    std::slice::from_ref(record),
                    &current.attributes,
                    self.limits,
                )?;
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                next.relative_records.insert(*record_number, record.clone());
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                Ok(result)
            }
            DatasetRequest::DeleteRelative {
                dataset,
                record_number,
                expected_version,
                mutation,
            } => {
                authorize_data_mutation(
                    state,
                    dataset,
                    &mainframe_env_host_api::DatasetLockTarget::Record(
                        record_number.to_be_bytes().to_vec(),
                    ),
                    mutation,
                    false,
                )?;
                let current = entry(state, dataset)?.clone();
                if !relative(current.attributes.organization) {
                    return Err(HostProblem::Unsupported);
                }
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                if next.relative_records.remove(record_number).is_none() {
                    return Err(condition("NOTFND", 13));
                }
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                Ok(result)
            }
            DatasetRequest::WriteRba {
                dataset,
                rba,
                data,
                expected_version,
                mutation,
            } => {
                if data.is_empty() {
                    return Err(HostProblem::Malformed);
                }
                if data.len() > self.limits.max_record_bytes {
                    return Err(HostProblem::ResourceExhausted);
                }
                let lock_target = if entry(state, dataset)?.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::EntrySequenced
                {
                    mainframe_env_host_api::DatasetLockTarget::Record(rba.to_be_bytes().to_vec())
                } else {
                    mainframe_env_host_api::DatasetLockTarget::Dataset
                };
                authorize_data_mutation(state, dataset, &lock_target, mutation, false)?;
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                match current.attributes.organization {
                    mainframe_env_host_api::DatasetOrganization::Linear => {
                        let mut content = linear_content(&current)?;
                        let start =
                            usize::try_from(*rba).map_err(|_| HostProblem::ResourceExhausted)?;
                        if start > content.len() {
                            return Err(condition("NOTFND", 13));
                        }
                        let end = start
                            .checked_add(data.len())
                            .ok_or(HostProblem::ResourceExhausted)?;
                        if end > self.limits.max_total_bytes {
                            return Err(HostProblem::ResourceExhausted);
                        }
                        content.resize(end.max(content.len()), 0);
                        content[start..end].copy_from_slice(data);
                        next.records = content
                            .chunks(self.limits.max_record_bytes)
                            .map(<[u8]>::to_vec)
                            .collect();
                    }
                    mainframe_env_host_api::DatasetOrganization::EntrySequenced => {
                        let position = esds_position_at_rba(&current, *rba)?;
                        if current.records[position].len() != data.len() {
                            return Err(condition("LENGERR", 22));
                        }
                        next.records[position] = data.clone();
                    }
                    _ => return Err(HostProblem::Unsupported),
                }
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let result = DatasetResult::Mutated {
                    version: next.version,
                };
                self.persist_with_indexes(
                    state,
                    dataset.as_str(),
                    &current,
                    &next,
                    mutation,
                    request_digest(request)?,
                    &result,
                )?;
                Ok(result)
            }
            DatasetRequest::DefineAlternateIndex {
                base,
                index,
                key_offset,
                key_length,
                allow_duplicates,
                upgrade,
                mutation,
            } => {
                if state
                    .entries
                    .len()
                    .checked_add(state.alternate_indexes.len())
                    .and_then(|total| total.checked_add(state.generation_groups.len()))
                    .is_none_or(|total| total >= self.limits.max_datasets)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if state.entries.contains_key(index.as_str())
                    || state.alternate_indexes.contains_key(index.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let base_entry = entry(state, base)?;
                ensure_update_allowed(base_entry)?;
                require_keyed(base_entry)?;
                validate_key_range(base_entry, *key_offset, *key_length)?;
                let definition = AlternateIndex {
                    base: base.as_str().into(),
                    parent: base.as_str().into(),
                    is_path: false,
                    key_offset: *key_offset,
                    key_length: *key_length,
                    allow_duplicates: *allow_duplicates,
                    upgrade: *upgrade,
                    identities: Vec::new(),
                    version: 1,
                };
                validate_alternate_index(base_entry, &definition)?;
                let mut definition = definition;
                definition.identities = build_alternate_identities(base_entry, &definition)?;
                let mut dependencies = state.dependencies.clone();
                dependencies.add_dependency(
                    index.as_str(),
                    base.as_str(),
                    dependency_limits(self.limits),
                )?;
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let writes = vec![
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-aix".into(),
                            key: index.as_str().into(),
                            version: 1,
                            payload: encode_alternate_index(&definition)?,
                        },
                        expected_version: None,
                    },
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-replay".into(),
                            key: mutation.idempotency_key.as_str().into(),
                            version: 2,
                            payload: encode_replay(&replay)?,
                        },
                        expected_version: Some(1),
                    },
                ];
                if self.store.put_provider_states_atomic(writes).is_err() {
                    let persisted = self
                        .store
                        .get_provider_state("dataset-replay", mutation.idempotency_key.as_str())
                        .map_err(store_error)?
                        .ok_or(HostProblem::UnknownOutcome)?;
                    if decode_replay(&persisted.payload)
                        .map_err(|_| HostProblem::InfrastructureFailure)?
                        != replay
                    {
                        return Err(HostProblem::UnknownOutcome);
                    }
                }
                state
                    .alternate_indexes
                    .insert(index.as_str().into(), definition);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::BuildAlternateIndex {
                base,
                index,
                mutation,
            } => {
                let current = state
                    .alternate_indexes
                    .get(index.as_str())
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                if current.is_path {
                    return Err(HostProblem::Unsupported);
                }
                if current.base != base.as_str() {
                    return Err(condition("INVREQ", 16));
                }
                let base_entry = entry_text(state, &current.base)?;
                ensure_update_allowed(base_entry)?;
                validate_alternate_index(base_entry, &current)?;
                let mut updates = state
                    .alternate_indexes
                    .iter()
                    .filter(|(name, candidate)| {
                        name.as_str() == index.as_str() || candidate.parent == index.as_str()
                    })
                    .map(|(name, candidate)| {
                        let mut next = candidate.clone();
                        next.identities = build_alternate_identities(base_entry, &next)?;
                        next.version = next
                            .version
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?;
                        Ok((name.clone(), candidate.version, next))
                    })
                    .collect::<Result<Vec<_>, HostProblem>>()?;
                updates.sort_by(|left, right| left.0.cmp(&right.0));
                let version = updates
                    .iter()
                    .find(|(name, _, _)| name == index.as_str())
                    .map(|(_, _, next)| next.version)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                let result = DatasetResult::Mutated { version };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let mut mutations = updates
                    .iter()
                    .map(|(name, expected, next)| {
                        Ok(ProviderStateMutation::Put(ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-aix".into(),
                                key: name.clone(),
                                version: next.version,
                                payload: encode_alternate_index(next)?,
                            },
                            expected_version: Some(*expected),
                        }))
                    })
                    .collect::<Result<Vec<_>, HostProblem>>()?;
                mutations.push(replay_mutation(mutation, &replay)?);
                self.commit_catalog_mutations(mutations, mutation, &replay)?;
                for (name, _, next) in updates {
                    state.alternate_indexes.insert(name, next);
                }
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::DefinePath {
                path,
                index,
                mutation,
            } => {
                if state.entries.contains_key(path.as_str())
                    || state.alternate_indexes.contains_key(path.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let mut definition = state
                    .alternate_indexes
                    .get(index.as_str())
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                if definition.is_path {
                    return Err(HostProblem::Unsupported);
                }
                definition.version = 1;
                definition.parent = index.as_str().into();
                definition.is_path = true;
                let mut dependencies = state.dependencies.clone();
                dependencies.add_dependency(
                    path.as_str(),
                    index.as_str(),
                    dependency_limits(self.limits),
                )?;
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                self.commit_catalog_writes(
                    vec![
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-aix".into(),
                                key: path.as_str().into(),
                                version: 1,
                                payload: encode_alternate_index(&definition)?,
                            },
                            expected_version: None,
                        },
                        ProviderStateWrite {
                            record: ProviderStateRecord {
                                namespace: "dataset-replay".into(),
                                key: mutation.idempotency_key.as_str().into(),
                                version: 2,
                                payload: encode_replay(&replay)?,
                            },
                            expected_version: Some(1),
                        },
                    ],
                    mutation,
                    &replay,
                )?;
                state
                    .alternate_indexes
                    .insert(path.as_str().into(), definition);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::DefineGenerationGroup {
                base,
                limit,
                scratch,
                empty,
                mutation,
            } => {
                if state
                    .entries
                    .len()
                    .checked_add(state.alternate_indexes.len())
                    .and_then(|total| total.checked_add(state.generation_groups.len()))
                    .is_none_or(|total| total >= self.limits.max_datasets)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if state.entries.contains_key(base.as_str())
                    || state.alternate_indexes.contains_key(base.as_str())
                    || state.generation_groups.contains_key(base.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let group = GenerationGroup {
                    limit: *limit,
                    scratch: *scratch,
                    empty: *empty,
                    next_generation: 1,
                    generations: Vec::new(),
                    retired: Vec::new(),
                    version: 1,
                };
                let mut dependencies = state.dependencies.clone();
                dependencies.add_node(base.as_str(), dependency_limits(self.limits))?;
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let writes = vec![
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-gdg".into(),
                            key: base.as_str().into(),
                            version: 1,
                            payload: encode_generation_group(&group)?,
                        },
                        expected_version: None,
                    },
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-replay".into(),
                            key: mutation.idempotency_key.as_str().into(),
                            version: 2,
                            payload: encode_replay(&replay)?,
                        },
                        expected_version: Some(1),
                    },
                ];
                self.commit_catalog_writes(writes, mutation, &replay)?;
                state.generation_groups.insert(base.as_str().into(), group);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::CreateGeneration {
                base,
                attributes,
                records,
                mutation,
            } => {
                attributes.validate(mainframe_env_host_api::HostLimits {
                    max_record_bytes: self.limits.max_record_bytes,
                    ..Default::default()
                })?;
                validate_records(records, attributes, self.limits)?;
                let current = state
                    .generation_groups
                    .get(base.as_str())
                    .cloned()
                    .ok_or(HostProblem::NotFound)?;
                if current.next_generation > 9_999 {
                    return Err(HostProblem::ResourceExhausted);
                }
                let absolute = current.next_generation;
                let name = DatasetName::new(format!("{}.G{absolute:04}V00", base.as_str()), 128)
                    .map_err(|_| HostProblem::ResourceExhausted)?;
                if state.entries.contains_key(name.as_str()) {
                    return Err(condition("DUPREC", 14));
                }
                let mut entry = Entry::from_definition(
                    mainframe_env_host_api::DatasetDefinition::compatibility(attributes.clone()),
                    1,
                );
                entry.records = records.clone();
                validate_entry_shape(&entry, self.limits)?;
                let mut next = current.clone();
                next.version = next
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                next.next_generation = next
                    .next_generation
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                next.generations.push(name.as_str().into());
                let mut rolled = Vec::new();
                if next.generations.len() > next.limit as usize {
                    let remove = if next.empty {
                        next.generations.len().saturating_sub(1)
                    } else {
                        next.generations.len() - next.limit as usize
                    };
                    rolled.extend(next.generations.drain(..remove));
                    if next.scratch {
                        next.retired.extend(rolled.iter().cloned());
                    }
                }
                if next.scratch
                    && rolled.iter().any(|retired| {
                        state
                            .catalog_aliases
                            .values()
                            .any(|alias| alias.target == *retired)
                    })
                {
                    return Err(condition("INUSE", 16));
                }
                if state
                    .entries
                    .len()
                    .checked_add(1)
                    .and_then(|total| {
                        total.checked_sub(if next.scratch { rolled.len() } else { 0 })
                    })
                    .and_then(|total| total.checked_add(state.alternate_indexes.len()))
                    .and_then(|total| total.checked_add(state.generation_groups.len()))
                    .is_none_or(|total| total > self.limits.max_datasets)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut dependencies = state.dependencies.clone();
                dependencies.add_dependency(
                    name.as_str(),
                    base.as_str(),
                    dependency_limits(self.limits),
                )?;
                if next.scratch {
                    for retired in &rolled {
                        dependencies.remove_node(retired);
                    }
                }
                let result = DatasetResult::Generation {
                    dataset: name.clone(),
                    absolute_generation: absolute,
                    version: next.version,
                };
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let writes = vec![
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset".into(),
                            key: name.as_str().into(),
                            version: 1,
                            payload: encode(&entry)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                        },
                        expected_version: None,
                    },
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-gdg".into(),
                            key: base.as_str().into(),
                            version: next.version,
                            payload: encode_generation_group(&next)?,
                        },
                        expected_version: Some(current.version),
                    },
                    ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: "dataset-replay".into(),
                            key: mutation.idempotency_key.as_str().into(),
                            version: 2,
                            payload: encode_replay(&replay)?,
                        },
                        expected_version: Some(1),
                    },
                ];
                let mut mutations = writes
                    .into_iter()
                    .map(ProviderStateMutation::Put)
                    .collect::<Vec<_>>();
                if next.scratch {
                    for retired in &rolled {
                        let version = state
                            .entries
                            .get(retired)
                            .ok_or(HostProblem::InfrastructureFailure)?
                            .version;
                        mutations.push(ProviderStateMutation::Delete {
                            namespace: "dataset".into(),
                            key: retired.clone(),
                            expected_version: version,
                        });
                    }
                }
                self.commit_catalog_mutations(mutations, mutation, &replay)?;
                state.entries.insert(name.as_str().into(), entry);
                if next.scratch {
                    for retired in rolled {
                        state.entries.remove(&retired);
                    }
                }
                state.generation_groups.insert(base.as_str().into(), next);
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::ResolveGeneration { base, relative } => {
                let group = state
                    .generation_groups
                    .get(base.as_str())
                    .ok_or(HostProblem::NotFound)?;
                let position = isize::try_from(group.generations.len())
                    .map_err(|_| HostProblem::ResourceExhausted)?
                    .checked_sub(1)
                    .and_then(|position| position.checked_add(*relative as isize))
                    .ok_or(HostProblem::NotFound)?;
                let name = group
                    .generations
                    .get(usize::try_from(position).map_err(|_| HostProblem::NotFound)?)
                    .ok_or(HostProblem::NotFound)?;
                let absolute = generation_number(name).ok_or(HostProblem::InfrastructureFailure)?;
                Ok(DatasetResult::Generation {
                    dataset: DatasetName::new(name, 128)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                    absolute_generation: absolute,
                    version: group.version,
                })
            }
            DatasetRequest::Rename { from, to, .. } => {
                if state_name_in_use(state, to.as_str()) {
                    return Err(condition("DUPREC", 14));
                }
                if state.locks.values().any(|lock| lock.dataset == *from)
                    || active_tvs_references(state, from)
                    || state
                        .cursors
                        .values()
                        .any(|cursor| cursor.dataset == from.as_str())
                {
                    return Err(condition("LOCKED", 16));
                }
                if state.generation_groups.values().any(|group| {
                    group
                        .generations
                        .iter()
                        .chain(&group.retired)
                        .any(|name| name == from.as_str())
                }) {
                    return Err(condition("INUSE", 16));
                }
                let mut moved = entry(state, from)?.clone();
                let member_renames = if moved.attributes.organization
                    == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                {
                    moved
                        .member_generations
                        .keys()
                        .chain(moved.member_aliases.keys())
                        .map(|member| {
                            (
                                format!("{}({member})", from.as_str()),
                                format!("{}({member})", to.as_str()),
                            )
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let invalidation = state
                    .dependencies
                    .invalidation_order(from.as_str(), dependency_limits(self.limits))?;
                let mut updated_indexes = state
                    .alternate_indexes
                    .iter()
                    .filter(|(_, index)| index.base == from.as_str())
                    .map(|(name, index)| {
                        let mut next = index.clone();
                        next.base = to.as_str().into();
                        if next.parent == from.as_str() {
                            next.parent = to.as_str().into();
                        }
                        next.version = next
                            .version
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?;
                        Ok((name.clone(), index.version, next))
                    })
                    .collect::<Result<Vec<_>, HostProblem>>()?;
                let mut updated_aliases = state
                    .catalog_aliases
                    .iter()
                    .filter(|(_, alias)| alias.target == from.as_str())
                    .map(|(name, alias)| {
                        let mut next = alias.clone();
                        next.target = to.as_str().into();
                        next.version = next
                            .version
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?;
                        Ok((name.clone(), alias.version, next))
                    })
                    .collect::<Result<Vec<_>, HostProblem>>()?;
                updated_indexes.sort_by(|left, right| left.0.cmp(&right.0));
                updated_aliases.sort_by(|left, right| left.0.cmp(&right.0));
                let supported_dependents = updated_indexes
                    .iter()
                    .map(|(name, _, _)| name.as_str())
                    .chain(updated_aliases.iter().map(|(name, _, _)| name.as_str()))
                    .chain(member_renames.iter().map(|(old, _)| old.as_str()))
                    .collect::<BTreeSet<_>>();
                if invalidation.iter().any(|name| {
                    name != from.as_str() && !supported_dependents.contains(name.as_str())
                }) {
                    return Err(condition("INUSE", 16));
                }
                let mut dependencies = state.dependencies.clone();
                dependencies.rename_node(
                    from.as_str(),
                    to.as_str(),
                    dependency_limits(self.limits),
                )?;
                for (old_member, new_member) in &member_renames {
                    dependencies.rename_node(
                        old_member,
                        new_member,
                        dependency_limits(self.limits),
                    )?;
                }
                let old = moved.version;
                moved.version += 1;
                let result = DatasetResult::Mutated {
                    version: moved.version,
                };
                let mutation = mutation(request).ok_or(HostProblem::MissingIdempotency)?;
                let replay = Replay {
                    request_digest: request_digest(request)?,
                    result: Some(result.clone()),
                };
                let mut mutations = vec![ProviderStateMutation::Move {
                    record: ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: to.as_str().into(),
                        version: moved.version,
                        payload: encode(&moved).map_err(|_| HostProblem::InfrastructureFailure)?,
                    },
                    old_key: from.as_str().into(),
                    expected_version: old,
                }];
                mutations.extend(
                    updated_indexes
                        .iter()
                        .map(|(name, expected, index)| {
                            Ok(ProviderStateMutation::Put(ProviderStateWrite {
                                record: ProviderStateRecord {
                                    namespace: "dataset-aix".into(),
                                    key: name.clone(),
                                    version: index.version,
                                    payload: encode_alternate_index(index)?,
                                },
                                expected_version: Some(*expected),
                            }))
                        })
                        .collect::<Result<Vec<_>, HostProblem>>()?,
                );
                mutations.extend(
                    updated_aliases
                        .iter()
                        .map(|(name, expected, alias)| {
                            Ok(ProviderStateMutation::Put(ProviderStateWrite {
                                record: ProviderStateRecord {
                                    namespace: "dataset-catalog-alias".into(),
                                    key: name.clone(),
                                    version: alias.version,
                                    payload: encode_catalog_alias(alias)?,
                                },
                                expected_version: Some(*expected),
                            }))
                        })
                        .collect::<Result<Vec<_>, HostProblem>>()?,
                );
                mutations.push(replay_mutation(mutation, &replay)?);
                self.commit_catalog_mutations(mutations, mutation, &replay)?;
                state.entries.remove(from.as_str());
                state.entries.insert(to.as_str().into(), moved.clone());
                for (name, _, index) in updated_indexes {
                    state.alternate_indexes.insert(name, index);
                }
                for (name, _, alias) in updated_aliases {
                    state.catalog_aliases.insert(name, alias);
                }
                state.dependencies = dependencies;
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::Delete {
                dataset,
                member,
                expected_version,
                purge,
                current_date,
                ..
            } => {
                if member.is_none()
                    && let Some(alias) = state.catalog_aliases.get(dataset.as_str()).cloned()
                {
                    if expected_version.is_some_and(|expected| expected != alias.version) {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    if state
                        .dependencies
                        .invalidation_order(dataset.as_str(), dependency_limits(self.limits))?
                        .len()
                        != 1
                    {
                        return Err(condition("INUSE", 16));
                    }
                    let result = DatasetResult::Mutated {
                        version: alias.version.saturating_add(1),
                    };
                    let mutation = mutation(request).ok_or(HostProblem::MissingIdempotency)?;
                    let replay = Replay {
                        request_digest: request_digest(request)?,
                        result: Some(result.clone()),
                    };
                    self.commit_catalog_mutations(
                        vec![
                            ProviderStateMutation::Delete {
                                namespace: "dataset-catalog-alias".into(),
                                key: dataset.as_str().into(),
                                expected_version: alias.version,
                            },
                            replay_mutation(mutation, &replay)?,
                        ],
                        mutation,
                        &replay,
                    )?;
                    state.catalog_aliases.remove(dataset.as_str());
                    state.dependencies.remove_node(dataset.as_str());
                    state
                        .replay
                        .insert(mutation.idempotency_key.as_str().into(), replay);
                    return Ok(result);
                }
                if member.is_none()
                    && let Some(catalog) = state.catalogs.get(dataset.as_str()).cloned()
                {
                    if expected_version.is_some_and(|expected| expected != catalog.version) {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    if state
                        .dependencies
                        .invalidation_order(dataset.as_str(), dependency_limits(self.limits))?
                        .len()
                        != 1
                    {
                        return Err(condition("INUSE", 16));
                    }
                    let result = DatasetResult::Mutated {
                        version: catalog.version.saturating_add(1),
                    };
                    let mutation = mutation(request).ok_or(HostProblem::MissingIdempotency)?;
                    let replay = Replay {
                        request_digest: request_digest(request)?,
                        result: Some(result.clone()),
                    };
                    self.commit_catalog_mutations(
                        vec![
                            ProviderStateMutation::Delete {
                                namespace: "dataset-catalog".into(),
                                key: dataset.as_str().into(),
                                expected_version: catalog.version,
                            },
                            replay_mutation(mutation, &replay)?,
                        ],
                        mutation,
                        &replay,
                    )?;
                    state.catalogs.remove(dataset.as_str());
                    state.dependencies.remove_node(dataset.as_str());
                    state
                        .replay
                        .insert(mutation.idempotency_key.as_str().into(), replay);
                    return Ok(result);
                }
                if member.is_none()
                    && let Some(group) = state.generation_groups.get(dataset.as_str()).cloned()
                {
                    if expected_version.is_some_and(|expected| expected != group.version) {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    if !group.generations.is_empty() {
                        return Err(condition("INUSE", 16));
                    }
                    let result = DatasetResult::Mutated {
                        version: group.version.saturating_add(1),
                    };
                    let mutation = mutation(request).ok_or(HostProblem::MissingIdempotency)?;
                    let replay = Replay {
                        request_digest: request_digest(request)?,
                        result: Some(result.clone()),
                    };
                    self.commit_catalog_mutations(
                        vec![
                            ProviderStateMutation::Delete {
                                namespace: "dataset-gdg".into(),
                                key: dataset.as_str().into(),
                                expected_version: group.version,
                            },
                            replay_mutation(mutation, &replay)?,
                        ],
                        mutation,
                        &replay,
                    )?;
                    state.generation_groups.remove(dataset.as_str());
                    state.dependencies.remove_node(dataset.as_str());
                    state
                        .replay
                        .insert(mutation.idempotency_key.as_str().into(), replay);
                    return Ok(result);
                }
                if member.is_none()
                    && let Some(index) = state.alternate_indexes.get(dataset.as_str()).cloned()
                {
                    if expected_version.is_some_and(|expected| expected != index.version) {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    if state
                        .dependencies
                        .invalidation_order(dataset.as_str(), dependency_limits(self.limits))?
                        .len()
                        != 1
                    {
                        return Err(condition("INUSE", 16));
                    }
                    let result = DatasetResult::Mutated {
                        version: index.version.saturating_add(1),
                    };
                    let mutation = mutation(request).ok_or(HostProblem::MissingIdempotency)?;
                    let replay = Replay {
                        request_digest: request_digest(request)?,
                        result: Some(result.clone()),
                    };
                    self.commit_catalog_mutations(
                        vec![
                            ProviderStateMutation::Delete {
                                namespace: "dataset-aix".into(),
                                key: dataset.as_str().into(),
                                expected_version: index.version,
                            },
                            replay_mutation(mutation, &replay)?,
                        ],
                        mutation,
                        &replay,
                    )?;
                    state.alternate_indexes.remove(dataset.as_str());
                    state.dependencies.remove_node(dataset.as_str());
                    state
                        .replay
                        .insert(mutation.idempotency_key.as_str().into(), replay);
                    return Ok(result);
                }
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if member.is_none() {
                    enforce_delete_policy(&current.catalog, *purge, *current_date)?;
                } else {
                    ensure_update_allowed(&current)?;
                }
                if let Some(member) = member {
                    let mut next = current.clone();
                    if next.attributes.organization
                        == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
                    {
                        if next.member_aliases.remove(member.as_str()).is_none()
                            && next.member_generations.remove(member.as_str()).is_none()
                        {
                            return Err(HostProblem::NotFound);
                        }
                        next.member_aliases
                            .retain(|_, target| target != member.as_str());
                    } else if next.members.remove(member.as_str()).is_none() {
                        return Err(HostProblem::NotFound);
                    }
                    next.version += 1;
                    let mut dependencies = state.dependencies.clone();
                    for name in current
                        .member_generations
                        .keys()
                        .chain(current.member_aliases.keys())
                        .filter(|name| {
                            !next.member_generations.contains_key(*name)
                                && !next.member_aliases.contains_key(*name)
                        })
                    {
                        let member = MemberName::new(name, 8)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        dependencies.remove_node(&member_node(dataset, &member));
                    }
                    let result = DatasetResult::Mutated {
                        version: next.version,
                    };
                    self.persist_with_indexes(
                        state,
                        dataset.as_str(),
                        &current,
                        &next,
                        mutation(request).ok_or(HostProblem::MissingIdempotency)?,
                        request_digest(request)?,
                        &result,
                    )?;
                    state.dependencies = dependencies;
                    Ok(result)
                } else {
                    if active_tvs_references(state, dataset) {
                        return Err(condition("LOCKED", 16));
                    }
                    let locks = state
                        .locks
                        .values()
                        .filter(|lock| lock.dataset == *dataset)
                        .cloned()
                        .collect::<Vec<_>>();
                    if locks.iter().any(|lock| lock.transaction.is_some()) {
                        return Err(condition("LOCKED", 16));
                    }
                    let invalidation = state
                        .dependencies
                        .invalidation_order(dataset.as_str(), dependency_limits(self.limits))?;
                    if invalidation
                        .iter()
                        .filter(|name| name.as_str() != dataset.as_str())
                        .any(|name| {
                            !state.alternate_indexes.contains_key(name)
                                && !(name.starts_with(&format!("{}(", dataset.as_str()))
                                    && name.ends_with(')'))
                        })
                    {
                        return Err(condition("INUSE", 16));
                    }
                    let indexes = state
                        .alternate_indexes
                        .iter()
                        .filter(|(_, index)| index.base == dataset.as_str())
                        .map(|(name, index)| (name.clone(), index.version))
                        .collect::<Vec<_>>();
                    let result = DatasetResult::Mutated {
                        version: current.version.saturating_add(1),
                    };
                    let mutation = mutation(request).ok_or(HostProblem::MissingIdempotency)?;
                    let replay = Replay {
                        request_digest: request_digest(request)?,
                        result: Some(result.clone()),
                    };
                    let mut mutations = indexes
                        .iter()
                        .map(|(name, version)| ProviderStateMutation::Delete {
                            namespace: "dataset-aix".into(),
                            key: name.clone(),
                            expected_version: *version,
                        })
                        .collect::<Vec<_>>();
                    mutations.push(ProviderStateMutation::Delete {
                        namespace: "dataset".into(),
                        key: dataset.as_str().into(),
                        expected_version: current.version,
                    });
                    mutations.extend(locks.iter().map(|lock| ProviderStateMutation::Delete {
                        namespace: "dataset-lock".into(),
                        key: lock.lock_id.clone(),
                        expected_version: lock.version,
                    }));
                    mutations.push(replay_mutation(mutation, &replay)?);
                    self.commit_catalog_mutations(mutations, mutation, &replay)?;
                    for (name, _) in indexes {
                        state.alternate_indexes.remove(&name);
                        state.dependencies.remove_node(&name);
                    }
                    state.entries.remove(dataset.as_str());
                    for lock in locks {
                        state.locks.remove(&lock.lock_id);
                    }
                    state
                        .cursors
                        .retain(|_, cursor| cursor.dataset != dataset.as_str());
                    for node in invalidation {
                        state.dependencies.remove_node(&node);
                    }
                    state
                        .replay
                        .insert(mutation.idempotency_key.as_str().into(), replay);
                    Ok(result)
                }
            }
            DatasetRequest::StartBrowse { dataset, key } => {
                let identities = browse_identities(state, dataset)?;
                let index =
                    identities.partition_point(|(logical, _)| logical.as_slice() < key.as_slice());
                let active_identities = state
                    .cursors
                    .values()
                    .map(|cursor| cursor.identities.len())
                    .sum::<usize>();
                let active_bytes = state
                    .cursors
                    .values()
                    .flat_map(|cursor| &cursor.identities)
                    .map(|(logical, identity)| logical.len() + identity.len())
                    .sum::<usize>();
                let added_bytes = identities
                    .iter()
                    .map(|(logical, identity)| logical.len() + identity.len())
                    .sum::<usize>();
                if state.cursors.len() >= self.limits.max_cursors
                    || active_identities
                        .checked_add(identities.len())
                        .is_none_or(|total| total > self.limits.max_records)
                    || active_bytes
                        .checked_add(added_bytes)
                        .is_none_or(|total| total > self.limits.max_total_bytes)
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
            DatasetRequest::ReadNext {
                dataset,
                cursor,
                reverse,
            } => {
                let identity = {
                    let state_cursor = state
                        .cursors
                        .get_mut(cursor)
                        .ok_or_else(|| condition("INVREQ", 16))?;
                    if state_cursor.dataset != dataset.as_str() {
                        return Err(condition("INVREQ", 16));
                    }
                    if *reverse {
                        state_cursor.index -= 1;
                    }
                    let current = state_cursor.index;
                    if !*reverse {
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
                    cursor: cursor.clone(),
                    record,
                    identity: base_identity,
                    key: logical_key,
                })
            }
            DatasetRequest::EndBrowse { dataset, cursor } => {
                let removed = state
                    .cursors
                    .remove(cursor)
                    .ok_or_else(|| condition("INVREQ", 16))?;
                if removed.dataset != dataset.as_str() {
                    return Err(condition("INVREQ", 16));
                }
                Ok(DatasetResult::Browse {
                    cursor: cursor.clone(),
                    record: None,
                    identity: None,
                    key: None,
                })
            }
        }
    }
    fn persist_with_indexes(
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
    fn persist_with_indexes_and_lock_deletes(
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
        validate_entry_shape(next, self.limits)?;
        validate_guaranteed_volume_capacity(state, dataset, next, self.limits)?;
        if bytes(next) > self.limits.max_total_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in state
            .alternate_indexes
            .iter()
            .filter(|(_, index)| index.base == dataset && index.upgrade)
        {
            validate_alternate_index(next, index)?;
            let mut updated = index.clone();
            updated.identities = build_alternate_identities(next, &updated)?;
            updated.version = updated
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            updated_indexes.push((name.clone(), index.version, updated));
        }
        let replay = Replay {
            request_digest,
            result: Some(result.clone()),
        };
        let mut writes = Vec::with_capacity(2 + updated_indexes.len());
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "dataset".into(),
                key: dataset.into(),
                version: next.version,
                payload: encode(next).map_err(|_| HostProblem::InfrastructureFailure)?,
            },
            expected_version: Some(current.version),
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
        if self.store.mutate_provider_states_atomic(mutations).is_err() {
            let persisted = self
                .store
                .get_provider_state("dataset-replay", mutation.idempotency_key.as_str())
                .map_err(store_error)?
                .ok_or(HostProblem::UnknownOutcome)?;
            if decode_replay(&persisted.payload).map_err(|_| HostProblem::InfrastructureFailure)?
                != replay
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        state.entries.insert(dataset.into(), next.clone());
        for (name, _, index) in updated_indexes {
            state.alternate_indexes.insert(name, index);
        }
        state
            .replay
            .insert(mutation.idempotency_key.as_str().into(), replay);
        Ok(())
    }

    fn commit_catalog_writes(
        &self,
        writes: Vec<ProviderStateWrite>,
        mutation: &mainframe_env_host_api::Mutation,
        replay: &Replay,
    ) -> Result<(), HostProblem> {
        self.commit_catalog_mutations(
            writes.into_iter().map(ProviderStateMutation::Put).collect(),
            mutation,
            replay,
        )
    }

    fn commit_catalog_mutations(
        &self,
        mutations: Vec<ProviderStateMutation>,
        mutation: &mainframe_env_host_api::Mutation,
        replay: &Replay,
    ) -> Result<(), HostProblem> {
        if self.store.mutate_provider_states_atomic(mutations).is_err() {
            let persisted = self
                .store
                .get_provider_state("dataset-replay", mutation.idempotency_key.as_str())
                .map_err(store_error)?
                .ok_or(HostProblem::UnknownOutcome)?;
            if decode_replay(&persisted.payload).map_err(|_| HostProblem::InfrastructureFailure)?
                != *replay
            {
                return Err(HostProblem::UnknownOutcome);
            }
        }
        Ok(())
    }

    fn persist(&self, key: &str, entry: &Entry, expected: Option<u64>) -> Result<(), HostProblem> {
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: key.into(),
                    version: entry.version,
                    payload: encode(entry).map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected,
            )
            .map_err(store_error)
    }

    pub fn list_members(
        &self,
        dataset: &DatasetName,
        start: Option<&str>,
        max: usize,
    ) -> Result<(Vec<String>, bool), HostProblem> {
        if max == 0 || max > self.limits.max_members {
            return Err(HostProblem::ResourceExhausted);
        }
        let state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let entry = entry(&state, dataset)?;
        if !partitioned(entry.attributes.organization) {
            return Err(HostProblem::Unsupported);
        }
        let mut members = Vec::new();
        let mut more = false;
        let directory = if entry.attributes.organization
            == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
        {
            entry
                .member_generations
                .keys()
                .chain(entry.member_aliases.keys())
                .cloned()
                .collect::<BTreeSet<_>>()
        } else {
            entry.members.keys().cloned().collect::<BTreeSet<_>>()
        };
        for name in directory
            .iter()
            .filter(|name| start.is_none_or(|start| name.as_str() > start))
        {
            if members.len() == max {
                more = true;
                break;
            }
            members.push(name.clone());
        }
        Ok((members, more))
    }
}
fn entry<'a>(state: &'a State, name: &DatasetName) -> Result<&'a Entry, HostProblem> {
    state
        .entries
        .get(name.as_str())
        .ok_or(HostProblem::NotFound)
}
fn entry_text<'a>(state: &'a State, name: &str) -> Result<&'a Entry, HostProblem> {
    state.entries.get(name).ok_or(HostProblem::NotFound)
}

fn state_name_in_use(state: &State, name: &str) -> bool {
    state.entries.contains_key(name)
        || state.alternate_indexes.contains_key(name)
        || state.generation_groups.contains_key(name)
        || state.catalogs.contains_key(name)
        || state.catalog_aliases.contains_key(name)
}

fn state_object_count(state: &State) -> usize {
    state.entries.len()
        + state.alternate_indexes.len()
        + state.generation_groups.len()
        + state.catalogs.len()
        + state.catalog_aliases.len()
}

fn member_node(dataset: &DatasetName, member: &MemberName) -> String {
    format!("{}({})", dataset.as_str(), member.as_str())
}

fn resolve_catalog(
    state: &State,
    requested: &DatasetName,
) -> Result<mainframe_env_host_api::CatalogResolution, HostProblem> {
    let limits = dependency_limits(DatasetLimits::default());
    let mut current = requested.as_str().to_string();
    let mut alias_chain = Vec::new();
    let mut version = 0u64;
    let mut seen = BTreeSet::new();
    while let Some(alias) = state.catalog_aliases.get(&current) {
        if !seen.insert(current.clone()) || alias_chain.len() >= limits.max_depth {
            return Err(condition("CATCYCLE", 16));
        }
        alias_chain
            .push(DatasetName::new(&current, 128).map_err(|_| HostProblem::InfrastructureFailure)?);
        if state.dependencies.direct_dependencies(&current) != [alias.target.clone()] {
            return Err(HostProblem::InfrastructureFailure);
        }
        version = version.max(alias.version);
        current.clone_from(&alias.target);
    }
    if let Some(catalog) = state.catalogs.get(&current) {
        if !catalog.connected {
            return Err(condition("CATLGERR", 16));
        }
        version = version.max(catalog.version);
        return Ok(mainframe_env_host_api::CatalogResolution {
            requested: requested.clone(),
            resolved: requested.clone(),
            catalog: Some(
                DatasetName::new(current, 128).map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
            alias_chain,
            version,
        });
    }
    let resolved =
        DatasetName::new(&current, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
    if let Some(entry) = state.entries.get(&current)
        && let Some(catalog_name) = &entry.catalog.catalog
    {
        let catalog = state
            .catalogs
            .get(catalog_name.as_str())
            .filter(|catalog| catalog.connected)
            .ok_or_else(|| condition("CATLGERR", 16))?;
        version = version.max(entry.version).max(catalog.version);
        return Ok(mainframe_env_host_api::CatalogResolution {
            requested: requested.clone(),
            resolved,
            catalog: Some(catalog_name.clone()),
            alias_chain,
            version,
        });
    }

    let mut prefixes = state
        .catalog_aliases
        .iter()
        .filter(|(alias, _)| {
            requested.as_str().starts_with(alias.as_str())
                && requested
                    .as_str()
                    .as_bytes()
                    .get(alias.len())
                    .is_some_and(|separator| *separator == b'.')
        })
        .collect::<Vec<_>>();
    prefixes.sort_by(|left, right| {
        right
            .0
            .len()
            .cmp(&left.0.len())
            .then_with(|| left.0.cmp(right.0))
    });
    for (alias_name, alias) in prefixes {
        let mut target = alias.target.as_str();
        let mut prefix_seen = BTreeSet::new();
        version = version.max(alias.version);
        while let Some(next) = state.catalog_aliases.get(target) {
            if !prefix_seen.insert(target.to_string()) {
                return Err(condition("CATCYCLE", 16));
            }
            version = version.max(next.version);
            target = &next.target;
        }
        if let Some(catalog) = state.catalogs.get(target)
            && catalog.connected
        {
            alias_chain.push(
                DatasetName::new(alias_name, 128)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            );
            return Ok(mainframe_env_host_api::CatalogResolution {
                requested: requested.clone(),
                resolved,
                catalog: Some(
                    DatasetName::new(target, 128)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                ),
                alias_chain,
                version: version.max(catalog.version),
            });
        }
    }
    let master = state.catalogs.iter().find(|(_, catalog)| {
        catalog.kind == mainframe_env_host_api::CatalogKind::Master && catalog.connected
    });
    if master.is_none() && version == 0 {
        return Err(HostProblem::NotFound);
    }
    Ok(mainframe_env_host_api::CatalogResolution {
        requested: requested.clone(),
        resolved,
        catalog: master
            .map(|(name, _)| DatasetName::new(name, 128))
            .transpose()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        alias_chain,
        version: master.map_or(version, |(_, catalog)| version.max(catalog.version)),
    })
}

fn require_keyed(entry: &Entry) -> Result<(), HostProblem> {
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::KeySequenced
        && entry.attributes.key_offset.is_some()
        && entry.attributes.key_length.is_some()
    {
        Ok(())
    } else {
        Err(HostProblem::Unsupported)
    }
}

fn validate_key_range(entry: &Entry, offset: u32, length: u32) -> Result<(), HostProblem> {
    if length == 0
        || offset
            .checked_add(length)
            .is_none_or(|end| end > entry.attributes.logical_record_length)
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn primary_key(entry: &Entry, record: &[u8]) -> Result<Vec<u8>, HostProblem> {
    require_keyed(entry)?;
    let (offset, length) = entry
        .attributes
        .key_offset
        .zip(entry.attributes.key_length)
        .ok_or(HostProblem::InfrastructureFailure)?;
    record
        .get(offset as usize..(offset + length) as usize)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| condition("LENGERR", 22))
}

fn keyed_record<'a>(entry: &'a Entry, key: &[u8]) -> Option<&'a Vec<u8>> {
    entry
        .records
        .iter()
        .find(|record| primary_key(entry, record).ok().as_deref() == Some(key))
}

fn sort_keyed_records(entry: &mut Entry) -> Result<(), HostProblem> {
    let (offset, length) = entry
        .attributes
        .key_offset
        .zip(entry.attributes.key_length)
        .ok_or(HostProblem::InfrastructureFailure)?;
    let end = offset
        .checked_add(length)
        .ok_or(HostProblem::ResourceExhausted)? as usize;
    let start = offset as usize;
    if entry.records.iter().any(|record| record.len() < end) {
        return Err(condition("LENGERR", 22));
    }
    entry
        .records
        .sort_by(|left, right| left[start..end].cmp(&right[start..end]));
    Ok(())
}

fn validate_keyed_entry(entry: &mut Entry) -> Result<(), HostProblem> {
    sort_keyed_records(entry)?;
    for pair in entry.records.windows(2) {
        if primary_key(entry, &pair[0])? == primary_key(entry, &pair[1])? {
            return Err(condition("DUPREC", 14));
        }
    }
    Ok(())
}

fn tvs_operation_dataset(operation: &mainframe_env_host_api::TvsRecordOperation) -> &DatasetName {
    match operation {
        mainframe_env_host_api::TvsRecordOperation::Insert { dataset, .. }
        | mainframe_env_host_api::TvsRecordOperation::Rewrite { dataset, .. }
        | mainframe_env_host_api::TvsRecordOperation::Delete { dataset, .. } => dataset,
    }
}

fn active_tvs_references(state: &State, dataset: &DatasetName) -> bool {
    state.tvs_units.values().any(|unit| {
        matches!(
            unit.state,
            mainframe_env_host_api::TvsUnitOfWorkState::Active
                | mainframe_env_host_api::TvsUnitOfWorkState::Unknown
        ) && unit
            .operations
            .iter()
            .any(|operation| tvs_operation_dataset(operation) == dataset)
    })
}

fn tvs_operation_identity(
    entry: &Entry,
    operation: &mainframe_env_host_api::TvsRecordOperation,
) -> Result<Vec<u8>, HostProblem> {
    match operation {
        mainframe_env_host_api::TvsRecordOperation::Insert { record, .. } => {
            primary_key(entry, record)
        }
        mainframe_env_host_api::TvsRecordOperation::Rewrite { key, .. }
        | mainframe_env_host_api::TvsRecordOperation::Delete { key, .. } => Ok(key.clone()),
    }
}

fn apply_tvs_operation(
    entry: &mut Entry,
    operation: &mainframe_env_host_api::TvsRecordOperation,
    limits: DatasetLimits,
) -> Result<(), HostProblem> {
    require_keyed(entry)?;
    match operation {
        mainframe_env_host_api::TvsRecordOperation::Insert { record, .. } => {
            validate_records(std::slice::from_ref(record), &entry.attributes, limits)?;
            let key = primary_key(entry, record)?;
            if keyed_record(entry, &key).is_some() {
                return Err(condition("DUPREC", 14));
            }
            entry.records.push(record.clone());
        }
        mainframe_env_host_api::TvsRecordOperation::Rewrite { key, record, .. } => {
            validate_records(std::slice::from_ref(record), &entry.attributes, limits)?;
            if primary_key(entry, record)? != *key {
                return Err(condition("INVREQ", 16));
            }
            let position = entry
                .records
                .iter()
                .position(|candidate| primary_key(entry, candidate).ok().as_deref() == Some(key))
                .ok_or_else(|| condition("NOTFND", 13))?;
            entry.records[position] = record.clone();
        }
        mainframe_env_host_api::TvsRecordOperation::Delete { key, .. } => {
            let position = entry
                .records
                .iter()
                .position(|candidate| primary_key(entry, candidate).ok().as_deref() == Some(key))
                .ok_or_else(|| condition("NOTFND", 13))?;
            entry.records.remove(position);
        }
    }
    validate_keyed_entry(entry)?;
    validate_entry_shape(entry, limits)
}

fn project_tvs_entries(
    state: &State,
    operations: &[mainframe_env_host_api::TvsRecordOperation],
    limits: DatasetLimits,
) -> Result<BTreeMap<String, Entry>, HostProblem> {
    let mut projected = BTreeMap::<String, Entry>::new();
    for operation in operations {
        let dataset = tvs_operation_dataset(operation);
        if !projected.contains_key(dataset.as_str()) {
            let current = entry(state, dataset)?;
            ensure_update_allowed(current)?;
            if current.vsam.access_mode != mainframe_env_host_api::VsamAccessMode::Tvs {
                return Err(HostProblem::UnsupportedCapability {
                    capability: "tvs".into(),
                    detail: "TVS operation target is not defined for TVS".into(),
                });
            }
            projected.insert(dataset.as_str().into(), current.clone());
        }
        let target = projected
            .get_mut(dataset.as_str())
            .ok_or(HostProblem::InfrastructureFailure)?;
        apply_tvs_operation(target, operation, limits)?;
    }
    Ok(projected)
}

fn require_active_tvs<'a>(
    state: &'a State,
    transaction: &str,
    owner: &str,
) -> Result<&'a TvsUnitOfWork, HostProblem> {
    let unit = state
        .tvs_units
        .get(transaction)
        .ok_or(HostProblem::NotFound)?;
    if unit.owner != owner {
        return Err(HostProblem::Unauthorized);
    }
    if unit.state != mainframe_env_host_api::TvsUnitOfWorkState::Active {
        return Err(condition("INVREQ", 16));
    }
    Ok(unit)
}

fn tvs_receipt(
    transaction: &str,
    unit: &TvsUnitOfWork,
) -> Result<mainframe_env_host_api::TvsUnitOfWorkReceipt, HostProblem> {
    Ok(mainframe_env_host_api::TvsUnitOfWorkReceipt {
        transaction: transaction.into(),
        owner: mainframe_env_execution_api::PrincipalId::new(
            unit.owner.clone(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?,
        state: unit.state,
        staged_operations: u32::try_from(unit.operations.len())
            .map_err(|_| HostProblem::ResourceExhausted)?,
        version: unit.version,
    })
}

fn validate_lock_target(
    entry: &Entry,
    target: &mainframe_env_host_api::DatasetLockTarget,
) -> Result<(), HostProblem> {
    let mainframe_env_host_api::DatasetLockTarget::Record(identity) = target else {
        return Ok(());
    };
    let exists = match entry.attributes.organization {
        mainframe_env_host_api::DatasetOrganization::KeySequenced => {
            keyed_record(entry, identity).is_some()
        }
        mainframe_env_host_api::DatasetOrganization::EntrySequenced => {
            let requested = identity
                .as_slice()
                .try_into()
                .map(u64::from_be_bytes)
                .map_err(|_| HostProblem::Malformed)?;
            let mut rba = 0u64;
            let mut found = false;
            for record in &entry.records {
                if rba == requested {
                    found = true;
                    break;
                }
                rba = rba
                    .checked_add(
                        u64::try_from(record.len()).map_err(|_| HostProblem::ResourceExhausted)?,
                    )
                    .ok_or(HostProblem::ResourceExhausted)?;
            }
            found
        }
        mainframe_env_host_api::DatasetOrganization::Relative
        | mainframe_env_host_api::DatasetOrganization::VariableRelative => {
            let record_number = identity
                .as_slice()
                .try_into()
                .map(u64::from_be_bytes)
                .map_err(|_| HostProblem::Malformed)?;
            entry.relative_records.contains_key(&record_number)
        }
        _ => return Err(HostProblem::Unsupported),
    };
    if exists {
        Ok(())
    } else {
        Err(condition("NOTFND", 13))
    }
}

fn lock_resource(
    dataset: &DatasetName,
    target: &mainframe_env_host_api::DatasetLockTarget,
) -> String {
    match target {
        mainframe_env_host_api::DatasetLockTarget::Dataset => {
            format!("{}|0", dataset.as_str())
        }
        mainframe_env_host_api::DatasetLockTarget::Record(identity) => {
            let encoded = identity
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            format!("{}|1|{encoded}", dataset.as_str())
        }
    }
}

fn lock_id(seed: &str, resource: &str) -> String {
    let mut digest = Sha256::new();
    digest_field(&mut digest, b"mainframe-env.dataset-lock@1");
    digest_field(&mut digest, seed.as_bytes());
    digest_field(&mut digest, resource.as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn lock_conflicts(
    dataset: &DatasetName,
    entry: &Entry,
    requested_target: &mainframe_env_host_api::DatasetLockTarget,
    requested_mode: mainframe_env_host_api::DatasetLockMode,
    existing: &mainframe_env_host_api::DatasetLockReceipt,
) -> bool {
    let same_dataset = existing.dataset == *dataset;
    let overlaps = matches!(
        requested_target,
        mainframe_env_host_api::DatasetLockTarget::Dataset
    ) || matches!(
        existing.target,
        mainframe_env_host_api::DatasetLockTarget::Dataset
    ) || requested_target == &existing.target;
    if !same_dataset || !overlaps {
        return false;
    }
    if entry.vsam.share_options.cross_region == 1 {
        return !matches!(
            (requested_mode, existing.mode),
            (
                mainframe_env_host_api::DatasetLockMode::Shared,
                mainframe_env_host_api::DatasetLockMode::Shared
            )
        );
    }
    !matches!(
        (requested_mode, existing.mode),
        (
            mainframe_env_host_api::DatasetLockMode::Shared,
            mainframe_env_host_api::DatasetLockMode::Shared
                | mainframe_env_host_api::DatasetLockMode::Update
        ) | (
            mainframe_env_host_api::DatasetLockMode::Update,
            mainframe_env_host_api::DatasetLockMode::Shared
        )
    )
}

fn authorize_data_mutation(
    state: &State,
    dataset: &DatasetName,
    target: &mainframe_env_host_api::DatasetLockTarget,
    mutation: &mainframe_env_host_api::Mutation,
    exclusive: bool,
) -> Result<(), HostProblem> {
    let current = entry(state, dataset)?;
    ensure_update_allowed(current)?;
    match current.vsam.access_mode {
        mainframe_env_host_api::VsamAccessMode::NonRls => Ok(()),
        mainframe_env_host_api::VsamAccessMode::Tvs => Err(HostProblem::UnsupportedCapability {
            capability: "tvs".into(),
            detail: "TVS data changes must be staged in a unit of work".into(),
        }),
        mainframe_env_host_api::VsamAccessMode::Rls => {
            let lock_id = mutation
                .transaction
                .as_deref()
                .ok_or_else(|| condition("LOCKED", 16))?;
            let lock = state
                .locks
                .get(lock_id)
                .ok_or_else(|| condition("LOCKED", 16))?;
            let covers = lock.dataset == *dataset
                && (matches!(
                    lock.target,
                    mainframe_env_host_api::DatasetLockTarget::Dataset
                ) || lock.target == *target);
            let mode_allows = if exclusive {
                lock.mode == mainframe_env_host_api::DatasetLockMode::Exclusive
            } else {
                matches!(
                    lock.mode,
                    mainframe_env_host_api::DatasetLockMode::Update
                        | mainframe_env_host_api::DatasetLockMode::Exclusive
                )
            };
            if covers && mode_allows && lock.expires_at > mutation.sequence {
                Ok(())
            } else {
                Err(condition("LOCKED", 16))
            }
        }
    }
}

fn ensure_update_allowed(entry: &Entry) -> Result<(), HostProblem> {
    if matches!(
        entry.lifecycle.state,
        mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired
            | mainframe_env_host_api::DatasetLifecycleState::Migrated
            | mainframe_env_host_api::DatasetLifecycleState::RecallPending
    ) {
        Err(condition("RECOVERY", 16))
    } else {
        Ok(())
    }
}

fn ordered_records(entry: &Entry) -> Result<Vec<&Vec<u8>>, HostProblem> {
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::Linear {
        return Err(HostProblem::Unsupported);
    }
    let mut records = entry.records.iter().collect::<Vec<_>>();
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::KeySequenced {
        records.sort_by_key(|record| primary_key(entry, record).unwrap_or_default());
    }
    Ok(records)
}

fn record_identity(entry: &Entry, record: &[u8], position: usize) -> Result<Vec<u8>, HostProblem> {
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::KeySequenced {
        primary_key(entry, record)
    } else if entry.attributes.organization
        == mainframe_env_host_api::DatasetOrganization::EntrySequenced
    {
        let rba = entry
            .records
            .iter()
            .take(position)
            .try_fold(0u64, |total, preceding| {
                total
                    .checked_add(
                        u64::try_from(preceding.len())
                            .map_err(|_| HostProblem::ResourceExhausted)?,
                    )
                    .ok_or(HostProblem::ResourceExhausted)
            })?;
        Ok(rba.to_be_bytes().to_vec())
    } else {
        Ok(u64::try_from(position)
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes()
            .to_vec())
    }
}

fn linear_content(entry: &Entry) -> Result<Vec<u8>, HostProblem> {
    if entry.attributes.organization != mainframe_env_host_api::DatasetOrganization::Linear {
        return Err(HostProblem::Unsupported);
    }
    let length = entry.records.iter().try_fold(0usize, |total, chunk| {
        total
            .checked_add(chunk.len())
            .ok_or(HostProblem::ResourceExhausted)
    })?;
    let mut content = Vec::with_capacity(length);
    for chunk in &entry.records {
        content.extend_from_slice(chunk);
    }
    Ok(content)
}

fn resolved_member_name<'a>(entry: &'a Entry, member: &'a MemberName) -> &'a str {
    entry
        .member_aliases
        .get(member.as_str())
        .map_or(member.as_str(), String::as_str)
}

fn pdse_generation<'a>(
    entry: &'a Entry,
    member: &MemberName,
    relative: i32,
) -> Result<&'a MemberGeneration, HostProblem> {
    if entry.attributes.organization
        != mainframe_env_host_api::DatasetOrganization::PartitionedExtended
        || relative > 0
    {
        return Err(HostProblem::Unsupported);
    }
    let member = resolved_member_name(entry, member);
    let generations = entry
        .member_generations
        .get(member)
        .ok_or(HostProblem::NotFound)?;
    let position = isize::try_from(generations.len())
        .map_err(|_| HostProblem::ResourceExhausted)?
        .checked_sub(1)
        .and_then(|position| position.checked_add(relative as isize))
        .ok_or(HostProblem::NotFound)?;
    generations
        .get(usize::try_from(position).map_err(|_| HostProblem::NotFound)?)
        .ok_or(HostProblem::NotFound)
}

fn member_records<'a>(
    entry: &'a Entry,
    member: &MemberName,
    relative: i32,
) -> Result<&'a [Vec<u8>], HostProblem> {
    match entry.attributes.organization {
        mainframe_env_host_api::DatasetOrganization::Partitioned => entry
            .members
            .get(member.as_str())
            .map(Vec::as_slice)
            .ok_or(HostProblem::NotFound),
        mainframe_env_host_api::DatasetOrganization::PartitionedExtended => {
            Ok(&pdse_generation(entry, member, relative)?.records)
        }
        _ => Err(HostProblem::Unsupported),
    }
}

fn write_pdse_generation(
    entry: &mut Entry,
    member: &MemberName,
    records: Vec<Vec<u8>>,
    program_object: bool,
) -> Result<u64, HostProblem> {
    if entry.attributes.organization
        != mainframe_env_host_api::DatasetOrganization::PartitionedExtended
    {
        return Err(HostProblem::Unsupported);
    }
    let member = resolved_member_name(entry, member).to_string();
    let generations = entry.member_generations.entry(member).or_default();
    let generation = generations
        .last()
        .map_or(1, |generation| generation.generation.saturating_add(1));
    if generation == 0 {
        return Err(HostProblem::ResourceExhausted);
    }
    generations.push(MemberGeneration {
        generation,
        program_object,
        records,
    });
    Ok(generation)
}

fn esds_position_at_rba(entry: &Entry, target: u64) -> Result<usize, HostProblem> {
    if entry.attributes.organization != mainframe_env_host_api::DatasetOrganization::EntrySequenced
    {
        return Err(HostProblem::Unsupported);
    }
    let mut rba = 0u64;
    for (position, record) in entry.records.iter().enumerate() {
        if rba == target {
            return Ok(position);
        }
        rba = rba
            .checked_add(u64::try_from(record.len()).map_err(|_| HostProblem::ResourceExhausted)?)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    Err(condition("NOTFND", 13))
}

fn esds_record_at_rba(entry: &Entry, rba: u64) -> Result<(&Vec<u8>, u64), HostProblem> {
    let position = esds_position_at_rba(entry, rba)?;
    let record = &entry.records[position];
    let next = rba
        .checked_add(u64::try_from(record.len()).map_err(|_| HostProblem::ResourceExhausted)?)
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok((record, next))
}

fn sequential_records(
    entry: &Entry,
    member: Option<&MemberName>,
) -> Result<Vec<SequentialRecord>, HostProblem> {
    if partitioned(entry.attributes.organization) {
        let member = member.ok_or(HostProblem::Malformed)?;
        return member_records(entry, member, 0)?
            .iter()
            .enumerate()
            .map(|(position, record)| {
                Ok((
                    u64::try_from(position)
                        .map_err(|_| HostProblem::ResourceExhausted)?
                        .to_be_bytes()
                        .to_vec(),
                    record.clone(),
                ))
            })
            .collect();
    }
    if member.is_some() {
        return Err(HostProblem::Malformed);
    }
    if relative(entry.attributes.organization) {
        return Ok(entry
            .relative_records
            .iter()
            .map(|(rrn, record)| (rrn.to_be_bytes().to_vec(), record.clone()))
            .collect());
    }
    match entry.attributes.organization {
        mainframe_env_host_api::DatasetOrganization::KeySequenced => ordered_records(entry)?
            .into_iter()
            .map(|record| Ok((primary_key(entry, record)?, record.clone())))
            .collect(),
        mainframe_env_host_api::DatasetOrganization::EntrySequenced => {
            let mut rba = 0u64;
            let mut records = Vec::with_capacity(entry.records.len());
            for record in &entry.records {
                records.push((rba.to_be_bytes().to_vec(), record.clone()));
                rba = rba
                    .checked_add(
                        u64::try_from(record.len()).map_err(|_| HostProblem::ResourceExhausted)?,
                    )
                    .ok_or(HostProblem::ResourceExhausted)?;
            }
            Ok(records)
        }
        mainframe_env_host_api::DatasetOrganization::Sequential => entry
            .records
            .iter()
            .enumerate()
            .map(|(position, record)| {
                Ok((
                    u64::try_from(position)
                        .map_err(|_| HostProblem::ResourceExhausted)?
                        .to_be_bytes()
                        .to_vec(),
                    record.clone(),
                ))
            })
            .collect(),
        mainframe_env_host_api::DatasetOrganization::Linear => Err(HostProblem::Unsupported),
        mainframe_env_host_api::DatasetOrganization::Partitioned
        | mainframe_env_host_api::DatasetOrganization::PartitionedExtended
        | mainframe_env_host_api::DatasetOrganization::Relative
        | mainframe_env_host_api::DatasetOrganization::VariableRelative => {
            Err(HostProblem::InfrastructureFailure)
        }
    }
}

fn select_sequential(
    records: &[SequentialRecord],
    start: Option<u64>,
    reverse: bool,
    max: usize,
) -> Result<Vec<SequentialRecord>, HostProblem> {
    if records.is_empty() {
        return Ok(Vec::new());
    }
    let default = if reverse { records.len() - 1 } else { 0 };
    let start = start
        .map(usize::try_from)
        .transpose()
        .map_err(|_| HostProblem::ResourceExhausted)?
        .unwrap_or(default);
    if start >= records.len() {
        return Err(condition("NOTFND", 13));
    }
    let mut selected = Vec::with_capacity(max.min(records.len()));
    let mut position = start;
    loop {
        selected.push(records[position].clone());
        if selected.len() == max {
            break;
        }
        if reverse {
            if position == 0 {
                break;
            }
            position -= 1;
        } else {
            position += 1;
            if position == records.len() {
                break;
            }
        }
    }
    Ok(selected)
}

fn alternate_key(index: &AlternateIndex, record: &[u8]) -> Result<Vec<u8>, HostProblem> {
    record
        .get(
            index.key_offset as usize
                ..index
                    .key_offset
                    .checked_add(index.key_length)
                    .ok_or(HostProblem::ResourceExhausted)? as usize,
        )
        .map(<[u8]>::to_vec)
        .ok_or_else(|| condition("LENGERR", 22))
}

fn alternate_identities(
    _base: &Entry,
    index: &AlternateIndex,
) -> Result<Vec<BrowseIdentity>, HostProblem> {
    Ok(index.identities.clone())
}

fn build_alternate_identities(
    base: &Entry,
    index: &AlternateIndex,
) -> Result<Vec<BrowseIdentity>, HostProblem> {
    let mut ordered: BTreeMap<Vec<u8>, Vec<Vec<u8>>> = BTreeMap::new();
    for record in &base.records {
        ordered
            .entry(alternate_key(index, record)?)
            .or_default()
            .push(primary_key(base, record)?);
    }
    for identities in ordered.values_mut() {
        identities.sort();
    }
    Ok(ordered
        .into_iter()
        .flat_map(|(alternate, identities)| {
            identities
                .into_iter()
                .map(move |identity| (alternate.clone(), identity))
        })
        .collect())
}

fn validate_index_identities(base: &Entry, index: &AlternateIndex) -> Result<(), HostProblem> {
    let primary_length = usize::try_from(
        base.attributes
            .key_length
            .ok_or(HostProblem::InfrastructureFailure)?,
    )
    .map_err(|_| HostProblem::ResourceExhausted)?;
    let alternate_length =
        usize::try_from(index.key_length).map_err(|_| HostProblem::ResourceExhausted)?;
    if index.identities.iter().any(|(alternate, primary)| {
        alternate.len() != alternate_length || primary.len() != primary_length
    }) || index.identities.windows(2).any(|pair| pair[0] > pair[1])
        || (!index.allow_duplicates
            && index
                .identities
                .windows(2)
                .any(|pair| pair[0].0 == pair[1].0))
    {
        Err(HostProblem::InfrastructureFailure)
    } else {
        Ok(())
    }
}

fn validate_alternate_index(base: &Entry, index: &AlternateIndex) -> Result<(), HostProblem> {
    validate_key_range(base, index.key_offset, index.key_length)?;
    if index.allow_duplicates {
        return Ok(());
    }
    let mut keys = BTreeMap::new();
    for record in &base.records {
        let alternate = alternate_key(index, record)?;
        let primary = primary_key(base, record)?;
        if keys.insert(alternate, primary).is_some() {
            return Err(condition("DUPREC", 14));
        }
    }
    Ok(())
}

fn browse_identities(
    state: &State,
    dataset: &DatasetName,
) -> Result<Vec<BrowseIdentity>, HostProblem> {
    if let Some(index) = state.alternate_indexes.get(dataset.as_str()) {
        let base = entry_text(state, &index.base)?;
        return Ok(alternate_identities(base, index)?
            .into_iter()
            .filter(|(_, identity)| keyed_record(base, identity).is_some())
            .collect());
    }
    let entry = entry(state, dataset)?;
    ordered_records(entry)?
        .into_iter()
        .enumerate()
        .map(|(position, record)| {
            let identity = record_identity(entry, record, position)?;
            let logical = if entry.attributes.organization
                == mainframe_env_host_api::DatasetOrganization::KeySequenced
            {
                identity.clone()
            } else {
                record.clone()
            };
            Ok((logical, identity))
        })
        .collect()
}

fn record_for_identity<'a>(
    state: &'a State,
    dataset: &DatasetName,
    identity: &[u8],
) -> Result<Option<&'a Vec<u8>>, HostProblem> {
    if let Some(index) = state.alternate_indexes.get(dataset.as_str()) {
        return Ok(keyed_record(entry_text(state, &index.base)?, identity));
    }
    let entry = entry(state, dataset)?;
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::KeySequenced {
        Ok(keyed_record(entry, identity))
    } else {
        let bytes: [u8; 8] = identity
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let position = usize::try_from(u64::from_be_bytes(bytes))
            .map_err(|_| HostProblem::ResourceExhausted)?;
        Ok(entry.records.get(position))
    }
}

fn encode_catalog(catalog: &CatalogRecord) -> Vec<u8> {
    let mut payload = b"MECAT1".to_vec();
    payload.push(match catalog.kind {
        mainframe_env_host_api::CatalogKind::Master => 0,
        mainframe_env_host_api::CatalogKind::User => 1,
    });
    payload.push(u8::from(catalog.connected));
    payload
}

fn encode_lock(lock: &mainframe_env_host_api::DatasetLockReceipt) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MELCK1".to_vec();
    dataset_field(&mut payload, lock.lock_id.as_bytes())?;
    dataset_field(&mut payload, lock.dataset.as_str().as_bytes())?;
    match &lock.target {
        mainframe_env_host_api::DatasetLockTarget::Dataset => payload.push(0),
        mainframe_env_host_api::DatasetLockTarget::Record(identity) => {
            payload.push(1);
            dataset_field(&mut payload, identity)?;
        }
    }
    dataset_field(&mut payload, lock.owner.as_str().as_bytes())?;
    payload.push(match lock.mode {
        mainframe_env_host_api::DatasetLockMode::Shared => 0,
        mainframe_env_host_api::DatasetLockMode::Update => 1,
        mainframe_env_host_api::DatasetLockMode::Exclusive => 2,
    });
    payload.extend_from_slice(&lock.expires_at.to_be_bytes());
    payload.push(u8::from(lock.transaction.is_some()));
    if let Some(transaction) = &lock.transaction {
        dataset_field(&mut payload, transaction.as_bytes())?;
    }
    Ok(payload)
}

fn decode_lock(
    payload: &[u8],
    version: u64,
) -> Result<mainframe_env_host_api::DatasetLockReceipt, HostProblem> {
    if payload.get(..6) != Some(b"MELCK1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let lock_id = String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let dataset = DatasetName::new(
        String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        128,
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let target = match payload.get(at) {
        Some(0) => {
            at += 1;
            mainframe_env_host_api::DatasetLockTarget::Dataset
        }
        Some(1) => {
            at += 1;
            mainframe_env_host_api::DatasetLockTarget::Record(dataset_take_field(
                payload,
                &mut at,
                1024 * 1024,
            )?)
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let owner = mainframe_env_execution_api::PrincipalId::new(
        String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mode = match payload.get(at) {
        Some(0) => mainframe_env_host_api::DatasetLockMode::Shared,
        Some(1) => mainframe_env_host_api::DatasetLockMode::Update,
        Some(2) => mainframe_env_host_api::DatasetLockMode::Exclusive,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    at += 1;
    let expires_at = u64::from_be_bytes(
        payload
            .get(at..at.saturating_add(8))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    at += 8;
    let transaction = match payload.get(at) {
        Some(0) => {
            at += 1;
            None
        }
        Some(1) => {
            at += 1;
            Some(
                String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            )
        }
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if at != payload.len() || lock_id.is_empty() || expires_at == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(mainframe_env_host_api::DatasetLockReceipt {
        lock_id,
        dataset,
        target,
        owner,
        mode,
        expires_at,
        transaction,
        version,
    })
}

fn encode_tvs(unit: &TvsUnitOfWork) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"METVS1".to_vec();
    dataset_field(&mut payload, unit.owner.as_bytes())?;
    payload.push(match unit.state {
        mainframe_env_host_api::TvsUnitOfWorkState::Active => 0,
        mainframe_env_host_api::TvsUnitOfWorkState::Committed => 1,
        mainframe_env_host_api::TvsUnitOfWorkState::RolledBack => 2,
        mainframe_env_host_api::TvsUnitOfWorkState::Unknown => 3,
    });
    payload.extend_from_slice(
        &u32::try_from(unit.operations.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for operation in &unit.operations {
        match operation {
            mainframe_env_host_api::TvsRecordOperation::Insert { dataset, record } => {
                payload.push(0);
                dataset_field(&mut payload, dataset.as_str().as_bytes())?;
                dataset_field(&mut payload, record)?;
            }
            mainframe_env_host_api::TvsRecordOperation::Rewrite {
                dataset,
                key,
                record,
            } => {
                payload.push(1);
                dataset_field(&mut payload, dataset.as_str().as_bytes())?;
                dataset_field(&mut payload, key)?;
                dataset_field(&mut payload, record)?;
            }
            mainframe_env_host_api::TvsRecordOperation::Delete { dataset, key } => {
                payload.push(2);
                dataset_field(&mut payload, dataset.as_str().as_bytes())?;
                dataset_field(&mut payload, key)?;
            }
        }
    }
    Ok(payload)
}

fn decode_tvs(
    payload: &[u8],
    version: u64,
    limits: DatasetLimits,
) -> Result<TvsUnitOfWork, HostProblem> {
    if payload.get(..6) != Some(b"METVS1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let owner = String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let state = match payload.get(at) {
        Some(0) => mainframe_env_host_api::TvsUnitOfWorkState::Active,
        Some(1) => mainframe_env_host_api::TvsUnitOfWorkState::Committed,
        Some(2) => mainframe_env_host_api::TvsUnitOfWorkState::RolledBack,
        Some(3) => mainframe_env_host_api::TvsUnitOfWorkState::Unknown,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    at += 1;
    let count = usize::try_from(u32::from_be_bytes(
        payload
            .get(at..at.saturating_add(4))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    at += 4;
    if count > limits.max_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut operations = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = *payload.get(at).ok_or(HostProblem::InfrastructureFailure)?;
        at += 1;
        let dataset = DatasetName::new(
            String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            128,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        operations.push(match tag {
            0 => mainframe_env_host_api::TvsRecordOperation::Insert {
                dataset,
                record: dataset_take_field(payload, &mut at, limits.max_record_bytes)?,
            },
            1 => mainframe_env_host_api::TvsRecordOperation::Rewrite {
                dataset,
                key: dataset_take_field(payload, &mut at, limits.max_record_bytes)?,
                record: dataset_take_field(payload, &mut at, limits.max_record_bytes)?,
            },
            2 => mainframe_env_host_api::TvsRecordOperation::Delete {
                dataset,
                key: dataset_take_field(payload, &mut at, limits.max_record_bytes)?,
            },
            _ => return Err(HostProblem::InfrastructureFailure),
        });
    }
    if at != payload.len() || owner.is_empty() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(TvsUnitOfWork {
        owner,
        state,
        operations,
        version,
    })
}

fn decode_catalog(payload: &[u8], version: u64) -> Result<CatalogRecord, HostProblem> {
    if payload.len() != 8 || payload.get(..6) != Some(b"MECAT1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let kind = match payload[6] {
        0 => mainframe_env_host_api::CatalogKind::Master,
        1 => mainframe_env_host_api::CatalogKind::User,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let connected = match payload[7] {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    Ok(CatalogRecord {
        kind,
        connected,
        version,
    })
}

fn encode_catalog_alias(alias: &CatalogAlias) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MECAL1".to_vec();
    dataset_field(&mut payload, alias.target.as_bytes())?;
    Ok(payload)
}

fn decode_catalog_alias(payload: &[u8], version: u64) -> Result<CatalogAlias, HostProblem> {
    if payload.get(..6) != Some(b"MECAL1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let target = String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if at != payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(CatalogAlias { target, version })
}

fn encode_alternate_index(index: &AlternateIndex) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MEAIX4".to_vec();
    dataset_field(&mut payload, index.base.as_bytes())?;
    dataset_field(&mut payload, index.parent.as_bytes())?;
    payload.push(u8::from(index.is_path));
    payload.extend_from_slice(&index.key_offset.to_be_bytes());
    payload.extend_from_slice(&index.key_length.to_be_bytes());
    payload.push(u8::from(index.allow_duplicates));
    payload.push(u8::from(index.upgrade));
    payload.extend_from_slice(
        &u32::try_from(index.identities.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (alternate, primary) in &index.identities {
        dataset_field(&mut payload, alternate)?;
        dataset_field(&mut payload, primary)?;
    }
    Ok(payload)
}

fn decode_alternate_index(payload: &[u8], version: u64) -> Result<AlternateIndex, HostProblem> {
    let schema = payload.get(..6);
    if !matches!(
        schema,
        Some(b"MEAIX1") | Some(b"MEAIX2") | Some(b"MEAIX3") | Some(b"MEAIX4")
    ) || version == 0
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let base = String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let (parent, is_path) = if matches!(schema, Some(b"MEAIX2") | Some(b"MEAIX3") | Some(b"MEAIX4"))
    {
        let parent = String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let is_path = match payload.get(at) {
            Some(0) => false,
            Some(1) => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        at += 1;
        (parent, is_path)
    } else {
        (base.clone(), false)
    };
    let key_offset = u32::from_be_bytes(
        payload
            .get(at..at + 4)
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    at += 4;
    let key_length = u32::from_be_bytes(
        payload
            .get(at..at + 4)
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    at += 4;
    let allow_duplicates = match payload.get(at) {
        Some(0) => false,
        Some(1) => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    at += 1;
    let upgrade = if matches!(schema, Some(b"MEAIX3") | Some(b"MEAIX4")) {
        let value = match payload.get(at) {
            Some(0) => false,
            Some(1) => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        at += 1;
        value
    } else {
        true
    };
    let mut identities = Vec::new();
    if schema == Some(b"MEAIX4") {
        let count = usize::try_from(u32::from_be_bytes(
            payload
                .get(at..at + 4)
                .ok_or(HostProblem::InfrastructureFailure)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        at += 4;
        if count > 65_536 {
            return Err(HostProblem::ResourceExhausted);
        }
        for _ in 0..count {
            identities.push((
                dataset_take_field(payload, &mut at, 1024 * 1024)?,
                dataset_take_field(payload, &mut at, 1024 * 1024)?,
            ));
        }
    }
    if at != payload.len() || base.is_empty() || key_length == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(AlternateIndex {
        base,
        parent,
        is_path,
        key_offset,
        key_length,
        allow_duplicates,
        upgrade,
        identities,
        version,
    })
}

fn encode_generation_group(group: &GenerationGroup) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MEGDG1".to_vec();
    payload.extend_from_slice(&group.limit.to_be_bytes());
    payload.push(u8::from(group.scratch));
    payload.push(u8::from(group.empty));
    payload.extend_from_slice(&group.next_generation.to_be_bytes());
    payload.extend_from_slice(
        &u32::try_from(group.generations.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for generation in &group.generations {
        dataset_field(&mut payload, generation.as_bytes())?;
    }
    payload.extend_from_slice(
        &u32::try_from(group.retired.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for retired in &group.retired {
        dataset_field(&mut payload, retired.as_bytes())?;
    }
    Ok(payload)
}

fn decode_generation_group(
    payload: &[u8],
    version: u64,
    limits: DatasetLimits,
) -> Result<GenerationGroup, HostProblem> {
    if payload.get(..6) != Some(b"MEGDG1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let mut take_u32 = || -> Result<u32, HostProblem> {
        let value = u32::from_be_bytes(
            payload
                .get(at..at.saturating_add(4))
                .ok_or(HostProblem::InfrastructureFailure)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        at = at
            .checked_add(4)
            .ok_or(HostProblem::InfrastructureFailure)?;
        Ok(value)
    };
    let limit = take_u32()?;
    let scratch = match payload.get(at) {
        Some(0) => false,
        Some(1) => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    at += 1;
    let empty = match payload.get(at) {
        Some(0) => false,
        Some(1) => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    at += 1;
    let next_generation = u32::from_be_bytes(
        payload
            .get(at..at.saturating_add(4))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    );
    at += 4;
    let generation_count = usize::try_from(u32::from_be_bytes(
        payload
            .get(at..at.saturating_add(4))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    at += 4;
    if limit == 0 || generation_count > limit as usize || generation_count > limits.max_records {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut generations = Vec::with_capacity(generation_count);
    for _ in 0..generation_count {
        generations.push(
            String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
    }
    let retired_count = usize::try_from(u32::from_be_bytes(
        payload
            .get(at..at.saturating_add(4))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    at += 4;
    if retired_count > limits.max_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut retired = Vec::with_capacity(retired_count);
    for _ in 0..retired_count {
        retired.push(
            String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
    }
    if at != payload.len()
        || next_generation == 0
        || generations
            .iter()
            .any(|name| generation_number(name).is_none())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(GenerationGroup {
        limit,
        scratch,
        empty,
        next_generation,
        generations,
        retired,
        version,
    })
}

fn generation_number(name: &str) -> Option<u32> {
    let marker = name.rfind(".G")? + 2;
    let digits = name.get(marker..marker + 4)?;
    (name.get(marker + 4..)? == "V00")
        .then(|| digits.parse().ok())
        .flatten()
}

fn validate_seed_label(value: &str) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_seed_source(value: &str) -> Result<(), HostProblem> {
    if value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value
            .split('/')
            .any(|component| component.is_empty() || component == "..")
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn build_seed_generation(
    package: &str,
    generation: &str,
    mut objects: Vec<DatasetSeedObject>,
    limits: DatasetLimits,
) -> Result<SeedGeneration, HostProblem> {
    validate_seed_label(package)?;
    validate_seed_label(generation)?;
    if objects.is_empty() || objects.len() > limits.max_datasets {
        return Err(HostProblem::ResourceExhausted);
    }
    objects.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    let mut sources = BTreeMap::new();
    let mut entries = BTreeMap::new();
    let mut total = 0usize;
    let mut digest = Sha256::new();
    for object in &objects {
        validate_seed_source(&object.source_id)?;
        if sources.insert(object.source_id.clone(), ()).is_some()
            || object.record_length == 0
            || object.record_length != object.attributes.logical_record_length
            || !matches!(
                object.attributes.record_format,
                mainframe_env_host_api::RecordFormat::Fixed
                    | mainframe_env_host_api::RecordFormat::FixedBlocked
                    | mainframe_env_host_api::RecordFormat::FixedBlockedStandard
            )
            || object.bytes.is_empty()
            || object.bytes.len() % object.record_length as usize != 0
        {
            return Err(HostProblem::Malformed);
        }
        object
            .attributes
            .validate(mainframe_env_host_api::HostLimits {
                max_record_bytes: limits.max_record_bytes,
                ..Default::default()
            })?;
        let actual = format!("sha256:{:x}", Sha256::digest(&object.bytes));
        if object.sha256 != actual {
            return Err(HostProblem::IdempotencyConflict);
        }
        total = total
            .checked_add(object.bytes.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if total > limits.max_total_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let records = object
            .bytes
            .chunks_exact(object.record_length as usize)
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>();
        let mut entry = Entry::from_definition(
            mainframe_env_host_api::DatasetDefinition::compatibility(object.attributes.clone()),
            1,
        );
        entry.records = records;
        validate_entry_shape(&entry, limits)?;
        if entry.attributes.organization
            == mainframe_env_host_api::DatasetOrganization::KeySequenced
        {
            validate_keyed_entry(&mut entry)?;
        }
        match entries.get(object.dataset.as_str()) {
            Some(existing) if existing != &entry => {
                return Err(HostProblem::IdempotencyConflict);
            }
            Some(_) => {}
            None => {
                entries.insert(object.dataset.as_str().into(), entry);
            }
        }
        digest.update((object.source_id.len() as u64).to_be_bytes());
        digest.update(object.source_id.as_bytes());
        digest.update((object.dataset.as_str().len() as u64).to_be_bytes());
        digest.update(object.dataset.as_str().as_bytes());
        digest.update((object.bytes.len() as u64).to_be_bytes());
        digest.update(&object.bytes);
    }
    Ok(SeedGeneration {
        package: package.to_ascii_uppercase(),
        generation: generation.to_string(),
        objects,
        entries,
        identity: format!("sha256:{:x}", digest.finalize()),
    })
}

fn seed_receipt(generation: &SeedGeneration, replayed: bool) -> SeedInstallReceipt {
    SeedInstallReceipt {
        package: generation.package.clone(),
        generation: generation.generation.clone(),
        seed_objects: generation.objects.len(),
        datasets: generation.entries.len(),
        records: generation
            .entries
            .values()
            .map(|entry| entry.records.len())
            .sum(),
        bytes: generation
            .objects
            .iter()
            .map(|object| object.bytes.len())
            .sum(),
        identity: generation.identity.clone(),
        replayed,
    }
}

fn seed_generation_key(package: &str, generation: &str) -> String {
    format!("{}@{generation}", package.to_ascii_uppercase())
}

fn encode_seed_generation(generation: &SeedGeneration) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MESEED1".to_vec();
    dataset_field(&mut payload, generation.package.as_bytes())?;
    dataset_field(&mut payload, generation.generation.as_bytes())?;
    dataset_field(&mut payload, generation.identity.as_bytes())?;
    payload.extend_from_slice(
        &u32::try_from(generation.objects.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for object in &generation.objects {
        dataset_field(&mut payload, object.source_id.as_bytes())?;
        dataset_field(&mut payload, object.dataset.as_str().as_bytes())?;
        dataset_field(&mut payload, object.sha256.as_bytes())?;
        let mut entry = Entry::from_definition(
            mainframe_env_host_api::DatasetDefinition::compatibility(object.attributes.clone()),
            1,
        );
        entry.records = object
            .bytes
            .chunks_exact(object.record_length as usize)
            .map(<[u8]>::to_vec)
            .collect();
        dataset_field(
            &mut payload,
            &encode(&entry).map_err(|_| HostProblem::InfrastructureFailure)?,
        )?;
    }
    Ok(payload)
}

fn decode_seed_generation(
    payload: &[u8],
    limits: DatasetLimits,
) -> Result<SeedGeneration, HostProblem> {
    if payload.get(..7) != Some(b"MESEED1") {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 7usize;
    let package = String::from_utf8(dataset_take_field(payload, &mut at, 64)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let generation = String::from_utf8(dataset_take_field(payload, &mut at, 64)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let identity = String::from_utf8(dataset_take_field(payload, &mut at, 80)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let count = usize::try_from(u32::from_be_bytes(
        payload
            .get(at..at.saturating_add(4))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    at += 4;
    if count == 0 || count > limits.max_datasets {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut objects = Vec::with_capacity(count);
    for _ in 0..count {
        let source_id = String::from_utf8(dataset_take_field(payload, &mut at, 256)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let dataset = DatasetName::new(
            String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            128,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let sha256 = String::from_utf8(dataset_take_field(payload, &mut at, 80)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let encoded = dataset_take_field(payload, &mut at, limits.max_total_bytes)?;
        let entry = decode(
            &encoded,
            limits.max_records,
            limits.max_record_bytes,
            limits.max_members,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if !entry.members.is_empty()
            || !entry.relative_records.is_empty()
            || !entry.member_generations.is_empty()
            || !entry.member_aliases.is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let bytes = entry.records.concat();
        objects.push(DatasetSeedObject {
            source_id,
            dataset,
            attributes: entry.attributes.clone(),
            record_length: entry.attributes.logical_record_length,
            sha256,
            bytes,
        });
    }
    if at != payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let decoded = build_seed_generation(&package, &generation, objects, limits)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if decoded.identity != identity {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(decoded)
}

fn encode_seed_selection(selection: &SeedSelection) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MESEL1".to_vec();
    dataset_field(&mut payload, selection.generation.as_bytes())?;
    Ok(payload)
}

fn decode_seed_selection(payload: &[u8], version: u64) -> Result<SeedSelection, HostProblem> {
    if payload.get(..6) != Some(b"MESEL1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let generation = String::from_utf8(dataset_take_field(payload, &mut at, 64)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if at != payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    validate_seed_label(&generation).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(SeedSelection {
        generation,
        version,
    })
}

fn dataset_field(payload: &mut Vec<u8>, value: &[u8]) -> Result<(), HostProblem> {
    payload.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(value);
    Ok(())
}

fn dataset_take_field(payload: &[u8], at: &mut usize, max: usize) -> Result<Vec<u8>, HostProblem> {
    let length = usize::try_from(u32::from_be_bytes(
        payload
            .get(*at..at.saturating_add(4))
            .ok_or(HostProblem::InfrastructureFailure)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    *at = at
        .checked_add(4)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if length > max {
        return Err(HostProblem::ResourceExhausted);
    }
    let value = payload
        .get(*at..at.saturating_add(length))
        .ok_or(HostProblem::InfrastructureFailure)?
        .to_vec();
    *at = at
        .checked_add(length)
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok(value)
}
fn mutation(request: &DatasetRequest) -> Option<&mainframe_env_host_api::Mutation> {
    match request {
        DatasetRequest::Create { mutation, .. }
        | DatasetRequest::Define { mutation, .. }
        | DatasetRequest::Alter { mutation, .. }
        | DatasetRequest::SetLifecycle { mutation, .. }
        | DatasetRequest::RecordBackup { mutation, .. }
        | DatasetRequest::Restore { mutation, .. }
        | DatasetRequest::DefineCatalog { mutation, .. }
        | DatasetRequest::SetCatalogConnection { mutation, .. }
        | DatasetRequest::DefineAlias { mutation, .. }
        | DatasetRequest::DefineMemberAlias { mutation, .. }
        | DatasetRequest::WriteMemberGeneration { mutation, .. }
        | DatasetRequest::DeleteMemberGeneration { mutation, .. }
        | DatasetRequest::AcquireLock { mutation, .. }
        | DatasetRequest::ReleaseLock { mutation, .. }
        | DatasetRequest::BeginTvs { mutation, .. }
        | DatasetRequest::StageTvs { mutation, .. }
        | DatasetRequest::CompleteTvs { mutation, .. }
        | DatasetRequest::ReconcileTvs { mutation, .. }
        | DatasetRequest::Write { mutation, .. }
        | DatasetRequest::Append { mutation, .. }
        | DatasetRequest::Truncate { mutation, .. }
        | DatasetRequest::RewriteRecord { mutation, .. }
        | DatasetRequest::DeleteRecord { mutation, .. }
        | DatasetRequest::DefineAlternateIndex { mutation, .. }
        | DatasetRequest::BuildAlternateIndex { mutation, .. }
        | DatasetRequest::DefinePath { mutation, .. }
        | DatasetRequest::WriteRelative { mutation, .. }
        | DatasetRequest::DeleteRelative { mutation, .. }
        | DatasetRequest::WriteRba { mutation, .. }
        | DatasetRequest::DefineGenerationGroup { mutation, .. }
        | DatasetRequest::CreateGeneration { mutation, .. }
        | DatasetRequest::Rename { mutation, .. }
        | DatasetRequest::Delete { mutation, .. } => Some(mutation),
        _ => None,
    }
}

fn request_owner(request: &DatasetRequest) -> Option<&mainframe_env_execution_api::PrincipalId> {
    match request {
        DatasetRequest::TvsStatus { owner, .. }
        | DatasetRequest::AcquireLock { owner, .. }
        | DatasetRequest::ReleaseLock { owner, .. }
        | DatasetRequest::BeginTvs { owner, .. }
        | DatasetRequest::StageTvs { owner, .. }
        | DatasetRequest::CompleteTvs { owner, .. }
        | DatasetRequest::ReconcileTvs { owner, .. } => Some(owner),
        _ => None,
    }
}

fn owned_mutation_dataset(request: &DatasetRequest) -> Option<&DatasetName> {
    match request {
        DatasetRequest::Alter { dataset, .. }
        | DatasetRequest::SetLifecycle { dataset, .. }
        | DatasetRequest::RecordBackup { dataset, .. }
        | DatasetRequest::Restore { dataset, .. }
        | DatasetRequest::DefineMemberAlias { dataset, .. }
        | DatasetRequest::WriteMemberGeneration { dataset, .. }
        | DatasetRequest::DeleteMemberGeneration { dataset, .. }
        | DatasetRequest::AcquireLock { dataset, .. }
        | DatasetRequest::ReleaseLock { dataset, .. }
        | DatasetRequest::Write { dataset, .. }
        | DatasetRequest::Append { dataset, .. }
        | DatasetRequest::Truncate { dataset, .. }
        | DatasetRequest::RewriteRecord { dataset, .. }
        | DatasetRequest::DeleteRecord { dataset, .. }
        | DatasetRequest::WriteRelative { dataset, .. }
        | DatasetRequest::DeleteRelative { dataset, .. }
        | DatasetRequest::WriteRba { dataset, .. }
        | DatasetRequest::BuildAlternateIndex { base: dataset, .. }
        | DatasetRequest::Delete { dataset, .. }
        | DatasetRequest::Rename { from: dataset, .. } => Some(dataset),
        DatasetRequest::StageTvs { operation, .. } => Some(tvs_operation_dataset(operation)),
        _ => None,
    }
}

fn rls_request_context(
    request: &DatasetRequest,
) -> Option<(&DatasetName, &mainframe_env_host_api::Mutation)> {
    match request {
        DatasetRequest::Write {
            dataset, mutation, ..
        }
        | DatasetRequest::Append {
            dataset, mutation, ..
        }
        | DatasetRequest::Truncate {
            dataset, mutation, ..
        }
        | DatasetRequest::RewriteRecord {
            dataset, mutation, ..
        }
        | DatasetRequest::DeleteRecord {
            dataset, mutation, ..
        }
        | DatasetRequest::WriteRelative {
            dataset, mutation, ..
        }
        | DatasetRequest::DeleteRelative {
            dataset, mutation, ..
        }
        | DatasetRequest::WriteRba {
            dataset, mutation, ..
        } => Some((dataset, mutation)),
        _ => None,
    }
}

fn atomic_dataset_request(request: &DatasetRequest) -> bool {
    matches!(
        request,
        DatasetRequest::Write { .. }
            | DatasetRequest::Define { .. }
            | DatasetRequest::Alter { .. }
            | DatasetRequest::SetLifecycle { .. }
            | DatasetRequest::RecordBackup { .. }
            | DatasetRequest::Restore { .. }
            | DatasetRequest::DefineCatalog { .. }
            | DatasetRequest::SetCatalogConnection { .. }
            | DatasetRequest::DefineAlias { .. }
            | DatasetRequest::DefineMemberAlias { .. }
            | DatasetRequest::WriteMemberGeneration { .. }
            | DatasetRequest::DeleteMemberGeneration { .. }
            | DatasetRequest::AcquireLock { .. }
            | DatasetRequest::ReleaseLock { .. }
            | DatasetRequest::BeginTvs { .. }
            | DatasetRequest::StageTvs { .. }
            | DatasetRequest::CompleteTvs { .. }
            | DatasetRequest::ReconcileTvs { .. }
            | DatasetRequest::Append { .. }
            | DatasetRequest::Truncate { .. }
            | DatasetRequest::RewriteRecord { .. }
            | DatasetRequest::DeleteRecord { .. }
            | DatasetRequest::DefineAlternateIndex { .. }
            | DatasetRequest::BuildAlternateIndex { .. }
            | DatasetRequest::DefinePath { .. }
            | DatasetRequest::WriteRelative { .. }
            | DatasetRequest::DeleteRelative { .. }
            | DatasetRequest::WriteRba { .. }
            | DatasetRequest::DefineGenerationGroup { .. }
            | DatasetRequest::CreateGeneration { .. }
            | DatasetRequest::Rename { .. }
            | DatasetRequest::Delete { .. }
    )
}

fn snapshot_record_count(snapshot: &DatasetSnapshot) -> Result<usize, HostProblem> {
    snapshot.members.iter().try_fold(
        snapshot
            .records
            .len()
            .checked_add(snapshot.relative_records.len())
            .ok_or(HostProblem::ResourceExhausted)?,
        |total, member| {
            member.generations.iter().try_fold(
                total
                    .checked_add(member.records.len())
                    .ok_or(HostProblem::ResourceExhausted)?,
                |total, generation| {
                    total
                        .checked_add(generation.records.len())
                        .ok_or(HostProblem::ResourceExhausted)
                },
            )
        },
    )
}

fn restore_snapshot_content(
    entry: &mut Entry,
    snapshot: &DatasetSnapshot,
    limits: DatasetLimits,
) -> Result<(), HostProblem> {
    let no_relative_members_or_linear = || {
        snapshot.relative_records.is_empty()
            && snapshot.members.is_empty()
            && snapshot.linear_data.is_empty()
    };
    match entry.attributes.organization {
        mainframe_env_host_api::DatasetOrganization::Sequential
        | mainframe_env_host_api::DatasetOrganization::KeySequenced
        | mainframe_env_host_api::DatasetOrganization::EntrySequenced => {
            if !no_relative_members_or_linear() {
                return Err(HostProblem::Malformed);
            }
            entry.records = snapshot.records.clone();
        }
        mainframe_env_host_api::DatasetOrganization::Relative
        | mainframe_env_host_api::DatasetOrganization::VariableRelative => {
            if !snapshot.records.is_empty()
                || !snapshot.members.is_empty()
                || !snapshot.linear_data.is_empty()
            {
                return Err(HostProblem::Malformed);
            }
            for relative in &snapshot.relative_records {
                if entry
                    .relative_records
                    .insert(relative.record_number, relative.record.clone())
                    .is_some()
                {
                    return Err(HostProblem::Malformed);
                }
            }
        }
        mainframe_env_host_api::DatasetOrganization::Partitioned => {
            if !snapshot.records.is_empty()
                || !snapshot.relative_records.is_empty()
                || !snapshot.linear_data.is_empty()
            {
                return Err(HostProblem::Malformed);
            }
            for member in &snapshot.members {
                if member.alias_of.is_some()
                    || !member.generations.is_empty()
                    || entry
                        .members
                        .insert(member.name.as_str().into(), member.records.clone())
                        .is_some()
                {
                    return Err(HostProblem::Malformed);
                }
            }
        }
        mainframe_env_host_api::DatasetOrganization::PartitionedExtended => {
            if !snapshot.records.is_empty()
                || !snapshot.relative_records.is_empty()
                || !snapshot.linear_data.is_empty()
            {
                return Err(HostProblem::Malformed);
            }
            for member in &snapshot.members {
                if !member.records.is_empty() {
                    return Err(HostProblem::Malformed);
                }
                if let Some(target) = &member.alias_of {
                    if !member.generations.is_empty()
                        || entry
                            .member_aliases
                            .insert(member.name.as_str().into(), target.as_str().into())
                            .is_some()
                    {
                        return Err(HostProblem::Malformed);
                    }
                } else {
                    if member.generations.is_empty()
                        || entry
                            .member_generations
                            .insert(
                                member.name.as_str().into(),
                                member
                                    .generations
                                    .iter()
                                    .map(|generation| MemberGeneration {
                                        generation: generation.generation,
                                        program_object: generation.program_object,
                                        records: generation.records.clone(),
                                    })
                                    .collect(),
                            )
                            .is_some()
                    {
                        return Err(HostProblem::Malformed);
                    }
                }
            }
        }
        mainframe_env_host_api::DatasetOrganization::Linear => {
            if !snapshot.records.is_empty()
                || !snapshot.relative_records.is_empty()
                || !snapshot.members.is_empty()
            {
                return Err(HostProblem::Malformed);
            }
            entry.records = snapshot
                .linear_data
                .chunks(limits.max_record_bytes)
                .map(<[u8]>::to_vec)
                .collect();
        }
    }
    if snapshot_record_count(snapshot)? > limits.max_records {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}

fn validate_entry_shape(entry: &Entry, limits: DatasetLimits) -> Result<(), HostProblem> {
    validate_records(&entry.records, &entry.attributes, limits)?;
    for records in entry.members.values() {
        validate_records(records, &entry.attributes, limits)?;
    }
    for record in entry.relative_records.values() {
        validate_records(std::slice::from_ref(record), &entry.attributes, limits)?;
    }
    for (member, generations) in &entry.member_generations {
        MemberName::new(member, 8).map_err(|_| HostProblem::Malformed)?;
        if generations.is_empty() {
            return Err(HostProblem::Malformed);
        }
        let mut previous = 0u64;
        for generation in generations {
            if generation.generation == 0 || generation.generation <= previous {
                return Err(HostProblem::Malformed);
            }
            previous = generation.generation;
            validate_records(&generation.records, &entry.attributes, limits)?;
        }
    }
    for (alias, target) in &entry.member_aliases {
        MemberName::new(alias, 8).map_err(|_| HostProblem::Malformed)?;
        MemberName::new(target, 8).map_err(|_| HostProblem::Malformed)?;
        if alias == target || !entry.member_generations.contains_key(target) {
            return Err(HostProblem::Malformed);
        }
    }
    let directory_entries = if entry.attributes.organization
        == mainframe_env_host_api::DatasetOrganization::PartitionedExtended
    {
        entry
            .member_generations
            .len()
            .checked_add(entry.member_aliases.len())
            .ok_or(HostProblem::ResourceExhausted)?
    } else if entry.attributes.organization
        == mainframe_env_host_api::DatasetOrganization::Partitioned
    {
        entry.members.len()
    } else {
        0
    };
    let directory_capacity = usize::try_from(entry.allocation.directory_blocks)
        .map_err(|_| HostProblem::ResourceExhausted)?
        .checked_mul(6)
        .ok_or(HostProblem::ResourceExhausted)?;
    if directory_entries > directory_capacity || directory_entries > limits.max_members {
        return Err(condition("NOSPACE", 12));
    }
    let count = entry
        .records
        .len()
        .checked_add(entry.members.values().map(Vec::len).sum::<usize>())
        .and_then(|count| count.checked_add(entry.relative_records.len()))
        .and_then(|count| {
            count.checked_add(
                entry
                    .member_generations
                    .values()
                    .flatten()
                    .map(|generation| generation.records.len())
                    .sum::<usize>(),
            )
        })
        .ok_or(HostProblem::ResourceExhausted)?;
    if count > limits.max_records || bytes(entry) > limits.max_total_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if entry.sms.guaranteed_space
        && usize::try_from(allocated_bytes(entry)?)
            .map_or(true, |value| value > limits.max_total_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    if usize::try_from(buffer_bytes(entry)?).map_or(true, |value| value > limits.max_total_bytes) {
        return Err(HostProblem::ResourceExhausted);
    }
    dataset_geometry(entry)?;
    match entry.attributes.organization {
        mainframe_env_host_api::DatasetOrganization::Partitioned
            if !entry.records.is_empty() || !entry.relative_records.is_empty() =>
        {
            Err(HostProblem::Malformed)
        }
        mainframe_env_host_api::DatasetOrganization::Partitioned
            if !entry.member_generations.is_empty() || !entry.member_aliases.is_empty() =>
        {
            Err(HostProblem::Malformed)
        }
        mainframe_env_host_api::DatasetOrganization::PartitionedExtended
            if !entry.records.is_empty()
                || !entry.members.is_empty()
                || !entry.relative_records.is_empty() =>
        {
            Err(HostProblem::Malformed)
        }
        mainframe_env_host_api::DatasetOrganization::Relative
        | mainframe_env_host_api::DatasetOrganization::VariableRelative
            if !entry.records.is_empty()
                || !entry.members.is_empty()
                || !entry.member_generations.is_empty()
                || !entry.member_aliases.is_empty() =>
        {
            Err(HostProblem::Malformed)
        }
        mainframe_env_host_api::DatasetOrganization::Sequential
        | mainframe_env_host_api::DatasetOrganization::KeySequenced
        | mainframe_env_host_api::DatasetOrganization::EntrySequenced
        | mainframe_env_host_api::DatasetOrganization::Linear
            if !entry.members.is_empty()
                || !entry.relative_records.is_empty()
                || !entry.member_generations.is_empty()
                || !entry.member_aliases.is_empty() =>
        {
            Err(HostProblem::Malformed)
        }
        _ => Ok(()),
    }
}
fn partitioned(organization: mainframe_env_host_api::DatasetOrganization) -> bool {
    matches!(
        organization,
        mainframe_env_host_api::DatasetOrganization::Partitioned
            | mainframe_env_host_api::DatasetOrganization::PartitionedExtended
    )
}
fn relative(organization: mainframe_env_host_api::DatasetOrganization) -> bool {
    matches!(
        organization,
        mainframe_env_host_api::DatasetOrganization::Relative
            | mainframe_env_host_api::DatasetOrganization::VariableRelative
    )
}
fn validate_records(
    records: &[Vec<u8>],
    attributes: &mainframe_env_host_api::DatasetAttributes,
    limits: DatasetLimits,
) -> Result<(), HostProblem> {
    if records.len() > limits.max_records
        || records
            .iter()
            .any(|record| record.len() > limits.max_record_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    if matches!(
        attributes.record_format,
        mainframe_env_host_api::RecordFormat::Fixed
            | mainframe_env_host_api::RecordFormat::FixedBlocked
            | mainframe_env_host_api::RecordFormat::FixedBlockedStandard
    ) && records
        .iter()
        .any(|record| record.len() != attributes.logical_record_length as usize)
    {
        return Err(condition("LENGERR", 22));
    }
    if matches!(
        attributes.record_format,
        mainframe_env_host_api::RecordFormat::Variable
            | mainframe_env_host_api::RecordFormat::VariableBlocked
            | mainframe_env_host_api::RecordFormat::VariableSpanned
            | mainframe_env_host_api::RecordFormat::VariableBlockedSpanned
            | mainframe_env_host_api::RecordFormat::Line
    ) && records
        .iter()
        .any(|record| record.len() > attributes.logical_record_length as usize)
    {
        return Err(condition("LENGERR", 22));
    }
    if attributes.record_format == mainframe_env_host_api::RecordFormat::Line
        && records
            .iter()
            .any(|record| record.contains(&b'\n') || record.contains(&b'\r'))
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}
fn wildcard(pattern: &str, value: &str) -> bool {
    let pattern = pattern.to_ascii_uppercase();
    if pattern == "*" || pattern == "**" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".**") {
        return value.starts_with(prefix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return value.starts_with(prefix);
    }
    pattern == value
}
fn bytes(entry: &Entry) -> usize {
    entry.records.iter().map(Vec::len).sum::<usize>()
        + entry
            .members
            .values()
            .flatten()
            .map(Vec::len)
            .sum::<usize>()
        + entry.relative_records.values().map(Vec::len).sum::<usize>()
        + entry
            .member_generations
            .values()
            .flatten()
            .flat_map(|generation| &generation.records)
            .map(Vec::len)
            .sum::<usize>()
}
fn allocated_bytes(entry: &Entry) -> Result<u64, HostProblem> {
    Ok(allocation_geometry(entry)?.allocated_bytes)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AllocationGeometry {
    allocated_bytes: u64,
    extents: Vec<mainframe_env_host_api::DatasetExtent>,
}

fn allocation_unit_bytes(entry: &Entry) -> u64 {
    match entry.allocation.unit {
        mainframe_env_host_api::SpaceUnit::Tracks => 56_664,
        mainframe_env_host_api::SpaceUnit::Cylinders => 849_960,
        mainframe_env_host_api::SpaceUnit::Blocks => u64::from(if entry.dcb.block_size == 0 {
            entry.attributes.logical_record_length
        } else {
            entry.dcb.block_size
        }),
        mainframe_env_host_api::SpaceUnit::Kilobytes => 1024,
        mainframe_env_host_api::SpaceUnit::Megabytes => 1024 * 1024,
        mainframe_env_host_api::SpaceUnit::Records => {
            u64::from(entry.attributes.logical_record_length)
        }
    }
}

fn allocation_geometry(entry: &Entry) -> Result<AllocationGeometry, HostProblem> {
    let unit = allocation_unit_bytes(entry);
    let primary = entry
        .allocation
        .primary
        .checked_mul(unit)
        .ok_or(HostProblem::ResourceExhausted)?;
    let secondary = entry
        .allocation
        .secondary
        .checked_mul(unit)
        .ok_or(HostProblem::ResourceExhausted)?;
    let nominal = primary
        .checked_add(secondary)
        .ok_or(HostProblem::ResourceExhausted)?;
    let mut allocated = if entry.allocation.release_unused
        && entry.lifecycle.state == mainframe_env_host_api::DatasetLifecycleState::Closed
    {
        let used = u64::try_from(bytes(entry)).map_err(|_| HostProblem::ResourceExhausted)?;
        let required = used.div_ceil(unit).saturating_mul(unit);
        primary.max(required).min(nominal)
    } else {
        nominal
    };
    if entry.allocation.round_to_cylinder {
        allocated = allocated
            .div_ceil(849_960)
            .checked_mul(849_960)
            .ok_or(HostProblem::ResourceExhausted)?;
    }
    let volume_ids = &entry.volumes.volume_ids;
    let extent_lengths = if entry.allocation.contiguous || secondary == 0 || allocated <= primary {
        vec![allocated]
    } else {
        vec![primary, allocated - primary]
    };
    let mut start = 0u64;
    let extents = extent_lengths
        .into_iter()
        .enumerate()
        .map(|(ordinal, length)| {
            let extent = mainframe_env_host_api::DatasetExtent {
                ordinal: u32::try_from(ordinal).map_err(|_| HostProblem::ResourceExhausted)?,
                start,
                volume_start: 0,
                length,
                volume_id: volume_ids
                    .get(ordinal % volume_ids.len())
                    .cloned()
                    .ok_or(HostProblem::InfrastructureFailure)?,
            };
            start = start
                .checked_add(length)
                .ok_or(HostProblem::ResourceExhausted)?;
            Ok(extent)
        })
        .collect::<Result<Vec<_>, HostProblem>>()?;
    Ok(AllocationGeometry {
        allocated_bytes: allocated,
        extents,
    })
}

fn abstract_volume_descriptions(
    state: &State,
) -> Result<Vec<mainframe_env_host_api::DatasetVolumeDescription>, HostProblem> {
    let mut volumes = BTreeMap::<String, mainframe_env_host_api::DatasetVolumeDescription>::new();
    for (dataset_name, entry) in &state.entries {
        let dataset =
            DatasetName::new(dataset_name, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
        let used_bytes = u64::try_from(bytes(entry)).map_err(|_| HostProblem::ResourceExhausted)?;
        for extent in allocation_geometry(entry)?.extents {
            let volume = volumes.entry(extent.volume_id.clone()).or_insert_with(|| {
                mainframe_env_host_api::DatasetVolumeDescription {
                    volume_id: extent.volume_id.clone(),
                    allocated_bytes: 0,
                    used_bytes: 0,
                    extents: Vec::new(),
                }
            });
            let used_in_extent = used_bytes.saturating_sub(extent.start).min(extent.length);
            let volume_start = volume.allocated_bytes;
            volume.allocated_bytes = volume
                .allocated_bytes
                .checked_add(extent.length)
                .ok_or(HostProblem::ResourceExhausted)?;
            volume.used_bytes = volume
                .used_bytes
                .checked_add(used_in_extent)
                .ok_or(HostProblem::ResourceExhausted)?;
            volume
                .extents
                .push(mainframe_env_host_api::DatasetVolumeExtent {
                    dataset: dataset.clone(),
                    dataset_extent_ordinal: extent.ordinal,
                    logical_start: extent.start,
                    volume_start,
                    length: extent.length,
                });
        }
    }
    Ok(volumes.into_values().collect())
}

fn validate_guaranteed_volume_capacity(
    state: &State,
    dataset: &str,
    next: &Entry,
    limits: DatasetLimits,
) -> Result<(), HostProblem> {
    if !next.sms.guaranteed_space {
        return Ok(());
    }
    let mut reserved = BTreeMap::<String, u64>::new();
    for (name, entry) in &state.entries {
        if name == dataset || !entry.sms.guaranteed_space {
            continue;
        }
        for extent in allocation_geometry(entry)?.extents {
            let total = reserved.entry(extent.volume_id).or_default();
            *total = total
                .checked_add(extent.length)
                .ok_or(HostProblem::ResourceExhausted)?;
        }
    }
    for extent in allocation_geometry(next)?.extents {
        let total = reserved.entry(extent.volume_id).or_default();
        *total = total
            .checked_add(extent.length)
            .ok_or(HostProblem::ResourceExhausted)?;
        if usize::try_from(*total).map_or(true, |total| total > limits.max_total_bytes) {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(())
}

fn validate_loaded_guaranteed_volume_capacity(
    entries: &BTreeMap<String, Entry>,
    limits: DatasetLimits,
) -> Result<(), HostProblem> {
    let mut reserved = BTreeMap::<String, u64>::new();
    for entry in entries.values().filter(|entry| entry.sms.guaranteed_space) {
        for extent in allocation_geometry(entry)?.extents {
            let total = reserved.entry(extent.volume_id).or_default();
            *total = total
                .checked_add(extent.length)
                .ok_or(HostProblem::ResourceExhausted)?;
            if usize::try_from(*total).map_or(true, |total| total > limits.max_total_bytes) {
                return Err(HostProblem::ResourceExhausted);
            }
        }
    }
    Ok(())
}

fn buffer_bytes(entry: &Entry) -> Result<u64, HostProblem> {
    let size = entry.dcb.buffer_size.unwrap_or({
        if entry.dcb.block_size == 0 {
            entry.attributes.logical_record_length
        } else {
            entry.dcb.block_size
        }
    });
    let sets = match entry.vsam.buffering {
        mainframe_env_host_api::BufferingMode::LocalSharedResources => {
            u64::from(entry.volumes.unit_count)
        }
        mainframe_env_host_api::BufferingMode::System
        | mainframe_env_host_api::BufferingMode::NonsharedResources
        | mainframe_env_host_api::BufferingMode::GlobalSharedResources => 1,
    };
    u64::from(size)
        .checked_mul(u64::from(entry.dcb.buffer_count))
        .and_then(|value| value.checked_mul(sets))
        .ok_or(HostProblem::ResourceExhausted)
}

fn abstract_placement(entry: &Entry) -> String {
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.abstract-placement@1");
    for value in [
        entry.sms.data_class.as_deref(),
        entry.sms.management_class.as_deref(),
        entry.sms.storage_class.as_deref(),
    ] {
        digest.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
    }
    digest.update([
        u8::from(entry.sms.guaranteed_space),
        u8::from(entry.sms.extended_format),
        u8::from(entry.sms.extended_addressable),
    ]);
    digest.update(entry.volumes.unit_count.to_be_bytes());
    let identity = digest.finalize()[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("ABSTRACT:{identity}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DatasetGeometry {
    control_intervals: u64,
    control_areas: u64,
    high_used_rba: u64,
}

fn dataset_geometry(entry: &Entry) -> Result<DatasetGeometry, HostProblem> {
    let high_used_rba = u64::try_from(bytes(entry)).map_err(|_| HostProblem::ResourceExhausted)?;
    let is_vsam = matches!(
        entry.attributes.organization,
        mainframe_env_host_api::DatasetOrganization::KeySequenced
            | mainframe_env_host_api::DatasetOrganization::EntrySequenced
            | mainframe_env_host_api::DatasetOrganization::Relative
            | mainframe_env_host_api::DatasetOrganization::VariableRelative
            | mainframe_env_host_api::DatasetOrganization::Linear
    );
    if !is_vsam || high_used_rba == 0 {
        return Ok(DatasetGeometry {
            control_intervals: 0,
            control_areas: 0,
            high_used_rba,
        });
    }
    let ci_size = u64::from(entry.vsam.control_interval_size.unwrap_or(4096));
    let usable = ci_size.checked_sub(7).ok_or(HostProblem::Malformed)?;
    let ca_size = entry.vsam.control_area_size.unwrap_or(
        ci_size
            .checked_mul(16)
            .ok_or(HostProblem::ResourceExhausted)?,
    );
    if ca_size < ci_size || !ca_size.is_multiple_of(ci_size) {
        return Err(HostProblem::Malformed);
    }
    let control_intervals =
        if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::Linear {
            high_used_rba.div_ceil(usable)
        } else {
            let records = if relative(entry.attributes.organization) {
                entry.relative_records.values().collect::<Vec<_>>()
            } else {
                entry.records.iter().collect::<Vec<_>>()
            };
            let mut intervals = 0u64;
            let mut remaining = 0u64;
            for record in records {
                let required = u64::try_from(record.len())
                    .map_err(|_| HostProblem::ResourceExhausted)?
                    .checked_add(3)
                    .ok_or(HostProblem::ResourceExhausted)?;
                if required > usable {
                    if !entry.vsam.spanned {
                        return Err(condition("LENGERR", 22));
                    }
                    intervals = intervals
                        .checked_add(required.div_ceil(usable))
                        .ok_or(HostProblem::ResourceExhausted)?;
                    remaining = 0;
                } else {
                    if remaining < required {
                        intervals = intervals
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?;
                        remaining = usable;
                    }
                    remaining -= required;
                }
            }
            intervals
        };
    let intervals_per_area = ca_size / ci_size;
    Ok(DatasetGeometry {
        control_intervals,
        control_areas: control_intervals.div_ceil(intervals_per_area),
        high_used_rba,
    })
}

fn lifecycle_transition_allowed(
    current: mainframe_env_host_api::DatasetLifecycleState,
    next: mainframe_env_host_api::DatasetLifecycleState,
) -> bool {
    use mainframe_env_host_api::DatasetLifecycleState as State;
    current == next
        || matches!(
            (current, next),
            (State::Allocated, State::Cataloged)
                | (State::Cataloged | State::Closed, State::Open)
                | (State::Open, State::Closed | State::RecoveryRequired)
                | (State::Cataloged | State::Closed, State::Migrated)
                | (State::Migrated, State::RecallPending)
                | (
                    State::RecallPending | State::RecoveryRequired,
                    State::Closed
                )
        )
}

fn enforce_delete_policy(
    catalog: &mainframe_env_host_api::CatalogMetadata,
    purge: bool,
    current_date: Option<u32>,
) -> Result<(), HostProblem> {
    if purge {
        return Ok(());
    }
    let protected_until = if let Some(expiration) = catalog.expiration_date {
        Some(julian_ordinal(expiration)?)
    } else if let (Some(created), Some(retention)) = (catalog.creation_date, catalog.retention_days)
    {
        Some(
            julian_ordinal(created)?
                .checked_add(i64::from(retention))
                .ok_or(HostProblem::ResourceExhausted)?,
        )
    } else {
        None
    };
    if let Some(protected_until) = protected_until
        && current_date
            .map(julian_ordinal)
            .transpose()?
            .is_none_or(|current| current < protected_until)
    {
        Err(condition("PROTECTED", 8))
    } else {
        Ok(())
    }
}

fn julian_ordinal(date: u32) -> Result<i64, HostProblem> {
    let year = i64::from(date / 1000);
    let day = i64::from(date % 1000);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    if !(1900..=9999).contains(&year) || day == 0 || day > if leap { 366 } else { 365 } {
        return Err(HostProblem::Malformed);
    }
    let prior = year - 1;
    let days = prior
        .checked_mul(365)
        .and_then(|value| value.checked_add(prior / 4 - prior / 100 + prior / 400))
        .and_then(|value| value.checked_add(day))
        .ok_or(HostProblem::ResourceExhausted)?;
    Ok(days)
}
fn dependency_limits(limits: DatasetLimits) -> DependencyLimits {
    DependencyLimits {
        max_nodes: limits.max_datasets,
        max_edges: limits.max_datasets.saturating_mul(8),
        max_depth: 128.min(limits.max_datasets.max(1)),
    }
}
fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}

fn dataset_capabilities() -> mainframe_env_host_api::DatasetProviderCapabilities {
    let mut capabilities =
        mainframe_env_host_api::DatasetProviderCapabilities::deterministic_abstract();
    capabilities.allocation_extents = true;
    capabilities.buffering = true;
    capabilities.catalog_metadata = true;
    capabilities.extended_format = true;
    capabilities.rls = true;
    capabilities.sharing = true;
    capabilities.sms_classes = true;
    capabilities.tvs = true;
    capabilities
}

fn unavailable_capability_diagnostics() -> Vec<mainframe_env_host_api::DatasetDiagnostic> {
    let capabilities = dataset_capabilities();
    [
        (
            !capabilities.physical_volumes,
            "physical-volumes",
            "VOLUME/UNIT requires a physical-volume adapter",
        ),
        (
            !capabilities.tape,
            "tape",
            "TAPE/LIBRARYENTRY/VOLUMEENTRY requires a tape adapter",
        ),
        (
            !capabilities.sms_acs,
            "sms-acs",
            "ACSROUTINE requires an SMS ACS adapter",
        ),
        (
            !capabilities.encryption,
            "encryption",
            "KEYLABEL requires an encryption adapter",
        ),
        (
            !capabilities.compression,
            "compression",
            "COMPRESS requires a compression adapter",
        ),
        (
            !capabilities.striping,
            "striping",
            "STRIPECOUNT requires a striping adapter",
        ),
        (
            !capabilities.migration_recall,
            "migration-recall",
            "MIGRATE/RECALL requires an artifact-backed lifecycle adapter",
        ),
        (
            !capabilities.vsam_data_options,
            "vsam-data-options",
            "REUSE/SPEED/WRITECHECK/ERASE requires VSAM data-option semantics",
        ),
    ]
    .into_iter()
    .filter(|(unavailable, _, _)| *unavailable)
    .map(
        |(_, capability, detail)| mainframe_env_host_api::DatasetDiagnostic {
            code: "UNAVAILABLE_CAPABILITY".into(),
            field: Some(format!("provider-capabilities.{capability}")),
            detail: detail.into(),
        },
    )
    .collect()
}

fn validate_provider_definition(
    definition: &mainframe_env_host_api::DatasetDefinition,
) -> Result<(), HostProblem> {
    if definition.vsam.share_options.cross_region > 2
        || definition.vsam.share_options.cross_system != 3
    {
        Err(HostProblem::UnsupportedCapability {
            capability: "sharing".into(),
            detail: "this deterministic provider implements SHAREOPTIONS (1,3) and (2,3)".into(),
        })
    } else {
        Ok(())
    }
}

fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::Conflict => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

fn request_digest(request: &DatasetRequest) -> Result<[u8; 32], HostProblem> {
    let mut digest = Sha256::new();
    digest_field(&mut digest, b"mainframe-env.dataset-request-digest@2");
    match request {
        DatasetRequest::Capabilities => digest_field(&mut digest, b"capabilities"),
        DatasetRequest::List {
            pattern,
            start,
            max_items,
        } => {
            digest_field(&mut digest, b"list");
            digest_field(&mut digest, pattern.as_bytes());
            digest_optional_name(&mut digest, start.as_ref());
            digest_field(&mut digest, &max_items.to_be_bytes());
        }
        DatasetRequest::Attributes { dataset }
        | DatasetRequest::Describe { dataset }
        | DatasetRequest::Diagnose { dataset } => {
            let tag = match request {
                DatasetRequest::Attributes { .. } => b"attributes".as_slice(),
                DatasetRequest::Describe { .. } => b"describe".as_slice(),
                _ => b"diagnose".as_slice(),
            };
            digest_field(&mut digest, tag);
            digest_field(&mut digest, dataset.as_str().as_bytes());
        }
        DatasetRequest::ResolveCatalog { name } => {
            digest_field(&mut digest, b"resolve-catalog");
            digest_field(&mut digest, name.as_str().as_bytes());
        }
        DatasetRequest::ListCatalog {
            pattern,
            start,
            max_items,
        } => {
            digest_field(&mut digest, b"list-catalog");
            digest_field(&mut digest, pattern.as_bytes());
            digest_optional_name(&mut digest, start.as_ref());
            digest_field(&mut digest, &max_items.to_be_bytes());
        }
        DatasetRequest::ListVolumes { start, max_items } => {
            digest_field(&mut digest, b"list-volumes");
            digest_optional_bytes(&mut digest, start.as_deref().map(str::as_bytes));
            digest_field(&mut digest, &max_items.to_be_bytes());
        }
        DatasetRequest::ListLocks {
            dataset,
            now_tick,
            max_items,
        } => {
            digest_field(&mut digest, b"list-locks");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &now_tick.to_be_bytes());
            digest_field(&mut digest, &max_items.to_be_bytes());
        }
        DatasetRequest::TvsStatus { transaction, owner } => {
            digest_field(&mut digest, b"tvs-status");
            digest_field(&mut digest, transaction.as_bytes());
            digest_field(&mut digest, owner.as_str().as_bytes());
        }
        DatasetRequest::ListMembers {
            dataset,
            start,
            max_items,
        } => {
            digest_field(&mut digest, b"list-members");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_member(&mut digest, start.as_ref());
            digest_field(&mut digest, &max_items.to_be_bytes());
        }
        DatasetRequest::ReadMemberGeneration {
            dataset,
            member,
            relative,
            max_records,
        } => {
            digest_field(&mut digest, b"read-member-generation");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, member.as_str().as_bytes());
            digest_field(&mut digest, &relative.to_be_bytes());
            digest_field(&mut digest, &max_records.to_be_bytes());
        }
        DatasetRequest::Read {
            dataset,
            member,
            key,
            max_records,
        } => {
            digest_field(&mut digest, b"read");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_member(&mut digest, member.as_ref());
            digest_optional_bytes(&mut digest, key.as_deref());
            digest_field(&mut digest, &max_records.to_be_bytes());
        }
        DatasetRequest::ReadGeneric {
            dataset,
            key_prefix,
            max_records,
        } => {
            digest_field(&mut digest, b"read-generic");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, key_prefix);
            digest_field(&mut digest, &max_records.to_be_bytes());
        }
        DatasetRequest::ReadConcatenation {
            datasets,
            member,
            max_records,
        } => {
            digest_field(&mut digest, b"read-concatenation");
            digest_field(&mut digest, &(datasets.len() as u64).to_be_bytes());
            for dataset in datasets {
                digest_field(&mut digest, dataset.as_str().as_bytes());
            }
            digest_optional_member(&mut digest, member.as_ref());
            digest_field(&mut digest, &max_records.to_be_bytes());
        }
        DatasetRequest::ReadRelative {
            dataset,
            record_number,
        } => {
            digest_field(&mut digest, b"read-relative");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &record_number.to_be_bytes());
        }
        DatasetRequest::ReadRba {
            dataset,
            rba,
            max_bytes,
        } => {
            digest_field(&mut digest, b"read-rba");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &rba.to_be_bytes());
            digest_field(&mut digest, &max_bytes.to_be_bytes());
        }
        DatasetRequest::ReadSequential {
            dataset,
            member,
            start,
            reverse,
            max_records,
        } => {
            digest_field(&mut digest, b"read-sequential");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_member(&mut digest, member.as_ref());
            digest_optional_u64(&mut digest, *start);
            digest_field(&mut digest, &[u8::from(*reverse)]);
            digest_field(&mut digest, &max_records.to_be_bytes());
        }
        DatasetRequest::Snapshot {
            dataset,
            max_records,
            max_members,
        } => {
            digest_field(&mut digest, b"snapshot");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &max_records.to_be_bytes());
            digest_field(&mut digest, &max_members.to_be_bytes());
        }
        DatasetRequest::Create {
            dataset,
            attributes,
            mutation,
        } => {
            digest_field(&mut digest, b"create");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_definition(
                &mut digest,
                &mainframe_env_host_api::DatasetDefinition::compatibility(attributes.clone()),
            )?;
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::Define {
            dataset,
            definition,
            mutation,
        } => {
            digest_field(&mut digest, b"define");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_definition(&mut digest, definition)?;
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::Alter {
            dataset,
            definition,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"alter");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_definition(&mut digest, definition)?;
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::SetLifecycle {
            dataset,
            state,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"set-lifecycle");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &[lifecycle_digest_tag(*state)]);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::RecordBackup {
            dataset,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"record-backup");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::Restore {
            dataset,
            snapshot,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"restore");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_snapshot(&mut digest, snapshot)?;
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DefineCatalog {
            catalog,
            kind,
            mutation,
        } => {
            digest_field(&mut digest, b"define-catalog");
            digest_field(&mut digest, catalog.as_str().as_bytes());
            digest_field(
                &mut digest,
                &[match kind {
                    mainframe_env_host_api::CatalogKind::Master => 0,
                    mainframe_env_host_api::CatalogKind::User => 1,
                }],
            );
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::SetCatalogConnection {
            catalog,
            connected,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"set-catalog-connection");
            digest_field(&mut digest, catalog.as_str().as_bytes());
            digest_field(&mut digest, &[u8::from(*connected)]);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DefineAlias {
            alias,
            target,
            mutation,
        } => {
            digest_field(&mut digest, b"define-alias");
            digest_field(&mut digest, alias.as_str().as_bytes());
            digest_field(&mut digest, target.as_str().as_bytes());
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DefineMemberAlias {
            dataset,
            alias,
            target,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"define-member-alias");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, alias.as_str().as_bytes());
            digest_field(&mut digest, target.as_str().as_bytes());
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::WriteMemberGeneration {
            dataset,
            member,
            records,
            program_object,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"write-member-generation");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, member.as_str().as_bytes());
            digest_records(&mut digest, records);
            digest_field(&mut digest, &[u8::from(*program_object)]);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DeleteMemberGeneration {
            dataset,
            member,
            generation,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"delete-member-generation");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, member.as_str().as_bytes());
            digest_field(&mut digest, &generation.to_be_bytes());
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::AcquireLock {
            dataset,
            target,
            owner,
            mode,
            now_tick,
            lease_ticks,
            transaction,
            mutation,
        } => {
            digest_field(&mut digest, b"acquire-lock");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_lock_target(&mut digest, target);
            digest_field(&mut digest, owner.as_str().as_bytes());
            digest_field(&mut digest, &[lock_mode_tag(*mode)]);
            digest_field(&mut digest, &now_tick.to_be_bytes());
            digest_field(&mut digest, &lease_ticks.to_be_bytes());
            digest_optional_bytes(&mut digest, transaction.as_deref().map(str::as_bytes));
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::ReleaseLock {
            dataset,
            lock_id,
            owner,
            mutation,
        } => {
            digest_field(&mut digest, b"release-lock");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, lock_id.as_bytes());
            digest_field(&mut digest, owner.as_str().as_bytes());
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::BeginTvs {
            transaction,
            owner,
            mutation,
        } => {
            digest_field(&mut digest, b"begin-tvs");
            digest_field(&mut digest, transaction.as_bytes());
            digest_field(&mut digest, owner.as_str().as_bytes());
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::StageTvs {
            transaction,
            owner,
            operation,
            mutation,
        } => {
            digest_field(&mut digest, b"stage-tvs");
            digest_field(&mut digest, transaction.as_bytes());
            digest_field(&mut digest, owner.as_str().as_bytes());
            digest_tvs_operation(&mut digest, operation);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::CompleteTvs {
            transaction,
            owner,
            commit,
            mutation,
        } => {
            digest_field(&mut digest, b"complete-tvs");
            digest_field(&mut digest, transaction.as_bytes());
            digest_field(&mut digest, owner.as_str().as_bytes());
            digest_field(&mut digest, &[u8::from(*commit)]);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::ReconcileTvs {
            transaction,
            owner,
            committed,
            mutation,
        } => {
            digest_field(&mut digest, b"reconcile-tvs");
            digest_field(&mut digest, transaction.as_bytes());
            digest_field(&mut digest, owner.as_str().as_bytes());
            digest_field(&mut digest, &[u8::from(*committed)]);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::Write {
            dataset,
            member,
            records,
            expected_version,
            mutation,
        }
        | DatasetRequest::Append {
            dataset,
            member,
            records,
            expected_version,
            mutation,
        } => {
            digest_field(
                &mut digest,
                if matches!(request, DatasetRequest::Write { .. }) {
                    b"write"
                } else {
                    b"append"
                },
            );
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_member(&mut digest, member.as_ref());
            digest_records(&mut digest, records);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::Truncate {
            dataset,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"truncate");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::RewriteRecord {
            dataset,
            key,
            record,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"rewrite-record");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, key);
            digest_field(&mut digest, record);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DeleteRecord {
            dataset,
            key,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"delete-record");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, key);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::WriteRelative {
            dataset,
            record_number,
            record,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"write-relative");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &record_number.to_be_bytes());
            digest_field(&mut digest, record);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DeleteRelative {
            dataset,
            record_number,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"delete-relative");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &record_number.to_be_bytes());
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::WriteRba {
            dataset,
            rba,
            data,
            expected_version,
            mutation,
        } => {
            digest_field(&mut digest, b"write-rba");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, &rba.to_be_bytes());
            digest_field(&mut digest, data);
            digest_optional_u64(&mut digest, *expected_version);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DefineAlternateIndex {
            base,
            index,
            key_offset,
            key_length,
            allow_duplicates,
            upgrade,
            mutation,
        } => {
            digest_field(&mut digest, b"define-alternate-index");
            digest_field(&mut digest, base.as_str().as_bytes());
            digest_field(&mut digest, index.as_str().as_bytes());
            digest_field(&mut digest, &key_offset.to_be_bytes());
            digest_field(&mut digest, &key_length.to_be_bytes());
            digest_field(&mut digest, &[u8::from(*allow_duplicates)]);
            digest_field(&mut digest, &[u8::from(*upgrade)]);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::BuildAlternateIndex {
            base,
            index,
            mutation,
        } => {
            digest_field(&mut digest, b"build-alternate-index");
            digest_field(&mut digest, base.as_str().as_bytes());
            digest_field(&mut digest, index.as_str().as_bytes());
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DefinePath {
            path,
            index,
            mutation,
        } => {
            digest_field(&mut digest, b"define-path");
            digest_field(&mut digest, path.as_str().as_bytes());
            digest_field(&mut digest, index.as_str().as_bytes());
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::DefineGenerationGroup {
            base,
            limit,
            scratch,
            empty,
            mutation,
        } => {
            digest_field(&mut digest, b"define-generation-group");
            digest_field(&mut digest, base.as_str().as_bytes());
            digest_field(&mut digest, &limit.to_be_bytes());
            digest_field(&mut digest, &[u8::from(*scratch), u8::from(*empty)]);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::CreateGeneration {
            base,
            attributes,
            records,
            mutation,
        } => {
            digest_field(&mut digest, b"create-generation");
            digest_field(&mut digest, base.as_str().as_bytes());
            digest_definition(
                &mut digest,
                &mainframe_env_host_api::DatasetDefinition::compatibility(attributes.clone()),
            )?;
            digest_records(&mut digest, records);
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::ResolveGeneration { base, relative } => {
            digest_field(&mut digest, b"resolve-generation");
            digest_field(&mut digest, base.as_str().as_bytes());
            digest_field(&mut digest, &relative.to_be_bytes());
        }
        DatasetRequest::Rename { from, to, mutation } => {
            digest_field(&mut digest, b"rename");
            digest_field(&mut digest, from.as_str().as_bytes());
            digest_field(&mut digest, to.as_str().as_bytes());
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::Delete {
            dataset,
            member,
            expected_version,
            purge,
            current_date,
            mutation,
        } => {
            digest_field(&mut digest, b"delete");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_optional_member(&mut digest, member.as_ref());
            digest_optional_u64(&mut digest, *expected_version);
            digest_field(&mut digest, &[u8::from(*purge)]);
            digest_optional_u64(&mut digest, current_date.map(u64::from));
            digest_mutation(&mut digest, mutation);
        }
        DatasetRequest::StartBrowse { dataset, key } => {
            digest_field(&mut digest, b"start-browse");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, key);
        }
        DatasetRequest::ReadNext {
            dataset,
            cursor,
            reverse,
        } => {
            digest_field(&mut digest, b"read-next");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, cursor.as_bytes());
            digest_field(&mut digest, &[u8::from(*reverse)]);
        }
        DatasetRequest::EndBrowse { dataset, cursor } => {
            digest_field(&mut digest, b"end-browse");
            digest_field(&mut digest, dataset.as_str().as_bytes());
            digest_field(&mut digest, cursor.as_bytes());
        }
    }
    Ok(digest.finalize().into())
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn digest_optional_bytes(digest: &mut Sha256, value: Option<&[u8]>) {
    digest_field(digest, &[u8::from(value.is_some())]);
    if let Some(value) = value {
        digest_field(digest, value);
    }
}

fn digest_optional_name(digest: &mut Sha256, value: Option<&DatasetName>) {
    digest_optional_bytes(digest, value.map(|name| name.as_str().as_bytes()));
}

fn digest_optional_member(digest: &mut Sha256, value: Option<&MemberName>) {
    digest_optional_bytes(digest, value.map(|name| name.as_str().as_bytes()));
}

fn digest_optional_u64(digest: &mut Sha256, value: Option<u64>) {
    digest_field(digest, &[u8::from(value.is_some())]);
    if let Some(value) = value {
        digest_field(digest, &value.to_be_bytes());
    }
}

fn digest_records(digest: &mut Sha256, records: &[Vec<u8>]) {
    digest_field(digest, &(records.len() as u64).to_be_bytes());
    for record in records {
        digest_field(digest, record);
    }
}

fn digest_snapshot(digest: &mut Sha256, snapshot: &DatasetSnapshot) -> Result<(), HostProblem> {
    digest_definition(digest, &snapshot.definition)?;
    digest_records(digest, &snapshot.records);
    digest_field(
        digest,
        &(snapshot.relative_records.len() as u64).to_be_bytes(),
    );
    for relative in &snapshot.relative_records {
        digest_field(digest, &relative.record_number.to_be_bytes());
        digest_field(digest, &relative.record);
    }
    digest_field(digest, &(snapshot.members.len() as u64).to_be_bytes());
    for member in &snapshot.members {
        digest_field(digest, member.name.as_str().as_bytes());
        digest_records(digest, &member.records);
        digest_field(digest, &(member.generations.len() as u64).to_be_bytes());
        for generation in &member.generations {
            digest_field(digest, &generation.generation.to_be_bytes());
            digest_field(digest, &[u8::from(generation.program_object)]);
            digest_records(digest, &generation.records);
        }
        digest_optional_member(digest, member.alias_of.as_ref());
    }
    digest_field(digest, &snapshot.linear_data);
    Ok(())
}

fn digest_lock_target(digest: &mut Sha256, target: &mainframe_env_host_api::DatasetLockTarget) {
    match target {
        mainframe_env_host_api::DatasetLockTarget::Dataset => digest_field(digest, &[0]),
        mainframe_env_host_api::DatasetLockTarget::Record(identity) => {
            digest_field(digest, &[1]);
            digest_field(digest, identity);
        }
    }
}

const fn lock_mode_tag(mode: mainframe_env_host_api::DatasetLockMode) -> u8 {
    match mode {
        mainframe_env_host_api::DatasetLockMode::Shared => 0,
        mainframe_env_host_api::DatasetLockMode::Update => 1,
        mainframe_env_host_api::DatasetLockMode::Exclusive => 2,
    }
}

fn digest_tvs_operation(
    digest: &mut Sha256,
    operation: &mainframe_env_host_api::TvsRecordOperation,
) {
    match operation {
        mainframe_env_host_api::TvsRecordOperation::Insert { dataset, record } => {
            digest_field(digest, &[0]);
            digest_field(digest, dataset.as_str().as_bytes());
            digest_field(digest, record);
        }
        mainframe_env_host_api::TvsRecordOperation::Rewrite {
            dataset,
            key,
            record,
        } => {
            digest_field(digest, &[1]);
            digest_field(digest, dataset.as_str().as_bytes());
            digest_field(digest, key);
            digest_field(digest, record);
        }
        mainframe_env_host_api::TvsRecordOperation::Delete { dataset, key } => {
            digest_field(digest, &[2]);
            digest_field(digest, dataset.as_str().as_bytes());
            digest_field(digest, key);
        }
    }
}

fn digest_definition(
    digest: &mut Sha256,
    definition: &mainframe_env_host_api::DatasetDefinition,
) -> Result<(), HostProblem> {
    let bytes =
        encode_definition_digest_v2(definition).map_err(|_| HostProblem::ResourceExhausted)?;
    digest_field(digest, &bytes);
    if definition.vsam.access_mode != mainframe_env_host_api::VsamAccessMode::NonRls {
        digest_field(
            digest,
            &[
                0x56,
                match definition.vsam.access_mode {
                    mainframe_env_host_api::VsamAccessMode::NonRls => 0,
                    mainframe_env_host_api::VsamAccessMode::Rls => 1,
                    mainframe_env_host_api::VsamAccessMode::Tvs => 2,
                },
            ],
        );
    }
    Ok(())
}

fn digest_mutation(digest: &mut Sha256, mutation: &mainframe_env_host_api::Mutation) {
    digest_field(digest, &mutation.sequence.to_be_bytes());
    digest_field(digest, mutation.idempotency_key.as_str().as_bytes());
    digest_optional_bytes(digest, mutation.transaction.as_deref().map(str::as_bytes));
}

fn lifecycle_digest_tag(state: mainframe_env_host_api::DatasetLifecycleState) -> u8 {
    match state {
        mainframe_env_host_api::DatasetLifecycleState::Allocated => 0,
        mainframe_env_host_api::DatasetLifecycleState::Cataloged => 1,
        mainframe_env_host_api::DatasetLifecycleState::Open => 2,
        mainframe_env_host_api::DatasetLifecycleState::Closed => 3,
        mainframe_env_host_api::DatasetLifecycleState::Migrated => 4,
        mainframe_env_host_api::DatasetLifecycleState::RecallPending => 5,
        mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired => 6,
    }
}

fn encode_replay(replay: &Replay) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MEDR1".to_vec();
    payload.extend_from_slice(&replay.request_digest);
    match &replay.result {
        None => payload.push(0),
        Some(DatasetResult::Created { version }) => {
            payload.push(1);
            payload.extend_from_slice(&version.to_be_bytes());
        }
        Some(DatasetResult::Mutated { version }) => {
            payload.push(2);
            payload.extend_from_slice(&version.to_be_bytes());
        }
        Some(DatasetResult::Generation {
            dataset,
            absolute_generation,
            version,
        }) => {
            payload.push(3);
            dataset_field(&mut payload, dataset.as_str().as_bytes())?;
            payload.extend_from_slice(&absolute_generation.to_be_bytes());
            payload.extend_from_slice(&version.to_be_bytes());
        }
        Some(DatasetResult::Locks { locks }) => {
            payload.push(4);
            payload.extend_from_slice(
                &u32::try_from(locks.len())
                    .map_err(|_| HostProblem::ResourceExhausted)?
                    .to_be_bytes(),
            );
            for lock in locks {
                payload.extend_from_slice(&lock.version.to_be_bytes());
                dataset_field(&mut payload, &encode_lock(lock)?)?;
            }
        }
        Some(DatasetResult::Tvs(receipt)) => {
            payload.push(5);
            dataset_field(&mut payload, receipt.transaction.as_bytes())?;
            dataset_field(&mut payload, receipt.owner.as_str().as_bytes())?;
            payload.push(match receipt.state {
                mainframe_env_host_api::TvsUnitOfWorkState::Active => 0,
                mainframe_env_host_api::TvsUnitOfWorkState::Committed => 1,
                mainframe_env_host_api::TvsUnitOfWorkState::RolledBack => 2,
                mainframe_env_host_api::TvsUnitOfWorkState::Unknown => 3,
            });
            payload.extend_from_slice(&receipt.staged_operations.to_be_bytes());
            payload.extend_from_slice(&receipt.version.to_be_bytes());
        }
        Some(_) => return Err(HostProblem::InfrastructureFailure),
    }
    Ok(payload)
}

fn replay_mutation(
    mutation: &mainframe_env_host_api::Mutation,
    replay: &Replay,
) -> Result<ProviderStateMutation, HostProblem> {
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "dataset-replay".into(),
            key: mutation.idempotency_key.as_str().into(),
            version: 2,
            payload: encode_replay(replay)?,
        },
        expected_version: Some(1),
    }))
}

fn decode_replay(payload: &[u8]) -> Result<Replay, ()> {
    if payload.len() < 38 || payload.get(..5) != Some(b"MEDR1") {
        return Err(());
    }
    let request_digest = payload.get(5..37).ok_or(())?.try_into().map_err(|_| ())?;
    let result = match payload[37] {
        0 if payload.len() == 38 => None,
        tag @ (1 | 2) if payload.len() == 46 => {
            let version = u64::from_be_bytes(payload[38..46].try_into().map_err(|_| ())?);
            Some(if tag == 1 {
                DatasetResult::Created { version }
            } else {
                DatasetResult::Mutated { version }
            })
        }
        3 => {
            let mut at = 38usize;
            let length = usize::try_from(u32::from_be_bytes(
                payload
                    .get(at..at + 4)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            ))
            .map_err(|_| ())?;
            at += 4;
            if length == 0 || length > 128 {
                return Err(());
            }
            let dataset = String::from_utf8(payload.get(at..at + length).ok_or(())?.to_vec())
                .map_err(|_| ())?;
            at += length;
            let absolute_generation = u32::from_be_bytes(
                payload
                    .get(at..at + 4)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            );
            at += 4;
            let version = u64::from_be_bytes(
                payload
                    .get(at..at + 8)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            );
            at += 8;
            if at != payload.len() {
                return Err(());
            }
            Some(DatasetResult::Generation {
                dataset: DatasetName::new(dataset, 128).map_err(|_| ())?,
                absolute_generation,
                version,
            })
        }
        4 => {
            let mut at = 38usize;
            let count = usize::try_from(u32::from_be_bytes(
                payload
                    .get(at..at + 4)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            ))
            .map_err(|_| ())?;
            at += 4;
            if count > 4096 {
                return Err(());
            }
            let mut locks = Vec::with_capacity(count);
            for _ in 0..count {
                let version = u64::from_be_bytes(
                    payload
                        .get(at..at + 8)
                        .ok_or(())?
                        .try_into()
                        .map_err(|_| ())?,
                );
                at += 8;
                let encoded = dataset_take_field(payload, &mut at, 1024 * 1024).map_err(|_| ())?;
                locks.push(decode_lock(&encoded, version).map_err(|_| ())?);
            }
            if at != payload.len() {
                return Err(());
            }
            Some(DatasetResult::Locks { locks })
        }
        5 => {
            let mut at = 38usize;
            let transaction =
                String::from_utf8(dataset_take_field(payload, &mut at, 128).map_err(|_| ())?)
                    .map_err(|_| ())?;
            let owner = mainframe_env_execution_api::PrincipalId::new(
                String::from_utf8(dataset_take_field(payload, &mut at, 128).map_err(|_| ())?)
                    .map_err(|_| ())?,
                InvocationLimits::default(),
            )
            .map_err(|_| ())?;
            let state = match payload.get(at) {
                Some(0) => mainframe_env_host_api::TvsUnitOfWorkState::Active,
                Some(1) => mainframe_env_host_api::TvsUnitOfWorkState::Committed,
                Some(2) => mainframe_env_host_api::TvsUnitOfWorkState::RolledBack,
                Some(3) => mainframe_env_host_api::TvsUnitOfWorkState::Unknown,
                _ => return Err(()),
            };
            at += 1;
            let staged_operations = u32::from_be_bytes(
                payload
                    .get(at..at + 4)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            );
            at += 4;
            let version = u64::from_be_bytes(
                payload
                    .get(at..at + 8)
                    .ok_or(())?
                    .try_into()
                    .map_err(|_| ())?,
            );
            at += 8;
            if at != payload.len() || transaction.is_empty() || version == 0 {
                return Err(());
            }
            Some(DatasetResult::Tvs(
                mainframe_env_host_api::TvsUnitOfWorkReceipt {
                    transaction,
                    owner,
                    state,
                    staged_operations,
                    version,
                },
            ))
        }
        _ => return Err(()),
    };
    Ok(Replay {
        request_digest,
        result,
    })
}

struct Provider {
    service: Arc<DatasetService>,
    descriptor: CapabilityDescriptor,
}
impl HostProvider for Provider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, request: EffectRequest) -> EffectResult {
        let sequence = request.sequence;
        let outcome = match request.request {
            HostRequest::Dataset(request) => self
                .service
                .invoke_for_principal(invocation.principal.id(), request)
                .map(HostResult::Dataset),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult { sequence, outcome }
    }
}
pub fn dataset_providers(
    service: Arc<DatasetService>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.dataset.read", "host.dataset.write"]
        .into_iter()
        .map(|capability| {
            Arc::new(Provider {
                service: Arc::clone(&service),
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits).expect("static capability"),
                    provider_id: "mainframe-env-dataset".into(),
                    generation: "1".into(),
                    request_schema: mainframe_env_host_api::DATASET_REQUEST_CONTRACT.into(),
                    result_schema: mainframe_env_host_api::DATASET_RESULT_CONTRACT.into(),
                    max_request_bytes: 1024 * 1024,
                    max_result_bytes: 1024 * 1024,
                    ready: true,
                },
            }) as Arc<dyn HostProvider>
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::IdempotencyKey;
    use mainframe_env_host_api::{
        DatasetAttributes, DatasetOrganization, HostLimits, Mutation, RecordFormat,
    };
    use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
    use std::sync::atomic::{AtomicBool, Ordering};
    fn mutation(n: u64) -> Mutation {
        Mutation {
            sequence: n,
            idempotency_key: IdempotencyKey::new(format!("id-{n}"), InvocationLimits::default())
                .unwrap(),
            transaction: None,
        }
    }
    fn transaction_mutation(n: u64, transaction: &str) -> Mutation {
        Mutation {
            sequence: n,
            idempotency_key: IdempotencyKey::new(
                format!("id-{n}-{transaction}"),
                InvocationLimits::default(),
            )
            .unwrap(),
            transaction: Some(transaction.into()),
        }
    }
    fn principal(value: &str) -> mainframe_env_execution_api::PrincipalId {
        mainframe_env_execution_api::PrincipalId::new(value, InvocationLimits::default()).unwrap()
    }

    struct FailAtomicOnceStore {
        inner: MemoryStore,
        fail_next: AtomicBool,
    }

    impl FailAtomicOnceStore {
        fn new() -> Self {
            Self {
                inner: MemoryStore::new(Default::default()),
                fail_next: AtomicBool::new(false),
            }
        }

        fn arm(&self) {
            self.fail_next.store(true, Ordering::SeqCst);
        }

        fn fail_now(&self) -> bool {
            self.fail_next.swap(false, Ordering::SeqCst)
        }
    }

    impl ProviderStateStore for FailAtomicOnceStore {
        fn get_provider_state(
            &self,
            namespace: &str,
            key: &str,
        ) -> Result<Option<ProviderStateRecord>, StoreError> {
            self.inner.get_provider_state(namespace, key)
        }

        fn list_provider_state(
            &self,
            namespace: &str,
            max: usize,
        ) -> Result<Vec<ProviderStateRecord>, StoreError> {
            self.inner.list_provider_state(namespace, max)
        }

        fn put_provider_state(
            &self,
            record: ProviderStateRecord,
            expected_version: Option<u64>,
        ) -> Result<(), StoreError> {
            self.inner.put_provider_state(record, expected_version)
        }

        fn delete_provider_state(
            &self,
            namespace: &str,
            key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .delete_provider_state(namespace, key, expected_version)
        }

        fn move_provider_state(
            &self,
            record: ProviderStateRecord,
            old_key: &str,
            expected_version: u64,
        ) -> Result<(), StoreError> {
            self.inner
                .move_provider_state(record, old_key, expected_version)
        }

        fn put_provider_states_atomic(
            &self,
            writes: Vec<ProviderStateWrite>,
        ) -> Result<(), StoreError> {
            if self.fail_now() {
                Err(StoreError::Infrastructure("injected-before-commit".into()))
            } else {
                self.inner.put_provider_states_atomic(writes)
            }
        }

        fn mutate_provider_states_atomic(
            &self,
            mutations: Vec<ProviderStateMutation>,
        ) -> Result<(), StoreError> {
            if self.fail_now() {
                Err(StoreError::Infrastructure("injected-before-commit".into()))
            } else {
                self.inner.mutate_provider_states_atomic(mutations)
            }
        }
    }
    fn service(store: Arc<dyn ProviderStateStore>) -> Arc<DatasetService> {
        DatasetService::open(store, DatasetLimits::default()).unwrap()
    }
    fn attrs(org: DatasetOrganization) -> DatasetAttributes {
        DatasetAttributes {
            organization: org,
            record_format: RecordFormat::Fixed,
            logical_record_length: 4,
            key_offset: (org == DatasetOrganization::KeySequenced).then_some(0),
            key_length: (org == DatasetOrganization::KeySequenced).then_some(2),
            ccsid: Some(37),
        }
    }
    fn seed_object(source: &str, dataset: &str, bytes: &[u8]) -> DatasetSeedObject {
        DatasetSeedObject {
            source_id: source.into(),
            dataset: DatasetName::new(dataset, 128).unwrap(),
            attributes: attrs(DatasetOrganization::Sequential),
            record_length: 4,
            sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
            bytes: bytes.to_vec(),
        }
    }
    #[test]
    fn sequential_create_write_read_and_replay() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.DATA", 44).unwrap();
        assert_eq!(
            service
                .invoke(DatasetRequest::Create {
                    dataset: name.clone(),
                    attributes: attrs(DatasetOrganization::Sequential),
                    mutation: mutation(1)
                })
                .unwrap(),
            DatasetResult::Created { version: 1 }
        );
        let request = DatasetRequest::Write {
            dataset: name.clone(),
            member: None,
            records: vec![b"ABCD".to_vec()],
            expected_version: Some(1),
            mutation: mutation(2),
        };
        let first = service.invoke(request.clone()).unwrap();
        assert_eq!(service.invoke(request).unwrap(), first);
        assert!(
            matches!(service.invoke(DatasetRequest::Read{dataset:name,member:None,key:None,max_records:1}).unwrap(),DatasetResult::Records{records,..}if records==vec![b"ABCD".to_vec()])
        );
    }

    #[test]
    fn typed_definition_capabilities_lifecycle_and_restart_are_exact() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        assert_eq!(
            dataset.invoke(DatasetRequest::Capabilities),
            Ok(DatasetResult::Capabilities {
                capabilities: dataset_capabilities(),
            })
        );

        let name = DatasetName::new("USER.DEFINED", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::Sequential,
        ));
        definition.dcb.block_size = 8;
        definition.allocation.primary = 2;
        assert_eq!(
            dataset.invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition.clone()),
                mutation: mutation(70),
            }),
            Ok(DatasetResult::Created { version: 1 })
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: name.clone()
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition == definition
                    && description.version == 1
                    && description.allocated_bytes == 8
                    && description.used_bytes == 0
        ));
        assert_eq!(
            dataset.invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Open,
                expected_version: Some(1),
                mutation: mutation(71),
            }),
            Ok(DatasetResult::Mutated { version: 2 })
        );
        let restarted = service(store.clone());
        assert!(matches!(
            restarted.invoke(DatasetRequest::Describe {
                dataset: name.clone()
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition.lifecycle.state
                    == mainframe_env_host_api::DatasetLifecycleState::Open
                    && description.version == 2
        ));

        let unsupported = DatasetName::new("USER.TAPE", 44).unwrap();
        let mut tape = definition;
        tape.volumes.kind = mainframe_env_host_api::VolumeKind::Tape;
        assert!(matches!(
            restarted.invoke(DatasetRequest::Define {
                dataset: unsupported.clone(),
                definition: Box::new(tape.clone()),
                mutation: mutation(72),
            }),
            Err(HostProblem::UnsupportedCapability { ref capability, .. })
                if capability == "tape"
        ));
        assert_eq!(
            restarted.invoke(DatasetRequest::Describe {
                dataset: unsupported.clone()
            }),
            Err(HostProblem::NotFound)
        );
        tape.volumes.kind = mainframe_env_host_api::VolumeKind::Abstract;
        assert_eq!(
            restarted.invoke(DatasetRequest::Define {
                dataset: unsupported,
                definition: Box::new(tape),
                mutation: mutation(72),
            }),
            Ok(DatasetResult::Created { version: 1 })
        );
    }

    #[test]
    fn request_digest_is_a_frozen_owned_codec() {
        let name = DatasetName::new("USER.DIGEST", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.dcb.block_size = 8;
        definition.allocation.primary = 3;
        let request = DatasetRequest::Define {
            dataset: name,
            definition: Box::new(definition),
            mutation: mutation(90),
        };
        let digest = request_digest(&request).unwrap();
        let actual = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            actual,
            "1073af8abde37f8c6c11b5aa253c9d8fe8cda29c21565f9db47f4762d96adb29"
        );
        assert_eq!(request_digest(&request).unwrap(), digest);
    }

    #[test]
    fn complete_vsam_organizations_and_access_modes_are_durable() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());

        let esds = DatasetName::new("USER.ESDS", 44).unwrap();
        let esds_attributes = DatasetAttributes {
            organization: DatasetOrganization::EntrySequenced,
            record_format: RecordFormat::Variable,
            logical_record_length: 8,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        };
        dataset
            .invoke(DatasetRequest::Create {
                dataset: esds.clone(),
                attributes: esds_attributes,
                mutation: mutation(100),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: esds.clone(),
                member: None,
                records: vec![b"AA".to_vec(), b"BBB".to_vec(), b"CCCC".to_vec()],
                expected_version: Some(1),
                mutation: mutation(101),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadRba {
                dataset: esds.clone(),
                rba: 2,
                max_bytes: 8,
            }),
            Ok(DatasetResult::Rba {
                ref data,
                record: true,
                rba: 2,
                next_rba: 5,
                version: 2,
            }) if data == b"BBB"
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadRba {
                dataset: esds.clone(),
                rba: 3,
                max_bytes: 8,
            }),
            Err(HostProblem::Condition { ref name, response: 13, .. }) if name == "NOTFND"
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadSequential {
                dataset: esds.clone(),
                member: None,
                start: None,
                reverse: true,
                max_records: 2,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"CCCC".to_vec(), b"BBB".to_vec()]
                    && identities == [5u64.to_be_bytes().to_vec(), 2u64.to_be_bytes().to_vec()]
        ));
        assert_eq!(
            dataset.invoke(DatasetRequest::WriteRba {
                dataset: esds.clone(),
                rba: 2,
                data: b"XYZ".to_vec(),
                expected_version: Some(2),
                mutation: mutation(102),
            }),
            Ok(DatasetResult::Mutated { version: 3 })
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::WriteRba {
                dataset: esds.clone(),
                rba: 2,
                data: b"TOO-LONG".to_vec(),
                expected_version: Some(3),
                mutation: mutation(103),
            }),
            Err(HostProblem::Condition { ref name, response: 22, .. }) if name == "LENGERR"
        ));

        let fixed_rrds = DatasetName::new("USER.RRDS", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: fixed_rrds.clone(),
                attributes: attrs(DatasetOrganization::Relative),
                mutation: mutation(104),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::WriteRelative {
                dataset: fixed_rrds.clone(),
                record_number: 2,
                record: b"F002".to_vec(),
                expected_version: Some(1),
                mutation: mutation(105),
            })
            .unwrap();

        let variable_rrds = DatasetName::new("USER.VRRDS", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: variable_rrds.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::VariableRelative,
                    record_format: RecordFormat::Variable,
                    logical_record_length: 8,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: mutation(106),
            })
            .unwrap();
        for (sequence, rrn, record, version) in [
            (107, 4, b"FOUR".as_slice(), 1),
            (108, 1, b"ONE".as_slice(), 2),
        ] {
            dataset
                .invoke(DatasetRequest::WriteRelative {
                    dataset: variable_rrds.clone(),
                    record_number: rrn,
                    record: record.to_vec(),
                    expected_version: Some(version),
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadSequential {
                dataset: variable_rrds.clone(),
                member: None,
                start: None,
                reverse: false,
                max_records: 8,
            }),
            Ok(DatasetResult::Records { records, identities, version: 3 })
                if records == [b"ONE".to_vec(), b"FOUR".to_vec()]
                    && identities == [1u64.to_be_bytes().to_vec(), 4u64.to_be_bytes().to_vec()]
        ));

        let lds = DatasetName::new("USER.LDS", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: lds.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Linear,
                    record_format: RecordFormat::Undefined,
                    logical_record_length: 1024,
                    key_offset: None,
                    key_length: None,
                    ccsid: None,
                },
                mutation: mutation(109),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::WriteRba {
                dataset: lds.clone(),
                rba: 0,
                data: b"HELLO".to_vec(),
                expected_version: Some(1),
                mutation: mutation(110),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::WriteRba {
                dataset: lds.clone(),
                rba: 5,
                data: b" WORLD".to_vec(),
                expected_version: Some(2),
                mutation: mutation(111),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadRba {
                dataset: lds.clone(),
                rba: 3,
                max_bytes: 5,
            }),
            Ok(DatasetResult::Rba {
                ref data,
                record: false,
                rba: 3,
                next_rba: 8,
                version: 3,
            }) if data == b"LO WO"
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::WriteRba {
                dataset: lds.clone(),
                rba: 20,
                data: b"X".to_vec(),
                expected_version: Some(3),
                mutation: mutation(112),
            }),
            Err(HostProblem::Condition { ref name, response: 13, .. }) if name == "NOTFND"
        ));

        let restarted = service(store.clone());
        assert!(matches!(
            restarted.invoke(DatasetRequest::ReadRba {
                dataset: lds,
                rba: 0,
                max_bytes: 11,
            }),
            Ok(DatasetResult::Rba { ref data, version: 3, .. }) if data == b"HELLO WORLD"
        ));
        assert!(matches!(
            restarted.invoke(DatasetRequest::ReadRelative {
                dataset: fixed_rrds,
                record_number: 2,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"F002".to_vec()]
        ));
        assert!(matches!(
            restarted.invoke(DatasetRequest::ReadRelative {
                dataset: variable_rrds,
                record_number: 4,
            }),
            Ok(DatasetResult::Records { records, version: 3, .. })
                if records == [b"FOUR".to_vec()]
        ));
    }

    #[test]
    fn pdse_catalog_alias_and_lifecycle_state_share_one_durable_graph() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let master = DatasetName::new("CAT.MASTER", 44).unwrap();
        let user = DatasetName::new("CAT.USER", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineCatalog {
                catalog: master.clone(),
                kind: mainframe_env_host_api::CatalogKind::Master,
                mutation: mutation(200),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefineCatalog {
                catalog: user.clone(),
                kind: mainframe_env_host_api::CatalogKind::User,
                mutation: mutation(201),
            })
            .unwrap();
        let prefix = DatasetName::new("APP", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlias {
                alias: prefix.clone(),
                target: user.clone(),
                mutation: mutation(202),
            })
            .unwrap();
        let library = DatasetName::new("APP.LIB", 44).unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ResolveCatalog {
                name: library.clone()
            }),
            Ok(DatasetResult::Catalog(ref resolution))
                if resolution.requested == library
                    && resolution.resolved == library
                    && resolution.catalog.as_ref() == Some(&user)
                    && resolution.alias_chain == [prefix.clone()]
        ));

        let mut definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::PartitionedExtended,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            });
        definition.catalog.catalog = Some(user.clone());
        dataset
            .invoke(DatasetRequest::Define {
                dataset: library.clone(),
                definition: Box::new(definition),
                mutation: mutation(203),
            })
            .unwrap();
        let program = MemberName::new("PROGRAM", 8).unwrap();
        dataset
            .invoke(DatasetRequest::WriteMemberGeneration {
                dataset: library.clone(),
                member: program.clone(),
                records: vec![b"P001".to_vec()],
                program_object: true,
                expected_version: Some(1),
                mutation: mutation(204),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::WriteMemberGeneration {
                dataset: library.clone(),
                member: program.clone(),
                records: vec![b"P002".to_vec()],
                program_object: false,
                expected_version: Some(2),
                mutation: mutation(205),
            })
            .unwrap();
        let member_alias = MemberName::new("PGMALIAS", 8).unwrap();
        dataset
            .invoke(DatasetRequest::DefineMemberAlias {
                dataset: library.clone(),
                alias: member_alias.clone(),
                target: program.clone(),
                expected_version: Some(3),
                mutation: mutation(206),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadMemberGeneration {
                dataset: library.clone(),
                member: member_alias.clone(),
                relative: -1,
                max_records: 8,
            }),
            Ok(DatasetResult::MemberGeneration {
                records,
                generation: 1,
                program_object: true,
                version: 4,
                ..
            }) if records == [b"P001".to_vec()]
        ));
        dataset
            .invoke(DatasetRequest::Append {
                dataset: library.clone(),
                member: Some(member_alias.clone()),
                records: vec![b"P003".to_vec()],
                expected_version: Some(4),
                mutation: mutation(207),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ReadMemberGeneration {
                dataset: library.clone(),
                member: member_alias.clone(),
                relative: 0,
                max_records: 8,
            }),
            Ok(DatasetResult::MemberGeneration {
                records,
                generation: 3,
                program_object: false,
                version: 5,
                ..
            }) if records == [b"P002".to_vec(), b"P003".to_vec()]
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListMembers {
                dataset: library.clone(),
                start: None,
                max_items: 8,
            }),
            Ok(DatasetResult::Members { names, more: false })
                if names == [member_alias.clone(), program.clone()]
        ));
        dataset
            .invoke(DatasetRequest::DeleteMemberGeneration {
                dataset: library.clone(),
                member: program.clone(),
                generation: 2,
                expected_version: Some(5),
                mutation: mutation(208),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: library.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Open,
                expected_version: Some(6),
                mutation: mutation(209),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: library.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Closed,
                expected_version: Some(7),
                mutation: mutation(210),
            })
            .unwrap();

        let library_alias = DatasetName::new("APP.LIBALT", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlias {
                alias: library_alias.clone(),
                target: library.clone(),
                mutation: mutation(211),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ResolveCatalog {
                name: library_alias.clone()
            }),
            Ok(DatasetResult::Catalog(ref resolution))
                if resolution.resolved == library
                    && resolution.catalog.as_ref() == Some(&user)
                    && resolution.alias_chain == [library_alias.clone()]
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Delete {
                dataset: library.clone(),
                member: None,
                expected_version: Some(8),
                purge: false,
                current_date: None,
                mutation: mutation(214),
            }),
            Err(HostProblem::Condition { ref name, response: 16, .. }) if name == "INUSE"
        ));
        dataset
            .invoke(DatasetRequest::SetCatalogConnection {
                catalog: user.clone(),
                connected: false,
                expected_version: Some(1),
                mutation: mutation(212),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ResolveCatalog {
                name: library_alias.clone()
            }),
            Err(HostProblem::Condition { ref name, response: 16, .. }) if name == "CATLGERR"
        ));
        dataset
            .invoke(DatasetRequest::SetCatalogConnection {
                catalog: user.clone(),
                connected: true,
                expected_version: Some(2),
                mutation: mutation(213),
            })
            .unwrap();

        let restarted = service(store.clone());
        assert!(matches!(
            restarted.invoke(DatasetRequest::ReadMemberGeneration {
                dataset: library.clone(),
                member: member_alias,
                relative: 0,
                max_records: 8,
            }),
            Ok(DatasetResult::MemberGeneration {
                records,
                generation: 3,
                version: 8,
                ..
            }) if records == [b"P002".to_vec(), b"P003".to_vec()]
        ));
        assert!(matches!(
            restarted.invoke(DatasetRequest::ResolveCatalog {
                name: library_alias.clone()
            }),
            Ok(DatasetResult::Catalog(ref resolution))
                if resolution.resolved == library
                    && resolution.catalog.as_ref() == Some(&user)
        ));
        assert!(matches!(
            restarted.invoke(DatasetRequest::Describe { dataset: library }),
            Ok(DatasetResult::Description(ref description))
                if description.version == 8
                    && description.definition.lifecycle.state
                        == mainframe_env_host_api::DatasetLifecycleState::Closed
        ));
        for (sequence, target, expected_version) in [
            (215, library_alias, 1),
            (216, DatasetName::new("APP.LIB", 44).unwrap(), 8),
            (217, prefix, 1),
            (218, user, 3),
            (219, master, 1),
        ] {
            restarted
                .invoke(DatasetRequest::Delete {
                    dataset: target,
                    member: None,
                    expected_version: Some(expected_version),
                    purge: false,
                    current_date: None,
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        assert_eq!(
            DatasetService::open(store, DatasetLimits::default())
                .unwrap()
                .invoke(DatasetRequest::List {
                    pattern: "APP*".into(),
                    start: None,
                    max_items: 8,
                }),
            Ok(DatasetResult::Listed {
                names: Vec::new(),
                more: false,
            })
        );
    }

    #[test]
    fn gdg_empty_scratch_and_noscratch_roll_in_are_distinct_after_restart() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let retained = DatasetName::new("USER.NOSCR", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineGenerationGroup {
                base: retained.clone(),
                limit: 2,
                scratch: false,
                empty: false,
                mutation: mutation(220),
            })
            .unwrap();
        let mut retained_names = Vec::new();
        for sequence in 221..=223 {
            let result = dataset
                .invoke(DatasetRequest::CreateGeneration {
                    base: retained.clone(),
                    attributes: attrs(DatasetOrganization::Sequential),
                    records: vec![format!("G{:03}", sequence - 220).into_bytes()],
                    mutation: mutation(sequence),
                })
                .unwrap();
            let DatasetResult::Generation { dataset, .. } = result else {
                panic!("unexpected generation result {result:?}");
            };
            retained_names.push(dataset);
        }
        assert!(matches!(
            dataset.invoke(DatasetRequest::ResolveGeneration {
                base: retained.clone(),
                relative: -1,
            }),
            Ok(DatasetResult::Generation {
                absolute_generation: 2,
                ..
            })
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Attributes {
                dataset: retained_names[0].clone()
            }),
            Ok(DatasetResult::Attributes { .. })
        ));

        let emptied = DatasetName::new("USER.EMPTY", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineGenerationGroup {
                base: emptied.clone(),
                limit: 2,
                scratch: true,
                empty: true,
                mutation: mutation(224),
            })
            .unwrap();
        let mut emptied_names = Vec::new();
        for sequence in 225..=227 {
            let result = dataset
                .invoke(DatasetRequest::CreateGeneration {
                    base: emptied.clone(),
                    attributes: attrs(DatasetOrganization::Sequential),
                    records: vec![format!("E{:03}", sequence - 224).into_bytes()],
                    mutation: mutation(sequence),
                })
                .unwrap();
            let DatasetResult::Generation { dataset, .. } = result else {
                panic!("unexpected generation result {result:?}");
            };
            emptied_names.push(dataset);
        }
        for retired in &emptied_names[..2] {
            assert_eq!(
                dataset.invoke(DatasetRequest::Attributes {
                    dataset: retired.clone()
                }),
                Err(HostProblem::NotFound)
            );
        }
        assert!(matches!(
            dataset.invoke(DatasetRequest::ResolveGeneration {
                base: emptied.clone(),
                relative: 0,
            }),
            Ok(DatasetResult::Generation {
                absolute_generation: 3,
                ..
            })
        ));

        let restarted = service(store);
        assert!(matches!(
            restarted.invoke(DatasetRequest::Attributes {
                dataset: retained_names[0].clone()
            }),
            Ok(DatasetResult::Attributes { .. })
        ));
        assert_eq!(
            restarted.invoke(DatasetRequest::Attributes {
                dataset: emptied_names[0].clone()
            }),
            Err(HostProblem::NotFound)
        );
        assert!(matches!(
            restarted.invoke(DatasetRequest::ResolveGeneration {
                base: emptied,
                relative: 0,
            }),
            Ok(DatasetResult::Generation {
                absolute_generation: 3,
                ..
            })
        ));
    }

    #[test]
    fn carddemo_ksds_write_inserts_records_without_replacing_the_cluster() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("CARDDEMO.ACCTDAT", 44).unwrap();
        service
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(1),
            })
            .unwrap();
        for (sequence, record) in [(2, b"AA11".to_vec()), (3, b"BB22".to_vec())] {
            service
                .invoke(DatasetRequest::Write {
                    dataset: name.clone(),
                    member: None,
                    records: vec![record],
                    expected_version: Some(sequence - 1),
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: name,
                member: None,
                key: None,
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, .. })
                if records == [b"AA11".to_vec(), b"BB22".to_vec()]
        ));
    }

    #[test]
    fn carddemo_esds_appends_records_in_arrival_order() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("CARDDEMO.USRSEC.ESDS", 44).unwrap();
        service
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: attrs(DatasetOrganization::EntrySequenced),
                mutation: mutation(1),
            })
            .unwrap();
        for (sequence, record) in [(2, b"AA11".to_vec()), (3, b"BB22".to_vec())] {
            service
                .invoke(DatasetRequest::Write {
                    dataset: name.clone(),
                    member: None,
                    records: vec![record],
                    expected_version: Some(sequence - 1),
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: name,
                member: None,
                key: None,
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, .. })
                if records == [b"AA11".to_vec(), b"BB22".to_vec()]
        ));
    }

    #[test]
    fn carddemo_rrds_pds_concatenation_and_record_formats_are_exact() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let rrds = DatasetName::new("CARDDEMO.USRSEC.RRDS", 44).unwrap();
        service
            .invoke(DatasetRequest::Create {
                dataset: rrds.clone(),
                attributes: attrs(DatasetOrganization::Relative),
                mutation: mutation(1),
            })
            .unwrap();
        for (sequence, number, record) in [(2, 2, b"BB22".to_vec()), (3, 1, b"AA11".to_vec())] {
            service
                .invoke(DatasetRequest::WriteRelative {
                    dataset: rrds.clone(),
                    record_number: number,
                    record,
                    expected_version: Some(sequence - 1),
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        assert!(matches!(
            service.invoke(DatasetRequest::ReadRelative {
                dataset: rrds.clone(),
                record_number: 2,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"BB22".to_vec()] && identities == [2u64.to_be_bytes().to_vec()]
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: rrds.clone(),
                member: None,
                key: None,
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AA11".to_vec(), b"BB22".to_vec()]
                    && identities == [1u64.to_be_bytes().to_vec(), 2u64.to_be_bytes().to_vec()]
        ));
        service
            .invoke(DatasetRequest::DeleteRelative {
                dataset: rrds.clone(),
                record_number: 2,
                expected_version: Some(3),
                mutation: mutation(4),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::ReadRelative {
                dataset: rrds,
                record_number: 2,
            }),
            Err(HostProblem::Condition { ref name, response: 13, .. }) if name == "NOTFND"
        ));

        let first = DatasetName::new("CARDDEMO.JCL.ONE", 44).unwrap();
        let second = DatasetName::new("CARDDEMO.JCL.TWO", 44).unwrap();
        let line_attributes = DatasetAttributes {
            organization: DatasetOrganization::Partitioned,
            record_format: RecordFormat::Line,
            logical_record_length: 80,
            key_offset: None,
            key_length: None,
            ccsid: Some(37),
        };
        for (sequence, dataset) in [(5, first.clone()), (6, second.clone())] {
            service
                .invoke(DatasetRequest::Create {
                    dataset,
                    attributes: line_attributes.clone(),
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        for (sequence, dataset, member, value) in [
            (7, first.clone(), "PROC1", b"//FIRST".to_vec()),
            (8, second.clone(), "PROC1", b"//SECOND".to_vec()),
            (9, second.clone(), "PROC2", b"//FALLBACK".to_vec()),
        ] {
            service
                .invoke(DatasetRequest::Write {
                    dataset,
                    member: Some(MemberName::new(member, 8).unwrap()),
                    records: vec![value],
                    expected_version: None,
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        for (member, expected) in [("PROC1", b"//FIRST".as_slice()), ("PROC2", b"//FALLBACK")] {
            assert!(matches!(
                service.invoke(DatasetRequest::ReadConcatenation {
                    datasets: vec![first.clone(), second.clone()],
                    member: Some(MemberName::new(member, 8).unwrap()),
                    max_records: 10,
                }),
                Ok(DatasetResult::Records { records, .. }) if records == [expected.to_vec()]
            ));
        }
        assert!(matches!(
            service.invoke(DatasetRequest::Write {
                dataset: first,
                member: Some(MemberName::new("BAD", 8).unwrap()),
                records: vec![b"LINE\nTWO".to_vec()],
                expected_version: None,
                mutation: mutation(10),
            }),
            Err(HostProblem::Malformed)
        ));

        let variable = DatasetName::new("CARDDEMO.VARIABLE", 44).unwrap();
        service
            .invoke(DatasetRequest::Create {
                dataset: variable.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: RecordFormat::VariableBlocked,
                    logical_record_length: 5,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: mutation(11),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::Write {
                dataset: variable,
                member: None,
                records: vec![b"TOO-LONG".to_vec()],
                expected_version: Some(1),
                mutation: mutation(12),
            }),
            Err(HostProblem::Condition { ref name, response: 22, .. }) if name == "LENGERR"
        ));
    }

    #[test]
    fn carddemo_gdg_roll_resolution_replay_and_restart_are_durable() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        let base = DatasetName::new("CARDDEMO.TRANSACT.BKUP", 44).unwrap();
        service
            .invoke(DatasetRequest::DefineGenerationGroup {
                base: base.clone(),
                limit: 2,
                scratch: true,
                empty: false,
                mutation: mutation(1),
            })
            .unwrap();
        let mut third = None;
        for sequence in 2..=4 {
            let request = DatasetRequest::CreateGeneration {
                base: base.clone(),
                attributes: attrs(DatasetOrganization::Sequential),
                records: vec![format!("G{sequence:03}").into_bytes()],
                mutation: mutation(sequence),
            };
            let result = service.invoke(request.clone()).unwrap();
            if sequence == 4 {
                assert_eq!(service.invoke(request).unwrap(), result);
                third = Some(result);
            }
        }
        assert!(matches!(
            third,
            Some(DatasetResult::Generation {
                absolute_generation: 3,
                ..
            })
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::ResolveGeneration {
                base: base.clone(),
                relative: 0,
            }),
            Ok(DatasetResult::Generation { ref dataset, absolute_generation: 3, .. })
                if dataset.as_str().ends_with(".G0003V00")
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::ResolveGeneration {
                base: base.clone(),
                relative: -1,
            }),
            Ok(DatasetResult::Generation {
                absolute_generation: 2,
                ..
            })
        ));
        assert_eq!(
            service.invoke(DatasetRequest::ResolveGeneration {
                base: base.clone(),
                relative: -2,
            }),
            Err(HostProblem::NotFound)
        );
        assert_eq!(
            service.invoke(DatasetRequest::Attributes {
                dataset: DatasetName::new("CARDDEMO.TRANSACT.BKUP.G0001V00", 64).unwrap(),
            }),
            Err(HostProblem::NotFound)
        );
        let restarted = DatasetService::open(store, DatasetLimits::default()).unwrap();
        assert!(matches!(
            restarted.invoke(DatasetRequest::ResolveGeneration { base, relative: 0 }),
            Ok(DatasetResult::Generation {
                absolute_generation: 3,
                ..
            })
        ));
    }

    #[test]
    fn carddemo_seed_install_reinstall_upgrade_rollback_and_failures_are_atomic() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = service(store.clone());
        let first = vec![
            seed_object("seed/account", "CARDDEMO.ACCOUNT", b"AA11BB22"),
            seed_object("seed/user", "CARDDEMO.USER", b"UU11"),
        ];
        let installed = service
            .install_seed_generation("CARDDEMO", "g1", first.clone())
            .unwrap();
        assert_eq!(
            (
                installed.seed_objects,
                installed.datasets,
                installed.records
            ),
            (2, 2, 3)
        );
        assert!(!installed.replayed);
        assert!(
            service
                .install_seed_generation("CARDDEMO", "g1", first)
                .unwrap()
                .replayed
        );
        let second = vec![
            seed_object("seed/account", "CARDDEMO.ACCOUNT", b"ZZ11YY22"),
            seed_object("seed/user", "CARDDEMO.USER", b"VV11"),
        ];
        service
            .install_seed_generation("CARDDEMO", "g2", second)
            .unwrap();
        assert_eq!(
            service
                .selected_seed_generation("carddemo")
                .unwrap()
                .as_deref(),
            Some("g2")
        );
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("CARDDEMO.ACCOUNT", 128).unwrap(),
                member: None,
                key: None,
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, .. })
                if records == [b"ZZ11".to_vec(), b"YY22".to_vec()]
        ));
        service.rollback_seed_generation("CARDDEMO", "g1").unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: DatasetName::new("CARDDEMO.ACCOUNT", 128).unwrap(),
                member: None,
                key: None,
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, .. })
                if records == [b"AA11".to_vec(), b"BB22".to_vec()]
        ));
        let restarted = DatasetService::open(store, DatasetLimits::default()).unwrap();
        assert_eq!(
            restarted
                .selected_seed_generation("CARDDEMO")
                .unwrap()
                .as_deref(),
            Some("g1")
        );

        let mut corrupt = seed_object("seed/bad", "CARDDEMO.BAD", b"BAD!");
        corrupt.sha256 = format!("sha256:{:064x}", 0);
        assert_eq!(
            restarted.install_seed_generation("BAD", "g1", vec![corrupt]),
            Err(HostProblem::IdempotencyConflict)
        );
        let limits = DatasetLimits {
            max_total_bytes: 4,
            ..DatasetLimits::default()
        };
        let bounded =
            DatasetService::open(Arc::new(MemoryStore::new(Default::default())), limits).unwrap();
        assert_eq!(
            bounded.install_seed_generation(
                "SMALL",
                "g1",
                vec![seed_object("seed/large", "SMALL.DATA", b"AA11BB22")],
            ),
            Err(HostProblem::ResourceExhausted)
        );
        let tiny_store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits {
            max_blob_bytes: 64,
            ..StoreLimits::default()
        }));
        let tiny = DatasetService::open(tiny_store, DatasetLimits::default()).unwrap();
        assert_eq!(
            tiny.install_seed_generation(
                "TINY",
                "g1",
                vec![seed_object("seed/tiny", "TINY.DATA", b"AA11")],
            ),
            Err(HostProblem::ResourceExhausted)
        );

        let corrupt_store: Arc<dyn ProviderStateStore> =
            Arc::new(MemoryStore::new(Default::default()));
        corrupt_store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset-seed-generation".into(),
                    key: "BAD@g1".into(),
                    version: 1,
                    payload: b"corrupt".to_vec(),
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            DatasetService::open(corrupt_store, DatasetLimits::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn carddemo_ksds_aix_mutations_and_browse_are_atomic_and_ordered() {
        let memory = Arc::new(MemoryStore::new(Default::default()));
        let store: Arc<dyn ProviderStateStore> = memory.clone();
        let service = service(store.clone());
        let base = DatasetName::new("CARDDEMO.CARDDAT", 44).unwrap();
        let duplicate_aix = DatasetName::new("CARDDEMO.CARDXREF", 44).unwrap();
        let unique_aix = DatasetName::new("CARDDEMO.CARDUNIQ", 44).unwrap();
        service
            .invoke(DatasetRequest::Create {
                dataset: base.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(1),
            })
            .unwrap();
        service
            .invoke(DatasetRequest::Write {
                dataset: base.clone(),
                member: None,
                records: [b"BBY2".to_vec(), b"AAX1".to_vec(), b"CCX3".to_vec()].into(),
                expected_version: Some(1),
                mutation: mutation(2),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: base.clone(),
                member: None,
                key: None,
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AAX1".to_vec(), b"BBY2".to_vec(), b"CCX3".to_vec()]
                    && identities == [b"AA".to_vec(), b"BB".to_vec(), b"CC".to_vec()]
        ));
        let reverse_cursor = match service
            .invoke(DatasetRequest::StartBrowse {
                dataset: base.clone(),
                key: b"CC".to_vec(),
            })
            .unwrap()
        {
            DatasetResult::Browse { cursor, .. } => cursor,
            other => panic!("unexpected browse result: {other:?}"),
        };
        assert!(matches!(
            service.invoke(DatasetRequest::ReadNext {
                dataset: base.clone(),
                cursor: reverse_cursor.clone(),
                reverse: true,
            }),
            Ok(DatasetResult::Browse {
                record: Some(ref record),
                identity: Some(ref identity),
                ..
            }) if record == b"BBY2" && identity == b"BB"
        ));
        service
            .invoke(DatasetRequest::EndBrowse {
                dataset: base.clone(),
                cursor: reverse_cursor,
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: duplicate_aix.clone(),
                key_offset: 2,
                key_length: 1,
                allow_duplicates: false,
                upgrade: true,
                mutation: mutation(3),
            }),
            Err(HostProblem::Condition { ref name, response: 14, .. }) if name == "DUPREC"
        ));
        service
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: duplicate_aix.clone(),
                key_offset: 2,
                key_length: 1,
                allow_duplicates: true,
                upgrade: true,
                mutation: mutation(4),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: duplicate_aix.clone(),
                member: None,
                key: Some(b"X".to_vec()),
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AAX1".to_vec(), b"CCX3".to_vec()]
                    && identities == [b"AA".to_vec(), b"CC".to_vec()]
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::ReadGeneric {
                dataset: base.clone(),
                key_prefix: b"A".to_vec(),
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AAX1".to_vec()] && identities == [b"AA".to_vec()]
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::ReadGeneric {
                dataset: duplicate_aix.clone(),
                key_prefix: b"X".to_vec(),
                max_records: 10,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AAX1".to_vec(), b"CCX3".to_vec()]
                    && identities == [b"AA".to_vec(), b"CC".to_vec()]
        ));

        let cursor = match service
            .invoke(DatasetRequest::StartBrowse {
                dataset: duplicate_aix.clone(),
                key: b"X".to_vec(),
            })
            .unwrap()
        {
            DatasetResult::Browse { cursor, .. } => cursor,
            other => panic!("unexpected browse result: {other:?}"),
        };
        for (record, identity) in [(b"AAX1".as_slice(), b"AA".as_slice()), (b"CCX3", b"CC")] {
            assert!(matches!(
                service.invoke(DatasetRequest::ReadNext {
                    dataset: duplicate_aix.clone(),
                    cursor: cursor.clone(),
                    reverse: false,
                }),
                Ok(DatasetResult::Browse {
                    record: Some(ref actual),
                    identity: Some(ref actual_identity),
                    key: Some(ref key),
                    ..
                }) if actual == record && actual_identity == identity && key == b"X"
            ));
        }
        service
            .invoke(DatasetRequest::EndBrowse {
                dataset: duplicate_aix.clone(),
                cursor,
            })
            .unwrap();

        service
            .invoke(DatasetRequest::RewriteRecord {
                dataset: base.clone(),
                key: b"AA".to_vec(),
                record: b"AAZ9".to_vec(),
                expected_version: Some(2),
                mutation: mutation(5),
            })
            .unwrap();
        service
            .invoke(DatasetRequest::DeleteRecord {
                dataset: base.clone(),
                key: b"CC".to_vec(),
                expected_version: Some(3),
                mutation: mutation(6),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: duplicate_aix.clone(),
                member: None,
                key: Some(b"X".to_vec()),
                max_records: 10,
            }),
            Err(HostProblem::Condition { ref name, response: 13, .. }) if name == "NOTFND"
        ));
        service
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: unique_aix.clone(),
                key_offset: 2,
                key_length: 1,
                allow_duplicates: false,
                upgrade: true,
                mutation: mutation(7),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::RewriteRecord {
                dataset: base.clone(),
                key: b"BB".to_vec(),
                record: b"BBZ8".to_vec(),
                expected_version: Some(4),
                mutation: mutation(8),
            }),
            Err(HostProblem::Condition { ref name, response: 14, .. }) if name == "DUPREC"
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::Read {
                dataset: base.clone(),
                member: None,
                key: Some(b"BB".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, .. }) if records == [b"BBY2".to_vec()]
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::Attributes {
                dataset: base.clone()
            }),
            Ok(DatasetResult::Attributes { version: 4, .. })
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::Attributes {
                dataset: duplicate_aix.clone()
            }),
            Ok(DatasetResult::Attributes { version: 3, .. })
        ));
        assert!(matches!(
            service.invoke(DatasetRequest::Attributes {
                dataset: unique_aix.clone()
            }),
            Ok(DatasetResult::Attributes { version: 1, .. })
        ));

        let retry_request = DatasetRequest::RewriteRecord {
            dataset: base.clone(),
            key: b"BB".to_vec(),
            record: b"BBW8".to_vec(),
            expected_version: Some(4),
            mutation: mutation(9),
        };
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset-replay".into(),
                    key: "id-9".into(),
                    version: 1,
                    payload: encode_replay(&Replay {
                        request_digest: request_digest(&retry_request).unwrap(),
                        result: None,
                    })
                    .unwrap(),
                },
                None,
            )
            .unwrap();
        let restarted = DatasetService::open(store, DatasetLimits::default()).unwrap();
        assert_eq!(
            restarted.invoke(retry_request).unwrap(),
            DatasetResult::Mutated { version: 5 }
        );
        assert!(matches!(
            restarted.invoke(DatasetRequest::Read {
                dataset: duplicate_aix.clone(),
                member: None,
                key: Some(b"Z".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AAZ9".to_vec()] && identities == [b"AA".to_vec()]
        ));
        assert_eq!(
            restarted.invoke(DatasetRequest::BuildAlternateIndex {
                base: base.clone(),
                index: duplicate_aix,
                mutation: mutation(10),
            }),
            Ok(DatasetResult::Mutated { version: 5 })
        );
        let stale = DatasetName::new("USER.BASE.STALE", 44).unwrap();
        assert_eq!(
            restarted.invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: stale.clone(),
                key_offset: 2,
                key_length: 1,
                allow_duplicates: true,
                upgrade: false,
                mutation: mutation(11),
            }),
            Ok(DatasetResult::Created { version: 1 })
        );
        restarted
            .invoke(DatasetRequest::RewriteRecord {
                dataset: base.clone(),
                key: b"AA".to_vec(),
                record: b"AAY7".to_vec(),
                expected_version: Some(5),
                mutation: mutation(12),
            })
            .unwrap();
        assert!(matches!(
            restarted.invoke(DatasetRequest::Read {
                dataset: stale.clone(),
                member: None,
                key: Some(b"Z".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 1, .. })
                if records == [b"AAY7".to_vec()]
        ));
        assert!(matches!(
            restarted.invoke(DatasetRequest::Read {
                dataset: stale.clone(),
                member: None,
                key: Some(b"Y".to_vec()),
                max_records: 1,
            }),
            Err(HostProblem::Condition { ref name, .. }) if name == "NOTFND"
        ));
        assert_eq!(
            restarted.invoke(DatasetRequest::BuildAlternateIndex {
                base,
                index: stale.clone(),
                mutation: mutation(13),
            }),
            Ok(DatasetResult::Mutated { version: 2 })
        );
        assert!(matches!(
            restarted.invoke(DatasetRequest::Read {
                dataset: stale,
                member: None,
                key: Some(b"Y".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"AAY7".to_vec()]
        ));
    }
    #[test]
    fn restart_reloads_provider_state() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let name = DatasetName::new("USER.DATA", 44).unwrap();
        service(Arc::clone(&store))
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: attrs(DatasetOrganization::Sequential),
                mutation: mutation(1),
            })
            .unwrap();
        assert!(matches!(
            service(store).invoke(DatasetRequest::Attributes { dataset: name }),
            Ok(DatasetResult::Attributes { version: 1, .. })
        ));
    }
    #[test]
    fn sqlite_restart_and_atomic_rename_preserve_state() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-dataset-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("dataset.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let from = DatasetName::new("USER.FROM", 44).unwrap();
        let to = DatasetName::new("USER.TO", 44).unwrap();
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 2 * 1024 * 1024, 65536).unwrap());
            let service = service(store);
            service
                .invoke(DatasetRequest::Create {
                    dataset: from.clone(),
                    attributes: attrs(DatasetOrganization::Sequential),
                    mutation: mutation(1),
                })
                .unwrap();
            assert_eq!(
                service
                    .invoke(DatasetRequest::Rename {
                        from: from.clone(),
                        to: to.clone(),
                        mutation: mutation(2),
                    })
                    .unwrap(),
                DatasetResult::Mutated { version: 2 }
            );
        }
        {
            let store: Arc<dyn ProviderStateStore> =
                Arc::new(SqliteStateStore::open(&url, 2 * 1024 * 1024, 65536).unwrap());
            let service = service(store);
            assert_eq!(
                service
                    .invoke(DatasetRequest::Rename {
                        from: from.clone(),
                        to: to.clone(),
                        mutation: mutation(2),
                    })
                    .unwrap(),
                DatasetResult::Mutated { version: 2 }
            );
            assert_eq!(
                service.invoke(DatasetRequest::Attributes { dataset: from }),
                Err(HostProblem::NotFound)
            );
            assert!(matches!(
                service.invoke(DatasetRequest::Attributes { dataset: to }),
                Ok(DatasetResult::Attributes { version: 2, .. })
            ));
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn control_interval_area_and_spanned_record_geometry_are_exact() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.SPANNED", 44).unwrap();
        let mut definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::VariableSpanned,
                logical_record_length: 8000,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: Some(37),
            });
        definition.vsam.spanned = true;
        definition.vsam.control_interval_size = Some(512);
        definition.vsam.control_area_size = Some(2048);
        dataset
            .invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition.clone()),
                mutation: mutation(400),
            })
            .unwrap();
        let mut record = vec![b'X'; 8000];
        record[..2].copy_from_slice(b"AA");
        dataset
            .invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![record.clone()],
                expected_version: Some(1),
                mutation: mutation(401),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: name.clone()
            }),
            Ok(DatasetResult::Description(ref description))
                if description.control_intervals == 16
                    && description.control_areas == 4
                    && description.high_used_rba == 8000
                    && description.used_bytes == 8000
        ));

        let rejected = DatasetName::new("USER.NOSPAN", 44).unwrap();
        definition.attributes.record_format = RecordFormat::Variable;
        definition.vsam.spanned = false;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: rejected.clone(),
                definition: Box::new(definition),
                mutation: mutation(402),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Write {
                dataset: rejected,
                member: None,
                records: vec![record],
                expected_version: Some(1),
                mutation: mutation(403),
            }),
            Err(HostProblem::Condition {
                ref name,
                response: 22,
                ..
            }) if name == "LENGERR"
        ));
    }

    #[test]
    fn rls_locks_enforce_shareoptions_lease_coverage_and_order_after_restart() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let name = DatasetName::new("USER.RLS.B", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Rls;
        definition.vsam.share_options.cross_region = 2;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition.clone()),
                mutation: mutation(410),
            })
            .unwrap();
        let owner = principal("IBMUSER");
        let lock_request = DatasetRequest::AcquireLock {
            dataset: name.clone(),
            target: mainframe_env_host_api::DatasetLockTarget::Dataset,
            owner: owner.clone(),
            mode: mainframe_env_host_api::DatasetLockMode::Exclusive,
            now_tick: 411,
            lease_ticks: 100,
            transaction: None,
            mutation: mutation(411),
        };
        let DatasetResult::Locks { locks } = dataset.invoke(lock_request.clone()).unwrap() else {
            panic!("expected lock receipt");
        };
        let lock_id = locks[0].lock_id.clone();
        assert_eq!(
            dataset.invoke(lock_request).unwrap(),
            DatasetResult::Locks { locks }
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: mutation(412),
            }),
            Err(HostProblem::Condition { ref name, .. }) if name == "LOCKED"
        ));
        assert_eq!(
            dataset.invoke_for_principal(
                &principal("OTHER"),
                DatasetRequest::Write {
                    dataset: name.clone(),
                    member: None,
                    records: vec![b"AA11".to_vec()],
                    expected_version: Some(1),
                    mutation: transaction_mutation(413, &lock_id),
                },
            ),
            Err(HostProblem::Unauthorized)
        );
        dataset
            .invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: transaction_mutation(412, &lock_id),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::ReleaseLock {
                dataset: name.clone(),
                lock_id,
                owner: owner.clone(),
                mutation: mutation(413),
            })
            .unwrap();

        let shared = |sequence, owner: &str| DatasetRequest::AcquireLock {
            dataset: name.clone(),
            target: mainframe_env_host_api::DatasetLockTarget::Record(b"AA".to_vec()),
            owner: principal(owner),
            mode: mainframe_env_host_api::DatasetLockMode::Shared,
            now_tick: sequence,
            lease_ticks: 100,
            transaction: None,
            mutation: mutation(sequence),
        };
        dataset.invoke(shared(414, "OWNER1")).unwrap();
        dataset.invoke(shared(415, "OWNER2")).unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::AcquireLock {
                dataset: name.clone(),
                target: mainframe_env_host_api::DatasetLockTarget::Record(b"AA".to_vec()),
                owner: principal("OWNER3"),
                mode: mainframe_env_host_api::DatasetLockMode::Exclusive,
                now_tick: 416,
                lease_ticks: 100,
                transaction: None,
                mutation: mutation(416),
            }),
            Err(HostProblem::Condition { ref name, .. }) if name == "LOCKED"
        ));
        let restarted = service(store);
        assert!(matches!(
            restarted.invoke(DatasetRequest::ListLocks {
                dataset: name.clone(),
                now_tick: 417,
                max_items: 8,
            }),
            Ok(DatasetResult::Locks { locks }) if locks.len() == 2
        ));

        let low = DatasetName::new("USER.RLS.A", 44).unwrap();
        restarted
            .invoke(DatasetRequest::Define {
                dataset: low.clone(),
                definition: Box::new(definition),
                mutation: mutation(417),
            })
            .unwrap();
        restarted
            .invoke(DatasetRequest::AcquireLock {
                dataset: name,
                target: mainframe_env_host_api::DatasetLockTarget::Dataset,
                owner: owner.clone(),
                mode: mainframe_env_host_api::DatasetLockMode::Shared,
                now_tick: 418,
                lease_ticks: 100,
                transaction: None,
                mutation: mutation(418),
            })
            .unwrap();
        assert!(matches!(
            restarted.invoke(DatasetRequest::AcquireLock {
                dataset: low,
                target: mainframe_env_host_api::DatasetLockTarget::Dataset,
                owner,
                mode: mainframe_env_host_api::DatasetLockMode::Shared,
                now_tick: 419,
                lease_ticks: 100,
                transaction: None,
                mutation: mutation(419),
            }),
            Err(HostProblem::Condition { ref name, .. }) if name == "LOCKORDER"
        ));
    }

    #[test]
    fn tvs_commit_rollback_replay_and_restart_are_atomic_across_datasets() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let first = DatasetName::new("USER.TVS.A", 44).unwrap();
        let second = DatasetName::new("USER.TVS.B", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Tvs;
        for (sequence, name) in [(420, first.clone()), (421, second.clone())] {
            dataset
                .invoke(DatasetRequest::Define {
                    dataset: name,
                    definition: Box::new(definition.clone()),
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        let owner = principal("TVSUSER");
        dataset
            .invoke(DatasetRequest::BeginTvs {
                transaction: "TX-COMMIT".into(),
                owner: owner.clone(),
                mutation: mutation(422),
            })
            .unwrap();
        for (sequence, name, record) in [
            (423, first.clone(), b"AA11".to_vec()),
            (424, second.clone(), b"BB22".to_vec()),
        ] {
            dataset
                .invoke(DatasetRequest::StageTvs {
                    transaction: "TX-COMMIT".into(),
                    owner: owner.clone(),
                    operation: mainframe_env_host_api::TvsRecordOperation::Insert {
                        dataset: name,
                        record,
                    },
                    mutation: mutation(sequence),
                })
                .unwrap();
        }
        assert!(matches!(
            dataset.invoke(DatasetRequest::Write {
                dataset: first.clone(),
                member: None,
                records: vec![b"CC33".to_vec()],
                expected_version: Some(1),
                mutation: mutation(425),
            }),
            Err(HostProblem::UnsupportedCapability { ref capability, .. }) if capability == "tvs"
        ));
        let complete = DatasetRequest::CompleteTvs {
            transaction: "TX-COMMIT".into(),
            owner: owner.clone(),
            commit: true,
            mutation: mutation(426),
        };
        let committed = dataset.invoke(complete.clone()).unwrap();
        assert!(matches!(
            committed,
            DatasetResult::Tvs(mainframe_env_host_api::TvsUnitOfWorkReceipt {
                state: mainframe_env_host_api::TvsUnitOfWorkState::Committed,
                staged_operations: 2,
                ..
            })
        ));
        assert_eq!(dataset.invoke(complete.clone()).unwrap(), committed);

        let restarted = service(store.clone());
        assert_eq!(restarted.invoke(complete).unwrap(), committed);
        for (name, key, record) in [
            (first.clone(), b"AA".to_vec(), b"AA11".to_vec()),
            (second, b"BB".to_vec(), b"BB22".to_vec()),
        ] {
            assert!(matches!(
                restarted.invoke(DatasetRequest::Read {
                    dataset: name,
                    member: None,
                    key: Some(key),
                    max_records: 1,
                }),
                Ok(DatasetResult::Records { records, version: 2, .. }) if records == [record]
            ));
        }
        assert!(matches!(
            restarted.invoke(DatasetRequest::ListLocks {
                dataset: first.clone(),
                now_tick: 1,
                max_items: 8,
            }),
            Ok(DatasetResult::Locks { locks }) if locks.is_empty()
        ));

        restarted
            .invoke(DatasetRequest::BeginTvs {
                transaction: "TX-ROLLBACK".into(),
                owner: owner.clone(),
                mutation: mutation(427),
            })
            .unwrap();
        restarted
            .invoke(DatasetRequest::StageTvs {
                transaction: "TX-ROLLBACK".into(),
                owner: owner.clone(),
                operation: mainframe_env_host_api::TvsRecordOperation::Rewrite {
                    dataset: first.clone(),
                    key: b"AA".to_vec(),
                    record: b"AA99".to_vec(),
                },
                mutation: mutation(428),
            })
            .unwrap();
        restarted
            .invoke(DatasetRequest::CompleteTvs {
                transaction: "TX-ROLLBACK".into(),
                owner,
                commit: false,
                mutation: mutation(429),
            })
            .unwrap();
        assert!(matches!(
            restarted.invoke(DatasetRequest::Read {
                dataset: first.clone(),
                member: None,
                key: Some(b"AA".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"AA11".to_vec()]
        ));
        assert_eq!(
            restarted.invoke(DatasetRequest::Delete {
                dataset: first.clone(),
                member: None,
                expected_version: Some(2),
                purge: true,
                current_date: None,
                mutation: mutation(430),
            }),
            Ok(DatasetResult::Mutated { version: 3 })
        );
        drop(restarted);
        let reopened = service(store);
        assert_eq!(
            reopened.invoke(DatasetRequest::Attributes { dataset: first }),
            Err(HostProblem::NotFound)
        );
        assert!(matches!(
            reopened.invoke(DatasetRequest::TvsStatus {
                transaction: "TX-COMMIT".into(),
                owner: principal("TVSUSER"),
            }),
            Ok(DatasetResult::Tvs(
                mainframe_env_host_api::TvsUnitOfWorkReceipt {
                    state: mainframe_env_host_api::TvsUnitOfWorkState::Committed,
                    ..
                }
            ))
        ));
    }

    #[test]
    fn tvs_unknown_outcome_requires_explicit_reconciliation() {
        let store = Arc::new(FailAtomicOnceStore::new());
        let dataset = service(store.clone());
        let name = DatasetName::new("USER.TVS.UNKNOWN", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Tvs;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition),
                mutation: mutation(430),
            })
            .unwrap();
        let owner = principal("RECOVER");
        dataset
            .invoke(DatasetRequest::BeginTvs {
                transaction: "TX-UNKNOWN".into(),
                owner: owner.clone(),
                mutation: mutation(431),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::StageTvs {
                transaction: "TX-UNKNOWN".into(),
                owner: owner.clone(),
                operation: mainframe_env_host_api::TvsRecordOperation::Insert {
                    dataset: name.clone(),
                    record: b"AA11".to_vec(),
                },
                mutation: mutation(432),
            })
            .unwrap();
        store.arm();
        assert_eq!(
            dataset.invoke(DatasetRequest::CompleteTvs {
                transaction: "TX-UNKNOWN".into(),
                owner: owner.clone(),
                commit: true,
                mutation: mutation(433),
            }),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::TvsStatus {
                transaction: "TX-UNKNOWN".into(),
                owner: owner.clone(),
            }),
            Ok(DatasetResult::Tvs(
                mainframe_env_host_api::TvsUnitOfWorkReceipt {
                    state: mainframe_env_host_api::TvsUnitOfWorkState::Unknown,
                    ..
                }
            ))
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: name.clone(),
                member: None,
                key: None,
                max_records: 8,
            }),
            Ok(DatasetResult::Records { records, version: 1, .. }) if records.is_empty()
        ));
        dataset
            .invoke(DatasetRequest::ReconcileTvs {
                transaction: "TX-UNKNOWN".into(),
                owner,
                committed: true,
                mutation: mutation(434),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: name,
                member: None,
                key: Some(b"AA".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"AA11".to_vec()]
        ));
    }

    #[test]
    fn injected_base_aix_and_gdg_failures_retry_without_partial_publication() {
        let store = Arc::new(FailAtomicOnceStore::new());
        let dataset = service(store.clone());
        let base = DatasetName::new("FAIL.BASE", 44).unwrap();
        let index = DatasetName::new("FAIL.BASE.AIX", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: base.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(560),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: base.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: mutation(561),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: index.clone(),
                key_offset: 2,
                key_length: 2,
                allow_duplicates: false,
                upgrade: true,
                mutation: mutation(562),
            })
            .unwrap();
        let rewrite = DatasetRequest::RewriteRecord {
            dataset: base.clone(),
            key: b"AA".to_vec(),
            record: b"AA22".to_vec(),
            expected_version: Some(2),
            mutation: mutation(563),
        };
        store.arm();
        assert_eq!(
            dataset.invoke(rewrite.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: index.clone(),
                member: None,
                key: Some(b"11".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 1, .. })
                if records == [b"AA11".to_vec()]
        ));
        assert_eq!(
            dataset.invoke(rewrite),
            Ok(DatasetResult::Mutated { version: 3 })
        );
        assert!(matches!(
            dataset.invoke(DatasetRequest::Read {
                dataset: index,
                member: None,
                key: Some(b"22".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"AA22".to_vec()]
        ));

        let gdg = DatasetName::new("FAIL.GDG", 44).unwrap();
        dataset
            .invoke(DatasetRequest::DefineGenerationGroup {
                base: gdg.clone(),
                limit: 2,
                scratch: true,
                empty: false,
                mutation: mutation(564),
            })
            .unwrap();
        let generation = DatasetRequest::CreateGeneration {
            base: gdg.clone(),
            attributes: attrs(DatasetOrganization::Sequential),
            records: vec![b"GEN1".to_vec()],
            mutation: mutation(565),
        };
        store.arm();
        assert_eq!(
            dataset.invoke(generation.clone()),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            dataset.invoke(DatasetRequest::ResolveGeneration {
                base: gdg.clone(),
                relative: 0,
            }),
            Err(HostProblem::NotFound)
        );
        assert!(matches!(
            dataset.invoke(generation),
            Ok(DatasetResult::Generation {
                absolute_generation: 1,
                version: 2,
                ..
            })
        ));
    }

    #[test]
    fn corruption_matrix_rejects_every_dataset_authority_before_publication() {
        for (namespace, key) in [
            ("dataset-replay", "bad-replay"),
            ("dataset", "USER.BAD"),
            ("dataset-aix", "USER.BAD.AIX"),
            ("dataset-gdg", "USER.BAD.GDG"),
            ("dataset-catalog", "USER.BAD.CAT"),
            ("dataset-catalog-alias", "USER.BAD.ALIAS"),
            ("dataset-lock", "bad-lock"),
            ("dataset-tvs", "bad-uow"),
        ] {
            let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: namespace.into(),
                        key: key.into(),
                        version: 1,
                        payload: b"corrupt".to_vec(),
                    },
                    None,
                )
                .unwrap();
            assert!(matches!(
                DatasetService::open(store, DatasetLimits::default()),
                Err(HostProblem::InfrastructureFailure)
            ));
        }
    }

    #[test]
    fn restart_rejects_state_requiring_an_unadvertised_capability() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::Sequential,
        ));
        definition.lifecycle.state = mainframe_env_host_api::DatasetLifecycleState::Migrated;
        definition.lifecycle.migration_level = 1;
        let entry = Entry::from_definition(definition, 1);
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "dataset".into(),
                    key: "USER.MIGRATED".into(),
                    version: 1,
                    payload: encode(&entry).unwrap(),
                },
                None,
            )
            .unwrap();
        let provider_store: Arc<dyn ProviderStateStore> = store;
        assert!(matches!(
            DatasetService::open(provider_store, DatasetLimits::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn sqlite_dataset_backup_restores_one_integrity_checked_snapshot() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-dataset-backup-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source_path = directory.join("source.db");
        let backup_path = directory.join("backup.db");
        let source = Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rwc", source_path.display()),
                2 * 1024 * 1024,
                65_536,
            )
            .unwrap(),
        );
        let store: Arc<dyn ProviderStateStore> = source.clone();
        let dataset = service(store);
        let name = DatasetName::new("USER.BACKUP", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: attrs(DatasetOrganization::Sequential),
                mutation: mutation(600),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![b"OLD1".to_vec()],
                expected_version: Some(1),
                mutation: mutation(601),
            })
            .unwrap();
        source.integrity_check().unwrap();
        source.backup_to(&backup_path).unwrap();
        dataset
            .invoke(DatasetRequest::Append {
                dataset: name.clone(),
                member: None,
                records: vec![b"NEW2".to_vec()],
                expected_version: Some(2),
                mutation: mutation(602),
            })
            .unwrap();
        drop(dataset);
        drop(source);

        let restored = Arc::new(
            SqliteStateStore::open(
                &format!("sqlite://{}?mode=rw", backup_path.display()),
                2 * 1024 * 1024,
                65_536,
            )
            .unwrap(),
        );
        restored.integrity_check().unwrap();
        let restored_store: Arc<dyn ProviderStateStore> = restored.clone();
        let restored_dataset = service(restored_store);
        assert!(matches!(
            restored_dataset.invoke(DatasetRequest::Read {
                dataset: name,
                member: None,
                key: None,
                max_records: 8,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"OLD1".to_vec()]
        ));
        drop(restored_dataset);
        drop(restored);
        let _ = std::fs::remove_file(source_path);
        let _ = std::fs::remove_file(backup_path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn scale_limit_and_sorted_restart_are_deterministic() {
        let limits = DatasetLimits {
            max_datasets: 64,
            max_records: 1_024,
            ..DatasetLimits::default()
        };
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let dataset = DatasetService::open(store.clone(), limits).unwrap();
        for sequence in 0..64u64 {
            dataset
                .invoke(DatasetRequest::Create {
                    dataset: DatasetName::new(format!("SCALE.D{sequence:04}"), 44).unwrap(),
                    attributes: attrs(DatasetOrganization::Sequential),
                    mutation: mutation(700 + sequence),
                })
                .unwrap();
        }
        assert_eq!(
            dataset.invoke(DatasetRequest::Create {
                dataset: DatasetName::new("SCALE.OVER", 44).unwrap(),
                attributes: attrs(DatasetOrganization::Sequential),
                mutation: mutation(800),
            }),
            Err(HostProblem::ResourceExhausted)
        );
        let restarted = DatasetService::open(store, limits).unwrap();
        let DatasetResult::Listed { names, more } = restarted
            .invoke(DatasetRequest::List {
                pattern: "SCALE.*".into(),
                start: None,
                max_items: 64,
            })
            .unwrap()
        else {
            panic!("expected scale listing");
        };
        assert!(!more);
        assert_eq!(names.len(), 64);
        assert!(
            names
                .windows(2)
                .all(|pair| pair[0].as_str() < pair[1].as_str())
        );
    }

    #[test]
    fn concurrent_mutation_and_lock_races_have_one_deterministic_winner() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        let keyed = DatasetName::new("RACE.KSDS", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: keyed.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(820),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: keyed.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: mutation(821),
            })
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for (sequence, record) in [(822, b"AA22".to_vec()), (823, b"AA33".to_vec())] {
            let service = dataset.clone();
            let dataset = keyed.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                service.invoke(DatasetRequest::RewriteRecord {
                    dataset,
                    key: b"AA".to_vec(),
                    record,
                    expected_version: Some(2),
                    mutation: mutation(sequence),
                })
            }));
        }
        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Ok(DatasetResult::Mutated { version: 3 })))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| **outcome == Err(HostProblem::IdempotencyConflict))
                .count(),
            1
        );

        let rls = DatasetName::new("RACE.RLS", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Rls;
        definition.vsam.share_options.cross_region = 2;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: rls.clone(),
                definition: Box::new(definition),
                mutation: mutation(824),
            })
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for (sequence, owner) in [(825, "RACE1"), (826, "RACE2")] {
            let service = dataset.clone();
            let dataset = rls.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                service.invoke(DatasetRequest::AcquireLock {
                    dataset,
                    target: mainframe_env_host_api::DatasetLockTarget::Dataset,
                    owner: principal(owner),
                    mode: mainframe_env_host_api::DatasetLockMode::Exclusive,
                    now_tick: sequence,
                    lease_ticks: 100,
                    transaction: None,
                    mutation: mutation(sequence),
                })
            }));
        }
        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Ok(DatasetResult::Locks { locks }) if locks.len() == 1))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Err(HostProblem::Condition { name, .. }) if name == "LOCKED"))
                .count(),
            1
        );
    }

    #[test]
    fn deterministic_allocation_buffer_sms_and_extended_geometry_is_observable() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.GEOMETRY", 44).unwrap();
        let mut definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 100,
                key_offset: Some(0),
                key_length: Some(4),
                ccsid: Some(37),
            });
        definition.allocation.primary = 2;
        definition.allocation.secondary = 3;
        definition.allocation.release_unused = true;
        definition.dcb.buffer_count = 3;
        definition.dcb.buffer_size = Some(50);
        definition.sms.data_class = Some("STANDARD".into());
        definition.sms.management_class = Some("ACTIVE".into());
        definition.sms.storage_class = Some("ABSTRACT".into());
        definition.sms.guaranteed_space = true;
        definition.sms.extended_format = true;
        definition.sms.extended_addressable = true;
        definition.volumes.volume_ids = vec!["VOLA".into(), "VOLB".into()];
        definition.volumes.unit_count = 2;
        definition.vsam.buffering = mainframe_env_host_api::BufferingMode::LocalSharedResources;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition),
                mutation: mutation(900),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![vec![b'X'; 100]],
                expected_version: Some(1),
                mutation: mutation(901),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: name.clone(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.allocated_bytes == 500
                    && description.extents.len() == 2
                    && description.extents[0].length == 200
                    && description.extents[1].start == 200
                    && description.extents[1].length == 300
                    && description.buffer_bytes == 300
                    && description.max_rba == u64::MAX
                    && description.abstract_placement.starts_with("ABSTRACT:")
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListVolumes {
                start: None,
                max_items: 1,
            }),
            Ok(DatasetResult::Volumes { volumes, more: true })
                if volumes.len() == 1
                    && volumes[0].volume_id == "VOLA"
                    && volumes[0].allocated_bytes == 200
                    && volumes[0].used_bytes == 100
                    && volumes[0].extents[0].volume_start == 0
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListVolumes {
                start: Some("VOLA".into()),
                max_items: 1,
            }),
            Ok(DatasetResult::Volumes { volumes, more: false })
                if volumes.len() == 1
                    && volumes[0].volume_id == "VOLB"
                    && volumes[0].allocated_bytes == 300
                    && volumes[0].used_bytes == 0
        ));
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Open,
                expected_version: Some(2),
                mutation: mutation(902),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Closed,
                expected_version: Some(3),
                mutation: mutation(903),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: name.clone(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.allocated_bytes == 200
                    && description.extents.len() == 1
                    && description.extents[0].length == 200
        ));
        let second = DatasetName::new("USER.ZGEOM", 44).unwrap();
        let mut second_definition =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::Sequential,
                record_format: RecordFormat::Fixed,
                logical_record_length: 100,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            });
        second_definition.volumes.volume_ids = vec!["VOLA".into()];
        dataset
            .invoke(DatasetRequest::Define {
                dataset: second,
                definition: Box::new(second_definition),
                mutation: mutation(907),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListVolumes {
                start: None,
                max_items: 8,
            }),
            Ok(DatasetResult::Volumes { volumes, more: false })
                if volumes.len() == 1
                    && volumes[0].allocated_bytes == 300
                    && volumes[0].extents.len() == 2
                    && volumes[0].extents[0].dataset.as_str() == "USER.GEOMETRY"
                    && volumes[0].extents[0].volume_start == 0
                    && volumes[0].extents[1].dataset.as_str() == "USER.ZGEOM"
                    && volumes[0].extents[1].volume_start == 200
        ));

        assert!(matches!(
            dataset.invoke(DatasetRequest::Capabilities),
            Ok(DatasetResult::Capabilities { capabilities })
                if capabilities.allocation_extents
                    && capabilities.buffering
                    && capabilities.sms_classes
                    && capabilities.extended_format
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Diagnose {
                dataset: name.clone(),
            }),
            Ok(DatasetResult::Diagnostics { diagnostics })
                if diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "UNAVAILABLE_CAPABILITY"
                        && diagnostic.field.as_deref()
                            == Some("provider-capabilities.physical-volumes")
                        && diagnostic.detail.contains("VOLUME/UNIT")
                })
                    && diagnostics.iter().any(|diagnostic| {
                        diagnostic.field.as_deref()
                            == Some("provider-capabilities.migration-recall")
                            && diagnostic.detail.contains("MIGRATE/RECALL")
                    })
        ));

        let allocated = DatasetName::new("USER.ALLOC", 44).unwrap();
        let mut allocated_definition = mainframe_env_host_api::DatasetDefinition::compatibility(
            attrs(DatasetOrganization::Sequential),
        );
        allocated_definition.lifecycle.state =
            mainframe_env_host_api::DatasetLifecycleState::Allocated;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: allocated.clone(),
                definition: Box::new(allocated_definition),
                mutation: mutation(905),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Describe {
                dataset: allocated.clone(),
            }),
            Ok(DatasetResult::Description(ref description))
                if description.definition.lifecycle.state
                    == mainframe_env_host_api::DatasetLifecycleState::Allocated
        ));
        assert_eq!(
            dataset.invoke(DatasetRequest::SetLifecycle {
                dataset: allocated,
                state: mainframe_env_host_api::DatasetLifecycleState::Cataloged,
                expected_version: Some(1),
                mutation: mutation(906),
            }),
            Ok(DatasetResult::Mutated { version: 2 })
        );

        let bounded = DatasetService::open(
            Arc::new(MemoryStore::new(Default::default())),
            DatasetLimits {
                max_total_bytes: 128,
                ..DatasetLimits::default()
            },
        )
        .unwrap();
        let mut oversized = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::Sequential,
        ));
        oversized.allocation.unit = mainframe_env_host_api::SpaceUnit::Kilobytes;
        oversized.sms.guaranteed_space = true;
        assert_eq!(
            bounded.invoke(DatasetRequest::Define {
                dataset: DatasetName::new("USER.NOSPACE", 44).unwrap(),
                definition: Box::new(oversized),
                mutation: mutation(904),
            }),
            Err(HostProblem::ResourceExhausted)
        );

        let volume_bounded = DatasetService::open(
            Arc::new(MemoryStore::new(Default::default())),
            DatasetLimits {
                max_total_bytes: 500,
                ..DatasetLimits::default()
            },
        )
        .unwrap();
        let mut reserved =
            mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                organization: DatasetOrganization::Sequential,
                record_format: RecordFormat::Fixed,
                logical_record_length: 100,
                key_offset: None,
                key_length: None,
                ccsid: Some(37),
            });
        reserved.allocation.primary = 3;
        reserved.sms.guaranteed_space = true;
        volume_bounded
            .invoke(DatasetRequest::Define {
                dataset: DatasetName::new("USER.RESV1", 44).unwrap(),
                definition: Box::new(reserved.clone()),
                mutation: mutation(908),
            })
            .unwrap();
        assert_eq!(
            volume_bounded.invoke(DatasetRequest::Define {
                dataset: DatasetName::new("USER.RESV2", 44).unwrap(),
                definition: Box::new(reserved),
                mutation: mutation(909),
            }),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            volume_bounded.invoke(DatasetRequest::Attributes {
                dataset: DatasetName::new("USER.RESV2", 44).unwrap(),
            }),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn every_abstract_allocation_unit_has_an_exact_capacity_constant() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        for (position, unit, primary, block_size, expected) in [
            (0, mainframe_env_host_api::SpaceUnit::Tracks, 1, 0, 56_664),
            (
                1,
                mainframe_env_host_api::SpaceUnit::Cylinders,
                1,
                0,
                849_960,
            ),
            (2, mainframe_env_host_api::SpaceUnit::Blocks, 2, 400, 800),
            (3, mainframe_env_host_api::SpaceUnit::Kilobytes, 2, 0, 2_048),
            (
                4,
                mainframe_env_host_api::SpaceUnit::Megabytes,
                1,
                0,
                1_048_576,
            ),
            (5, mainframe_env_host_api::SpaceUnit::Records, 3, 0, 300),
        ] {
            let name = DatasetName::new(format!("USER.UNIT{position}"), 44).unwrap();
            let mut definition =
                mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                    organization: DatasetOrganization::Sequential,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: 100,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                });
            definition.allocation.unit = unit;
            definition.allocation.primary = primary;
            definition.dcb.block_size = block_size;
            dataset
                .invoke(DatasetRequest::Define {
                    dataset: name.clone(),
                    definition: Box::new(definition),
                    mutation: mutation(910 + position),
                })
                .unwrap();
            assert!(matches!(
                dataset.invoke(DatasetRequest::Describe { dataset: name }),
                Ok(DatasetResult::Description(description))
                    if description.allocated_bytes == expected
                        && description.extents.len() == 1
                        && description.extents[0].length == expected
            ));
        }
    }

    #[test]
    fn all_record_formats_enforce_exact_positive_and_negative_boundaries() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        for (position, format) in [
            RecordFormat::Fixed,
            RecordFormat::FixedBlocked,
            RecordFormat::FixedBlockedStandard,
            RecordFormat::Variable,
            RecordFormat::VariableBlocked,
            RecordFormat::VariableSpanned,
            RecordFormat::VariableBlockedSpanned,
            RecordFormat::Undefined,
            RecordFormat::Line,
        ]
        .into_iter()
        .enumerate()
        {
            let spanned = matches!(
                format,
                RecordFormat::VariableSpanned | RecordFormat::VariableBlockedSpanned
            );
            let logical_record_length = if spanned { 600 } else { 4 };
            let name = DatasetName::new(format!("USER.FMT{position}"), 44).unwrap();
            let mut definition =
                mainframe_env_host_api::DatasetDefinition::compatibility(DatasetAttributes {
                    organization: if spanned {
                        DatasetOrganization::EntrySequenced
                    } else {
                        DatasetOrganization::Sequential
                    },
                    record_format: format,
                    logical_record_length,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                });
            definition.dcb.block_size = if matches!(
                format,
                RecordFormat::FixedBlocked | RecordFormat::FixedBlockedStandard
            ) {
                8
            } else {
                0
            };
            definition.vsam.spanned = spanned;
            if spanned {
                definition.vsam.control_interval_size = Some(512);
                definition.vsam.control_area_size = Some(8_192);
            }
            dataset
                .invoke(DatasetRequest::Define {
                    dataset: name.clone(),
                    definition: Box::new(definition),
                    mutation: mutation(1_000 + position as u64 * 3),
                })
                .unwrap();
            let valid = if spanned {
                vec![b'S'; 550]
            } else {
                match format {
                    RecordFormat::Variable | RecordFormat::VariableBlocked => b"ABC".to_vec(),
                    RecordFormat::Undefined => b"OPAQUE".to_vec(),
                    RecordFormat::Line => b"TEXT".to_vec(),
                    _ => b"DATA".to_vec(),
                }
            };
            dataset
                .invoke(DatasetRequest::Write {
                    dataset: name.clone(),
                    member: None,
                    records: vec![valid.clone()],
                    expected_version: Some(1),
                    mutation: mutation(1_001 + position as u64 * 3),
                })
                .unwrap();
            assert!(matches!(
                dataset.invoke(DatasetRequest::Read {
                    dataset: name.clone(),
                    member: None,
                    key: None,
                    max_records: 2,
                }),
                Ok(DatasetResult::Records { records, version: 2, .. })
                    if records == [valid]
            ));
            let invalid = match format {
                RecordFormat::Fixed
                | RecordFormat::FixedBlocked
                | RecordFormat::FixedBlockedStandard => Some(b"BAD".to_vec()),
                RecordFormat::Variable | RecordFormat::VariableBlocked => Some(b"EXCESS".to_vec()),
                RecordFormat::VariableSpanned | RecordFormat::VariableBlockedSpanned => {
                    Some(vec![b'X'; 601])
                }
                RecordFormat::Line => Some(b"A\nB".to_vec()),
                RecordFormat::Undefined => None,
            };
            if let Some(invalid) = invalid {
                let result = dataset.invoke(DatasetRequest::Write {
                    dataset: name,
                    member: None,
                    records: vec![invalid],
                    expected_version: Some(2),
                    mutation: mutation(1_002 + position as u64 * 3),
                });
                if format == RecordFormat::Line {
                    assert_eq!(result, Err(HostProblem::Malformed));
                } else {
                    assert!(matches!(
                        result,
                        Err(HostProblem::Condition { ref name, response: 22, .. })
                            if name == "LENGERR"
                    ));
                }
            }
        }
    }

    #[test]
    fn catalog_owner_expiration_retention_and_purge_are_enforced() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.PROTECT", 44).unwrap();
        let owner = principal("OWNER1");
        let other = principal("OWNER2");
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::Sequential,
        ));
        definition.catalog.owner = Some(owner.as_str().into());
        definition.catalog.creation_date = Some(2_026_001);
        definition.catalog.retention_days = Some(30);
        assert_eq!(
            dataset.invoke_for_principal(
                &other,
                DatasetRequest::Define {
                    dataset: name.clone(),
                    definition: Box::new(definition.clone()),
                    mutation: mutation(920),
                },
            ),
            Err(HostProblem::Unauthorized)
        );
        dataset
            .invoke_for_principal(
                &owner,
                DatasetRequest::Define {
                    dataset: name.clone(),
                    definition: Box::new(definition),
                    mutation: mutation(920),
                },
            )
            .unwrap();
        assert_eq!(
            dataset.invoke_for_principal(
                &other,
                DatasetRequest::Write {
                    dataset: name.clone(),
                    member: None,
                    records: vec![b"DATA".to_vec()],
                    expected_version: Some(1),
                    mutation: mutation(927),
                },
            ),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            dataset.invoke_for_principal(
                &other,
                DatasetRequest::RecordBackup {
                    dataset: name.clone(),
                    expected_version: Some(1),
                    mutation: mutation(928),
                },
            ),
            Err(HostProblem::Unauthorized)
        );
        assert!(matches!(
            dataset.invoke_for_principal(
                &owner,
                DatasetRequest::Delete {
                    dataset: name.clone(),
                    member: None,
                    expected_version: Some(1),
                    purge: false,
                    current_date: Some(2_026_010),
                    mutation: mutation(921),
                },
            ),
            Err(HostProblem::Condition { ref name, response: 8, .. }) if name == "PROTECTED"
        ));
        assert_eq!(
            dataset.invoke_for_principal(
                &other,
                DatasetRequest::Delete {
                    dataset: name.clone(),
                    member: None,
                    expected_version: Some(1),
                    purge: true,
                    current_date: None,
                    mutation: mutation(922),
                },
            ),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(
            dataset.invoke_for_principal(
                &owner,
                DatasetRequest::Delete {
                    dataset: name.clone(),
                    member: None,
                    expected_version: Some(1),
                    purge: false,
                    current_date: Some(2_026_031),
                    mutation: mutation(923),
                },
            ),
            Ok(DatasetResult::Mutated { version: 2 })
        );
        assert_eq!(
            dataset.invoke(DatasetRequest::Attributes { dataset: name }),
            Err(HostProblem::NotFound)
        );

        let expiring = DatasetName::new("USER.EXPIRE", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::Sequential,
        ));
        definition.catalog.owner = Some(owner.as_str().into());
        definition.catalog.creation_date = Some(2_024_365);
        definition.catalog.expiration_date = Some(2_025_002);
        dataset
            .invoke_for_principal(
                &owner,
                DatasetRequest::Define {
                    dataset: expiring.clone(),
                    definition: Box::new(definition),
                    mutation: mutation(924),
                },
            )
            .unwrap();
        assert!(matches!(
            dataset.invoke_for_principal(
                &owner,
                DatasetRequest::Delete {
                    dataset: expiring.clone(),
                    member: None,
                    expected_version: Some(1),
                    purge: false,
                    current_date: Some(2_025_001),
                    mutation: mutation(925),
                },
            ),
            Err(HostProblem::Condition { ref name, .. }) if name == "PROTECTED"
        ));
        assert!(matches!(
            dataset.invoke_for_principal(
                &owner,
                DatasetRequest::Delete {
                    dataset: expiring,
                    member: None,
                    expected_version: Some(1),
                    purge: true,
                    current_date: None,
                    mutation: mutation(926),
                },
            ),
            Ok(DatasetResult::Mutated { version: 2 })
        ));
    }

    #[test]
    fn recovery_required_blocks_updates_until_verified_closed() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.RECOVER", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(930),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: mutation(931),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Open,
                expected_version: Some(2),
                mutation: mutation(932),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::RecoveryRequired,
                expected_version: Some(3),
                mutation: mutation(933),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::RewriteRecord {
                dataset: name.clone(),
                key: b"AA".to_vec(),
                record: b"AA22".to_vec(),
                expected_version: Some(4),
                mutation: mutation(934),
            }),
            Err(HostProblem::Condition { ref name, response: 16, .. }) if name == "RECOVERY"
        ));
        assert_eq!(
            dataset.invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Closed,
                expected_version: Some(4),
                mutation: mutation(935),
            }),
            Ok(DatasetResult::Mutated { version: 5 })
        );
        assert_eq!(
            dataset.invoke(DatasetRequest::RewriteRecord {
                dataset: name,
                key: b"AA".to_vec(),
                record: b"AA22".to_vec(),
                expected_version: Some(5),
                mutation: mutation(936),
            }),
            Ok(DatasetResult::Mutated { version: 6 })
        );
    }

    #[test]
    fn lifecycle_close_atomically_releases_nontransactional_locks_across_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let name = DatasetName::new("USER.CLSLOCK", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Rls;
        dataset
            .invoke(DatasetRequest::Define {
                dataset: name.clone(),
                definition: Box::new(definition),
                mutation: mutation(937),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Open,
                expected_version: Some(1),
                mutation: mutation(938),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::AcquireLock {
                dataset: name.clone(),
                target: mainframe_env_host_api::DatasetLockTarget::Dataset,
                owner: principal("OWNER1"),
                mode: mainframe_env_host_api::DatasetLockMode::Exclusive,
                now_tick: 10,
                lease_ticks: 100,
                transaction: None,
                mutation: mutation(939),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListLocks {
                dataset: name.clone(),
                now_tick: 10,
                max_items: 8,
            }),
            Ok(DatasetResult::Locks { locks }) if locks.len() == 1
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::Rename {
                from: name.clone(),
                to: DatasetName::new("USER.NEWLOCK", 44).unwrap(),
                mutation: mutation(940),
            }),
            Err(HostProblem::Condition { ref name, response: 16, .. }) if name == "LOCKED"
        ));
        assert_eq!(
            dataset.invoke(DatasetRequest::SetLifecycle {
                dataset: name.clone(),
                state: mainframe_env_host_api::DatasetLifecycleState::Closed,
                expected_version: Some(2),
                mutation: mutation(941),
            }),
            Ok(DatasetResult::Mutated { version: 3 })
        );
        drop(dataset);
        let restarted = service(store.clone());
        assert!(matches!(
            restarted.invoke(DatasetRequest::ListLocks {
                dataset: name,
                now_tick: 10,
                max_items: 8,
            }),
            Ok(DatasetResult::Locks { locks }) if locks.is_empty()
        ));
        let deleted = DatasetName::new("USER.DELLOCK", 44).unwrap();
        let mut definition = mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
            DatasetOrganization::KeySequenced,
        ));
        definition.vsam.access_mode = mainframe_env_host_api::VsamAccessMode::Rls;
        restarted
            .invoke(DatasetRequest::Define {
                dataset: deleted.clone(),
                definition: Box::new(definition),
                mutation: mutation(942),
            })
            .unwrap();
        restarted
            .invoke(DatasetRequest::AcquireLock {
                dataset: deleted.clone(),
                target: mainframe_env_host_api::DatasetLockTarget::Dataset,
                owner: principal("OWNER1"),
                mode: mainframe_env_host_api::DatasetLockMode::Exclusive,
                now_tick: 20,
                lease_ticks: 100,
                transaction: None,
                mutation: mutation(943),
            })
            .unwrap();
        assert_eq!(
            restarted.invoke(DatasetRequest::Delete {
                dataset: deleted.clone(),
                member: None,
                expected_version: Some(1),
                purge: true,
                current_date: None,
                mutation: mutation(944),
            }),
            Ok(DatasetResult::Mutated { version: 2 })
        );
        drop(restarted);
        let reopened = service(store);
        assert_eq!(
            reopened.invoke(DatasetRequest::Attributes { dataset: deleted }),
            Err(HostProblem::NotFound)
        );
    }

    #[test]
    fn directory_blocks_bound_partitioned_entries_without_partial_member() {
        let dataset = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.SMALLPDS", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::Partitioned,
                    record_format: RecordFormat::Fixed,
                    logical_record_length: 4,
                    key_offset: None,
                    key_length: None,
                    ccsid: Some(37),
                },
                mutation: mutation(940),
            })
            .unwrap();
        for position in 0..6u64 {
            dataset
                .invoke(DatasetRequest::Write {
                    dataset: name.clone(),
                    member: Some(MemberName::new(format!("M{position:07}"), 8).unwrap()),
                    records: vec![b"DATA".to_vec()],
                    expected_version: Some(position + 1),
                    mutation: mutation(941 + position),
                })
                .unwrap();
        }
        assert!(matches!(
            dataset.invoke(DatasetRequest::Write {
                dataset: name.clone(),
                member: Some(MemberName::new("OVERFLOW", 8).unwrap()),
                records: vec![b"DATA".to_vec()],
                expected_version: Some(7),
                mutation: mutation(947),
            }),
            Err(HostProblem::Condition { ref name, response: 12, .. }) if name == "NOSPACE"
        ));
        assert!(matches!(
            dataset.invoke(DatasetRequest::ListMembers {
                dataset: name,
                start: None,
                max_items: 8,
            }),
            Ok(DatasetResult::Members { names, more: false }) if names.len() == 6
        ));
    }

    #[test]
    fn snapshots_restore_every_organization_without_flattening_and_survive_restart() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let cases = [
            (
                "USER.SNAP.SEQ",
                DatasetSnapshot {
                    definition: mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                        DatasetOrganization::Sequential,
                    )),
                    records: vec![b"SEQ1".to_vec()],
                    relative_records: Vec::new(),
                    members: Vec::new(),
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.KSDS",
                DatasetSnapshot {
                    definition: mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                        DatasetOrganization::KeySequenced,
                    )),
                    records: vec![b"K101".to_vec()],
                    relative_records: Vec::new(),
                    members: Vec::new(),
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.ESDS",
                DatasetSnapshot {
                    definition: mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                        DatasetOrganization::EntrySequenced,
                    )),
                    records: vec![b"ESD1".to_vec()],
                    relative_records: Vec::new(),
                    members: Vec::new(),
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.RRDS",
                DatasetSnapshot {
                    definition: mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                        DatasetOrganization::Relative,
                    )),
                    records: Vec::new(),
                    relative_records: vec![DatasetRelativeRecordSnapshot {
                        record_number: 7,
                        record: b"RRD7".to_vec(),
                    }],
                    members: Vec::new(),
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.VRRDS",
                DatasetSnapshot {
                    definition: {
                        let mut definition =
                            mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                                DatasetOrganization::VariableRelative,
                            ));
                        definition.attributes.record_format = RecordFormat::Variable;
                        definition
                    },
                    records: Vec::new(),
                    relative_records: vec![DatasetRelativeRecordSnapshot {
                        record_number: 9,
                        record: b"VR9".to_vec(),
                    }],
                    members: Vec::new(),
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.PDS",
                DatasetSnapshot {
                    definition: mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                        DatasetOrganization::Partitioned,
                    )),
                    records: Vec::new(),
                    relative_records: Vec::new(),
                    members: vec![DatasetMemberSnapshot {
                        name: MemberName::new("MEMBER", 8).unwrap(),
                        records: vec![b"PDS1".to_vec()],
                        generations: Vec::new(),
                        alias_of: None,
                    }],
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.PDSE",
                DatasetSnapshot {
                    definition: mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                        DatasetOrganization::PartitionedExtended,
                    )),
                    records: Vec::new(),
                    relative_records: Vec::new(),
                    members: vec![
                        DatasetMemberSnapshot {
                            name: MemberName::new("ALIAS", 8).unwrap(),
                            records: Vec::new(),
                            generations: Vec::new(),
                            alias_of: Some(MemberName::new("PROGRAM", 8).unwrap()),
                        },
                        DatasetMemberSnapshot {
                            name: MemberName::new("PROGRAM", 8).unwrap(),
                            records: Vec::new(),
                            generations: vec![DatasetMemberGenerationSnapshot {
                                generation: 3,
                                program_object: true,
                                records: vec![b"OBJ3".to_vec()],
                            }],
                            alias_of: None,
                        },
                    ],
                    linear_data: Vec::new(),
                },
            ),
            (
                "USER.SNAP.LDS",
                DatasetSnapshot {
                    definition: {
                        let mut definition =
                            mainframe_env_host_api::DatasetDefinition::compatibility(attrs(
                                DatasetOrganization::Linear,
                            ));
                        definition.attributes.record_format = RecordFormat::Undefined;
                        definition
                    },
                    records: Vec::new(),
                    relative_records: Vec::new(),
                    members: Vec::new(),
                    linear_data: b"LDS1".to_vec(),
                },
            ),
        ];
        for (position, (name, snapshot)) in cases.iter().enumerate() {
            assert_eq!(
                dataset.invoke(DatasetRequest::Restore {
                    dataset: DatasetName::new(*name, 44).unwrap(),
                    snapshot: Box::new(snapshot.clone()),
                    expected_version: None,
                    mutation: mutation(960 + position as u64),
                }),
                Ok(DatasetResult::Mutated { version: 1 })
            );
            assert!(matches!(
                dataset.invoke(DatasetRequest::Snapshot {
                    dataset: DatasetName::new(*name, 44).unwrap(),
                    max_records: 32,
                    max_members: 32,
                }),
                Ok(DatasetResult::Snapshot { snapshot: actual, version: 1 })
                    if actual.as_ref() == snapshot
            ));
        }
        drop(dataset);
        let reopened = service(store);
        for (name, snapshot) in cases {
            assert!(matches!(
                reopened.invoke(DatasetRequest::Snapshot {
                    dataset: DatasetName::new(name, 44).unwrap(),
                    max_records: 32,
                    max_members: 32,
                }),
                Ok(DatasetResult::Snapshot { snapshot: actual, version: 1 })
                    if actual.as_ref() == &snapshot
            ));
        }
    }

    #[test]
    fn alternate_index_delete_rejects_a_live_path_without_partial_invalidation() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let base = DatasetName::new("USER.PBASE", 44).unwrap();
        let index = DatasetName::new("USER.PBASE.AIX", 44).unwrap();
        let path = DatasetName::new("USER.PBASE.PATH", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: base.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(980),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base,
                index: index.clone(),
                key_offset: 2,
                key_length: 2,
                allow_duplicates: false,
                upgrade: true,
                mutation: mutation(981),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefinePath {
                path: path.clone(),
                index: index.clone(),
                mutation: mutation(982),
            })
            .unwrap();
        assert!(matches!(
            dataset.invoke(DatasetRequest::Delete {
                dataset: index.clone(),
                member: None,
                expected_version: Some(1),
                purge: true,
                current_date: None,
                mutation: mutation(983),
            }),
            Err(HostProblem::Condition { ref name, response: 16, .. }) if name == "INUSE"
        ));
        drop(dataset);
        let reopened = service(store);
        for expected in [index, path] {
            assert!(matches!(
                reopened.invoke(DatasetRequest::ListCatalog {
                    pattern: expected.as_str().into(),
                    start: None,
                    max_items: 8,
                }),
                Ok(DatasetResult::CatalogEntries { entries, more: false })
                    if entries.iter().any(|entry| entry.name == expected)
            ));
        }
    }

    #[test]
    fn rename_atomically_retargets_indexes_paths_and_catalog_aliases() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let dataset = service(store.clone());
        let base = DatasetName::new("USER.RBASE", 44).unwrap();
        let renamed = DatasetName::new("USER.RNEW", 44).unwrap();
        let index = DatasetName::new("USER.RBASE.AIX", 44).unwrap();
        let path = DatasetName::new("USER.RBASE.PATH", 44).unwrap();
        let alias = DatasetName::new("USER.RALIAS", 44).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: base.clone(),
                attributes: attrs(DatasetOrganization::KeySequenced),
                mutation: mutation(985),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::Write {
                dataset: base.clone(),
                member: None,
                records: vec![b"AA11".to_vec()],
                expected_version: Some(1),
                mutation: mutation(986),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlternateIndex {
                base: base.clone(),
                index: index.clone(),
                key_offset: 2,
                key_length: 2,
                allow_duplicates: false,
                upgrade: true,
                mutation: mutation(987),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefinePath {
                path: path.clone(),
                index: index.clone(),
                mutation: mutation(988),
            })
            .unwrap();
        dataset
            .invoke(DatasetRequest::DefineAlias {
                alias: alias.clone(),
                target: base.clone(),
                mutation: mutation(989),
            })
            .unwrap();
        assert_eq!(
            dataset.invoke(DatasetRequest::Rename {
                from: base.clone(),
                to: renamed.clone(),
                mutation: mutation(990),
            }),
            Ok(DatasetResult::Mutated { version: 3 })
        );
        drop(dataset);
        let reopened = service(store);
        assert_eq!(
            reopened.invoke(DatasetRequest::Attributes { dataset: base }),
            Err(HostProblem::NotFound)
        );
        assert!(matches!(
            reopened.invoke(DatasetRequest::Read {
                dataset: path,
                member: None,
                key: Some(b"11".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, version: 2, .. })
                if records == [b"AA11".to_vec()]
        ));
        assert!(matches!(
            reopened.invoke(DatasetRequest::ResolveCatalog { name: alias }),
            Ok(DatasetResult::Catalog(resolution))
                if resolution.resolved == renamed
        ));
    }

    #[test]
    fn missing_and_length_fail_typed() {
        let service = service(Arc::new(MemoryStore::new(Default::default())));
        let name = DatasetName::new("USER.DATA", 44).unwrap();
        assert_eq!(
            service.invoke(DatasetRequest::Read {
                dataset: name.clone(),
                member: None,
                key: None,
                max_records: 1
            }),
            Err(HostProblem::NotFound)
        );
        service
            .invoke(DatasetRequest::Create {
                dataset: name.clone(),
                attributes: attrs(DatasetOrganization::Sequential),
                mutation: mutation(1),
            })
            .unwrap();
        assert!(matches!(
            service.invoke(DatasetRequest::Write {
                dataset: name,
                member: None,
                records: vec![b"X".to_vec()],
                expected_version: Some(1),
                mutation: mutation(2)
            }),
            Err(HostProblem::Condition { response: 22, .. })
        ));
        let _ = HostLimits::default();
    }
}
