use mainframe_env_execution_api::{
    CapabilityId, IdempotencyKey, Invocation, InvocationLimits, ServiceClass,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, ImsOperation, ImsRequest, ImsResult, ImsSegment, canonical_ims_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

const STATE_NAMESPACE: &str = "ims-state";
const STATE_KEY: &str = "catalog";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImsLimits {
    pub max_databases: usize,
    pub max_psbs: usize,
    pub max_pcbs: usize,
    pub max_segments: usize,
    pub max_roots: usize,
    pub max_children_per_root: usize,
    pub max_segment_bytes: usize,
    pub max_sessions: usize,
    pub max_checkpoints: usize,
    pub max_replays: usize,
    pub max_state_bytes: usize,
}

impl Default for ImsLimits {
    fn default() -> Self {
        Self {
            max_databases: 64,
            max_psbs: 256,
            max_pcbs: 64,
            max_segments: 64,
            max_roots: 65_536,
            max_children_per_root: 4_096,
            max_segment_bytes: 32 * 1024,
            max_sessions: 4_096,
            max_checkpoints: 65_536,
            max_replays: 65_536,
            max_state_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsSegmentDefinition {
    pub name: String,
    pub parent: Option<String>,
    pub length: usize,
    pub key_field: String,
    pub key_offset: usize,
    pub key_length: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsDatabaseDefinition {
    pub name: String,
    pub access: String,
    pub secondary_index: Option<String>,
    pub segments: Vec<ImsSegmentDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsPcbDefinition {
    pub name: String,
    pub database: String,
    pub processing_options: String,
    pub segments: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsPsbDefinition {
    pub name: String,
    pub pcbs: Vec<ImsPcbDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsApplicationDefinition {
    pub databases: Vec<ImsDatabaseDefinition>,
    pub psbs: Vec<ImsPsbDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsLoadRoot {
    pub data: Vec<u8>,
    pub children: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImsLoadImage {
    pub database: String,
    pub roots: Vec<ImsLoadRoot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsInstallReceipt {
    pub databases: usize,
    pub psbs: usize,
    pub pcbs: usize,
    pub identity: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct RootRecord {
    data: Vec<u8>,
    children: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct DatabaseState {
    roots: BTreeMap<String, RootRecord>,
    secondary_index: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum SegmentLocation {
    Root { key: String },
    Child { root_key: String, key: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Session {
    psb: String,
    pcb: u16,
    root_position: usize,
    child_position: usize,
    current_root: Option<String>,
    last: Option<SegmentLocation>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum ReplayDigestFormat {
    #[default]
    #[serde(rename = "legacy-debug@0")]
    LegacyDebugV0,
    #[serde(rename = "mainframe-env.provider-replay-canonical@1")]
    CanonicalHostV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RecordedResult {
    #[serde(default)]
    request_digest_format: ReplayDigestFormat,
    request_sha256: [u8; 32],
    status: String,
    segments: Vec<(String, Option<Vec<u8>>, Vec<u8>)>,
    checkpoint_id: Option<String>,
    affected_segments: u64,
}

impl RecordedResult {
    fn from_result(request_sha256: [u8; 32], result: &ImsResult) -> Self {
        Self {
            request_digest_format: ReplayDigestFormat::CanonicalHostV1,
            request_sha256,
            status: result.status.clone(),
            segments: result
                .segments
                .iter()
                .map(|segment| {
                    (
                        segment.name.clone(),
                        segment.parent_key.clone(),
                        segment.data.clone(),
                    )
                })
                .collect(),
            checkpoint_id: result.checkpoint_id.clone(),
            affected_segments: result.affected_segments,
        }
    }

    fn result(&self) -> ImsResult {
        ImsResult {
            status: self.status.clone(),
            segments: self
                .segments
                .iter()
                .cloned()
                .map(|(name, parent_key, data)| ImsSegment {
                    name,
                    parent_key,
                    data,
                })
                .collect(),
            checkpoint_id: self.checkpoint_id.clone(),
            affected_segments: self.affected_segments,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct State {
    definitions: Option<ImsApplicationDefinition>,
    databases: BTreeMap<String, DatabaseState>,
    sessions: BTreeMap<String, Session>,
    checkpoints: BTreeMap<String, Session>,
    replay: BTreeMap<String, RecordedResult>,
    #[serde(default)]
    pending_undo: BTreeMap<String, BTreeMap<String, DatabaseState>>,
}

struct DurableState {
    version: u64,
    state: State,
}

pub struct ImsService {
    store: Arc<dyn ProviderStateStore>,
    limits: ImsLimits,
    durable: Mutex<DurableState>,
}

impl ImsService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: ImsLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let (version, state) = match store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .map_err(store_error)?
        {
            Some(record) => (
                record.version,
                serde_json::from_slice(&record.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            ),
            None => (0, State::default()),
        };
        validate_state(&state, limits)?;
        Ok(Arc::new(Self {
            store,
            limits,
            durable: Mutex::new(DurableState { version, state }),
        }))
    }

    pub fn install(
        &self,
        definition: ImsApplicationDefinition,
    ) -> Result<ImsInstallReceipt, HostProblem> {
        validate_definition(&definition, self.limits)?;
        let identity = format!(
            "sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&definition).map_err(|_| HostProblem::ProviderFailure)?
            )
        );
        let mut durable = self.lock()?;
        if let Some(current) = &durable.state.definitions {
            if current != &definition {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(install_receipt(&definition, identity, true));
        }
        let mut next = durable.state.clone();
        for database in &definition.databases {
            next.databases.entry(normalize(&database.name)).or_default();
        }
        next.definitions = Some(definition.clone());
        self.persist(&mut durable, next)?;
        Ok(install_receipt(&definition, identity, false))
    }

    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
    ) -> Result<ImsResult, HostProblem> {
        let mut durable = self.lock()?;
        let request_sha256 = request_digest(request)?;
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str());
        if let Some(key) = replay_key
            && let Some(recorded) = durable.state.replay.get(key)
        {
            return match recorded.request_digest_format {
                ReplayDigestFormat::LegacyDebugV0 => Err(HostProblem::UnknownOutcome),
                ReplayDigestFormat::CanonicalHostV1
                    if recorded.request_sha256 == request_sha256 =>
                {
                    Ok(recorded.result())
                }
                ReplayDigestFormat::CanonicalHostV1 => Err(HostProblem::IdempotencyConflict),
            };
        }
        let mut next = durable.state.clone();
        let result = apply_request(
            &mut next,
            invocation.run_unit_id.as_str(),
            request,
            self.limits,
        )?;
        if invocation.service_class == ServiceClass::Batch
            && matches!(
                request.operation,
                ImsOperation::Insert | ImsOperation::Replace | ImsOperation::Delete
            )
        {
            next.pending_undo.remove(invocation.run_unit_id.as_str());
        }
        if request.operation.is_mutating() {
            let key = replay_key.ok_or(HostProblem::MissingIdempotency)?;
            if next.replay.len() >= self.limits.max_replays {
                return Err(HostProblem::ResourceExhausted);
            }
            next.replay.insert(
                key.into(),
                RecordedResult::from_result(request_sha256, &result),
            );
            validate_state(&next, self.limits)?;
            self.persist(&mut durable, next)?;
        }
        Ok(result)
    }

    /// Bind a retained pre-canonical replay receipt to a reviewed typed request.
    ///
    /// Legacy receipts are never replayed or redispatched implicitly. The caller
    /// must attest the exact retained digest before this metadata-only migration.
    pub fn reconcile_legacy_replay(
        &self,
        key: &IdempotencyKey,
        expected_legacy_digest: [u8; 32],
        request: &ImsRequest,
    ) -> Result<(), HostProblem> {
        if !request.operation.is_mutating()
            || request
                .mutation
                .as_ref()
                .map(|mutation| &mutation.idempotency_key)
                != Some(key)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let canonical = request_digest(request)?;
        let mut durable = self.lock()?;
        let retained = durable
            .state
            .replay
            .get(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        match retained.request_digest_format {
            ReplayDigestFormat::CanonicalHostV1 => {
                return if retained.request_sha256 == canonical {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            ReplayDigestFormat::LegacyDebugV0
                if retained.request_sha256 != expected_legacy_digest =>
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            ReplayDigestFormat::LegacyDebugV0 => {}
        }
        let mut next = durable.state.clone();
        let retained = next
            .replay
            .get_mut(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        retained.request_digest_format = ReplayDigestFormat::CanonicalHostV1;
        retained.request_sha256 = canonical;
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)
    }

    pub fn hierarchy(&self, database: &str) -> Result<Vec<ImsLoadRoot>, HostProblem> {
        let durable = self.lock()?;
        let database = durable
            .state
            .databases
            .get(&normalize(database))
            .ok_or(HostProblem::NotFound)?;
        Ok(database
            .roots
            .values()
            .map(|root| ImsLoadRoot {
                data: root.data.clone(),
                children: root.children.values().cloned().collect(),
            })
            .collect())
    }

    pub fn checkpoint_count(&self) -> Result<usize, HostProblem> {
        Ok(self.lock()?.state.checkpoints.len())
    }

    pub fn secondary_index_entries(&self, database: &str) -> Result<usize, HostProblem> {
        self.lock()?
            .state
            .databases
            .get(&normalize(database))
            .map(|database| database.secondary_index.len())
            .ok_or(HostProblem::NotFound)
    }

    fn lock(&self) -> Result<MutexGuard<'_, DurableState>, HostProblem> {
        self.durable
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn persist(&self, durable: &mut DurableState, state: State) -> Result<(), HostProblem> {
        let payload = serde_json::to_vec(&state).map_err(|_| HostProblem::ProviderFailure)?;
        if payload.len() > self.limits.max_state_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let version = durable
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version,
                    payload,
                },
                (durable.version != 0).then_some(durable.version),
            )
            .map_err(store_error)?;
        durable.version = version;
        durable.state = state;
        Ok(())
    }
}

fn install_receipt(
    definition: &ImsApplicationDefinition,
    identity: String,
    replayed: bool,
) -> ImsInstallReceipt {
    ImsInstallReceipt {
        databases: definition.databases.len(),
        psbs: definition.psbs.len(),
        pcbs: definition.psbs.iter().map(|psb| psb.pcbs.len()).sum(),
        identity,
        replayed,
    }
}

fn apply_request(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    match request.operation {
        ImsOperation::Schedule => schedule(state, run, request, limits),
        ImsOperation::Terminate => terminate(state, run),
        ImsOperation::GetUnique => get_unique(state, run, request),
        ImsOperation::GetNext => get_next(state, run, request),
        ImsOperation::GetNextParent => get_next_parent(state, run, request),
        ImsOperation::Insert => insert(state, run, request, limits),
        ImsOperation::Replace => replace(state, run, request),
        ImsOperation::Delete => delete(state, run),
        ImsOperation::Checkpoint => checkpoint(state, run, request, limits),
        ImsOperation::Load => load(state, request, limits),
        ImsOperation::Unload => unload(state, run, request),
        ImsOperation::Commit => {
            state.pending_undo.remove(run);
            Ok(status("  "))
        }
        ImsOperation::Rollback => {
            if let Some(databases) = state.pending_undo.remove(run) {
                state.databases = databases;
            }
            Ok(status("  "))
        }
    }
}

fn schedule(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    if state.sessions.contains_key(run) {
        return Ok(status("TC"));
    }
    if state.sessions.len() >= limits.max_sessions {
        return Err(HostProblem::ResourceExhausted);
    }
    let psb = normalize(request.psb.as_deref().ok_or(HostProblem::Malformed)?);
    let definition = state
        .definitions
        .as_ref()
        .and_then(|definition| {
            definition
                .psbs
                .iter()
                .find(|value| normalize(&value.name) == psb)
        })
        .ok_or(HostProblem::NotFound)?;
    if usize::from(request.pcb) > definition.pcbs.len() {
        return Ok(status("AK"));
    }
    state.sessions.insert(
        run.into(),
        Session {
            psb,
            pcb: request.pcb,
            root_position: 0,
            child_position: 0,
            current_root: None,
            last: None,
        },
    );
    Ok(status("  "))
}

fn terminate(state: &mut State, run: &str) -> Result<ImsResult, HostProblem> {
    state.sessions.remove(run).ok_or(HostProblem::NotFound)?;
    Ok(status("  "))
}

fn get_unique(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let (database_name, root_definition, child_definition) = context(state, run)?;
    let target = request
        .segments
        .last()
        .map(|value| normalize(value))
        .unwrap_or_else(|| root_definition.name.clone());
    let database = state
        .databases
        .get(&database_name)
        .ok_or(HostProblem::NotFound)?;
    if target == root_definition.name {
        let key = qualifier_key(request, &root_definition)
            .or_else(|| database.roots.keys().next().cloned());
        let Some(key) = key else {
            return Ok(status("GE"));
        };
        let Some(root) = database.roots.get(&key) else {
            return Ok(status("GE"));
        };
        let result = segment(&root_definition.name, None, root.data.clone());
        let session = state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?;
        session.current_root = Some(key.clone());
        session.child_position = 0;
        session.last = Some(SegmentLocation::Root { key });
        return Ok(result);
    }
    let child_definition = child_definition.ok_or(HostProblem::NotFound)?;
    if target != child_definition.name {
        return Ok(status("AK"));
    }
    let root_key = qualifier_key(request, &root_definition).or_else(|| {
        state
            .sessions
            .get(run)
            .and_then(|session| session.current_root.clone())
    });
    let Some(root_key) = root_key else {
        return Ok(status("GE"));
    };
    let child_key = qualifier_key(request, &child_definition);
    let root = database.roots.get(&root_key).ok_or(HostProblem::NotFound)?;
    let child = child_key
        .as_ref()
        .and_then(|key| {
            root.children
                .get(key)
                .map(|data| (key.clone(), data.clone()))
        })
        .or_else(|| {
            root.children
                .iter()
                .next()
                .map(|(key, data)| (key.clone(), data.clone()))
        });
    let Some((child_key, data)) = child else {
        return Ok(status("GE"));
    };
    let result = segment(&child_definition.name, Some(decode_key(&root_key)?), data);
    let session = state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?;
    session.current_root = Some(root_key.clone());
    session.last = Some(SegmentLocation::Child {
        root_key,
        key: child_key,
    });
    Ok(result)
}

fn get_next(state: &mut State, run: &str, _request: &ImsRequest) -> Result<ImsResult, HostProblem> {
    let (database_name, root_definition, _) = context(state, run)?;
    let database = state
        .databases
        .get(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let keys = database.roots.keys().cloned().collect::<Vec<_>>();
    let session = state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?;
    let Some(key) = keys.get(session.root_position).cloned() else {
        return Ok(status("GB"));
    };
    session.root_position += 1;
    session.child_position = 0;
    session.current_root = Some(key.clone());
    session.last = Some(SegmentLocation::Root { key: key.clone() });
    Ok(segment(
        &root_definition.name,
        None,
        database.roots[&key].data.clone(),
    ))
}

fn get_next_parent(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
) -> Result<ImsResult, HostProblem> {
    let (database_name, _, child_definition) = context(state, run)?;
    let child_definition = child_definition.ok_or(HostProblem::NotFound)?;
    if request
        .segments
        .last()
        .is_some_and(|segment| normalize(segment) != child_definition.name)
    {
        return Ok(status("AK"));
    }
    let session_snapshot = state
        .sessions
        .get(run)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let root_key = session_snapshot.current_root.ok_or(HostProblem::NotFound)?;
    let database = state
        .databases
        .get(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let root = database.roots.get(&root_key).ok_or(HostProblem::NotFound)?;
    let children = root.children.iter().collect::<Vec<_>>();
    let Some((key, data)) = children.get(session_snapshot.child_position) else {
        return Ok(status("GE"));
    };
    let result = segment(
        &child_definition.name,
        Some(decode_key(&root_key)?),
        (*data).clone(),
    );
    let session = state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?;
    session.child_position += 1;
    session.last = Some(SegmentLocation::Child {
        root_key,
        key: (*key).clone(),
    });
    Ok(result)
}

fn insert(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    begin_unit(state, run);
    let (database_name, root_definition, child_definition) = context(state, run)?;
    let target = request
        .segments
        .last()
        .map(|value| normalize(value))
        .ok_or(HostProblem::Malformed)?;
    let database = state
        .databases
        .get_mut(&database_name)
        .ok_or(HostProblem::NotFound)?;
    if target == root_definition.name {
        validate_segment_data(&request.data, &root_definition, limits)?;
        let key = data_key(&request.data, &root_definition)?;
        if database.roots.contains_key(&key) {
            return Ok(status("II"));
        }
        if database.roots.len() >= limits.max_roots {
            return Err(HostProblem::ResourceExhausted);
        }
        database.roots.insert(
            key.clone(),
            RootRecord {
                data: request.data.clone(),
                children: BTreeMap::new(),
            },
        );
        database.secondary_index.insert(key.clone(), key.clone());
        state
            .sessions
            .get_mut(run)
            .ok_or(HostProblem::NotFound)?
            .last = Some(SegmentLocation::Root { key });
        return Ok(affected(1));
    }
    let child_definition = child_definition.ok_or(HostProblem::NotFound)?;
    if target != child_definition.name {
        return Ok(status("AK"));
    }
    validate_segment_data(&request.data, &child_definition, limits)?;
    let root_key = qualifier_key(request, &root_definition)
        .or_else(|| {
            state
                .sessions
                .get(run)
                .and_then(|session| session.current_root.clone())
        })
        .ok_or(HostProblem::NotFound)?;
    let child_key = data_key(&request.data, &child_definition)?;
    let root = database
        .roots
        .get_mut(&root_key)
        .ok_or(HostProblem::NotFound)?;
    if root.children.contains_key(&child_key) {
        return Ok(status("II"));
    }
    if root.children.len() >= limits.max_children_per_root {
        return Err(HostProblem::ResourceExhausted);
    }
    root.children
        .insert(child_key.clone(), request.data.clone());
    state
        .sessions
        .get_mut(run)
        .ok_or(HostProblem::NotFound)?
        .last = Some(SegmentLocation::Child {
        root_key,
        key: child_key,
    });
    Ok(affected(1))
}

fn replace(state: &mut State, run: &str, request: &ImsRequest) -> Result<ImsResult, HostProblem> {
    begin_unit(state, run);
    let (database_name, root_definition, child_definition) = context(state, run)?;
    let location = state
        .sessions
        .get(run)
        .and_then(|session| session.last.clone())
        .ok_or(HostProblem::NotFound)?;
    let database = state
        .databases
        .get_mut(&database_name)
        .ok_or(HostProblem::NotFound)?;
    match location {
        SegmentLocation::Root { key } => {
            if data_key(&request.data, &root_definition)? != key {
                return Ok(status("DA"));
            }
            database
                .roots
                .get_mut(&key)
                .ok_or(HostProblem::NotFound)?
                .data = request.data.clone();
        }
        SegmentLocation::Child { root_key, key } => {
            let definition = child_definition.ok_or(HostProblem::NotFound)?;
            if data_key(&request.data, &definition)? != key {
                return Ok(status("DA"));
            }
            *database
                .roots
                .get_mut(&root_key)
                .and_then(|root| root.children.get_mut(&key))
                .ok_or(HostProblem::NotFound)? = request.data.clone();
        }
    }
    Ok(affected(1))
}

fn delete(state: &mut State, run: &str) -> Result<ImsResult, HostProblem> {
    begin_unit(state, run);
    let (database_name, _, _) = context(state, run)?;
    let location = state
        .sessions
        .get(run)
        .and_then(|session| session.last.clone())
        .ok_or(HostProblem::NotFound)?;
    let database = state
        .databases
        .get_mut(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let (removed, next_location) = match location {
        SegmentLocation::Root { key } => {
            database.secondary_index.retain(|_, root| root != &key);
            (database.roots.remove(&key).is_some(), None)
        }
        SegmentLocation::Child { root_key, key } => (
            database
                .roots
                .get_mut(&root_key)
                .and_then(|root| root.children.remove(&key))
                .is_some(),
            Some(SegmentLocation::Root { key: root_key }),
        ),
    };
    if !removed {
        return Ok(status("GE"));
    }
    if let Some(session) = state.sessions.get_mut(run) {
        match next_location {
            Some(location) => {
                session.child_position = session.child_position.saturating_sub(1);
                session.last = Some(location);
            }
            None => {
                session.root_position = session.root_position.saturating_sub(1);
                session.current_root = None;
                session.last = None;
            }
        }
    }
    Ok(affected(1))
}

fn checkpoint(
    state: &mut State,
    run: &str,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let id = request
        .checkpoint_id
        .clone()
        .ok_or(HostProblem::Malformed)?;
    if state.checkpoints.len() >= limits.max_checkpoints && !state.checkpoints.contains_key(&id) {
        return Err(HostProblem::ResourceExhausted);
    }
    let session = state
        .sessions
        .get(run)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    state.checkpoints.insert(id.clone(), session);
    Ok(ImsResult {
        status: "  ".into(),
        segments: Vec::new(),
        checkpoint_id: Some(id),
        affected_segments: 0,
    })
}

fn load(
    state: &mut State,
    request: &ImsRequest,
    limits: ImsLimits,
) -> Result<ImsResult, HostProblem> {
    let image: ImsLoadImage =
        serde_json::from_slice(&request.data).map_err(|_| HostProblem::Malformed)?;
    let database_name = normalize(&image.database);
    let (_, root_definition, child_definition) = database_definition(state, &database_name)?;
    let child_definition = child_definition.ok_or(HostProblem::NotFound)?;
    if image.roots.len() > limits.max_roots {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut database = DatabaseState::default();
    let mut affected_count = 0u64;
    for root in image.roots {
        validate_segment_data(&root.data, &root_definition, limits)?;
        if root.children.len() > limits.max_children_per_root {
            return Err(HostProblem::ResourceExhausted);
        }
        let root_key = data_key(&root.data, &root_definition)?;
        let mut record = RootRecord {
            data: root.data,
            children: BTreeMap::new(),
        };
        for child in root.children {
            validate_segment_data(&child, &child_definition, limits)?;
            let child_key = data_key(&child, &child_definition)?;
            if record.children.insert(child_key, child).is_some() {
                return Err(HostProblem::Malformed);
            }
            affected_count += 1;
        }
        if database.roots.insert(root_key.clone(), record).is_some() {
            return Err(HostProblem::Malformed);
        }
        database.secondary_index.insert(root_key.clone(), root_key);
        affected_count += 1;
    }
    state.databases.insert(database_name, database);
    Ok(affected(affected_count))
}

fn unload(state: &State, run: &str, request: &ImsRequest) -> Result<ImsResult, HostProblem> {
    let database_name = request
        .psb
        .as_ref()
        .map(|name| normalize(name))
        .or_else(|| context(state, run).ok().map(|context| context.0))
        .ok_or(HostProblem::Malformed)?;
    let (_, root_definition, child_definition) = database_definition(state, &database_name)?;
    let database = state
        .databases
        .get(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let mut segments = Vec::new();
    for (root_key, root) in &database.roots {
        segments.push(ImsSegment {
            name: root_definition.name.clone(),
            parent_key: None,
            data: root.data.clone(),
        });
        if let Some(child_definition) = &child_definition {
            for child in root.children.values() {
                segments.push(ImsSegment {
                    name: child_definition.name.clone(),
                    parent_key: Some(decode_key(root_key)?),
                    data: child.clone(),
                });
            }
        }
        if segments.len() >= request.max_segments as usize {
            break;
        }
    }
    Ok(ImsResult {
        status: "  ".into(),
        segments,
        checkpoint_id: None,
        affected_segments: 0,
    })
}

fn context(
    state: &State,
    run: &str,
) -> Result<(String, ImsSegmentDefinition, Option<ImsSegmentDefinition>), HostProblem> {
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    let definitions = state.definitions.as_ref().ok_or(HostProblem::NotFound)?;
    let psb = definitions
        .psbs
        .iter()
        .find(|psb| normalize(&psb.name) == session.psb)
        .ok_or(HostProblem::NotFound)?;
    let pcb = psb
        .pcbs
        .get(usize::from(session.pcb) - 1)
        .ok_or(HostProblem::NotFound)?;
    let database_name = normalize(&pcb.database);
    let (_, root, child) = database_definition(state, &database_name)?;
    Ok((database_name, root, child))
}

fn begin_unit(state: &mut State, run: &str) {
    if !state.pending_undo.contains_key(run) {
        state
            .pending_undo
            .insert(run.into(), state.databases.clone());
    }
}

fn database_definition(
    state: &State,
    database_name: &str,
) -> Result<
    (
        ImsDatabaseDefinition,
        ImsSegmentDefinition,
        Option<ImsSegmentDefinition>,
    ),
    HostProblem,
> {
    let database = state
        .definitions
        .as_ref()
        .and_then(|definition| {
            definition
                .databases
                .iter()
                .find(|database| normalize(&database.name) == normalize(database_name))
        })
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let root = database
        .segments
        .iter()
        .find(|segment| segment.parent.is_none())
        .cloned()
        .ok_or(HostProblem::Malformed)?;
    let child = database
        .segments
        .iter()
        .find(|segment| {
            segment
                .parent
                .as_ref()
                .is_some_and(|parent| normalize(parent) == normalize(&root.name))
        })
        .cloned();
    Ok((database, root, child))
}

fn qualifier_key(request: &ImsRequest, definition: &ImsSegmentDefinition) -> Option<String> {
    request
        .qualifiers
        .iter()
        .find(|qualifier| normalize(&qualifier.field) == normalize(&definition.key_field))
        .map(|qualifier| encode_key(&qualifier.value))
}

fn validate_segment_data(
    data: &[u8],
    definition: &ImsSegmentDefinition,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if data.len() != definition.length || data.len() > limits.max_segment_bytes {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn data_key(data: &[u8], definition: &ImsSegmentDefinition) -> Result<String, HostProblem> {
    let end = definition
        .key_offset
        .checked_add(definition.key_length)
        .ok_or(HostProblem::Malformed)?;
    data.get(definition.key_offset..end)
        .map(encode_key)
        .ok_or(HostProblem::Malformed)
}

fn encode_key(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_key(value: &str) -> Result<Vec<u8>, HostProblem> {
    if !value.len().is_multiple_of(2) {
        return Err(HostProblem::InfrastructureFailure);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| HostProblem::InfrastructureFailure)
        })
        .collect()
}

fn status(status: &str) -> ImsResult {
    ImsResult {
        status: status.into(),
        segments: Vec::new(),
        checkpoint_id: None,
        affected_segments: 0,
    }
}

fn affected(count: u64) -> ImsResult {
    ImsResult {
        affected_segments: count,
        ..status("  ")
    }
}

fn segment(name: &str, parent_key: Option<Vec<u8>>, data: Vec<u8>) -> ImsResult {
    ImsResult {
        status: "  ".into(),
        segments: vec![ImsSegment {
            name: name.into(),
            parent_key,
            data,
        }],
        checkpoint_id: None,
        affected_segments: 0,
    }
}

fn validate_definition(
    definition: &ImsApplicationDefinition,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if definition.databases.is_empty()
        || definition.databases.len() > limits.max_databases
        || definition.psbs.is_empty()
        || definition.psbs.len() > limits.max_psbs
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let databases = definition
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    if databases.len() != definition.databases.len() {
        return Err(HostProblem::Malformed);
    }
    for database in &definition.databases {
        if !matches!(normalize(&database.access).as_str(), "HIDAM" | "INDEX")
            || database.segments.is_empty()
            || database.segments.len() > limits.max_segments
        {
            return Err(HostProblem::Unsupported);
        }
        let names = database
            .segments
            .iter()
            .map(|segment| normalize(&segment.name))
            .collect::<BTreeSet<_>>();
        if names.len() != database.segments.len()
            || database.segments.iter().any(|segment| {
                segment.name.is_empty()
                    || segment.length == 0
                    || segment.length > limits.max_segment_bytes
                    || segment.key_length == 0
                    || segment
                        .key_offset
                        .checked_add(segment.key_length)
                        .is_none_or(|end| end > segment.length)
                    || segment
                        .parent
                        .as_ref()
                        .is_some_and(|parent| !names.contains(&normalize(parent)))
            })
        {
            return Err(HostProblem::Malformed);
        }
    }
    let mut psb_names = BTreeSet::new();
    for psb in &definition.psbs {
        if !psb_names.insert(normalize(&psb.name))
            || psb.pcbs.is_empty()
            || psb.pcbs.len() > limits.max_pcbs
            || psb.pcbs.iter().any(|pcb| {
                !databases.contains(&normalize(&pcb.database))
                    || pcb.segments.is_empty()
                    || pcb.processing_options.is_empty()
            })
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn validate_state(state: &State, limits: ImsLimits) -> Result<(), HostProblem> {
    if state.sessions.len() > limits.max_sessions
        || state.checkpoints.len() > limits.max_checkpoints
        || state.replay.len() > limits.max_replays
        || state.pending_undo.len() > limits.max_sessions
        || state.databases.len() > limits.max_databases
        || state.databases.values().any(|database| {
            database.roots.len() > limits.max_roots
                || database.roots.values().any(|root| {
                    root.data.len() > limits.max_segment_bytes
                        || root.children.len() > limits.max_children_per_root
                        || root
                            .children
                            .values()
                            .any(|child| child.len() > limits.max_segment_bytes)
                })
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn request_digest(request: &ImsRequest) -> Result<[u8; 32], HostProblem> {
    canonical_ims_request_digest(request)
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn store_error(problem: StoreError) -> HostProblem {
    match problem {
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}

struct ImsProvider {
    service: Arc<ImsService>,
    descriptor: CapabilityDescriptor,
}

impl HostProvider for ImsProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let sequence = effect.sequence;
        let outcome = match effect.request {
            HostRequest::Ims(request) => self
                .service
                .execute(invocation, &request)
                .map(HostResult::Ims),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult { sequence, outcome }
    }
}

pub fn ims_providers(
    service: Arc<ImsService>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.ims.read", "host.ims.write"]
        .into_iter()
        .map(|capability| {
            Arc::new(ImsProvider {
                service: service.clone(),
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits)
                        .expect("static IMS capability"),
                    provider_id: "mainframe-env-ims".into(),
                    generation: "1".into(),
                    request_schema: "mainframe-env.ims-request@1".into(),
                    result_schema: "mainframe-env.ims-result@1".into(),
                    max_request_bytes: 4 * 1024 * 1024,
                    max_result_bytes: 16 * 1024 * 1024,
                    ready: true,
                },
            }) as Arc<dyn HostProvider>
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::{ImsQualifier, Mutation};
    use mainframe_env_store::MemoryStore;

    fn definition() -> ImsApplicationDefinition {
        ImsApplicationDefinition {
            databases: vec![ImsDatabaseDefinition {
                name: "AUTHDB".into(),
                access: "HIDAM".into(),
                secondary_index: Some("AUTHX".into()),
                segments: vec![
                    ImsSegmentDefinition {
                        name: "ROOT".into(),
                        parent: None,
                        length: 10,
                        key_field: "ROOTKEY".into(),
                        key_offset: 0,
                        key_length: 6,
                    },
                    ImsSegmentDefinition {
                        name: "CHILD".into(),
                        parent: Some("ROOT".into()),
                        length: 12,
                        key_field: "CHILDKEY".into(),
                        key_offset: 0,
                        key_length: 8,
                    },
                ],
            }],
            psbs: vec![ImsPsbDefinition {
                name: "AUTHPSB".into(),
                pcbs: vec![ImsPcbDefinition {
                    name: "AUTHPCB".into(),
                    database: "AUTHDB".into(),
                    processing_options: "AP".into(),
                    segments: vec!["ROOT".into(), "CHILD".into()],
                }],
            }],
        }
    }

    fn invocation(run: &str) -> Invocation {
        let limits = InvocationLimits::default();
        let grants = ["host.ims.read", "host.ims.write"]
            .into_iter()
            .map(|capability| CapabilityId::new(capability, limits).unwrap())
            .collect();
        Invocation::new(
            RequestId::new(format!("request-{run}"), limits).unwrap(),
            ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
            RunUnitId::new(run, limits).unwrap(),
            None,
            Selector::new("ims:test", limits).unwrap(),
            ArtifactRef::new("ims:test", limits).unwrap(),
            Principal::new(PrincipalId::new("IBMUSER", limits).unwrap(), grants, limits).unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new(format!("trace-{run}"), limits).unwrap(),
            IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap()
    }

    fn request(
        operation: ImsOperation,
        sequence: u64,
        segments: &[&str],
        data: &[u8],
        qualifiers: Vec<ImsQualifier>,
    ) -> ImsRequest {
        let limits = InvocationLimits::default();
        ImsRequest {
            operation,
            psb: (operation == ImsOperation::Schedule).then(|| "AUTHPSB".into()),
            pcb: 1,
            segments: segments.iter().map(|value| (*value).into()).collect(),
            data: data.to_vec(),
            qualifiers,
            checkpoint_id: (operation == ImsOperation::Checkpoint).then(|| "CHK00001".into()),
            max_segments: 64,
            mutation: operation.is_mutating().then(|| Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(format!("effect-{sequence}"), limits).unwrap(),
                transaction: Some("IMS-TEST".into()),
            }),
        }
    }

    fn retain_as_legacy_replay(
        store: &dyn ProviderStateStore,
        key: &IdempotencyKey,
        digest: [u8; 32],
    ) {
        let row = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let replay = state["replay"][key.as_str()].as_object_mut().unwrap();
        replay.remove("request_digest_format");
        replay.insert("request_sha256".into(), serde_json::json!(digest));
        let version = row.version;
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: version + 1,
                    payload: serde_json::to_vec(&state).unwrap(),
                    ..row
                },
                Some(version),
            )
            .unwrap();
    }

    fn qualifier(segment: &str, field: &str, value: &[u8]) -> ImsQualifier {
        ImsQualifier {
            segment: segment.into(),
            field: field.into(),
            value: value.to_vec(),
        }
    }

    #[test]
    fn hierarchy_ssa_checkpoint_load_unload_and_restart_are_durable() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        assert!(!service.install(definition()).unwrap().replayed);
        assert!(service.install(definition()).unwrap().replayed);
        let invocation = invocation("run-a");
        assert_eq!(
            service
                .execute(
                    &invocation,
                    &request(ImsOperation::Schedule, 1, &[], &[], Vec::new())
                )
                .unwrap()
                .status,
            "  "
        );
        let root = b"000001ROOT";
        service
            .execute(
                &invocation,
                &request(ImsOperation::Insert, 2, &["ROOT"], root, Vec::new()),
            )
            .unwrap();
        let child = b"00000001AUTH";
        service
            .execute(
                &invocation,
                &request(
                    ImsOperation::Insert,
                    3,
                    &["ROOT", "CHILD"],
                    child,
                    vec![qualifier("ROOT", "ROOTKEY", b"000001")],
                ),
            )
            .unwrap();
        let root_result = service
            .execute(
                &invocation,
                &request(
                    ImsOperation::GetUnique,
                    4,
                    &["ROOT"],
                    &[],
                    vec![qualifier("ROOT", "ROOTKEY", b"000001")],
                ),
            )
            .unwrap();
        assert_eq!(root_result.segments[0].data, root);
        let child_result = service
            .execute(
                &invocation,
                &request(ImsOperation::GetNextParent, 5, &["CHILD"], &[], Vec::new()),
            )
            .unwrap();
        assert_eq!(child_result.segments[0].data, child);
        let replacement = b"00000001EDIT";
        service
            .execute(
                &invocation,
                &request(
                    ImsOperation::Replace,
                    6,
                    &["CHILD"],
                    replacement,
                    Vec::new(),
                ),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &request(ImsOperation::Checkpoint, 7, &[], &[], Vec::new()),
            )
            .unwrap();
        assert_eq!(service.checkpoint_count().unwrap(), 1);
        let replay = request(ImsOperation::Delete, 8, &["CHILD"], &[], Vec::new());
        assert_eq!(
            service
                .execute(&invocation, &replay)
                .unwrap()
                .affected_segments,
            1
        );
        assert_eq!(
            service
                .execute(&invocation, &replay)
                .unwrap()
                .affected_segments,
            1
        );

        let image = ImsLoadImage {
            database: "AUTHDB".into(),
            roots: vec![ImsLoadRoot {
                data: root.to_vec(),
                children: vec![child.to_vec()],
            }],
        };
        service
            .execute(
                &invocation,
                &request(
                    ImsOperation::Load,
                    9,
                    &[],
                    &serde_json::to_vec(&image).unwrap(),
                    Vec::new(),
                ),
            )
            .unwrap();
        drop(service);
        let reopened = ImsService::open(store, ImsLimits::default()).unwrap();
        let mut unload = request(ImsOperation::Unload, 10, &[], &[], Vec::new());
        unload.psb = Some("AUTHDB".into());
        let result = reopened.execute(&invocation, &unload).unwrap();
        assert_eq!(result.segments.len(), 2);
        assert_eq!(reopened.hierarchy("AUTHDB").unwrap()[0].children, [child]);
    }

    #[test]
    fn legacy_replay_requires_attested_canonical_migration_without_redispatch() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        service.install(definition()).unwrap();
        let invocation = invocation("legacy-replay");
        service
            .execute(
                &invocation,
                &request(ImsOperation::Schedule, 90, &[], &[], Vec::new()),
            )
            .unwrap();
        let insert = request(
            ImsOperation::Insert,
            91,
            &["ROOT"],
            b"000091ONCE",
            Vec::new(),
        );
        let key = insert.mutation.as_ref().unwrap().idempotency_key.clone();
        let original = service.execute(&invocation, &insert).unwrap();
        assert_eq!(service.hierarchy("AUTHDB").unwrap().len(), 1);
        drop(service);

        retain_as_legacy_replay(store.as_ref(), &key, [0x22; 32]);
        let reopened = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        assert_eq!(
            reopened.execute(&invocation, &insert),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(reopened.hierarchy("AUTHDB").unwrap().len(), 1);
        assert_eq!(
            reopened.reconcile_legacy_replay(&key, [0x33; 32], &insert),
            Err(HostProblem::IdempotencyConflict)
        );
        reopened
            .reconcile_legacy_replay(&key, [0x22; 32], &insert)
            .unwrap();
        assert_eq!(reopened.execute(&invocation, &insert), Ok(original.clone()));
        assert_eq!(reopened.hierarchy("AUTHDB").unwrap().len(), 1);
        drop(reopened);

        let restarted = ImsService::open(store, ImsLimits::default()).unwrap();
        assert_eq!(restarted.execute(&invocation, &insert), Ok(original));
        assert_eq!(restarted.hierarchy("AUTHDB").unwrap().len(), 1);
    }
}
