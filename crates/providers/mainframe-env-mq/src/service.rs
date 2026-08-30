use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, EffectResult, HostProblem, HostProvider, HostRequest,
    HostResult, MqOperation, MqRequest, MqResult,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

const STATE_NAMESPACE: &str = "mq-state";
const STATE_KEY: &str = "queues";

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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RecordedResult {
    request_sha256: [u8; 32],
    completion_code: i32,
    reason_code: i32,
    handle: Option<u32>,
    message: Vec<u8>,
    message_id: Option<Vec<u8>>,
    correlation_id: Option<Vec<u8>>,
    trigger_program: Option<String>,
}

impl RecordedResult {
    fn from_result(request_sha256: [u8; 32], result: &MqResult) -> Self {
        Self {
            request_sha256,
            completion_code: result.completion_code,
            reason_code: result.reason_code,
            handle: result.handle,
            message: result.message.clone(),
            message_id: result.message_id.clone(),
            correlation_id: result.correlation_id.clone(),
            trigger_program: result.trigger_program.clone(),
        }
    }

    fn result(&self) -> MqResult {
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
    queues: BTreeMap<String, Queue>,
    handles: BTreeMap<String, BTreeMap<u32, String>>,
    pending: BTreeMap<String, PendingUnit>,
    replay: BTreeMap<String, RecordedResult>,
    next_handle: u32,
}

struct DurableState {
    version: u64,
    state: State,
}

pub struct MqService {
    store: Arc<dyn ProviderStateStore>,
    limits: MqLimits,
    durable: Mutex<DurableState>,
    unknown_after_persist: AtomicBool,
}

impl MqService {
    pub fn open(
        store: Arc<dyn ProviderStateStore>,
        limits: MqLimits,
    ) -> Result<Arc<Self>, HostProblem> {
        let (version, mut state) = match store
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
        if state.next_handle == 0 {
            state.next_handle = 1;
        }
        validate_state(&state, limits)?;
        Ok(Arc::new(Self {
            store,
            limits,
            durable: Mutex::new(DurableState { version, state }),
            unknown_after_persist: AtomicBool::new(false),
        }))
    }

    pub fn install(
        &self,
        definitions: Vec<MqQueueDefinition>,
    ) -> Result<MqInstallReceipt, HostProblem> {
        validate_definitions(&definitions, self.limits)?;
        let identity = format!(
            "sha256:{:x}",
            Sha256::digest(
                serde_json::to_vec(&definitions).map_err(|_| HostProblem::ProviderFailure)?
            )
        );
        let mut durable = self.lock()?;
        if let Some(current) = &durable.state.definitions {
            if current != &definitions {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(install_receipt(&definitions, identity, true));
        }
        let mut next = durable.state.clone();
        for definition in &definitions {
            next.queues.insert(
                normalize(&definition.name),
                Queue {
                    trigger_program: definition
                        .trigger_program
                        .as_ref()
                        .map(|value| normalize(value)),
                    messages: Vec::new(),
                },
            );
        }
        next.definitions = Some(definitions.clone());
        self.persist(&mut durable, next)?;
        Ok(install_receipt(&definitions, identity, false))
    }

    pub fn execute(
        &self,
        invocation: &Invocation,
        request: &MqRequest,
    ) -> Result<MqResult, HostProblem> {
        let mut durable = self.lock()?;
        let request_sha256 = request_digest(request);
        let replay_key = request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.as_str())
            .ok_or(HostProblem::MissingIdempotency)?;
        if let Some(recorded) = durable.state.replay.get(replay_key) {
            return if recorded.request_sha256 == request_sha256 {
                Ok(recorded.result())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let mut next = durable.state.clone();
        let result = apply_request(
            &mut next,
            invocation.run_unit_id.as_str(),
            request,
            self.limits,
        )?;
        if next.replay.len() >= self.limits.max_replays {
            return Err(HostProblem::ResourceExhausted);
        }
        next.replay.insert(
            replay_key.into(),
            RecordedResult::from_result(request_sha256, &result),
        );
        validate_state(&next, self.limits)?;
        self.persist(&mut durable, next)?;
        if self.unknown_after_persist.swap(false, Ordering::SeqCst) {
            Err(HostProblem::UnknownOutcome)
        } else {
            Ok(result)
        }
    }

    pub fn inject_unknown_outcome_once(&self) {
        self.unknown_after_persist.store(true, Ordering::SeqCst);
    }

    pub fn queue_depth(&self, queue: &str) -> Result<usize, HostProblem> {
        self.lock()?
            .state
            .queues
            .get(&normalize(queue))
            .map(|queue| queue.messages.len())
            .ok_or(HostProblem::NotFound)
    }

    pub fn queue_messages(&self, queue: &str) -> Result<Vec<Vec<u8>>, HostProblem> {
        self.lock()?
            .state
            .queues
            .get(&normalize(queue))
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
    let queue = normalize(request.queue.as_deref().ok_or(HostProblem::Malformed)?);
    if !state.queues.contains_key(&queue) {
        return Ok(condition(2, 2085));
    }
    let handle_count: usize = state.handles.values().map(BTreeMap::len).sum();
    if handle_count >= limits.max_handles {
        return Err(HostProblem::ResourceExhausted);
    }
    let handle = state.next_handle;
    state.next_handle = state
        .next_handle
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    state
        .handles
        .entry(run.into())
        .or_default()
        .insert(handle, queue);
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
        .and_then(|handles| handles.remove(&handle))
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
    let message_id = request
        .message_id
        .clone()
        .filter(|value| value.iter().any(|byte| *byte != 0))
        .unwrap_or_else(|| Sha256::digest(format!("{run}:{request:?}").as_bytes())[..24].to_vec());
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
        state
            .pending
            .entry(run.into())
            .or_default()
            .puts
            .push((queue_name.clone(), message));
    } else {
        state
            .queues
            .get_mut(&queue_name)
            .ok_or(HostProblem::NotFound)?
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
        state
            .pending
            .entry(run.into())
            .or_default()
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
        for (queue_name, message) in pending.puts {
            let queue = state
                .queues
                .get_mut(&queue_name)
                .ok_or(HostProblem::NotFound)?;
            if queue.messages.len() >= limits.max_messages_per_queue {
                return Err(HostProblem::ResourceExhausted);
            }
            queue.messages.push(message);
        }
    }
    Ok(success())
}

fn rollback(state: &mut State, run: &str, limits: MqLimits) -> Result<MqResult, HostProblem> {
    if let Some(pending) = state.pending.remove(run) {
        for (queue_name, message) in pending.gets.into_iter().rev() {
            let queue = state
                .queues
                .get_mut(&queue_name)
                .ok_or(HostProblem::NotFound)?;
            if queue.messages.len() >= limits.max_messages_per_queue {
                return Err(HostProblem::ResourceExhausted);
            }
            queue.messages.insert(0, message);
        }
    }
    Ok(success())
}

fn resolve_queue(state: &State, run: &str, request: &MqRequest) -> Result<String, HostProblem> {
    if let Some(queue) = &request.queue {
        return Ok(normalize(queue));
    }
    let handle = request.handle.ok_or(HostProblem::Malformed)?;
    state
        .handles
        .get(run)
        .and_then(|handles| handles.get(&handle))
        .cloned()
        .ok_or(HostProblem::NotFound)
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
        .collect::<BTreeSet<_>>();
    if names.len() != definitions.len()
        || definitions.iter().any(|definition| {
            definition.name.is_empty()
                || definition.name.len() > 128
                || definition
                    .trigger_program
                    .as_ref()
                    .is_some_and(|program| program.is_empty() || program.len() > 128)
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn validate_state(state: &State, limits: MqLimits) -> Result<(), HostProblem> {
    if state.queues.len() > limits.max_queues
        || state.pending.len() > limits.max_pending_units
        || state.replay.len() > limits.max_replays
        || state.handles.values().map(BTreeMap::len).sum::<usize>() > limits.max_handles
        || state.queues.values().any(|queue| {
            queue.messages.len() > limits.max_messages_per_queue
                || queue.messages.iter().any(|message| {
                    message.data.len() > limits.max_message_bytes
                        || message.message_id.len() != 24
                        || message.correlation_id.len() != 24
                })
        })
    {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn request_digest(request: &MqRequest) -> [u8; 32] {
    Sha256::digest(format!("{request:?}").as_bytes()).into()
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
        let outcome = match effect.request {
            HostRequest::Mq(request) => self
                .service
                .execute(invocation, &request)
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
        ArtifactRef, ExecutionId, IdempotencyKey, Principal, PrincipalId, RequestId,
        ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
    };
    use mainframe_env_host_api::Mutation;
    use mainframe_env_store::MemoryStore;

    fn invocation(run: &str) -> Invocation {
        let limits = InvocationLimits::default();
        Invocation::new(
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
        .unwrap()
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
}
