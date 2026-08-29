use crate::codec::{Entry, decode, encode};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, DatasetName, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    HostProblem, HostProvider, HostRequest, HostResult, MemberName,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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
#[derive(Clone, Debug, Eq, PartialEq)]
struct Replay {
    request_digest: [u8; 32],
    result: Option<DatasetResult>,
}
type BrowseIdentity = (Vec<u8>, Vec<u8>);
#[derive(Clone, Debug)]
struct Cursor {
    dataset: String,
    identities: Vec<BrowseIdentity>,
    index: isize,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct AlternateIndex {
    base: String,
    key_offset: u32,
    key_length: u32,
    allow_duplicates: bool,
    version: u64,
}
struct State {
    entries: BTreeMap<String, Entry>,
    alternate_indexes: BTreeMap<String, AlternateIndex>,
    cursors: BTreeMap<String, Cursor>,
    next_cursor: u64,
    replay: BTreeMap<String, Replay>,
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
            validate_records(&entry.records, &entry.attributes, limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
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
            let index = decode_alternate_index(&row.payload, row.version)?;
            let base = entries
                .get(&index.base)
                .ok_or(HostProblem::InfrastructureFailure)?;
            validate_alternate_index(base, &index)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            DatasetName::new(&row.key, 128).map_err(|_| HostProblem::InfrastructureFailure)?;
            alternate_indexes.insert(row.key, index);
        }
        if entries
            .len()
            .checked_add(alternate_indexes.len())
            .is_none_or(|total| total > limits.max_datasets)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(Arc::new(Self {
            store,
            limits,
            state: Mutex::new(State {
                entries,
                alternate_indexes,
                cursors: BTreeMap::new(),
                next_cursor: 1,
                replay,
            }),
        }))
    }
    pub fn invoke(&self, request: DatasetRequest) -> Result<DatasetResult, HostProblem> {
        let mutation = mutation(&request);
        let mut state = self
            .state
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let digest = request_digest(&request);
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
    fn apply(
        &self,
        state: &mut State,
        request: &DatasetRequest,
    ) -> Result<DatasetResult, HostProblem> {
        match request {
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
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                for name in source_names
                    .iter()
                    .filter(|name| wildcard(pattern, name))
                    .filter(|name| {
                        start
                            .as_ref()
                            .is_none_or(|start| name.as_str() > start.as_str())
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
                if entry.attributes.organization
                    != mainframe_env_host_api::DatasetOrganization::Partitioned
                {
                    return Err(HostProblem::Unsupported);
                }
                let mut names = Vec::new();
                let mut more = false;
                for name in entry.members.keys().filter(|name| {
                    start
                        .as_ref()
                        .is_none_or(|start| name.as_str() > start.as_str())
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
                    let records = if let Some(member) = member {
                        entry
                            .members
                            .get(member.as_str())
                            .cloned()
                            .ok_or(HostProblem::NotFound)?
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
                    .is_none_or(|total| total >= self.limits.max_datasets)
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                if state.entries.contains_key(dataset.as_str())
                    || state.alternate_indexes.contains_key(dataset.as_str())
                {
                    return Err(condition("DUPREC", 14));
                }
                let created = Entry {
                    attributes: attributes.clone(),
                    version: 1,
                    records: Vec::new(),
                    members: BTreeMap::new(),
                };
                self.persist(dataset.as_str(), &created, None)?;
                state.entries.insert(dataset.as_str().into(), created);
                Ok(DatasetResult::Created { version: 1 })
            }
            DatasetRequest::Write {
                dataset,
                member,
                records,
                expected_version,
                mutation,
            } => {
                validate_records(records, &entry(state, dataset)?.attributes, self.limits)?;
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let mut next = current.clone();
                next.version += 1;
                if let Some(member) = member {
                    if next.attributes.organization
                        != mainframe_env_host_api::DatasetOrganization::Partitioned
                    {
                        return Err(HostProblem::Unsupported);
                    }
                    next.members.insert(member.as_str().into(), records.clone());
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
                    request_digest(request),
                    &result,
                )?;
                Ok(result)
            }
            DatasetRequest::RewriteRecord {
                dataset,
                key,
                record,
                expected_version,
                mutation,
            } => {
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
                    request_digest(request),
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
                    request_digest(request),
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
                mutation,
            } => {
                if state
                    .entries
                    .len()
                    .checked_add(state.alternate_indexes.len())
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
                require_keyed(base_entry)?;
                validate_key_range(base_entry, *key_offset, *key_length)?;
                let definition = AlternateIndex {
                    base: base.as_str().into(),
                    key_offset: *key_offset,
                    key_length: *key_length,
                    allow_duplicates: *allow_duplicates,
                    version: 1,
                };
                validate_alternate_index(base_entry, &definition)?;
                let result = DatasetResult::Created { version: 1 };
                let replay = Replay {
                    request_digest: request_digest(request),
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
                state
                    .replay
                    .insert(mutation.idempotency_key.as_str().into(), replay);
                Ok(result)
            }
            DatasetRequest::Rename { from, to, .. } => {
                if state.entries.contains_key(to.as_str()) {
                    return Err(condition("DUPREC", 14));
                }
                let mut moved = entry(state, from)?.clone();
                let old = moved.version;
                moved.version += 1;
                self.store
                    .move_provider_state(
                        ProviderStateRecord {
                            namespace: "dataset".into(),
                            key: to.as_str().into(),
                            version: moved.version,
                            payload: encode(&moved)
                                .map_err(|_| HostProblem::InfrastructureFailure)?,
                        },
                        from.as_str(),
                        old,
                    )
                    .map_err(store_error)?;
                state.entries.remove(from.as_str());
                state.entries.insert(to.as_str().into(), moved.clone());
                Ok(DatasetResult::Mutated {
                    version: moved.version,
                })
            }
            DatasetRequest::Delete {
                dataset,
                member,
                expected_version,
                ..
            } => {
                let current = entry(state, dataset)?.clone();
                if expected_version.is_some_and(|expected| expected != current.version) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                if let Some(member) = member {
                    let mut next = current.clone();
                    if next.members.remove(member.as_str()).is_none() {
                        return Err(HostProblem::NotFound);
                    }
                    next.version += 1;
                    self.persist(dataset.as_str(), &next, Some(current.version))?;
                    state.entries.insert(dataset.as_str().into(), next.clone());
                    Ok(DatasetResult::Mutated {
                        version: next.version,
                    })
                } else {
                    if state
                        .alternate_indexes
                        .values()
                        .any(|index| index.base == dataset.as_str())
                    {
                        return Err(condition("INVREQ", 16));
                    }
                    self.store
                        .delete_provider_state("dataset", dataset.as_str(), current.version)
                        .map_err(store_error)?;
                    state.entries.remove(dataset.as_str());
                    Ok(DatasetResult::Mutated {
                        version: current.version + 1,
                    })
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
        if bytes(next) > self.limits.max_total_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut updated_indexes = Vec::new();
        for (name, index) in state
            .alternate_indexes
            .iter()
            .filter(|(_, index)| index.base == dataset)
        {
            validate_alternate_index(next, index)?;
            let mut updated = index.clone();
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
        if self.store.put_provider_states_atomic(writes).is_err() {
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
        if entry.attributes.organization != mainframe_env_host_api::DatasetOrganization::Partitioned
        {
            return Err(HostProblem::Unsupported);
        }
        let mut members = Vec::new();
        let mut more = false;
        for name in entry
            .members
            .keys()
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

fn ordered_records(entry: &Entry) -> Result<Vec<&Vec<u8>>, HostProblem> {
    let mut records = entry.records.iter().collect::<Vec<_>>();
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::KeySequenced {
        records.sort_by_key(|record| primary_key(entry, record).unwrap_or_default());
    }
    Ok(records)
}

fn record_identity(entry: &Entry, record: &[u8], position: usize) -> Result<Vec<u8>, HostProblem> {
    if entry.attributes.organization == mainframe_env_host_api::DatasetOrganization::KeySequenced {
        primary_key(entry, record)
    } else {
        Ok(u64::try_from(position)
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes()
            .to_vec())
    }
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
        return alternate_identities(entry_text(state, &index.base)?, index);
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

fn encode_alternate_index(index: &AlternateIndex) -> Result<Vec<u8>, HostProblem> {
    let mut payload = b"MEAIX1".to_vec();
    dataset_field(&mut payload, index.base.as_bytes())?;
    payload.extend_from_slice(&index.key_offset.to_be_bytes());
    payload.extend_from_slice(&index.key_length.to_be_bytes());
    payload.push(u8::from(index.allow_duplicates));
    Ok(payload)
}

fn decode_alternate_index(payload: &[u8], version: u64) -> Result<AlternateIndex, HostProblem> {
    if payload.get(..6) != Some(b"MEAIX1") || version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 6usize;
    let base = String::from_utf8(dataset_take_field(payload, &mut at, 128)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
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
    if at != payload.len() || base.is_empty() || key_length == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(AlternateIndex {
        base,
        key_offset,
        key_length,
        allow_duplicates,
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
        | DatasetRequest::Write { mutation, .. }
        | DatasetRequest::RewriteRecord { mutation, .. }
        | DatasetRequest::DeleteRecord { mutation, .. }
        | DatasetRequest::DefineAlternateIndex { mutation, .. }
        | DatasetRequest::Rename { mutation, .. }
        | DatasetRequest::Delete { mutation, .. } => Some(mutation),
        _ => None,
    }
}

fn atomic_dataset_request(request: &DatasetRequest) -> bool {
    matches!(
        request,
        DatasetRequest::Write { .. }
            | DatasetRequest::RewriteRecord { .. }
            | DatasetRequest::DeleteRecord { .. }
            | DatasetRequest::DefineAlternateIndex { .. }
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
    ) && records
        .iter()
        .any(|record| record.len() != attributes.logical_record_length as usize)
    {
        return Err(condition("LENGERR", 22));
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
}
fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
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

fn request_digest(request: &DatasetRequest) -> [u8; 32] {
    Sha256::digest(format!("{request:?}").as_bytes()).into()
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
        Some(_) => return Err(HostProblem::InfrastructureFailure),
    }
    Ok(payload)
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
    fn invoke(&self, _: &Invocation, request: EffectRequest) -> EffectResult {
        let sequence = request.sequence;
        let outcome = match request.request {
            HostRequest::Dataset(request) => self.service.invoke(request).map(HostResult::Dataset),
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
                    request_schema: "mainframe-env.host.dataset-request@1".into(),
                    result_schema: "mainframe-env.host.dataset-result@1".into(),
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
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    fn mutation(n: u64) -> Mutation {
        Mutation {
            sequence: n,
            idempotency_key: IdempotencyKey::new(format!("id-{n}"), InvocationLimits::default())
                .unwrap(),
            transaction: None,
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
                        request_digest: request_digest(&retry_request),
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
                dataset: duplicate_aix,
                member: None,
                key: Some(b"Z".to_vec()),
                max_records: 1,
            }),
            Ok(DatasetResult::Records { records, identities, .. })
                if records == [b"AAZ9".to_vec()] && identities == [b"AA".to_vec()]
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
