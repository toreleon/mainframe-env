use crate::codec::{Entry, decode, encode};
use mainframe_env_execution_api::{CapabilityId, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, DatasetName, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    HostProblem, HostProvider, HostRequest, HostResult,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
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
#[derive(Clone, Debug)]
struct Cursor {
    dataset: String,
    index: isize,
}
struct State {
    entries: BTreeMap<String, Entry>,
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
            let entry = decode(
                &row.payload,
                limits.max_records,
                limits.max_record_bytes,
                limits.max_members,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?;
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
        Ok(Arc::new(Self {
            store,
            limits,
            state: Mutex::new(State {
                entries,
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
            return if replay.request_digest != digest {
                Err(HostProblem::IdempotencyConflict)
            } else {
                replay.result.clone().ok_or(HostProblem::UnknownOutcome)
            };
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
                for name in state
                    .entries
                    .keys()
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
                let entry = entry(state, dataset)?;
                Ok(DatasetResult::Attributes {
                    attributes: entry.attributes.clone(),
                    version: entry.version,
                })
            }
            DatasetRequest::Read {
                dataset,
                member,
                key,
                max_records,
            } => {
                let entry = entry(state, dataset)?;
                let records = if let Some(member) = member {
                    entry
                        .members
                        .get(member.as_str())
                        .cloned()
                        .ok_or(HostProblem::NotFound)?
                } else if let Some(key) = key {
                    let (key_offset, key_length) = entry
                        .attributes
                        .key_offset
                        .zip(entry.attributes.key_length)
                        .ok_or(HostProblem::Unsupported)?;
                    entry
                        .records
                        .iter()
                        .find(|record| {
                            record.get(key_offset as usize..(key_offset + key_length) as usize)
                                == Some(key.as_slice())
                        })
                        .cloned()
                        .into_iter()
                        .collect()
                } else {
                    entry
                        .records
                        .iter()
                        .take(*max_records as usize)
                        .cloned()
                        .collect()
                };
                Ok(DatasetResult::Records {
                    records,
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
                if state.entries.len() >= self.limits.max_datasets {
                    return Err(HostProblem::ResourceExhausted);
                }
                if state.entries.contains_key(dataset.as_str()) {
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
                ..
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
                } else {
                    next.records = records.clone();
                }
                if bytes(&next) > self.limits.max_total_bytes {
                    return Err(HostProblem::ResourceExhausted);
                }
                self.persist(dataset.as_str(), &next, Some(current.version))?;
                state.entries.insert(dataset.as_str().into(), next.clone());
                Ok(DatasetResult::Mutated {
                    version: next.version,
                })
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
                let entry = entry(state, dataset)?;
                let index = entry
                    .records
                    .iter()
                    .position(|record| record.as_slice() >= key.as_slice())
                    .unwrap_or(entry.records.len());
                if state.cursors.len() >= self.limits.max_cursors {
                    return Err(HostProblem::ResourceExhausted);
                }
                let cursor = format!("cursor-{}", state.next_cursor);
                state.next_cursor += 1;
                state.cursors.insert(
                    cursor.clone(),
                    Cursor {
                        dataset: dataset.as_str().into(),
                        index: index as isize,
                    },
                );
                Ok(DatasetResult::Browse {
                    cursor,
                    record: None,
                })
            }
            DatasetRequest::ReadNext {
                dataset,
                cursor,
                reverse,
            } => {
                let position = {
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
                    current
                };
                let record = if position < 0 {
                    None
                } else {
                    entry(state, dataset)?
                        .records
                        .get(position as usize)
                        .cloned()
                };
                Ok(DatasetResult::Browse {
                    cursor: cursor.clone(),
                    record,
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
                })
            }
        }
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
fn mutation(request: &DatasetRequest) -> Option<&mainframe_env_host_api::Mutation> {
    match request {
        DatasetRequest::Create { mutation, .. }
        | DatasetRequest::Write { mutation, .. }
        | DatasetRequest::Rename { mutation, .. }
        | DatasetRequest::Delete { mutation, .. } => Some(mutation),
        _ => None,
    }
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
    fn invoke(&self, request: EffectRequest) -> EffectResult {
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
