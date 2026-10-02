use mainframe_env_execution_api::{CapabilityId, IdempotencyKey, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CapabilityDescriptor, EffectRequest, EffectResult, EnterpriseAuthorizer,
    EnterpriseResource, EnterpriseResourceClass, HostProblem, HostProvider, HostRequest,
    HostResult, MqOperation, MqRequest, MqResult, canonical_mq_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::host_context::reject_host_owned_syncpoint;
use crate::message::canonical_message_id;
use crate::object::{
    MqObjectCapability, MqObjectCatalog, canonical_definition, canonical_name as normalize,
    is_canonical_name,
};
use crate::object_service::{legacy_catalog, local_definitions, local_target};
use crate::retention::{
    MqReplayOwnerKind, mq_pending_replay_matches, prepare_mq_replay, resolve_mq_replay,
    validate_mq_recorded_result,
};

const STATE_NAMESPACE: &str = "mq-state";
const STATE_KEY: &str = "queues";
const ROW_STORE_SCHEMA: &str = "mainframe-env.mq-row-store@1";
pub(crate) const OBJECT_ROW_SCHEMA: &str = "mainframe-env.mq-object-row@1";
const QUEUE_NAMESPACE: &str = "mq-v1-queue";
const CATALOG_NAMESPACE: &str = "mq-v1-object-catalog";
const CATALOG_KEY: &str = "catalog";
const HANDLE_NAMESPACE: &str = "mq-v1-handle-index";
const PENDING_NAMESPACE: &str = "mq-v1-unit-of-work";
pub(crate) const REPLAY_NAMESPACE: &str = "mq-v1-replay";

mod row_store;
use row_store::{
    commit_row_changes, encode_object_row, load_or_migrate, load_row_map, row_changes,
};

#[path = "service_object_integration.rs"]
mod object_integration;
use object_integration::{encode_catalog_row, load_catalog_row};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqLimits {
    pub max_queues: usize,
    pub max_messages_per_queue: usize,
    pub max_message_bytes: usize,
    pub max_handles: usize,
    pub max_pending_units: usize,
    pub max_replays: usize,
    pub max_state_bytes: usize,
}

impl Default for MqLimits {
    fn default() -> Self {
        Self {
            max_queues: 256,
            max_messages_per_queue: 65_536,
            max_message_bytes: 1024 * 1024,
            max_handles: 16_384,
            max_pending_units: 4_096,
            max_replays: 65_536,
            max_state_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MqQueueDefinition {
    pub name: String,
    pub trigger_program: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqInstallReceipt {
    pub queues: usize,
    pub triggers: usize,
    pub identity: String,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Message {
    data: Vec<u8>,
    message_id: Vec<u8>,
    correlation_id: Vec<u8>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct Queue {
    trigger_program: Option<String>,
    messages: Vec<Message>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct PendingUnit {
    puts: Vec<(String, Message)>,
    gets: Vec<(String, Message)>,
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
    pub(crate) owner_kind: Option<MqReplayOwnerKind>,
    #[serde(default)]
    pub(crate) outer_effect_key: Option<String>,
    #[serde(default)]
    pub(crate) result_sha256: [u8; 32],
    #[serde(default)]
    pub(crate) retention_binding_sha256: [u8; 32],
    pub(crate) completion_code: i32,
    pub(crate) reason_code: i32,
    pub(crate) handle: Option<u32>,
    pub(crate) message: Vec<u8>,
    pub(crate) message_id: Option<Vec<u8>>,
    pub(crate) correlation_id: Option<Vec<u8>>,
    pub(crate) trigger_program: Option<String>,
}

impl RecordedResult {
    fn from_result(request_sha256: [u8; 32], result: &MqResult) -> Self {
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
            completion_code: result.completion_code,
            reason_code: result.reason_code,
            handle: result.handle,
            message: result.message.clone(),
            message_id: result.message_id.clone(),
            correlation_id: result.correlation_id.clone(),
            trigger_program: result.trigger_program.clone(),
        }
    }

    pub(crate) fn result(&self) -> MqResult {
        MqResult {
            completion_code: self.completion_code,
            reason_code: self.reason_code,
            handle: self.handle,
            message: self.message.clone(),
            message_id: self.message_id.clone(),
            correlation_id: self.correlation_id.clone(),
            trigger_program: self.trigger_program.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct State {
    definitions: Option<Vec<MqQueueDefinition>>,
    #[serde(skip)]
    catalog: Option<Arc<MqObjectCatalog>>,
    queues: BTreeMap<String, Arc<Queue>>,
    handles: BTreeMap<String, Arc<BTreeMap<u32, String>>>,
    pending: BTreeMap<String, Arc<PendingUnit>>,
    replay: BTreeMap<String, Arc<RecordedResult>>,
    next_handle: u32,
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
    definitions: Option<Vec<MqQueueDefinition>>,
    next_handle: u32,
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
pub trait MqReplayClock: Send + Sync {
    /// Observe the current nonzero durable logical tick.
    fn now_tick(&self) -> Result<u64, HostProblem>;
}

pub struct MqService {
    store: Arc<dyn ProviderStateStore>,
    limits: MqLimits,
    durable: Mutex<DurableState>,
    unknown_after_persist: AtomicBool,
    authorizer: Option<Arc<dyn EnterpriseAuthorizer>>,
    replay_clock: Option<Arc<dyn MqReplayClock>>,
}

impl MqService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: MqLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, None, None)
    }

    /// Open with a trusted durable clock so new replay rows become retention-eligible.
    pub fn open_with_replay_clock(
        store: Arc<dyn ProviderStateStore>,
        limits: MqLimits,
        replay_clock: Arc<dyn MqReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, None, Some(replay_clock))
    }

    pub fn open_authorized(
        store: Arc<dyn ProviderStateStore>,
        limits: MqLimits,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, Some(authorizer), None)
    }

    /// Open with enterprise authorization and a trusted durable replay clock.
    pub fn open_authorized_with_replay_clock(
        store: Arc<dyn ProviderStateStore>,
        limits: MqLimits,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        replay_clock: Arc<dyn MqReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        Self::open_inner(store, limits, Some(authorizer), Some(replay_clock))
    }

    fn open_inner(
        store: Arc<dyn ProviderStateStore>,
        limits: MqLimits,
        authorizer: Option<Arc<dyn EnterpriseAuthorizer>>,
        replay_clock: Option<Arc<dyn MqReplayClock>>,
    ) -> Result<Arc<Self>, HostProblem> {
        let (state, versions) = load_or_migrate(&*store, limits)?;
        Ok(Arc::new(Self {
            store,
            limits,
            durable: Mutex::new(DurableState { versions, state }),
            unknown_after_persist: AtomicBool::new(false),
            authorizer,
            replay_clock,
        }))
    }

    pub fn install(
        &self,
        definitions: Vec<MqQueueDefinition>,
    ) -> Result<MqInstallReceipt, HostProblem> {
        let definitions = (definitions.into_iter().map(canonical_definition))
            .collect::<Result<Vec<_>, HostProblem>>()?;
        validate_definitions(&definitions, self.limits)?;
        let identity = format!(
            "sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&definitions).map_err(|_| HostProblem::ProviderFailure)?
            )
        );
        let catalog = legacy_catalog(&definitions, self.limits)?;
        let mut durable = self.lock()?;
        if let Some(current) = &durable.state.catalog {
            return if *current.as_ref() == catalog {
                Ok(install_receipt(&definitions, identity, true))
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let mut next = durable.state.scoped_snapshot();
        for definition in &definitions {
            next.queues.insert(
                definition.name.clone(),
                Arc::new(Queue {
                    trigger_program: definition.trigger_program.clone(),
                    messages: Vec::new(),
                }),
            );
        }
        next.catalog = Some(Arc::new(catalog));
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)?;
        Ok(install_receipt(&definitions, identity, false))
    }

    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &MqRequest,
    ) -> Result<MqResult, HostProblem> {
        self.execute_at(invocation, request, invocation.deadline_tick)
    }

    fn execute_at(
        &self,
        invocation: &Invocation,
        request: &MqRequest,
        resolution_lower_bound: u64,
    ) -> Result<MqResult, HostProblem> {
        if resolution_lower_bound == 0 {
            return Err(HostProblem::Malformed);
        }
        if let Some(queue) = request.queue.as_deref() {
            normalize(queue)?;
        }
        if let Some(rejection) = reject_host_owned_syncpoint(invocation, request)? {
            return Ok(rejection);
        }
        let mut durable = self.lock()?;
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        if let Some(authorizer) = &self.authorizer {
            for resource in mq_resources(&durable.state, invocation, request)? {
                authorizer.authorize(invocation.principal.id(), &resource)?;
            }
        }
        refresh_replay(&*self.store, self.limits, &mut durable)?;
        let request_sha256 = request_digest(request)?;
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str())
            .ok_or(HostProblem::MissingIdempotency)?;
        let sequence = request
            .mutation
            .as_ref()
            .ok_or(HostProblem::MissingIdempotency)?
            .sequence;
        if let Some(recorded) = durable.state.replay.get(replay_key) {
            match recorded.request_digest_format {
                ReplayDigestFormat::LegacyDebugV0 => return Err(HostProblem::UnknownOutcome),
                ReplayDigestFormat::CanonicalHostV1
                    if recorded.request_sha256 == request_sha256 =>
                {
                    let result = recorded.result();
                    let pending =
                        mq_pending_replay_matches(recorded, replay_key, invocation, sequence)?;
                    if pending {
                        self.finalize_replay_metadata(
                            &mut durable,
                            replay_key,
                            resolution_lower_bound,
                        )
                        .map_err(|_| HostProblem::UnknownOutcome)?;
                    }
                    return Ok(result);
                }
                ReplayDigestFormat::CanonicalHostV1 => {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
        let run = invocation.run_unit_id.as_str();
        let mut next = durable.state.scoped_snapshot();
        let result = apply_request(&mut next, run, request, self.limits)?;
        let uow = [MqOperation::Commit, MqOperation::Rollback].contains(&request.operation);
        if uow && durable.state.catalog.is_none() && !durable.state.pending.contains_key(run) {
            return Ok(result);
        }
        if next.replay.len() >= self.limits.max_replays {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut recorded = RecordedResult::from_result(request_sha256, &result);
        prepare_mq_replay(&mut recorded, replay_key, invocation, sequence, self.limits)?;
        next.replay.insert(replay_key.into(), Arc::new(recorded));
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)?;
        if self.unknown_after_persist.swap(false, Ordering::SeqCst) {
            return Err(HostProblem::UnknownOutcome);
        }
        self.finalize_replay_metadata(&mut durable, replay_key, resolution_lower_bound)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        Ok(result)
    }

    pub fn inject_unknown_outcome_once(&self) {
        self.unknown_after_persist.store(true, Ordering::SeqCst);
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
        resolve_mq_replay(
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
        request: &MqRequest,
    ) -> Result<(), HostProblem> {
        if request
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

    pub fn queue_depth(&self, queue: &str) -> Result<usize, HostProblem> {
        let durable = self.lock()?;
        let queue = resolve_named_queue(&durable.state, queue, MqOperation::Get)?.0;
        durable
            .state
            .queues
            .get(&queue)
            .map(|queue| queue.messages.len())
            .ok_or(HostProblem::NotFound)
    }

    pub fn queue_messages(&self, queue: &str) -> Result<Vec<Vec<u8>>, HostProblem> {
        let durable = self.lock()?;
        let queue = resolve_named_queue(&durable.state, queue, MqOperation::Get)?.0;
        durable
            .state
            .queues
            .get(&queue)
            .map(|queue| {
                queue
                    .messages
                    .iter()
                    .map(|message| message.data.clone())
                    .collect()
            })
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
    limits: MqLimits,
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
        .any(|(key, recorded)| validate_mq_recorded_result(key, recorded, limits).is_err())
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

fn mq_resources(
    state: &State,
    invocation: &Invocation,
    request: &MqRequest,
) -> Result<Vec<EnterpriseResource>, HostProblem> {
    let run = invocation.run_unit_id.as_str();
    let intent = match request.operation {
        MqOperation::Get => AccessIntent::Read,
        MqOperation::Open | MqOperation::Close => AccessIntent::Execute,
        MqOperation::Put | MqOperation::PutOne | MqOperation::Commit | MqOperation::Rollback => {
            AccessIntent::Update
        }
    };
    let mut queues = BTreeSet::new();
    match request.operation {
        MqOperation::Commit | MqOperation::Rollback if state.pending.contains_key(run) => {
            let pending = &state.pending[run];
            queues.extend(pending.puts.iter().map(|(queue, _)| queue.clone()));
            queues.extend(pending.gets.iter().map(|(queue, _)| queue.clone()));
        }
        MqOperation::Commit | MqOperation::Rollback => return Ok(Vec::new()),
        _ => match resolve_queue_target(state, run, request) {
            Ok((_, path)) => queues.extend(path),
            Err(HostProblem::NotFound) if request.operation == MqOperation::Open => {
                queues.insert(normalize(
                    request.queue.as_deref().ok_or(HostProblem::Malformed)?,
                )?);
            }
            Err(problem) => return Err(problem),
        },
    }
    if queues.is_empty() {
        return Ok(vec![EnterpriseResource::new(
            EnterpriseResourceClass::MqUnitOfWork,
            "CURRENT",
            intent,
        )?]);
    }
    queues
        .into_iter()
        .map(|queue| EnterpriseResource::new(EnterpriseResourceClass::MqQueue, queue, intent))
        .collect()
}

fn apply_request(
    state: &mut State,
    run: &str,
    request: &MqRequest,
    limits: MqLimits,
) -> Result<MqResult, HostProblem> {
    match request.operation {
        MqOperation::Open => open(state, run, request, limits),
        MqOperation::Get => get(state, run, request),
        MqOperation::Put | MqOperation::PutOne => put(state, run, request, limits),
        MqOperation::Close => close(state, run, request),
        MqOperation::Commit => commit(state, run, limits),
        MqOperation::Rollback => rollback(state, run, limits),
    }
}

fn open(
    state: &mut State,
    run: &str,
    request: &MqRequest,
    limits: MqLimits,
) -> Result<MqResult, HostProblem> {
    let queue = match resolve_queue_target(state, run, request) {
        Ok((queue, _)) if state.queues.contains_key(&queue) => queue,
        Ok(_) | Err(HostProblem::NotFound) => return Ok(condition(2, 2085)),
        Err(problem) => return Err(problem),
    };
    let handle_count: usize = state.handles.values().map(|handles| handles.len()).sum();
    if handle_count >= limits.max_handles {
        return Err(HostProblem::ResourceExhausted);
    }
    let handle = state.next_handle;
    state.next_handle = state
        .next_handle
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    Arc::make_mut(state.handles.entry(run.into()).or_default()).insert(handle, queue);
    Ok(MqResult {
        handle: Some(handle),
        ..success()
    })
}

fn close(state: &mut State, run: &str, request: &MqRequest) -> Result<MqResult, HostProblem> {
    let handle = request.handle.ok_or(HostProblem::Malformed)?;
    if state
        .handles
        .get_mut(run)
        .and_then(|handles| Arc::make_mut(handles).remove(&handle))
        .is_none()
    {
        return Ok(condition(2, 2019));
    }
    Ok(success())
}

fn put(
    state: &mut State,
    run: &str,
    request: &MqRequest,
    limits: MqLimits,
) -> Result<MqResult, HostProblem> {
    if request.message.len() > limits.max_message_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let queue_name = resolve_queue(state, run, request)?;
    let (queue_depth, trigger_program) = state
        .queues
        .get(&queue_name)
        .map(|queue| (queue.messages.len(), queue.trigger_program.clone()))
        .ok_or(HostProblem::NotFound)?;
    if queue_depth >= limits.max_messages_per_queue {
        return Err(HostProblem::ResourceExhausted);
    }
    let message_id = match request
        .message_id
        .clone()
        .filter(|value| value.iter().any(|byte| *byte != 0))
    {
        Some(message_id) => message_id,
        None => canonical_message_id(run, request)?,
    };
    let correlation_id = request
        .correlation_id
        .clone()
        .unwrap_or_else(|| vec![0; 24]);
    let message = Message {
        data: request.message.clone(),
        message_id: message_id.clone(),
        correlation_id: correlation_id.clone(),
    };
    let syncpoint = request.options & 2 != 0;
    if syncpoint {
        if state.pending.len() >= limits.max_pending_units && !state.pending.contains_key(run) {
            return Err(HostProblem::ResourceExhausted);
        }
        Arc::make_mut(state.pending.entry(run.into()).or_default())
            .puts
            .push((queue_name.clone(), message));
    } else {
        Arc::make_mut(
            state
                .queues
                .get_mut(&queue_name)
                .ok_or(HostProblem::NotFound)?,
        )
        .messages
        .push(message);
    }
    Ok(MqResult {
        message_id: Some(message_id),
        correlation_id: Some(correlation_id),
        trigger_program,
        ..success()
    })
}

fn get(state: &mut State, run: &str, request: &MqRequest) -> Result<MqResult, HostProblem> {
    let queue_name = resolve_queue(state, run, request)?;
    let queue = state
        .queues
        .get_mut(&queue_name)
        .ok_or(HostProblem::NotFound)?;
    let queue = Arc::make_mut(queue);
    let wanted = request
        .correlation_id
        .as_ref()
        .filter(|value| value.iter().any(|byte| *byte != 0));
    let position = queue
        .messages
        .iter()
        .position(|message| wanted.is_none_or(|wanted| &message.correlation_id == wanted));
    let Some(position) = position else {
        return Ok(condition(2, 2033));
    };
    let message = queue.messages.remove(position);
    if request.options & 2 != 0 {
        Arc::make_mut(state.pending.entry(run.into()).or_default())
            .gets
            .push((queue_name, message.clone()));
    }
    let mut data = message.data.clone();
    data.truncate(request.max_message_bytes as usize);
    Ok(MqResult {
        message: data,
        message_id: Some(message.message_id),
        correlation_id: Some(message.correlation_id),
        ..success()
    })
}

fn commit(state: &mut State, run: &str, limits: MqLimits) -> Result<MqResult, HostProblem> {
    if let Some(pending) = state.pending.remove(run) {
        for (queue_name, message) in &pending.puts {
            let queue = Arc::make_mut(
                state
                    .queues
                    .get_mut(queue_name)
                    .ok_or(HostProblem::NotFound)?,
            );
            if queue.messages.len() >= limits.max_messages_per_queue {
                return Err(HostProblem::ResourceExhausted);
            }
            queue.messages.push(message.clone());
        }
    }
    Ok(success())
}

fn rollback(state: &mut State, run: &str, limits: MqLimits) -> Result<MqResult, HostProblem> {
    if let Some(pending) = state.pending.remove(run) {
        for (queue_name, message) in pending.gets.iter().rev() {
            let queue = Arc::make_mut(
                state
                    .queues
                    .get_mut(queue_name)
                    .ok_or(HostProblem::NotFound)?,
            );
            if queue.messages.len() >= limits.max_messages_per_queue {
                return Err(HostProblem::ResourceExhausted);
            }
            queue.messages.insert(0, message.clone());
        }
    }
    Ok(success())
}

fn resolve_queue(state: &State, run: &str, request: &MqRequest) -> Result<String, HostProblem> {
    resolve_queue_target(state, run, request).map(|(target, _)| target)
}

fn resolve_queue_target(
    state: &State,
    run: &str,
    request: &MqRequest,
) -> Result<(String, Vec<String>), HostProblem> {
    if request.operation != MqOperation::Open
        && request.operation != MqOperation::PutOne
        && let Some(handle) = request.handle
    {
        let target = state
            .handles
            .get(run)
            .and_then(|handles| handles.get(&handle))
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut path = vec![target.clone()];
        if let Some(name) = &request.queue {
            let (supplied, supplied_path) = resolve_named_queue(state, name, request.operation)?;
            if supplied != target {
                return Err(HostProblem::Malformed);
            }
            path.extend(supplied_path);
        }
        return Ok((target, path));
    }
    if request.operation == MqOperation::Close {
        return Err(HostProblem::Malformed);
    }
    let name = request.queue.as_deref().ok_or(HostProblem::Malformed)?;
    resolve_named_queue(state, name, request.operation)
}

fn resolve_named_queue(
    state: &State,
    name: &str,
    operation: MqOperation,
) -> Result<(String, Vec<String>), HostProblem> {
    let name = normalize(name)?;
    let Some(catalog) = &state.catalog else {
        return Ok((name.clone(), vec![name]));
    };
    let capability = if operation == MqOperation::Get {
        MqObjectCapability::Input
    } else {
        MqObjectCapability::Output
    };
    local_target(catalog, &name, capability)
}

fn success() -> MqResult {
    MqResult {
        completion_code: 0,
        reason_code: 0,
        handle: None,
        message: Vec::new(),
        message_id: None,
        correlation_id: None,
        trigger_program: None,
    }
}

fn condition(completion_code: i32, reason_code: i32) -> MqResult {
    MqResult {
        completion_code,
        reason_code,
        ..success()
    }
}

fn install_receipt(
    definitions: &[MqQueueDefinition],
    identity: String,
    replayed: bool,
) -> MqInstallReceipt {
    MqInstallReceipt {
        queues: definitions.len(),
        triggers: definitions
            .iter()
            .filter(|definition| definition.trigger_program.is_some())
            .count(),
        identity,
        replayed,
    }
}

fn validate_definitions(
    definitions: &[MqQueueDefinition],
    limits: MqLimits,
) -> Result<(), HostProblem> {
    if definitions.is_empty() || definitions.len() > limits.max_queues {
        return Err(HostProblem::ResourceExhausted);
    }
    let names = definitions
        .iter()
        .map(|definition| normalize(&definition.name))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if names.len() != definitions.len()
        || definitions.iter().any(|definition| {
            !is_canonical_name(&definition.name)
                || definition
                    .trigger_program
                    .as_ref()
                    .is_some_and(|program| !is_canonical_name(program))
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_state(state: &State, limits: MqLimits) -> Result<(), HostProblem> {
    if state.queues.len() > limits.max_queues
        || state.handles.len() > limits.max_handles
        || state.pending.len() > limits.max_pending_units
        || state.replay.len() > limits.max_replays
        || state
            .handles
            .values()
            .map(|handles| handles.len())
            .sum::<usize>()
            > limits.max_handles
        || state.queues.values().any(|queue| {
            queue.messages.len() > limits.max_messages_per_queue
                || queue
                    .messages
                    .iter()
                    .any(|message| !valid_message(message, limits))
        })
        || state.pending.values().any(|pending| {
            pending.puts.len() > limits.max_messages_per_queue
                || pending.gets.len() > limits.max_messages_per_queue
                || pending
                    .puts
                    .iter()
                    .chain(&pending.gets)
                    .any(|(queue, message)| {
                        !state.queues.contains_key(queue) || !valid_message(message, limits)
                    })
        })
    {
        return Err(HostProblem::ResourceExhausted);
    }
    if state.next_handle == 0
        || !state.queues.keys().all(|name| is_canonical_name(name))
        || state
            .handles
            .values()
            .flat_map(|handles| handles.iter())
            .any(|(handle, queue)| *handle == 0 || !state.queues.contains_key(queue))
        || state.replay.keys().any(String::is_empty)
        || state
            .replay
            .iter()
            .any(|(key, recorded)| validate_mq_recorded_result(key, recorded, limits).is_err())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    match (&state.catalog, &state.definitions) {
        (Some(catalog), definitions) => {
            if catalog.encode().is_err() || catalog.model_instances().next().is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
            let expected = local_definitions(catalog)
                .map(|(name, trigger)| {
                    (
                        name.as_str().to_owned(),
                        trigger.as_ref().map(|name| name.as_str().to_owned()),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if expected.len() != state.queues.len()
                || expected.iter().any(|(name, trigger)| {
                    state
                        .queues
                        .get(name)
                        .is_none_or(|queue| &queue.trigger_program != trigger)
                })
                || state.queues.keys().any(|name| !expected.contains_key(name))
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            if let Some(definitions) = definitions {
                validate_definitions(definitions, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if legacy_catalog(definitions, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?
                    != **catalog
                {
                    return Err(HostProblem::InfrastructureFailure);
                }
            }
        }
        (None, _)
            if !state.queues.is_empty()
                || !state.handles.is_empty()
                || !state.pending.is_empty()
                || !state.replay.is_empty() =>
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        (None, Some(_)) => return Err(HostProblem::InfrastructureFailure),
        (None, None) => {}
    }
    Ok(())
}

fn valid_message(message: &Message, limits: MqLimits) -> bool {
    message.data.len() <= limits.max_message_bytes
        && message.message_id.len() == 24
        && message.correlation_id.len() == 24
}

fn request_digest(request: &MqRequest) -> Result<[u8; 32], HostProblem> {
    canonical_mq_request_digest(request)
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

struct MqProvider {
    service: Arc<MqService>,
    descriptor: CapabilityDescriptor,
}

impl HostProvider for MqProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let sequence = effect.sequence;
        let resolution_tick = effect.deadline_tick.max(invocation.deadline_tick);
        let outcome = match effect.request {
            HostRequest::Mq(request) => self
                .service
                .execute_at(invocation, &request, resolution_tick)
                .map(HostResult::Mq),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult { sequence, outcome }
    }
}

pub fn mq_providers(
    service: Arc<MqService>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ["host.mq.read", "host.mq.write"]
        .into_iter()
        .map(|capability| {
            Arc::new(MqProvider {
                service: service.clone(),
                descriptor: CapabilityDescriptor {
                    capability: CapabilityId::new(capability, limits)
                        .expect("static MQ capability"),
                    provider_id: "mainframe-env-mq".into(),
                    generation: "1".into(),
                    request_schema: "mainframe-env.mq-request@1".into(),
                    result_schema: "mainframe-env.mq-result@1".into(),
                    max_request_bytes: 4 * 1024 * 1024,
                    max_result_bytes: 4 * 1024 * 1024,
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
        ArtifactRef, BoundedPayload, ExecutionId, IdempotencyKey, Principal, PrincipalId,
        RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::Mutation;
    use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
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

    impl MqReplayClock for PersistAwareReplayClock {
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

    fn invocation(run: &str) -> Invocation {
        let limits = InvocationLimits::default();
        let mut invocation = Invocation::new(
            RequestId::new(format!("request-{run}"), limits).unwrap(),
            ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
            RunUnitId::new(run, limits).unwrap(),
            None,
            Selector::new("mq:test", limits).unwrap(),
            ArtifactRef::new("mq:test", limits).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", limits).unwrap(),
                BTreeSet::from([CapabilityId::new("host.mq.write", limits).unwrap()]),
                limits,
            )
            .unwrap(),
            ServiceClass::Interactive,
            0,
            100,
            TraceId::new(format!("trace-{run}"), limits).unwrap(),
            IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            limits,
        )
        .unwrap();
        bind_host_context(&mut invocation, b"other-bindings|queue-manager");
        invocation
    }

    fn bind_host_context(invocation: &mut Invocation, value: &[u8]) {
        let (binding, schema) = crate::host_context::host_context_contract();
        invocation.bindings.insert(
            binding.into(),
            BoundedPayload::new(schema, value.to_vec(), InvocationLimits::default()).unwrap(),
        );
    }

    fn request(operation: MqOperation, sequence: u64) -> MqRequest {
        let limits = InvocationLimits::default();
        MqRequest {
            operation,
            queue: None,
            handle: None,
            options: 0,
            message: Vec::new(),
            message_id: None,
            correlation_id: None,
            wait_ticks: 0,
            max_message_bytes: 1024,
            mutation: Some(Mutation {
                sequence,
                idempotency_key: IdempotencyKey::new(
                    format!("effect-{operation:?}-{sequence}"),
                    limits,
                )
                .unwrap(),
                transaction: Some("MQ-TEST".into()),
            }),
        }
    }

    #[test]
    fn lowercase_queue_names_are_preserved_and_distinct() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        service
            .install(vec![
                MqQueueDefinition {
                    name: "queue".into(),
                    trigger_program: None,
                },
                MqQueueDefinition {
                    name: "QUEUE".into(),
                    trigger_program: None,
                },
            ])
            .unwrap();
        let mut put = request(MqOperation::PutOne, 1);
        put.queue = Some("queue".into());
        put.message = b"lowercase".to_vec();
        service.execute(&invocation("case-distinct"), &put).unwrap();
        assert_eq!(service.queue_depth("queue"), Ok(1));
        assert_eq!(service.queue_depth("QUEUE"), Ok(0));
        drop(service);
        let reopened = MqService::open(store, Default::default()).unwrap();
        assert_eq!(
            reopened.queue_messages("queue"),
            Ok(vec![b"lowercase".to_vec()])
        );
        assert_eq!(reopened.queue_depth("QUEUE"), Ok(0));
    }

    #[test]
    fn leading_blank_queue_name_is_rejected_before_mutation() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(
            service.install(vec![MqQueueDefinition {
                name: " queue".into(),
                trigger_program: None,
            }]),
            Err(HostProblem::Malformed)
        );
        assert!(
            store
                .list_provider_state(QUEUE_NAMESPACE, 2)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            service.install(vec![MqQueueDefinition {
                name: "queue".into(),
                trigger_program: Some(" bad".into()),
            }]),
            Err(HostProblem::Malformed)
        );
        assert!(
            store
                .list_provider_state(QUEUE_NAMESPACE, 2)
                .unwrap()
                .is_empty()
        );
        service
            .install(vec![MqQueueDefinition {
                name: "queue".into(),
                trigger_program: Some("trigger".into()),
            }])
            .unwrap();
        let mut open = request(MqOperation::Open, 1);
        open.queue = Some(" queue".into());
        assert_eq!(
            service.execute(&invocation("leading-blank"), &open),
            Err(HostProblem::Malformed)
        );
        assert!(
            store
                .list_provider_state(REPLAY_NAMESPACE, 2)
                .unwrap()
                .is_empty()
        );
        assert_eq!(service.queue_depth("queue"), Ok(0));
    }

    #[test]
    fn trailing_blank_and_null_queue_names_use_canonical_identity() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "queue   ".into(),
                trigger_program: Some("trigger\0padding".into()),
            }])
            .unwrap();
        let mut put = request(MqOperation::PutOne, 1);
        put.queue = Some("queue\0padding".into());
        put.message = b"canonical".to_vec();
        service
            .execute(&invocation("canonical-name"), &put)
            .unwrap();
        drop(service);
        let reopened = MqService::open(store, Default::default()).unwrap();
        assert_eq!(
            reopened.queue_messages("queue"),
            Ok(vec![b"canonical".to_vec()])
        );
    }

    fn cics_invocation(run: &str) -> Invocation {
        let mut invocation = invocation(run);
        invocation
            .bindings
            .remove(crate::host_context::host_context_contract().0);
        let (binding, schema) = crate::host_context::cics_execution_context_contract();
        invocation.bindings.insert(
            binding.into(),
            BoundedPayload::new(schema, b"local".to_vec(), InvocationLimits::default()).unwrap(),
        );
        invocation
    }

    fn cics_coordinator_invocation(run: &str, key: &IdempotencyKey, outer: &str) -> Invocation {
        let mut invocation = cics_invocation(run);
        invocation.bindings.insert(
            crate::retention::CICS_NESTED_EFFECT_ORIGIN_BINDING.into(),
            BoundedPayload::new(
                crate::retention::CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                key.as_str().as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        invocation.bindings.insert(
            crate::retention::CICS_OUTER_EFFECT_ORIGIN_BINDING.into(),
            BoundedPayload::new(
                crate::retention::CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
                outer.as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        invocation
    }

    fn cics_coordinator_request(operation: MqOperation, run: &str, sequence: u64) -> MqRequest {
        let mut request = request(operation, sequence);
        request.mutation.as_mut().unwrap().idempotency_key = IdempotencyKey::new(
            format!("cics:{run}:{sequence}"),
            InvocationLimits::default(),
        )
        .unwrap();
        request
    }

    #[test]
    fn attested_context_controls_direct_commit_and_backout_before_store_mutation() {
        for operation in [MqOperation::Commit, MqOperation::Rollback] {
            for (index, (binding, allowed)) in [
                (b"zos-batch|queue-manager".as_slice(), true),
                (b"zos-ims-batch-dli|queue-manager".as_slice(), true),
                (b"mqi-client|queue-manager".as_slice(), true),
                (b"other-bindings|queue-manager".as_slice(), true),
                (b"zos-batch|host-coordinator".as_slice(), false),
                (b"zos-ims-batch-dli|host-coordinator".as_slice(), false),
                (b"zos-cics|host-coordinator".as_slice(), false),
                (b"zos-ims|host-coordinator".as_slice(), false),
                (b"mqi-client|host-coordinator".as_slice(), false),
                (b"other-bindings|host-coordinator".as_slice(), false),
            ]
            .into_iter()
            .enumerate()
            {
                let store: Arc<dyn ProviderStateStore> =
                    Arc::new(MemoryStore::new(Default::default()));
                let service = MqService::open(store.clone(), Default::default()).unwrap();
                service
                    .install(vec![MqQueueDefinition {
                        name: "HOST.Q".into(),
                        trigger_program: None,
                    }])
                    .unwrap();
                let run = format!("host-{operation:?}-{index}");
                let mut invocation = invocation(&run);
                let mut put = request(MqOperation::PutOne, 1);
                put.queue = Some("HOST.Q".into());
                put.message = b"pending".to_vec();
                put.options = 2;
                service.execute(&invocation, &put).unwrap();
                let before_queue = store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap();
                let before_pending = store.list_provider_state(PENDING_NAMESPACE, 8).unwrap();
                let before_replay = store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap();
                bind_host_context(&mut invocation, binding);
                let direct = request(operation, 2);
                let replay_key = direct.mutation.as_ref().unwrap().idempotency_key.as_str();
                let result = service.execute(&invocation, &direct).unwrap();
                if allowed {
                    assert_eq!((result.completion_code, result.reason_code), (0, 0));
                    assert!(
                        store
                            .get_provider_state(REPLAY_NAMESPACE, replay_key)
                            .unwrap()
                            .is_some()
                    );
                } else {
                    assert_eq!((result.completion_code, result.reason_code), (2, 2012));
                    assert!(
                        store
                            .get_provider_state(REPLAY_NAMESPACE, replay_key)
                            .unwrap()
                            .is_none()
                    );
                    assert!(service.lock().unwrap().state.pending.contains_key(&run));
                    assert_eq!(
                        store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap(),
                        before_queue
                    );
                    assert_eq!(
                        store.list_provider_state(PENDING_NAMESPACE, 8).unwrap(),
                        before_pending
                    );
                    assert_eq!(
                        store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap(),
                        before_replay
                    );
                }
                assert_eq!(
                    service.queue_depth("HOST.Q"),
                    Ok(usize::from(allowed && operation == MqOperation::Commit))
                );
            }
        }
    }

    #[test]
    fn missing_malformed_and_contradictory_contexts_leave_pending_and_replay_untouched() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "HOST.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let mut caller = invocation("malformed-host");
        let mut put = request(MqOperation::PutOne, 1);
        put.queue = Some("HOST.Q".into());
        put.message = b"pending".to_vec();
        put.options = 2;
        service.execute(&caller, &put).unwrap();
        let (binding, _) = crate::host_context::host_context_contract();
        let (cics_binding, cics_schema) = crate::host_context::cics_execution_context_contract();
        let before_queue = store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap();
        let before_pending = store.list_provider_state(PENDING_NAMESPACE, 8).unwrap();
        let before_replay = store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap();
        for index in 0..7 {
            let mut candidate = caller.clone();
            match index {
                0 => {
                    candidate.bindings.remove(binding);
                }
                1 => {
                    candidate.bindings.insert(
                        binding.into(),
                        BoundedPayload::new(
                            "mainframe-env.mq.host-context@2",
                            b"zos-batch|queue-manager".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    );
                }
                2 => bind_host_context(&mut candidate, b"zos-batch|unknown"),
                3 => bind_host_context(&mut candidate, b"zos-cics|queue-manager"),
                4 => bind_host_context(&mut candidate, b"zos-ims|queue-manager"),
                5 => {
                    candidate.bindings.insert(
                        cics_binding.into(),
                        BoundedPayload::new(
                            cics_schema,
                            b"local".to_vec(),
                            InvocationLimits::default(),
                        )
                        .unwrap(),
                    );
                }
                _ => {
                    for (name, schema, value) in [
                        (
                            crate::retention::CICS_NESTED_EFFECT_ORIGIN_BINDING,
                            crate::retention::CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                            b"cics:malformed-host:106".as_slice(),
                        ),
                        (
                            crate::retention::CICS_OUTER_EFFECT_ORIGIN_BINDING,
                            crate::retention::CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
                            b"outer-effect".as_slice(),
                        ),
                    ] {
                        candidate.bindings.insert(
                            name.into(),
                            BoundedPayload::new(
                                schema,
                                value.to_vec(),
                                InvocationLimits::default(),
                            )
                            .unwrap(),
                        );
                    }
                }
            }
            let direct = request(MqOperation::Commit, 100 + index);
            assert_eq!(
                service.execute(&candidate, &direct),
                Err(HostProblem::Malformed)
            );
            assert_eq!(
                store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap(),
                before_replay
            );
            assert_eq!(
                store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap(),
                before_queue
            );
            assert_eq!(
                store.list_provider_state(PENDING_NAMESPACE, 8).unwrap(),
                before_pending
            );
            assert!(
                service
                    .lock()
                    .unwrap()
                    .state
                    .pending
                    .contains_key("malformed-host")
            );
            assert_eq!(service.queue_depth("HOST.Q"), Ok(0));
        }
        caller.bindings.remove(binding);
        caller.bindings.insert(
            cics_binding.into(),
            BoundedPayload::new(cics_schema, b"local".to_vec(), InvocationLimits::default())
                .unwrap(),
        );
        let direct = request(MqOperation::Commit, 200);
        let rejected = service.execute(&caller, &direct).unwrap();
        assert_eq!((rejected.completion_code, rejected.reason_code), (2, 2012));
        assert_eq!(
            store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap(),
            before_replay
        );
        assert_eq!(
            store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap(),
            before_queue
        );
        assert_eq!(
            store.list_provider_state(PENDING_NAMESPACE, 8).unwrap(),
            before_pending
        );
        let mut malformed_put = request(MqOperation::PutOne, 300);
        malformed_put.queue = Some("HOST.Q".into());
        malformed_put.message = b"must-not-write".to_vec();
        caller.bindings.remove(cics_binding);
        bind_host_context(&mut caller, b"zos-batch|unknown");
        assert_eq!(
            service.execute(&caller, &malformed_put),
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap(),
            before_queue
        );
        assert_eq!(
            store.list_provider_state(PENDING_NAMESPACE, 8).unwrap(),
            before_pending
        );
        assert_eq!(
            store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap(),
            before_replay
        );
    }

    #[test]
    fn selected_host_provider_rejects_ims_syncpoint_without_sqlite_row_changes() {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap());
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "IMS.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let mut invocation = invocation("ims-host-route");
        let mut put = request(MqOperation::PutOne, 1);
        put.queue = Some("IMS.Q".into());
        put.message = b"pending".to_vec();
        put.options = 2;
        service.execute(&invocation, &put).unwrap();
        let before_queue = store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap();
        let before_pending = store.list_provider_state(PENDING_NAMESPACE, 8).unwrap();
        let before_replay = store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap();
        let provider = mq_providers(service.clone(), InvocationLimits::default())
            .into_iter()
            .find(|provider| provider.descriptor().capability.as_str() == "host.mq.write")
            .unwrap();
        let direct = request(MqOperation::Commit, 2);
        let effect = EffectRequest {
            run_unit: invocation.run_unit_id.clone(),
            sequence: 2,
            deadline_tick: invocation.deadline_tick,
            idempotency_key: direct
                .mutation
                .as_ref()
                .map(|value| value.idempotency_key.clone()),
            request: HostRequest::Mq(direct),
        };
        bind_host_context(&mut invocation, b"zos-ims|host-coordinator");
        let result = provider.invoke(&invocation, effect.clone());
        let Ok(HostResult::Mq(rejected)) = result.outcome else {
            panic!("IMS direct commit must have an MQ context result");
        };
        assert_eq!((rejected.completion_code, rejected.reason_code), (2, 2012));
        invocation
            .bindings
            .remove(crate::host_context::host_context_contract().0);
        assert_eq!(
            provider.invoke(&invocation, effect).outcome,
            Err(HostProblem::Malformed)
        );
        assert_eq!(
            store.list_provider_state(QUEUE_NAMESPACE, 8).unwrap(),
            before_queue
        );
        assert_eq!(
            store.list_provider_state(PENDING_NAMESPACE, 8).unwrap(),
            before_pending
        );
        assert_eq!(
            store.list_provider_state(REPLAY_NAMESPACE, 8).unwrap(),
            before_replay
        );
        assert_eq!(service.queue_depth("IMS.Q"), Ok(0));
    }

    #[test]
    fn cics_direct_commit_and_backout_are_rejected_but_host_syncpoint_can_coordinate() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store, Default::default()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "CICS.SYNC.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let run = "cics-syncpoint-owner";

        let mut first_put = request(MqOperation::PutOne, 1);
        first_put.queue = Some("CICS.SYNC.Q".into());
        first_put.message = b"COMMIT".to_vec();
        first_put.options = 2;
        service.execute(&invocation(run), &first_put).unwrap();
        assert_eq!(service.queue_depth("CICS.SYNC.Q"), Ok(0));

        let direct_commit = request(MqOperation::Commit, 2);
        let rejected = service
            .execute(&cics_invocation(run), &direct_commit)
            .unwrap();
        assert_eq!((rejected.completion_code, rejected.reason_code), (2, 2012));
        assert_eq!(service.queue_depth("CICS.SYNC.Q"), Ok(0));

        let coordinated_commit = cics_coordinator_request(MqOperation::Commit, run, 3);
        let commit_key = coordinated_commit
            .mutation
            .as_ref()
            .unwrap()
            .idempotency_key
            .clone();
        let committed = service
            .execute(
                &cics_coordinator_invocation(run, &commit_key, "outer-cics-commit"),
                &coordinated_commit,
            )
            .unwrap();
        assert_eq!((committed.completion_code, committed.reason_code), (0, 0));
        assert_eq!(service.queue_depth("CICS.SYNC.Q"), Ok(1));

        let mut second_put = request(MqOperation::PutOne, 4);
        second_put.queue = Some("CICS.SYNC.Q".into());
        second_put.message = b"ROLLBACK".to_vec();
        second_put.options = 2;
        service.execute(&invocation(run), &second_put).unwrap();

        let direct_backout = request(MqOperation::Rollback, 5);
        let rejected = service
            .execute(&cics_invocation(run), &direct_backout)
            .unwrap();
        assert_eq!((rejected.completion_code, rejected.reason_code), (2, 2012));
        assert_eq!(service.queue_depth("CICS.SYNC.Q"), Ok(1));

        let coordinated_backout = cics_coordinator_request(MqOperation::Rollback, run, 6);
        let backout_key = coordinated_backout
            .mutation
            .as_ref()
            .unwrap()
            .idempotency_key
            .clone();
        let backed_out = service
            .execute(
                &cics_coordinator_invocation(run, &backout_key, "outer-cics-backout"),
                &coordinated_backout,
            )
            .unwrap();
        assert_eq!((backed_out.completion_code, backed_out.reason_code), (0, 0));
        assert_eq!(service.queue_depth("CICS.SYNC.Q"), Ok(1));
    }

    #[test]
    fn malformed_or_partial_cics_syncpoint_provenance_fails_closed() {
        let service = MqService::open(
            Arc::new(MemoryStore::new(Default::default())),
            Default::default(),
        )
        .unwrap();
        let request = request(MqOperation::Commit, 10);
        let mut malformed = invocation("malformed-cics-context");
        let (binding, schema) = crate::host_context::cics_execution_context_contract();
        malformed.bindings.insert(
            binding.into(),
            BoundedPayload::new(schema, b"unknown".to_vec(), InvocationLimits::default()).unwrap(),
        );
        assert_eq!(
            service.execute(&malformed, &request),
            Err(HostProblem::Malformed)
        );

        let mut partial = cics_invocation("partial-cics-context");
        partial.bindings.insert(
            crate::retention::CICS_NESTED_EFFECT_ORIGIN_BINDING.into(),
            BoundedPayload::new(
                crate::retention::CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                b"cics:partial-cics-context:10".to_vec(),
                InvocationLimits::default(),
            )
            .unwrap(),
        );
        assert_eq!(
            service.execute(&partial, &request),
            Err(HostProblem::Malformed)
        );
    }

    #[test]
    fn cics_nested_owner_mismatch_preserves_pending_work() {
        let service = MqService::open(
            Arc::new(MemoryStore::new(Default::default())),
            Default::default(),
        )
        .unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "CICS.OWNER.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let run = "cics-owner-mismatch";
        let mut put = request(MqOperation::PutOne, 1);
        put.queue = Some("CICS.OWNER.Q".into());
        put.message = b"pending".to_vec();
        put.options = 2;
        service.execute(&invocation(run), &put).unwrap();

        let commit = cics_coordinator_request(MqOperation::Commit, run, 2);
        let wrong_key =
            IdempotencyKey::new("cics:other-run:2", InvocationLimits::default()).unwrap();
        let wrong_owner = cics_coordinator_invocation(run, &wrong_key, "outer-cics-commit");
        assert_eq!(
            service.execute(&wrong_owner, &commit),
            Err(HostProblem::Malformed)
        );
        assert_eq!(service.queue_depth("CICS.OWNER.Q"), Ok(0));

        let key = commit.mutation.as_ref().unwrap().idempotency_key.clone();
        let accepted = service
            .execute(
                &cics_coordinator_invocation(run, &key, "outer-cics-commit"),
                &commit,
            )
            .unwrap();
        assert_eq!((accepted.completion_code, accepted.reason_code), (0, 0));
        assert_eq!(service.queue_depth("CICS.OWNER.Q"), Ok(1));
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
        let limits = MqLimits {
            max_replays: 1,
            ..MqLimits::default()
        };
        let first = MqService::open(store.clone(), limits).unwrap();
        first
            .install(vec![MqQueueDefinition {
                name: "REFRESH.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let invocation = invocation("refresh-mq");
        let mut old = request(MqOperation::PutOne, 801);
        old.queue = Some("REFRESH.Q".into());
        old.message = b"old".to_vec();
        first.execute(&invocation, &old).unwrap();

        let second = MqService::open(store.clone(), limits).unwrap();
        let old_key = old.mutation.as_ref().unwrap().idempotency_key.as_str();
        let old_row = store
            .get_provider_state(REPLAY_NAMESPACE, old_key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(REPLAY_NAMESPACE, old_key, old_row.version)
            .unwrap();

        let mut fresh = request(MqOperation::PutOne, 802);
        fresh.queue = Some("REFRESH.Q".into());
        fresh.message = b"fresh".to_vec();
        second.execute(&invocation, &fresh).unwrap();
        assert_eq!(second.queue_depth("REFRESH.Q").unwrap(), 2);

        let fresh_key = fresh.mutation.as_ref().unwrap().idempotency_key.as_str();
        let fresh_row = store
            .get_provider_state(REPLAY_NAMESPACE, fresh_key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(REPLAY_NAMESPACE, fresh_key, fresh_row.version)
            .unwrap();
        second.execute(&invocation, &old).unwrap();
        assert_eq!(second.queue_depth("REFRESH.Q").unwrap(), 3);

        // Keep both live instances in scope: this specifically exercises stale
        // process-local state, not a close/reopen refresh.
        assert_eq!(first.queue_depth("REFRESH.Q").unwrap(), 1);
    }

    #[test]
    fn delayed_resolution_is_observed_after_protected_replay_persistence() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let invocation = invocation("delayed-mq");
        let mut request = request(MqOperation::PutOne, 803);
        request.queue = Some("DELAYED.Q".into());
        request.message = b"delayed".to_vec();
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
            MqService::open_with_replay_clock(store.clone(), MqLimits::default(), clock).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "DELAYED.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        service.inject_unknown_outcome_once();
        assert_eq!(
            service.execute(&invocation, &request),
            Err(HostProblem::UnknownOutcome)
        );
        let pending = store
            .get_provider_state(REPLAY_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        let pending: serde_json::Value = serde_json::from_slice(&pending.payload).unwrap();
        assert_eq!(pending["value"]["resolution_tick"], 0);
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
        let invocation = invocation("fault-mq");
        let mut request = request(MqOperation::PutOne, 804);
        request.queue = Some("FAULT.Q".into());
        request.message = b"once".to_vec();
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
            MqService::open_with_replay_clock(store.clone(), MqLimits::default(), clock).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "FAULT.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        assert_eq!(
            service.execute(&invocation, &request),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            service.execute(&invocation, &request),
            Err(HostProblem::UnknownOutcome)
        );
        service.execute(&invocation, &request).unwrap();
        assert_eq!(service.queue_depth("FAULT.Q").unwrap(), 1);
        let row = store
            .get_provider_state(REPLAY_NAMESPACE, &key)
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        assert_eq!(value["value"]["resolution_tick"], 250);
    }

    #[test]
    fn mq_queue_denial_precedes_mutation() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let policy = Arc::new(DenyEnterprise::default());
        let service =
            MqService::open_authorized(store, MqLimits::default(), policy.clone()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "DENIED.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let mut put = request(MqOperation::PutOne, 700);
        put.queue = Some("DENIED.Q".into());
        put.message = b"must-not-commit".to_vec();
        assert_eq!(
            service.execute(&invocation("deny-mq"), &put),
            Err(HostProblem::Unauthorized)
        );
        assert!(
            service.lock().unwrap().state.queues["DENIED.Q"]
                .messages
                .is_empty()
        );
        assert_eq!(
            policy.seen.lock().unwrap().as_slice(),
            &[EnterpriseResource::new(
                EnterpriseResourceClass::MqQueue,
                "DENIED.Q",
                AccessIntent::Update,
            )
            .unwrap()]
        );
    }

    /// #183 regression: a run that never opened an MQ unit of work, with
    /// MQ never installed at all, must be able to ROLLBACK even under a
    /// denying authorizer and without persisting a durable replay row.
    #[test]
    fn mq_rollback_with_no_pending_work_is_not_authorized_against_a_phantom_unit_of_work() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let policy = Arc::new(DenyEnterprise::default());
        let service =
            MqService::open_authorized(store, MqLimits::default(), policy.clone()).unwrap();
        // No `install` and no prior mutating MQ request under this run:
        // this run unit never opened an MQ unit of work, and MQ itself has
        // no installed definitions.
        let rolled_back = service.execute(
            &invocation("never-touched-mq"),
            &request(MqOperation::Rollback, 1),
        );
        assert_eq!(rolled_back, Ok(success()));
        assert!(
            policy.seen.lock().unwrap().is_empty(),
            "a rollback with nothing pending must not ask the authorizer about a phantom unit of work"
        );
    }

    /// #183: a run that opened a real MQ unit of work must still be
    /// authorized to ROLLBACK it against the staged queue -- this is the
    /// surviving branch the phantom-unit-of-work fix must not break.
    #[test]
    fn mq_rollback_with_pending_put_is_authorized_against_the_staged_queue() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let unauthorized = MqService::open(store.clone(), MqLimits::default()).unwrap();
        unauthorized
            .install(vec![MqQueueDefinition {
                name: "REQUEST.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let staging = invocation("pending-mq");
        let mut open = request(MqOperation::Open, 1);
        open.queue = Some("REQUEST.Q".into());
        let handle = unauthorized
            .execute(&staging, &open)
            .unwrap()
            .handle
            .unwrap();
        let mut put = request(MqOperation::Put, 2);
        put.handle = Some(handle);
        put.options = 2;
        put.message = b"STAGED".to_vec();
        unauthorized.execute(&staging, &put).unwrap();

        let policy = Arc::new(DenyEnterprise::default());
        let service =
            MqService::open_authorized(store, MqLimits::default(), policy.clone()).unwrap();
        let rolled_back = service.execute(&staging, &request(MqOperation::Rollback, 3));
        assert_eq!(rolled_back, Err(HostProblem::Unauthorized));
        assert_eq!(
            policy.seen.lock().unwrap().as_slice(),
            &[EnterpriseResource::new(
                EnterpriseResourceClass::MqQueue,
                "REQUEST.Q",
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
    fn mq_no_op_rollback_replay_survives_later_staged_work_on_the_same_run() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store, MqLimits::default()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "REQUEST.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let invocation = invocation("redeliver-mq");
        let noop = request(MqOperation::Rollback, 1);
        let first = service.execute(&invocation, &noop).unwrap();

        let mut open = request(MqOperation::Open, 2);
        open.queue = Some("REQUEST.Q".into());
        let handle = service.execute(&invocation, &open).unwrap().handle.unwrap();
        let mut put = request(MqOperation::Put, 3);
        put.handle = Some(handle);
        put.options = 2;
        put.message = b"STAGED".to_vec();
        service.execute(&invocation, &put).unwrap();

        let redelivered = service.execute(&invocation, &noop).unwrap();
        assert_eq!(redelivered, first);
        assert!(
            service
                .lock()
                .unwrap()
                .state
                .pending
                .contains_key("redeliver-mq"),
            "a replayed no-op rollback must not roll back work staged after it"
        );
    }

    #[test]
    fn generated_message_id_has_a_canonical_golden_identity() {
        let mut put = request(MqOperation::PutOne, 77);
        put.queue = Some("GOLDEN.Q".into());
        put.message = vec![0, 10, 255];
        let message_id = canonical_message_id("RUN-GOLDEN", &put).unwrap();
        let encoded = message_id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(encoded, "c2f60b319528714c9aee52203dc22c9b6b3b7464e7fca383");
        put.message.push(1);
        assert_ne!(
            canonical_message_id("RUN-GOLDEN", &put).unwrap(),
            message_id
        );
    }

    #[test]
    fn correlation_syncpoint_timeout_unknown_outcome_and_restart_are_durable() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
        let definitions = vec![
            MqQueueDefinition {
                name: "REQUEST.Q".into(),
                trigger_program: Some("PROCESS".into()),
            },
            MqQueueDefinition {
                name: "REPLY.Q".into(),
                trigger_program: None,
            },
        ];
        assert!(!service.install(definitions.clone()).unwrap().replayed);
        assert!(service.install(definitions).unwrap().replayed);
        let invocation = invocation("run-a");
        let mut open = request(MqOperation::Open, 1);
        open.queue = Some("REQUEST.Q".into());
        let handle = service.execute(&invocation, &open).unwrap().handle.unwrap();
        let correlation = vec![7; 24];
        let mut put = request(MqOperation::Put, 2);
        put.handle = Some(handle);
        put.options = 2;
        put.message = b"REQUEST".to_vec();
        put.correlation_id = Some(correlation.clone());
        assert_eq!(
            service
                .execute(&invocation, &put)
                .unwrap()
                .trigger_program
                .as_deref(),
            Some("PROCESS")
        );
        assert_eq!(service.queue_depth("REQUEST.Q").unwrap(), 0);
        service
            .execute(&invocation, &request(MqOperation::Commit, 3))
            .unwrap();
        assert_eq!(service.queue_depth("REQUEST.Q").unwrap(), 1);
        let mut get = request(MqOperation::Get, 4);
        get.handle = Some(handle);
        get.options = 2;
        get.correlation_id = Some(correlation.clone());
        assert_eq!(
            service.execute(&invocation, &get).unwrap().message,
            b"REQUEST"
        );
        service
            .execute(&invocation, &request(MqOperation::Rollback, 5))
            .unwrap();
        assert_eq!(service.queue_depth("REQUEST.Q").unwrap(), 1);
        let mut timeout = request(MqOperation::Get, 6);
        timeout.handle = Some(handle);
        timeout.correlation_id = Some(vec![9; 24]);
        timeout.wait_ticks = 10;
        assert_eq!(
            service.execute(&invocation, &timeout).unwrap().reason_code,
            2033
        );
        let mut put_one = request(MqOperation::PutOne, 7);
        put_one.queue = Some("REPLY.Q".into());
        put_one.message = b"REPLY".to_vec();
        service.inject_unknown_outcome_once();
        assert_eq!(
            service.execute(&invocation, &put_one),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(
            service
                .execute(&invocation, &put_one)
                .unwrap()
                .completion_code,
            0
        );
        drop(service);
        let reopened = MqService::open(store, MqLimits::default()).unwrap();
        assert_eq!(reopened.queue_messages("REPLY.Q").unwrap(), [b"REPLY"]);
    }

    #[test]
    fn legacy_replay_requires_attested_canonical_migration_without_redispatch() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let service = MqService::open(store.clone(), MqLimits::default()).unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "LEGACY.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let invocation = invocation("legacy-replay");
        let mut put = request(MqOperation::PutOne, 99);
        put.queue = Some("LEGACY.Q".into());
        put.message = b"ONCE".to_vec();
        let key = put.mutation.as_ref().unwrap().idempotency_key.clone();
        let original = service.execute(&invocation, &put).unwrap();
        assert_eq!(service.queue_depth("LEGACY.Q"), Ok(1));
        drop(service);

        retain_as_legacy_replay(store.as_ref(), &key, [0x11; 32]);
        let reopened = MqService::open(store.clone(), MqLimits::default()).unwrap();
        assert_eq!(
            reopened.execute(&invocation, &put),
            Err(HostProblem::UnknownOutcome)
        );
        assert_eq!(reopened.queue_depth("LEGACY.Q"), Ok(1));
        assert_eq!(
            reopened.reconcile_legacy_replay(&key, [0x22; 32], &put),
            Err(HostProblem::IdempotencyConflict)
        );
        reopened
            .reconcile_legacy_replay(&key, [0x11; 32], &put)
            .unwrap();
        assert_eq!(reopened.execute(&invocation, &put), Ok(original.clone()));
        assert_eq!(reopened.queue_depth("LEGACY.Q"), Ok(1));
        drop(reopened);

        let restarted = MqService::open(store, MqLimits::default()).unwrap();
        assert_eq!(restarted.execute(&invocation, &put), Ok(original));
        assert_eq!(restarted.queue_depth("LEGACY.Q"), Ok(1));
    }

    #[test]
    fn legacy_blob_migrates_atomically_to_versioned_scoped_rows_and_corruption_fails_closed() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let definitions = vec![
            MqQueueDefinition {
                name: "QUEUE.A".into(),
                trigger_program: Some("TRIGGERA".into()),
            },
            MqQueueDefinition {
                name: "QUEUE.B".into(),
                trigger_program: None,
            },
        ];
        let legacy = State {
            definitions: Some(definitions),
            catalog: None,
            queues: BTreeMap::from([
                (
                    "QUEUE.A".into(),
                    Arc::new(Queue {
                        trigger_program: Some("TRIGGERA".into()),
                        messages: vec![Message {
                            data: b"BEFORE".to_vec(),
                            message_id: vec![1; 24],
                            correlation_id: vec![2; 24],
                        }],
                    }),
                ),
                ("QUEUE.B".into(), Arc::new(Queue::default())),
            ]),
            handles: BTreeMap::new(),
            pending: BTreeMap::new(),
            replay: BTreeMap::new(),
            next_handle: 7,
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
                    namespace: QUEUE_NAMESPACE.into(),
                    key: "ORPHAN".into(),
                    version: 1,
                    payload: encode_object_row("ORPHAN", &Queue::default()).unwrap(),
                },
                None,
            )
            .unwrap();
        assert!(matches!(
            MqService::open(store.clone(), Default::default()),
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
            .delete_provider_state(QUEUE_NAMESPACE, "ORPHAN", 1)
            .unwrap();

        let service = MqService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(service.queue_messages("QUEUE.A").unwrap(), [b"BEFORE"]);
        let manifest_before = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let manifest: RowStoreManifest = serde_json::from_slice(&manifest_before.payload).unwrap();
        assert_eq!(manifest.schema_version, ROW_STORE_SCHEMA);
        assert_eq!(manifest_before.version, 2);
        assert_eq!(
            store.list_provider_state(QUEUE_NAMESPACE, 3).unwrap().len(),
            2
        );
        let queue_b_before = store
            .get_provider_state(QUEUE_NAMESPACE, "QUEUE.B")
            .unwrap()
            .unwrap();
        let queue_a_version = store
            .get_provider_state(QUEUE_NAMESPACE, "QUEUE.A")
            .unwrap()
            .unwrap()
            .version;
        let mut put = request(MqOperation::PutOne, 301);
        put.queue = Some("QUEUE.A".into());
        put.message = b"AFTER".to_vec();
        service.execute(&invocation("row-scope"), &put).unwrap();
        assert_eq!(
            store
                .get_provider_state(QUEUE_NAMESPACE, "QUEUE.B")
                .unwrap()
                .unwrap(),
            queue_b_before
        );
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap(),
            manifest_before
        );
        assert!(
            store
                .get_provider_state(
                    REPLAY_NAMESPACE,
                    put.mutation.as_ref().unwrap().idempotency_key.as_str(),
                )
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store
                .get_provider_state(QUEUE_NAMESPACE, "QUEUE.A")
                .unwrap()
                .unwrap()
                .version,
            queue_a_version + 1
        );
        drop(service);

        let queue_a = store
            .get_provider_state(QUEUE_NAMESPACE, "QUEUE.A")
            .unwrap()
            .unwrap();
        let mut corrupt: serde_json::Value = serde_json::from_slice(&queue_a.payload).unwrap();
        corrupt["schema_version"] = serde_json::json!("mainframe-env.mq-object-row@999");
        store
            .put_provider_state(
                ProviderStateRecord {
                    version: queue_a.version + 1,
                    payload: serde_json::to_vec(&corrupt).unwrap(),
                    ..queue_a
                },
                Some(queue_a.version),
            )
            .unwrap();
        assert!(matches!(
            MqService::open(store, Default::default()),
            Err(HostProblem::InfrastructureFailure)
        ));
    }

    #[test]
    fn minimal_legacy_blob_is_always_replaced_by_a_versioned_manifest() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let legacy = State {
            next_handle: 1,
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

        drop(MqService::open(store.clone(), Default::default()).unwrap());
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
    fn capacity_failure_leaves_the_legacy_blob_and_all_target_namespaces_unchanged() {
        let store = Arc::new(MemoryStore::new(StoreLimits {
            max_provider_state: 2,
            ..StoreLimits::default()
        }));
        let definitions = vec![
            MqQueueDefinition {
                name: "ONE".into(),
                trigger_program: None,
            },
            MqQueueDefinition {
                name: "TWO".into(),
                trigger_program: None,
            },
        ];
        let legacy = State {
            definitions: Some(definitions),
            queues: BTreeMap::from([
                ("ONE".into(), Arc::new(Queue::default())),
                ("TWO".into(), Arc::new(Queue::default())),
            ]),
            next_handle: 1,
            ..State::default()
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
        assert_eq!(
            MqService::open(store.clone(), Default::default()).map(|_| ()),
            Err(HostProblem::ResourceExhausted)
        );
        assert_eq!(
            store
                .get_provider_state(STATE_NAMESPACE, STATE_KEY)
                .unwrap()
                .unwrap()
                .payload,
            legacy_payload
        );
        assert!(
            store
                .list_provider_state(QUEUE_NAMESPACE, 2)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sqlite_executes_and_reopens_the_versioned_row_layout() {
        let store: Arc<dyn ProviderStateStore> =
            Arc::new(SqliteStateStore::open("sqlite::memory:", 64 * 1024 * 1024, 262_144).unwrap());
        let legacy = State {
            definitions: Some(vec![MqQueueDefinition {
                name: "SQLITE.Q".into(),
                trigger_program: None,
            }]),
            queues: BTreeMap::from([("SQLITE.Q".into(), Arc::new(Queue::default()))]),
            next_handle: 1,
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
        let service = MqService::open(store.clone(), Default::default()).unwrap();
        let mut put = request(MqOperation::PutOne, 601);
        put.queue = Some("SQLITE.Q".into());
        put.message = b"ROW".to_vec();
        service.execute(&invocation("sqlite-row"), &put).unwrap();
        drop(service);

        let reopened = MqService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(reopened.queue_messages("SQLITE.Q").unwrap(), [b"ROW"]);
        assert_eq!(
            store.list_provider_state(QUEUE_NAMESPACE, 2).unwrap().len(),
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
    fn independent_queue_rows_commit_from_separate_service_instances_without_global_cas() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let installer = MqService::open(store.clone(), Default::default()).unwrap();
        installer
            .install(vec![
                MqQueueDefinition {
                    name: "LEFT.Q".into(),
                    trigger_program: None,
                },
                MqQueueDefinition {
                    name: "RIGHT.Q".into(),
                    trigger_program: None,
                },
            ])
            .unwrap();
        drop(installer);
        let manifest = store
            .get_provider_state(STATE_NAMESPACE, STATE_KEY)
            .unwrap()
            .unwrap();
        let left = MqService::open(store.clone(), Default::default()).unwrap();
        let right = MqService::open(store.clone(), Default::default()).unwrap();
        let left_worker = std::thread::spawn(move || {
            let mut put = request(MqOperation::PutOne, 901);
            put.queue = Some("LEFT.Q".into());
            put.message = b"LEFT".to_vec();
            left.execute(&invocation("left-row"), &put)
        });
        let right_worker = std::thread::spawn(move || {
            let mut put = request(MqOperation::PutOne, 902);
            put.queue = Some("RIGHT.Q".into());
            put.message = b"RIGHT".to_vec();
            right.execute(&invocation("right-row"), &put)
        });
        left_worker.join().unwrap().unwrap();
        right_worker.join().unwrap().unwrap();

        let reopened = MqService::open(store.clone(), Default::default()).unwrap();
        assert_eq!(reopened.queue_messages("LEFT.Q").unwrap(), [b"LEFT"]);
        assert_eq!(reopened.queue_messages("RIGHT.Q").unwrap(), [b"RIGHT"]);
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
        let service = MqService::open(
            store.clone(),
            MqLimits {
                max_replays: 2,
                ..Default::default()
            },
        )
        .unwrap();
        service
            .install(vec![MqQueueDefinition {
                name: "RETENTION.Q".into(),
                trigger_program: None,
            }])
            .unwrap();
        let mut expired_invocation = invocation("expired-replay");
        expired_invocation.deadline_tick = 10;
        let mut expired = request(MqOperation::PutOne, 981);
        expired.queue = Some("RETENTION.Q".into());
        expired.message = b"EXPIRED".to_vec();
        service.execute(&expired_invocation, &expired).unwrap();
        let mut live_invocation = invocation("live-replay");
        live_invocation.deadline_tick = 95;
        let mut live = request(MqOperation::PutOne, 982);
        live.queue = Some("RETENTION.Q".into());
        live.message = b"LIVE".to_vec();
        let live_result = service.execute(&live_invocation, &live).unwrap();
        finish_invocation(store.as_ref(), &expired_invocation);
        finish_invocation(store.as_ref(), &live_invocation);
        let expired_key = expired.mutation.as_ref().unwrap().idempotency_key.clone();
        let live_key = live.mutation.as_ref().unwrap().idempotency_key.clone();
        let raw_retention = store.archive_and_prune(
            retention_policy(),
            RetentionRequest {
                target: RetentionTarget::MqReplay,
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
        let mut fresh = request(MqOperation::PutOne, 983);
        fresh.queue = Some("RETENTION.Q".into());
        fresh.message = b"FRESH".to_vec();
        service
            .execute(&invocation("fresh-replay"), &fresh)
            .unwrap();
        assert_eq!(service.queue_messages("RETENTION.Q").unwrap().len(), 3);
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
