use mainframe_env_execution_api::{
    CapabilityId, IdempotencyKey, Invocation, InvocationLimits, ServiceClass,
};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, EffectRequest, EffectResult, EnterpriseAuthorizer,
    EnterpriseResource, EnterpriseResourceClass, HostProblem, HostProvider, HostRequest,
    HostResult, ImsOperation, ImsRequest, ImsResult, ImsSegment, ImsStatusGroup,
    canonical_ims_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::database::{DatabaseEngineImage, PcbPosition};
use crate::retention::{
    ImsReplayOwnerKind, ims_pending_replay_matches, prepare_ims_replay, resolve_ims_replay,
    validate_ims_recorded_result,
};
use crate::{ImsMetadataCatalog, ImsMetadataLimits, validate_ims_metadata};

mod row_store;
use row_store::{commit_row_changes, load_or_migrate, load_row_map, row_changes};

#[cfg(test)]
use row_store::encode_object_row;

mod generic;
mod system;
mod utility_bridge;
pub use generic::{ImsGenericLoadImage, ImsGenericLoadRecord};

const STATE_NAMESPACE: &str = "ims-state";
const STATE_KEY: &str = "catalog";
const ROW_STORE_SCHEMA: &str = "mainframe-env.ims-row-store@1";
pub(crate) const OBJECT_ROW_SCHEMA: &str = "mainframe-env.ims-object-row@1";
const DATABASE_NAMESPACE: &str = "ims-v1-database";
const SESSION_NAMESPACE: &str = "ims-v1-session-index";
const CHECKPOINT_NAMESPACE: &str = "ims-v1-checkpoint";
pub(crate) const REPLAY_NAMESPACE: &str = "ims-v1-replay";
const PENDING_NAMESPACE: &str = "ims-v1-unit-of-work";
pub(crate) const GENERIC_DATABASE_NAMESPACE: &str = "ims-v1-generic-database";
const GENERIC_PENDING_NAMESPACE: &str = "ims-v1-generic-unit-of-work";
const SYSTEM_NAMESPACE: &str = "ims-v1-system";

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
    #[serde(default)]
    generic: bool,
    root_position: usize,
    child_position: usize,
    current_root: Option<String>,
    last: Option<SegmentLocation>,
    #[serde(default)]
    position: PcbPosition,
    #[serde(default)]
    system: system::SystemSession,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ReplayDigestFormat {
    #[default]
    #[serde(rename = "legacy-debug@0")]
    LegacyDebugV0,
    #[serde(rename = "mainframe-env.provider-replay-canonical@1")]
    CanonicalHostV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecordedResult {
    #[serde(default)]
    pub(crate) request_digest_format: ReplayDigestFormat,
    pub(crate) request_sha256: [u8; 32],
    #[serde(default)]
    pub(crate) recorded_deadline_tick: u64,
    #[serde(default)]
    pub(crate) owner_execution: Option<String>,
    #[serde(default)]
    pub(crate) owner_run_unit: Option<String>,
    #[serde(default)]
    pub(crate) recorded_sequence: u64,
    #[serde(default)]
    pub(crate) resolution_tick: u64,
    #[serde(default)]
    pub(crate) owner_kind: Option<ImsReplayOwnerKind>,
    #[serde(default)]
    pub(crate) outer_effect_key: Option<String>,
    #[serde(default)]
    pub(crate) result_sha256: [u8; 32],
    #[serde(default)]
    pub(crate) retention_binding_sha256: [u8; 32],
    pub(crate) status: String,
    pub(crate) segments: Vec<(String, Option<Vec<u8>>, Vec<u8>)>,
    pub(crate) checkpoint_id: Option<String>,
    pub(crate) affected_segments: u64,
    #[serde(default)]
    pub(crate) system: Option<mainframe_env_host_api::ImsSystemResult>,
}

impl RecordedResult {
    fn from_result(request_sha256: [u8; 32], result: &ImsResult) -> Self {
        Self {
            request_digest_format: ReplayDigestFormat::CanonicalHostV1,
            request_sha256,
            recorded_deadline_tick: 0,
            owner_execution: None,
            owner_run_unit: None,
            recorded_sequence: 0,
            resolution_tick: 0,
            owner_kind: None,
            outer_effect_key: None,
            result_sha256: [0; 32],
            retention_binding_sha256: [0; 32],
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
            system: result.system.clone(),
        }
    }

    pub(crate) fn result(&self) -> ImsResult {
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
            system: self.system.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct State {
    definitions: Option<ImsApplicationDefinition>,
    #[serde(default)]
    metadata: Option<ImsMetadataCatalog>,
    databases: BTreeMap<String, Arc<DatabaseState>>,
    #[serde(default)]
    generic_databases: BTreeMap<String, Arc<DatabaseEngineImage>>,
    sessions: BTreeMap<String, Arc<Session>>,
    checkpoints: BTreeMap<String, Arc<Session>>,
    replay: BTreeMap<String, Arc<RecordedResult>>,
    #[serde(default)]
    pending_undo: BTreeMap<String, Arc<BTreeMap<String, Arc<DatabaseState>>>>,
    #[serde(default)]
    generic_pending_undo: BTreeMap<String, Arc<BTreeMap<String, Arc<DatabaseEngineImage>>>>,
    #[serde(default)]
    system: BTreeMap<String, Arc<system::SystemState>>,
}

impl State {
    /// Fork a transaction by sharing immutable object payloads until touched.
    fn scoped_snapshot(&self) -> Self {
        self.clone()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RowStoreManifest {
    schema_version: String,
    definitions: Option<ImsApplicationDefinition>,
    #[serde(default)]
    metadata: Option<ImsMetadataCatalog>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObjectRow<T> {
    schema_version: String,
    object_key: String,
    value: T,
}

type RowVersions = BTreeMap<(String, String), u64>;

struct DurableState {
    versions: RowVersions,
    state: State,
}

/// Trusted durable logical-time source used to age newly persisted replay rows.
pub trait ImsReplayClock: Send + Sync {
    /// Observe the current nonzero durable logical tick.
    fn now_tick(&self) -> Result<u64, HostProblem>;
}

pub struct ImsService {
    pub(crate) store: Arc<dyn ProviderStateStore>,
    limits: ImsLimits,
    durable: Mutex<DurableState>,
    authorizer: Option<Arc<dyn EnterpriseAuthorizer>>,
    replay_clock: Option<Arc<dyn ImsReplayClock>>,
}

impl ImsService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: ImsLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, None, None)
    }

    /// Open with a trusted durable clock so new replay rows become retention-eligible.
    pub fn open_with_replay_clock(
        store: Arc<dyn ProviderStateStore>,
        limits: ImsLimits,
        replay_clock: Arc<dyn ImsReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, None, Some(replay_clock))
    }

    pub fn open_authorized(
        store: Arc<dyn ProviderStateStore>,
        limits: ImsLimits,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, Some(authorizer), None)
    }

    /// Open with enterprise authorization and a trusted durable replay clock.
    pub fn open_authorized_with_replay_clock(
        store: Arc<dyn ProviderStateStore>,
        limits: ImsLimits,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        replay_clock: Arc<dyn ImsReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, Some(authorizer), Some(replay_clock))
    }

    fn open_inner(
        store: Arc<dyn ProviderStateStore>,
        limits: ImsLimits,
        authorizer: Option<Arc<dyn EnterpriseAuthorizer>>,
        replay_clock: Option<Arc<dyn ImsReplayClock>>,
    ) -> Result<Arc<Self>, HostProblem> {
        let (state, versions) = load_or_migrate(&*store, limits)?;
        Ok(Arc::new(Self {
            store,
            limits,
            durable: Mutex::new(DurableState { versions, state }),
            authorizer,
            replay_clock,
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
        if durable.state.metadata.as_ref().is_some_and(|metadata| {
            definition.databases.iter().any(|db| {
                metadata
                    .databases
                    .iter()
                    .any(|candidate| normalize(&candidate.name) == normalize(&db.name))
            }) || definition.psbs.iter().any(|psb| {
                metadata
                    .psbs
                    .iter()
                    .any(|candidate| normalize(&candidate.name) == normalize(&psb.name))
            })
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = durable.state.scoped_snapshot();
        for database in &definition.databases {
            next.databases.entry(normalize(&database.name)).or_default();
        }
        next.definitions = Some(definition.clone());
        self.persist(&mut durable, next)?;
        Ok(install_receipt(&definition, identity, false))
    }

    /// Publish one validated generic DBD/PSB catalog through the existing IMS row store.
    pub fn install_metadata(&self, metadata: ImsMetadataCatalog) -> Result<String, HostProblem> {
        generic::install_metadata(self, metadata)
    }

    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
    ) -> Result<ImsResult, HostProblem> {
        self.execute_at(invocation, request, invocation.deadline_tick)
    }

    fn execute_at(
        &self,
        invocation: &Invocation,
        request: &ImsRequest,
        resolution_lower_bound: u64,
    ) -> Result<ImsResult, HostProblem> {
        if resolution_lower_bound == 0 {
            return Err(HostProblem::Malformed);
        }
        let mut durable = self.lock()?;
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        if durable.state.metadata.is_some() {
            generic::refresh_databases(&*self.store, self.limits, &mut durable)?;
        }
        let system_resources = if request.operation == ImsOperation::System {
            Some(system::resources(&durable.state, invocation, request)?)
        } else {
            None
        };
        if let Some(authorizer) = &self.authorizer {
            for resource in if let Some(resources) = system_resources {
                resources
            } else {
                ims_resources(&durable.state, invocation, request)?
            } {
                authorizer.authorize(invocation.principal.id(), &resource)?;
            }
        }
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        let request_sha256 = request_digest(request)?;
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str());
        let sequence = request.mutation.as_ref().map(|mutation| mutation.sequence);
        if let Some(key) = replay_key
            && let Some(recorded) = durable.state.replay.get(key)
        {
            match recorded.request_digest_format {
                ReplayDigestFormat::LegacyDebugV0 => return Err(HostProblem::UnknownOutcome),
                ReplayDigestFormat::CanonicalHostV1
                    if recorded.request_sha256 == request_sha256 =>
                {
                    let result = recorded.result();
                    let pending = ims_pending_replay_matches(
                        recorded,
                        key,
                        invocation,
                        sequence.ok_or(HostProblem::MissingIdempotency)?,
                    )?;
                    if pending {
                        self.finalize_replay_metadata(&mut durable, key, resolution_lower_bound)
                            .map_err(|_| HostProblem::UnknownOutcome)?;
                    }
                    return Ok(result);
                }
                ReplayDigestFormat::CanonicalHostV1 => {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
        let mut next = durable.state.scoped_snapshot();
        let run = invocation.run_unit_id.as_str();
        let result = if request.operation == ImsOperation::System {
            system::apply_request(&mut next, run, request, self.limits)?
        } else if generic::is_generic(&next, run, request) {
            generic::apply_request(&mut next, run, request, self.limits)?
        } else {
            apply_request(&mut next, run, request, self.limits)?
        };
        if request.operation != ImsOperation::System {
            system::observe_database_call(&mut next, run, request, &result, self.limits)?;
        }
        if invocation.service_class == ServiceClass::Batch
            && matches!(
                request.operation,
                ImsOperation::Insert | ImsOperation::Replace | ImsOperation::Delete
            )
        {
            next.pending_undo.remove(run);
            next.generic_pending_undo.remove(run);
        }
        let uow = request.operation == ImsOperation::Commit
            || request.operation == ImsOperation::Rollback;
        if uow
            && next.definitions.is_none()
            && next.metadata.is_none()
            && !durable.state.pending_undo.contains_key(run)
            && !durable.state.generic_pending_undo.contains_key(run)
        {
            return Ok(result);
        }
        if request.operation.is_mutating() {
            let key = replay_key.ok_or(HostProblem::MissingIdempotency)?;
            if next.replay.len() >= self.limits.max_replays {
                return Err(HostProblem::ResourceExhausted);
            }
            let mut recorded = RecordedResult::from_result(request_sha256, &result);
            prepare_ims_replay(
                &mut recorded,
                key,
                invocation,
                sequence.ok_or(HostProblem::MissingIdempotency)?,
                self.limits,
            )?;
            next.replay.insert(key.into(), Arc::new(recorded));
            validate_state(&next, self.limits)?;
            self.persist(&mut durable, next)?;
            self.finalize_replay_metadata(&mut durable, key, resolution_lower_bound)
                .map_err(|_| HostProblem::UnknownOutcome)?;
        }
        Ok(result)
    }

    fn finalize_replay_metadata(
        &self,
        durable: &mut DurableState,
        key: &str,
        resolution_lower_bound: u64,
    ) -> Result<(), HostProblem> {
        let Some(clock) = &self.replay_clock else {
            return Ok(());
        };
        let observed_tick = clock.now_tick()?;
        if observed_tick == 0 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let mut next = durable.state.scoped_snapshot();
        let recorded = next
            .replay
            .get_mut(key)
            .ok_or(HostProblem::InfrastructureFailure)?;
        resolve_ims_replay(
            Arc::make_mut(recorded),
            key,
            observed_tick,
            resolution_lower_bound,
        )?;
        validate_state(&next, self.limits)?;
        self.persist(durable, next)
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
        refresh_replay(&*self.store, self.limits, &mut durable)?;
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
        let mut next = durable.state.scoped_snapshot();
        let retained = next
            .replay
            .get_mut(key.as_str())
            .ok_or(HostProblem::NotFound)?;
        let retained = Arc::make_mut(retained);
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
        let changes = row_changes(
            &durable.state,
            &state,
            &durable.versions,
            self.limits,
            false,
        )?;
        commit_row_changes(&*self.store, changes, &mut durable.versions)?;
        durable.state = state;
        Ok(())
    }
}

fn refresh_replay(
    store: &dyn ProviderStateStore,
    limits: ImsLimits,
    durable: &mut DurableState,
) -> Result<(), HostProblem> {
    let mut replay_versions = RowVersions::new();
    let replay: BTreeMap<String, Arc<RecordedResult>> = load_row_map(
        store,
        REPLAY_NAMESPACE,
        limits.max_replays,
        limits,
        &mut replay_versions,
    )?;
    if replay
        .iter()
        .any(|(key, recorded)| validate_ims_recorded_result(key, recorded, limits).is_err())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    durable
        .versions
        .retain(|(namespace, _), _| namespace != REPLAY_NAMESPACE);
    durable.versions.extend(replay_versions);
    durable.state.replay = replay;
    Ok(())
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

fn ims_resources(
    state: &State,
    invocation: &Invocation,
    request: &ImsRequest,
) -> Result<Vec<EnterpriseResource>, HostProblem> {
    if generic::is_generic(state, invocation.run_unit_id.as_str(), request) {
        return generic::resources(state, invocation, request);
    }
    let run = invocation.run_unit_id.as_str();
    let intent = if request.operation.is_mutating() {
        AccessIntent::Update
    } else {
        AccessIntent::Read
    };
    let mut resources = Vec::new();
    let mut databases = BTreeSet::new();
    let psb = request
        .psb
        .as_ref()
        .map(|psb| normalize(psb))
        .or_else(|| state.sessions.get(run).map(|session| session.psb.clone()));
    if let Some(psb) = psb {
        resources.push(EnterpriseResource::new(
            EnterpriseResourceClass::ImsPsb,
            psb,
            AccessIntent::Execute,
        )?);
    }
    match request.operation {
        ImsOperation::Load => {
            let image: ImsLoadImage =
                serde_json::from_slice(&request.data).map_err(|_| HostProblem::Malformed)?;
            databases.insert(normalize(&image.database));
        }
        ImsOperation::Unload => {
            if let Some(database) = request.psb.as_ref() {
                databases.insert(normalize(database));
            } else {
                databases.insert(context(state, run)?.0);
            }
        }
        ImsOperation::Commit | ImsOperation::Rollback => match state.pending_undo.get(run) {
            Some(pending) => databases.extend(pending.keys().cloned()),
            None => return Ok(resources),
        },
        ImsOperation::Schedule => {
            let psb = normalize(request.psb.as_deref().ok_or(HostProblem::Malformed)?);
            let definition = state
                .definitions
                .as_ref()
                .and_then(|definition| {
                    definition
                        .psbs
                        .iter()
                        .find(|candidate| normalize(&candidate.name) == psb)
                })
                .ok_or(HostProblem::NotFound)?;
            let pcb = definition
                .pcbs
                .get(usize::from(request.pcb).saturating_sub(1))
                .ok_or(HostProblem::NotFound)?;
            databases.insert(normalize(&pcb.database));
        }
        _ => {
            if let Ok((database, _, _)) = context(state, run) {
                databases.insert(database);
            }
        }
    }
    if databases.is_empty()
        && matches!(
            request.operation,
            ImsOperation::Commit | ImsOperation::Rollback
        )
    {
        resources.push(EnterpriseResource::new(
            EnterpriseResourceClass::ImsUnitOfWork,
            "CURRENT",
            AccessIntent::Update,
        )?);
    }
    resources.extend(
        databases
            .into_iter()
            .map(|database| {
                EnterpriseResource::new(EnterpriseResourceClass::ImsDatabase, database, intent)
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    Ok(resources)
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
        ImsOperation::GetHoldUnique
        | ImsOperation::GetHoldNext
        | ImsOperation::GetHoldNextParent => Ok(status("AC")),
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
                for (name, database) in databases.iter() {
                    state.databases.insert(name.clone(), database.clone());
                }
            }
            Ok(status("  "))
        }
        ImsOperation::System => Err(HostProblem::Malformed),
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
        Arc::new(Session {
            psb,
            pcb: request.pcb,
            generic: false,
            root_position: 0,
            child_position: 0,
            current_root: None,
            last: None,
            position: PcbPosition::default(),
            system: system::SystemSession::default(),
        }),
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
        let session = Arc::make_mut(session);
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
    let session = Arc::make_mut(session);
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
    let session = Arc::make_mut(session);
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
    let root_key = session_snapshot
        .current_root
        .clone()
        .ok_or(HostProblem::NotFound)?;
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
    let session = Arc::make_mut(session);
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
    let (database_name, root_definition, child_definition) = context(state, run)?;
    begin_unit(state, run, &database_name)?;
    let target = request
        .segments
        .last()
        .map(|value| normalize(value))
        .ok_or(HostProblem::Malformed)?;
    let database = state
        .databases
        .get_mut(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let database = Arc::make_mut(database);
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
        Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).last =
            Some(SegmentLocation::Root { key });
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
    Arc::make_mut(state.sessions.get_mut(run).ok_or(HostProblem::NotFound)?).last =
        Some(SegmentLocation::Child {
            root_key,
            key: child_key,
        });
    Ok(affected(1))
}

fn replace(state: &mut State, run: &str, request: &ImsRequest) -> Result<ImsResult, HostProblem> {
    let (database_name, root_definition, child_definition) = context(state, run)?;
    begin_unit(state, run, &database_name)?;
    let location = state
        .sessions
        .get(run)
        .and_then(|session| session.last.clone())
        .ok_or(HostProblem::NotFound)?;
    let database = state
        .databases
        .get_mut(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let database = Arc::make_mut(database);
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
    let (database_name, _, _) = context(state, run)?;
    begin_unit(state, run, &database_name)?;
    let location = state
        .sessions
        .get(run)
        .and_then(|session| session.last.clone())
        .ok_or(HostProblem::NotFound)?;
    let database = state
        .databases
        .get_mut(&database_name)
        .ok_or(HostProblem::NotFound)?;
    let database = Arc::make_mut(database);
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
        let session = Arc::make_mut(session);
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
        system: None,
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
    state.databases.insert(database_name, Arc::new(database));
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
        system: None,
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

fn begin_unit(state: &mut State, run: &str, database_name: &str) -> Result<(), HostProblem> {
    if state
        .pending_undo
        .get(run)
        .is_some_and(|databases| databases.contains_key(database_name))
    {
        return Ok(());
    }
    let original = state
        .databases
        .get(database_name)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    Arc::make_mut(state.pending_undo.entry(run.into()).or_default())
        .insert(database_name.into(), original);
    Ok(())
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
        system: None,
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
        system: None,
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
        || state.sessions.keys().any(String::is_empty)
        || state.checkpoints.keys().any(String::is_empty)
        || state.pending_undo.keys().any(String::is_empty)
        || state
            .replay
            .iter()
            .any(|(key, recorded)| validate_ims_recorded_result(key, recorded, limits).is_err())
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
        return Err(HostProblem::ResourceExhausted);
    }
    generic::validate_state(state, limits)?;
    system::validate_state(state, limits)?;
    if let (Some(legacy), Some(metadata)) = (&state.definitions, &state.metadata)
        && (legacy.databases.iter().any(|database| {
            metadata
                .databases
                .iter()
                .any(|candidate| normalize(&candidate.name) == normalize(&database.name))
        }) || legacy.psbs.iter().any(|psb| {
            metadata
                .psbs
                .iter()
                .any(|candidate| normalize(&candidate.name) == normalize(&psb.name))
        }))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let Some(definitions) = &state.definitions else {
        return if state.databases.is_empty()
            && state.sessions.values().all(|session| session.generic)
            && state.checkpoints.values().all(|session| session.generic)
            && state.pending_undo.is_empty()
        {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        };
    };
    validate_definition(definitions, limits)?;
    let defined = definitions
        .databases
        .iter()
        .map(|database| normalize(&database.name))
        .collect::<BTreeSet<_>>();
    if defined != state.databases.keys().cloned().collect()
        || state
            .databases
            .iter()
            .any(|(name, database)| !valid_database_state(definitions, name, database, limits))
        || state
            .sessions
            .values()
            .chain(state.checkpoints.values())
            .filter(|session| !session.generic)
            .any(|session| !valid_session(definitions, session))
        || state.pending_undo.values().any(|databases| {
            databases.iter().any(|(name, database)| {
                !defined.contains(name)
                    || !valid_database_state(definitions, name, database, limits)
            })
        })
        || state.replay.keys().any(String::is_empty)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn valid_database_state(
    definitions: &ImsApplicationDefinition,
    name: &str,
    database: &DatabaseState,
    limits: ImsLimits,
) -> bool {
    let Some(definition) = definitions
        .databases
        .iter()
        .find(|definition| normalize(&definition.name) == name)
    else {
        return false;
    };
    let Some(root_definition) = definition
        .segments
        .iter()
        .find(|segment| segment.parent.is_none())
    else {
        return false;
    };
    let child_definition = definition.segments.iter().find(|segment| {
        segment
            .parent
            .as_ref()
            .is_some_and(|parent| normalize(parent) == normalize(&root_definition.name))
    });
    database.secondary_index.len() == database.roots.len()
        && database
            .secondary_index
            .iter()
            .all(|(key, root)| key == root && database.roots.contains_key(root))
        && database.roots.iter().all(|(key, root)| {
            validate_segment_data(&root.data, root_definition, limits).is_ok()
                && data_key(&root.data, root_definition).as_deref() == Ok(key.as_str())
                && root.children.iter().all(|(key, child)| {
                    child_definition.is_some_and(|definition| {
                        validate_segment_data(child, definition, limits).is_ok()
                            && data_key(child, definition).as_deref() == Ok(key.as_str())
                    })
                })
        })
}

fn valid_session(definitions: &ImsApplicationDefinition, session: &Session) -> bool {
    definitions
        .psbs
        .iter()
        .find(|psb| normalize(&psb.name) == session.psb)
        .is_some_and(|psb| session.pcb > 0 && usize::from(session.pcb) <= psb.pcbs.len())
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
        let resolution_tick = effect.deadline_tick.max(invocation.deadline_tick);
        let outcome = match effect.request {
            HostRequest::Ims(request) => self
                .service
                .execute_at(invocation, &request, resolution_tick)
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
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use mainframe_env_store_api::{
        ExecutionRecord, ExecutionState, ExecutionStore, RetentionPolicy, RetentionRequest,
        RetentionStore, RetentionTarget,
    };
    use std::sync::atomic::{AtomicU8, Ordering as AtomicOrdering};

    fn finish_invocation(store: &MemoryStore, invocation: &Invocation) {
        store
            .create_execution(ExecutionRecord {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                selector: invocation.selector.clone(),
                artifact: invocation.artifact.clone(),
                principal: invocation.principal.id().clone(),
                state: ExecutionState::Admitted,
                attempt: invocation.attempt,
                version: 1,
                owner_lease: None,
                lease_expiry_tick: None,
                terminal_tick: None,
            })
            .unwrap();
        let queued = store
            .transition_execution(&invocation.execution_id, 1, ExecutionState::Queued, 1)
            .unwrap();
        let running = store
            .transition_execution(
                &invocation.execution_id,
                queued.version,
                ExecutionState::Running,
                2,
            )
            .unwrap();
        let completing = store
            .transition_execution(
                &invocation.execution_id,
                running.version,
                ExecutionState::Completing,
                3,
            )
            .unwrap();
        store
            .transition_execution(
                &invocation.execution_id,
                completing.version,
                ExecutionState::Completed,
                4,
            )
            .unwrap();
    }

    #[derive(Default)]
    struct DenyEnterprise {
        seen: Mutex<Vec<EnterpriseResource>>,
    }

    impl EnterpriseAuthorizer for DenyEnterprise {
        fn authorize(
            &self,
            _: &PrincipalId,
            resource: &EnterpriseResource,
        ) -> Result<(), HostProblem> {
            self.seen.lock().unwrap().push(resource.clone());
            Err(HostProblem::Unauthorized)
        }
    }

    struct PersistAwareReplayClock {
        store: Arc<dyn ProviderStateStore>,
        key: String,
        tick: u64,
        fault_stage: AtomicU8,
    }

    impl ImsReplayClock for PersistAwareReplayClock {
        fn now_tick(&self) -> Result<u64, HostProblem> {
            match self.fault_stage.load(AtomicOrdering::SeqCst) {
                1 => {
                    self.fault_stage.store(2, AtomicOrdering::SeqCst);
                    return Err(HostProblem::InfrastructureFailure);
                }
                2 => {
                    self.fault_stage.store(0, AtomicOrdering::SeqCst);
                    let mut row = self
                        .store
                        .get_provider_state(REPLAY_NAMESPACE, &self.key)
                        .map_err(store_error)?
                        .ok_or(HostProblem::InfrastructureFailure)?;
                    let expected = row.version;
                    row.version = row
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)?;
                    self.store
                        .put_provider_state(row, Some(expected))
                        .map_err(store_error)?;
                }
                _ => {}
            }
            let row = self
                .store
                .get_provider_state(REPLAY_NAMESPACE, &self.key)
                .map_err(store_error)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let value: serde_json::Value = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            assert_eq!(value["value"]["resolution_tick"], 0);
            assert!(value["value"]["owner_execution"].is_string());
            Ok(self.tick)
        }
    }

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
        invocation_as(run, ServiceClass::Batch)
    }

    /// Like `invocation`, but for a caller-chosen service class -- e.g. an
    /// Interactive run, whose Insert/Replace/Delete does not clear
    /// `pending_undo` the way `execute_at` clears it for Batch.
    fn invocation_as(run: &str, service_class: ServiceClass) -> Invocation {
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
            service_class,
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
            system: None,
            q_class: None,
        }
    }

    /// #183 regression: a run that never opened an IMS unit of work, with
    /// IMS never installed at all, must be able to ROLLBACK even under a
    /// denying authorizer and without persisting a durable replay row.
    #[test]
    fn ims_rollback_with_no_pending_work_is_not_authorized_against_a_phantom_unit_of_work() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let policy = Arc::new(DenyEnterprise::default());
        let service =
            ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
        // No `install` and no prior mutating IMS request under this run:
        // this run unit never opened an IMS unit of work, and IMS itself
        // has no installed definitions.
        let rolled_back = service.execute(
            &invocation("never-touched-ims"),
            &request(ImsOperation::Rollback, 1, &[], &[], Vec::new()),
        );
        assert_eq!(rolled_back, Ok(status("  ")));
        assert!(
            policy.seen.lock().unwrap().is_empty(),
            "a rollback with nothing pending must not ask the authorizer about a phantom unit of work"
        );
    }

    /// #183: a run that opened a real IMS unit of work must still be
    /// authorized to ROLLBACK it against the staged database -- this is
    /// the surviving branch the phantom-unit-of-work fix must not break.
    #[test]
    fn ims_rollback_with_pending_work_is_authorized_against_the_staged_database() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let unauthorized = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        unauthorized.install(definition()).unwrap();
        let staging = invocation_as("pending-ims", ServiceClass::Interactive);
        unauthorized
            .execute(
                &staging,
                &request(ImsOperation::Schedule, 1, &[], &[], Vec::new()),
            )
            .unwrap();
        unauthorized
            .execute(
                &staging,
                &request(
                    ImsOperation::Insert,
                    2,
                    &["ROOT"],
                    b"000801ROOT",
                    Vec::new(),
                ),
            )
            .unwrap();
        // Drop the session (not the staged unit of work) so this Rollback's
        // only enterprise resource is the staged database, not the PSB.
        unauthorized
            .execute(
                &staging,
                &request(ImsOperation::Terminate, 3, &[], &[], Vec::new()),
            )
            .unwrap();

        let policy = Arc::new(DenyEnterprise::default());
        let service =
            ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
        let rolled_back = service.execute(
            &staging,
            &request(ImsOperation::Rollback, 4, &[], &[], Vec::new()),
        );
        assert_eq!(rolled_back, Err(HostProblem::Unauthorized));
        assert_eq!(
            policy.seen.lock().unwrap().as_slice(),
            &[EnterpriseResource::new(
                EnterpriseResourceClass::ImsDatabase,
                "AUTHDB",
                AccessIntent::Update,
            )
            .unwrap()]
        );
    }

    /// #183: an installed provider's no-op Commit/Rollback must persist its
    /// replay row like any other mutating request, so a redelivery of the
    /// same idempotency key replays the recorded no-op instead of acting on
    /// whatever real unit of work the run has since opened.
    #[test]
    fn ims_no_op_rollback_replay_survives_later_staged_work_on_the_same_run() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(store, ImsLimits::default()).unwrap();
        service.install(definition()).unwrap();
        let invocation = invocation_as("redeliver-ims", ServiceClass::Interactive);
        let noop = request(ImsOperation::Rollback, 1, &[], &[], Vec::new());
        let first = service.execute(&invocation, &noop).unwrap();

        service
            .execute(
                &invocation,
                &request(ImsOperation::Schedule, 2, &[], &[], Vec::new()),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &request(
                    ImsOperation::Insert,
                    3,
                    &["ROOT"],
                    b"000801ROOT",
                    Vec::new(),
                ),
            )
            .unwrap();

        let redelivered = service.execute(&invocation, &noop).unwrap();
        assert_eq!(redelivered, first);
        assert!(
            service
                .lock()
                .unwrap()
                .state
                .pending_undo
                .contains_key("redeliver-ims"),
            "a replayed no-op rollback must not roll back work staged after it"
        );
    }

    fn retain_as_legacy_replay(
        store: &dyn ProviderStateStore,
        key: &IdempotencyKey,
        digest: [u8; 32],
    ) {
        let row = store
            .get_provider_state(REPLAY_NAMESPACE, key.as_str())
            .unwrap()
            .unwrap();
        let mut state: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        let replay = state["value"].as_object_mut().unwrap();
        replay.remove("request_digest_format");
        for field in [
            "recorded_deadline_tick",
            "owner_execution",
            "owner_run_unit",
            "recorded_sequence",
            "resolution_tick",
            "owner_kind",
            "outer_effect_key",
            "result_sha256",
            "retention_binding_sha256",
        ] {
            replay.remove(field);
        }
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

    #[test]
    fn external_replay_prune_refreshes_live_cache_before_replay_and_capacity() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let limits = ImsLimits {
            max_replays: 1,
            ..ImsLimits::default()
        };
        let first = ImsService::open(store.clone(), limits).unwrap();
        first.install(definition()).unwrap();
        let invocation = invocation("refresh-ims");
        let schedule = request(ImsOperation::Schedule, 801, &[], &[], Vec::new());
        first.execute(&invocation, &schedule).unwrap();

        let second = ImsService::open(store.clone(), limits).unwrap();
        let schedule_key = schedule.mutation.as_ref().unwrap().idempotency_key.as_str();
        let schedule_row = store
            .get_provider_state(REPLAY_NAMESPACE, schedule_key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(REPLAY_NAMESPACE, schedule_key, schedule_row.version)
            .unwrap();

        let insert = request(
            ImsOperation::Insert,
            802,
            &["ROOT"],
            b"000801ROOT",
            Vec::new(),
        );
        assert_eq!(
            second
                .execute(&invocation, &insert)
                .unwrap()
                .affected_segments,
            1
        );

        let insert_key = insert.mutation.as_ref().unwrap().idempotency_key.as_str();
        let insert_row = store
            .get_provider_state(REPLAY_NAMESPACE, insert_key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(REPLAY_NAMESPACE, insert_key, insert_row.version)
            .unwrap();
        let redispatched = second.execute(&invocation, &insert).unwrap();
        assert_eq!(redispatched.status, "II");
        assert_eq!(redispatched.affected_segments, 0);

        assert_eq!(first.lock().unwrap().state.replay.len(), 1);
    }

    #[test]
    fn delayed_resolution_is_observed_after_protected_replay_persistence() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation("delayed-ims");
        let request = request(ImsOperation::Schedule, 803, &[], &[], Vec::new());
        let key = request
            .mutation
            .as_ref()
            .unwrap()
            .idempotency_key
            .as_str()
            .to_string();
        let clock = Arc::new(PersistAwareReplayClock {
            store: store.clone(),
            key: key.clone(),
            tick: 250,
            fault_stage: AtomicU8::new(0),
        });
        let service =
            ImsService::open_with_replay_clock(store.clone(), ImsLimits::default(), clock).unwrap();
        service.install(definition()).unwrap();
        service.execute(&invocation, &request).unwrap();

        let row = store
            .get_provider_state(REPLAY_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(value["value"]["recorded_deadline_tick"], 100);
        assert_eq!(value["value"]["resolution_tick"], 250);
        assert_eq!(
            value["value"]["owner_execution"],
            invocation.execution_id.as_str()
        );
    }

    #[test]
    fn post_commit_clock_and_cas_failures_are_unknown_then_retry_recovers() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation("fault-ims");
        let request = request(ImsOperation::Schedule, 804, &[], &[], Vec::new());
        let key = request
            .mutation
            .as_ref()
            .unwrap()
            .idempotency_key
            .as_str()
            .to_string();
        let clock = Arc::new(PersistAwareReplayClock {
            store: store.clone(),
            key: key.clone(),
            tick: 250,
            fault_stage: AtomicU8::new(1),
        });
        let service =
            ImsService::open_with_replay_clock(store.clone(), ImsLimits::default(), clock).unwrap();
        service.install(definition()).unwrap();
        assert_eq!(
            service.execute(&invocation, &request),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            service.execute(&invocation, &request),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(service.execute(&invocation, &request).unwrap().status, "  ");
        let row = store
            .get_provider_state(REPLAY_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(value["value"]["resolution_tick"], 250);
    }

    fn qualifier(segment: &str, field: &str, value: &[u8]) -> ImsQualifier {
        ImsQualifier {
            segment: segment.into(),
            field: field.into(),
            value: value.to_vec(),
        }
    }

    #[test]
    fn ims_database_denial_precedes_mutation() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let policy = Arc::new(DenyEnterprise::default());
        let service =
            ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
        service.install(definition()).unwrap();
        let image = ImsLoadImage {
            database: "AUTHDB".into(),
            roots: vec![ImsLoadRoot {
                data: b"ROOT01DATA".to_vec(),
                children: Vec::new(),
            }],
        };
        let denied = service.execute(
            &invocation("deny-ims"),
            &request(
                ImsOperation::Load,
                700,
                &[],
                &serde_json::to_vec(&image).unwrap(),
                Vec::new(),
            ),
        );
        assert_eq!(denied, Err(HostProblem::Unauthorized));
        assert!(
            service.lock().unwrap().state.databases["AUTHDB"]
                .roots
                .is_empty()
        );
        assert_eq!(
            policy.seen.lock().unwrap().as_slice(),
            &[EnterpriseResource::new(
                EnterpriseResourceClass::ImsDatabase,
                "AUTHDB",
                AccessIntent::Update,
            )
            .unwrap()]
        );
    }

    #[test]
    fn ims_database_read_denial_hides_retained_data() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let writer = ImsService::open(store.clone(), ImsLimits::default()).unwrap();
        writer.install(definition()).unwrap();
        let image = ImsLoadImage {
            database: "AUTHDB".into(),
            roots: vec![ImsLoadRoot {
                data: b"ROOT01DATA".to_vec(),
                children: Vec::new(),
            }],
        };
        writer
            .execute(
                &invocation("load-before-deny"),
                &request(
                    ImsOperation::Load,
                    701,
                    &[],
                    &serde_json::to_vec(&image).unwrap(),
                    Vec::new(),
                ),
            )
            .unwrap();
        let policy = Arc::new(DenyEnterprise::default());
        let reader =
            ImsService::open_authorized(store, ImsLimits::default(), policy.clone()).unwrap();
        let mut read = request(ImsOperation::Unload, 702, &[], &[], Vec::new());
        read.psb = Some("AUTHDB".into());
        assert_eq!(
            reader.execute(&invocation("read-after-deny"), &read),
            Err(HostProblem::Unauthorized)
        );
        assert_eq!(reader.hierarchy("AUTHDB").unwrap(), image.roots);
        assert_eq!(policy.seen.lock().unwrap().len(), 1);
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

    #[test]
    fn legacy_blob_migrates_atomically_to_versioned_scoped_rows_and_corruption_fails_closed() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let mut definitions = definition();
        let mut other = definitions.databases[0].clone();
        other.name = "OTHERDB".into();
        other.secondary_index = Some("OTHERIX".into());
        definitions.databases.push(other);
        let legacy = State {
            definitions: Some(definitions),
            metadata: None,
            databases: BTreeMap::from([
                ("AUTHDB".into(), Arc::new(DatabaseState::default())),
                ("OTHERDB".into(), Arc::new(DatabaseState::default())),
            ]),
            generic_databases: BTreeMap::new(),
            sessions: BTreeMap::new(),
            checkpoints: BTreeMap::new(),
            replay: BTreeMap::new(),
            pending_undo: BTreeMap::new(),
            generic_pending_undo: BTreeMap::new(),
            system: BTreeMap::new(),
        };
        let legacy_payload = serde_json::to_vec(&legacy).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: legacy_payload.clone(),
                },
                None,
            )
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: DATABASE_NAMESPACE.into(),
                    key: "ORPHAN".into(),
                    version: 1,
                    payload: encode_object_row("ORPHAN", &DatabaseState::default()).unwrap(),
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            ImsService::open(store.clone(), Default::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap()
                .payload,
            legacy_payload
        );
        store
            .delete_provider_state(DATABASE_NAMESPACE, "ORPHAN", 1)
            .unwrap();

        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        let manifest_before = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let manifest: RowStoreManifest = serde_json::from_slice(&manifest_before.payload).unwrap();
        assert_eq!(manifest.schema_version, ROW_STORE_SCHEMA);
        assert_eq!(manifest_before.version, 2);
        assert_eq!(
            store
                .list_provider_state(DATABASE_NAMESPACE, 3)
                .unwrap()
                .len(),
            2
        );
        let other_before = store
            .get_provider_state(DATABASE_NAMESPACE, "OTHERDB")
            .unwrap()
            .unwrap();
        let auth_version = store
            .get_provider_state(DATABASE_NAMESPACE, "AUTHDB")
            .unwrap()
            .unwrap()
            .version;
        let invocation = invocation("row-scope");
        service
            .execute(
                &invocation,
                &request(ImsOperation::Schedule, 401, &[], &[], Vec::new()),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &request(
                    ImsOperation::Insert,
                    402,
                    &["ROOT"],
                    b"000402SCOP",
                    Vec::new(),
                ),
            )
            .unwrap();
        assert_eq!(service.hierarchy("AUTHDB").unwrap().len(), 1);
        assert_eq!(
            store
                .get_provider_state(DATABASE_NAMESPACE, "AUTHDB")
                .unwrap()
                .unwrap()
                .version,
            auth_version + 1
        );
        assert_eq!(
            store
                .get_provider_state(DATABASE_NAMESPACE, "OTHERDB")
                .unwrap()
                .unwrap(),
            other_before
        );
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap(),
            manifest_before
        );
        drop(service);

        let other = store
            .get_provider_state(DATABASE_NAMESPACE, "OTHERDB")
            .unwrap()
            .unwrap();
        let mut corrupt: serde_json::Value = serde_json::from_slice(&other.payload).unwrap();
        corrupt["object_key"] = serde_json::json!("DIFFERENT");
        let version = other.version;
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: version + 1,
                    payload: serde_json::to_vec(&corrupt).unwrap(),
                    ..other
                },
                Some(version),
            )
            .unwrap();
        assert!(matches!(
            ImsService::open(store, Default::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn empty_legacy_blob_is_always_replaced_by_a_versioned_manifest() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: serde_json::to_vec(&State::default()).unwrap(),
                },
                None,
            )
            .unwrap();

        drop(ImsService::open(store.clone(), Default::default()).unwrap());
        let manifest = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        assert_eq!(manifest.version, 2);
        assert_eq!(
            serde_json::from_slice::<RowStoreManifest>(&manifest.payload)
                .unwrap()
                .schema_version,
            ROW_STORE_SCHEMA
        );
    }

    #[test]
    fn sqlite_executes_and_reopens_the_versioned_row_layout() {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap());
        let legacy = State {
            definitions: Some(definition()),
            databases: BTreeMap::from([("AUTHDB".into(), Arc::new(DatabaseState::default()))]),
            ..State::default()
        };
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: STATE_NAMESPACE.into(),
                    key: STATE_KEY.into(),
                    version: 1,
                    payload: serde_json::to_vec(&legacy).unwrap(),
                },
                None,
            )
            .unwrap();
        let service = ImsService::open(store.clone(), Default::default()).unwrap();
        let invocation = invocation("sqlite-row");
        service
            .execute(
                &invocation,
                &request(ImsOperation::Schedule, 701, &[], &[], Vec::new()),
            )
            .unwrap();
        service
            .execute(
                &invocation,
                &request(
                    ImsOperation::Insert,
                    702,
                    &["ROOT"],
                    b"000702DATA",
                    Vec::new(),
                ),
            )
            .unwrap();
        drop(service);

        let reopened = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(reopened.hierarchy("AUTHDB").unwrap().len(), 1);
        assert_eq!(
            store
                .list_provider_state(DATABASE_NAMESPACE, 2)
                .unwrap()
                .len(),
            1
        );
        assert!(
            serde_json::from_slice::<RowStoreManifest>(
                &store
                    .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                    .unwrap()
                    .unwrap()
                    .payload
            )
            .is_ok()
        );
    }

    #[test]
    fn independent_database_rows_commit_from_separate_service_instances_without_global_cas() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let mut definitions = definition();
        let mut other_database = definitions.databases[0].clone();
        other_database.name = "OTHERDB".into();
        other_database.secondary_index = Some("OTHERIX".into());
        definitions.databases.push(other_database);
        let mut other_psb = definitions.psbs[0].clone();
        other_psb.name = "OTHERPSB".into();
        other_psb.pcbs[0].database = "OTHERDB".into();
        definitions.psbs.push(other_psb);
        let installer = ImsService::open(store.clone(), Default::default()).unwrap();
        installer.install(definitions).unwrap();
        drop(installer);
        let manifest = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let left = ImsService::open(store.clone(), Default::default()).unwrap();
        let right = ImsService::open(store.clone(), Default::default()).unwrap();
        let left_worker = std::thread::spawn(move || {
            let invocation = invocation("left-db");
            left.execute(
                &invocation,
                &request(ImsOperation::Schedule, 911, &[], &[], Vec::new()),
            )?;
            left.execute(
                &invocation,
                &request(
                    ImsOperation::Insert,
                    912,
                    &["ROOT"],
                    b"000911LEFT",
                    Vec::new(),
                ),
            )
        });
        let right_worker = std::thread::spawn(move || {
            let invocation = invocation("right-db");
            let mut schedule = request(ImsOperation::Schedule, 921, &[], &[], Vec::new());
            schedule.psb = Some("OTHERPSB".into());
            right.execute(&invocation, &schedule)?;
            right.execute(
                &invocation,
                &request(
                    ImsOperation::Insert,
                    922,
                    &["ROOT"],
                    b"000921RGHT",
                    Vec::new(),
                ),
            )
        });
        left_worker.join().unwrap().unwrap();
        right_worker.join().unwrap().unwrap();

        let reopened = ImsService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(reopened.hierarchy("AUTHDB").unwrap().len(), 1);
        assert_eq!(reopened.hierarchy("OTHERDB").unwrap().len(), 1);
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap(),
            manifest
        );
    }

    #[test]
    fn replay_retention_preserves_the_live_idempotency_window() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let service = ImsService::open(
            store.clone(),
            ImsLimits {
                max_replays: 2,
                ..Default::default()
            },
        )
        .unwrap();
        service.install(definition()).unwrap();
        let mut expired_invocation = invocation("expired-replay");
        expired_invocation.deadline_tick = 10;
        let expired = request(ImsOperation::Schedule, 981, &[], &[], Vec::new());
        service.execute(&expired_invocation, &expired).unwrap();
        let mut live_invocation = invocation("live-replay");
        live_invocation.deadline_tick = 95;
        let live = request(ImsOperation::Schedule, 982, &[], &[], Vec::new());
        let live_result = service.execute(&live_invocation, &live).unwrap();
        finish_invocation(store.as_ref(), &expired_invocation);
        finish_invocation(store.as_ref(), &live_invocation);
        let expired_key = expired.mutation.as_ref().unwrap().idempotency_key.clone();
        let live_key = live.mutation.as_ref().unwrap().idempotency_key.clone();
        let raw_retention = store.archive_and_prune(
            retention_policy(),
            RetentionRequest {
                target: RetentionTarget::ImsReplay,
                now_tick: 100,
                max_records: 8,
            },
        );
        assert_eq!(raw_retention, Err(StoreError::InvalidTransition));
        if raw_retention.is_err() {
            return;
        }
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, expired_key.as_str())
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .get_provider_state(REPLAY_NAMESPACE, live_key.as_str())
                .unwrap()
                .is_some()
        );
        assert_eq!(service.execute(&live_invocation, &live), Ok(live_result));
        let fresh = request(ImsOperation::Schedule, 983, &[], &[], Vec::new());
        service
            .execute(&invocation("fresh-replay"), &fresh)
            .unwrap();
        assert_eq!(
            store
                .list_provider_state(REPLAY_NAMESPACE, 3)
                .unwrap()
                .len(),
            2
        );
    }

    fn retention_policy() -> RetentionPolicy {
        RetentionPolicy {
            lifecycle_ticks: 10,
            idempotency_ticks: 10,
            audit_ticks: 20,
            archive_ticks: 50,
            low_watermark_percent: 70,
            high_watermark_percent: 85,
            max_batch: 8,
        }
    }
}
