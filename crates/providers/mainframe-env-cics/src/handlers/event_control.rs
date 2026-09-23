//! Durable BTS activity event authority and its explicit execution context.

use super::super::{CicsService, Run, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

const CONTEXT_NAMESPACE: &str = "cics-event-context-v1";
const ACTIVITY_NAMESPACE: &str = "cics-event-activity-v1";
const MAX_ACTIVITY_BYTES: usize = 1_048_576;
const MAX_EVENTS: usize = 256;
const MAX_TIMERS: usize = 256;
const MAX_QUEUE: usize = 256;
const MAX_REPLAYS: usize = 512;
const MAX_CAS_ATTEMPTS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventContext {
    pub(super) activity: String,
    pub(super) acquired_process: Option<String>,
    pub(super) acquired_activity: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventRecord {
    pub(super) kind: EventKind,
    pub(super) fired: bool,
    pub(super) parent: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum EventKind {
    Input,
    Composite {
        all: bool,
        children: Vec<String>,
        fired_queue: VecDeque<String>,
    },
    Timer {
        timer: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) enum TimerStatus {
    Pending,
    Expired,
    Forced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TimerRecord {
    pub(super) event: String,
    pub(super) due_tick: u64,
    pub(super) status: TimerStatus,
    pub(super) acknowledged: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventReplay {
    pub(super) request_digest: [u8; 32],
    pub(super) condition: String,
    pub(super) response: i32,
    pub(super) response2: i32,
    pub(super) outputs: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActivityState {
    pub(super) events: BTreeMap<String, EventRecord>,
    pub(super) timers: BTreeMap<String, TimerRecord>,
    pub(super) reattach: VecDeque<String>,
    pub(super) replays: BTreeMap<String, EventReplay>,
    #[serde(skip)]
    pub(super) version: u64,
}

impl CicsService {
    /// Bind one registered run to its active BTS activity and any acquired scope.
    ///
    /// The BTS adapter calls this before an event command. The binding is durable
    /// and immutable for the run unit; an unbound run receives `INVREQ` 16/1.
    pub fn bind_event_activity(
        &self,
        run_unit: &RunUnitId,
        activity: &str,
        acquired_process: Option<&str>,
        acquired_activity: Option<&str>,
    ) -> Result<(), HostProblem> {
        let context = EventContext {
            activity: event_name(activity)?,
            acquired_process: acquired_process.map(event_name).transpose()?,
            acquired_activity: acquired_activity.map(event_name).transpose()?,
        };
        if !self.lock()?.runs.contains_key(run_unit) {
            return Err(HostProblem::Unauthorized);
        }
        let key = run_unit.as_str();
        if let Some(existing) = self
            .store
            .get_provider_state(CONTEXT_NAMESPACE, key)
            .map_err(store_error)?
        {
            return if existing.version == 1 && decode_context(&existing.payload)? == context {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let payload = serde_json::to_vec(&context).map_err(|_| HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONTEXT_NAMESPACE.into(),
                    key: key.into(),
                    version: 1,
                    payload,
                },
                None,
            )
            .map_err(store_error)
    }
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::DefineInputEvent => define_input_event(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn define_input_event(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if request.arguments.iter().any(|(name, value)| {
        !matches!(
            name.as_str(),
            "EVENT" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
        ) || if name == "OPTION.NOHANDLE" {
            value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
        } else if name == "EVENT" {
            !matches!(
                value.schema(),
                "mainframe-env.cics.argument@1"
                    | "mainframe-env.cics.literal@1"
                    | "mainframe-env.cics.storage-value@1"
            )
        } else {
            value.schema() != "mainframe-env.cics.argument@1"
        }
    }) {
        return Err(HostProblem::Malformed);
    }
    let name = request
        .arguments
        .get("EVENT")
        .ok_or(HostProblem::Malformed)
        .and_then(|value| std::str::from_utf8(value.bytes()).map_err(|_| event_error(6)))
        .and_then(|value| event_name(value).map_err(|_| event_error(6)))?;
    let context = context(service, run)?;
    service.authorize(
        run,
        "BTSEVENT",
        &format!("CICS.BTS.{}.{}", context.activity, name),
        AccessIntent::Update,
    )?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    for _ in 0..MAX_CAS_ATTEMPTS {
        let mut state = load_activity(service, &context.activity)?;
        if let Some(replay) = state.replays.get(mutation.idempotency_key.as_str()) {
            if replay.request_digest != digest {
                return Err(HostProblem::IdempotencyConflict);
            }
            return event_response(service, run, replay);
        }
        if name == "DFHINITIAL" || state.events.contains_key(&name) {
            return Err(event_error(7));
        }
        if state.events.len() == MAX_EVENTS || state.replays.len() == MAX_REPLAYS {
            return Err(HostProblem::ResourceExhausted);
        }
        let replay = EventReplay {
            request_digest: digest,
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            outputs: BTreeMap::new(),
        };
        let response = event_response(service, run, &replay)?;
        state.events.insert(
            name.clone(),
            EventRecord {
                kind: EventKind::Input,
                fired: false,
                parent: None,
            },
        );
        state
            .replays
            .insert(mutation.idempotency_key.as_str().into(), replay);
        match persist_activity(service, &context.activity, &mut state) {
            Ok(()) => return Ok(response),
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn event_response(
    service: &CicsService,
    run: &Run,
    replay: &EventReplay,
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        &replay.condition,
        replay.response,
        replay.response2,
        None,
        None,
        Vec::new(),
    )?;
    for (name, bytes) in &replay.outputs {
        response
            .outputs
            .insert(name.clone(), super::super::bounded(bytes.clone())?);
    }
    Ok(response)
}

fn event_error(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "EVENTERR".into(),
        response: 111,
        response2,
    }
}

pub(super) fn context(service: &CicsService, run: &Run) -> Result<EventContext, HostProblem> {
    let row = service
        .store
        .get_provider_state(CONTEXT_NAMESPACE, run.invocation.run_unit_id.as_str())
        .map_err(store_error)?
        .ok_or_else(outside_activity)?;
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    decode_context(&row.payload)
}

fn decode_context(payload: &[u8]) -> Result<EventContext, HostProblem> {
    if payload.len() > 256 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let context: EventContext =
        serde_json::from_slice(payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if event_name(&context.activity)? != context.activity
        || context
            .acquired_process
            .as_deref()
            .is_some_and(|name| event_name(name).ok().as_deref() != Some(name))
        || context
            .acquired_activity
            .as_deref()
            .is_some_and(|name| event_name(name).ok().as_deref() != Some(name))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(context)
}

pub(super) fn load_activity(
    service: &CicsService,
    activity: &str,
) -> Result<ActivityState, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(ACTIVITY_NAMESPACE, activity)
        .map_err(store_error)?
    else {
        return Ok(ActivityState::default());
    };
    if row.version == 0 || row.payload.len() > MAX_ACTIVITY_BYTES {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut state: ActivityState =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    state.version = row.version;
    validate_activity(&state)?;
    Ok(state)
}

pub(super) fn persist_activity(
    service: &CicsService,
    activity: &str,
    state: &mut ActivityState,
) -> Result<(), HostProblem> {
    validate_activity(state)?;
    let payload = serde_json::to_vec(state).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_ACTIVITY_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    let version = state
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: ACTIVITY_NAMESPACE.into(),
                key: activity.into(),
                version,
                payload,
            },
            (state.version != 0).then_some(state.version),
        )
        .map_err(|error| match error {
            StoreError::AlreadyExists | StoreError::Conflict => HostProblem::IdempotencyConflict,
            other => store_error(other),
        })?;
    state.version = version;
    Ok(())
}

fn validate_activity(state: &ActivityState) -> Result<(), HostProblem> {
    if state.events.len() > MAX_EVENTS
        || state.timers.len() > MAX_TIMERS
        || state.reattach.len() > MAX_QUEUE
        || state.replays.len() > MAX_REPLAYS
    {
        return Err(HostProblem::ResourceExhausted);
    }
    for (name, event) in &state.events {
        if event_name(name).ok().as_deref() != Some(name)
            || event
                .parent
                .as_deref()
                .is_some_and(|parent| event_name(parent).ok().as_deref() != Some(parent))
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        match &event.kind {
            EventKind::Composite {
                children,
                fired_queue,
                ..
            } if children.len() > MAX_EVENTS || fired_queue.len() > MAX_QUEUE => {
                return Err(HostProblem::ResourceExhausted);
            }
            EventKind::Timer { timer } if !state.timers.contains_key(timer) => {
                return Err(HostProblem::InfrastructureFailure);
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn event_name(name: &str) -> Result<String, HostProblem> {
    let name = name.trim_end_matches(' ');
    if name.is_empty()
        || name.len() > 16
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'@' | b'#' | b'.' | b'-' | b'_')
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name.into())
}

pub(super) fn outside_activity() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 1,
    }
}
