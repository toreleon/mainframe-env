//! Durable child-token ownership for the BTS FETCH/FREE slice.

use super::super::{CicsService, Run, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits, RunUnitId};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const NAMESPACE: &str = "cics-bts-child-ownership-v1";
const MAX_CHILDREN: usize = 256;
const MAX_REPLAYS: usize = 512;
const MAX_BYTES: usize = 1_048_576;
const CAS_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CicsBtsChildCompletion {
    /// Child task completed normally.
    Normal,
    /// Child task ended with an abend code.
    Abend,
    /// Child task could not attach because of security denial.
    SecurityError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Child {
    token: [u8; 16],
    reply_channel: Option<String>,
    completion: Option<CicsBtsChildCompletion>,
    abcode: Option<String>,
    fetched: bool,
    #[serde(default)]
    channel_fetched: bool,
    freed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replay {
    run_unit: String,
    execution: String,
    request_digest: [u8; 32],
    #[serde(default = "normal_condition")]
    condition: String,
    #[serde(default)]
    response: i32,
    #[serde(default)]
    response2: i32,
    outputs: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    children: BTreeMap<String, Child>,
    replays: BTreeMap<String, Replay>,
    #[serde(default)]
    waits: BTreeMap<String, u64>,
    #[serde(skip)]
    version: u64,
}

impl CicsService {
    /// Register a child token issued by the common BTS RUN TRANSID authority.
    pub fn register_bts_child(
        &self,
        parent: &RunUnitId,
        token: [u8; 16],
        reply_channel: Option<&str>,
    ) -> Result<(), HostProblem> {
        if !self.lock()?.runs.contains_key(parent) || token == [0; 16] {
            return Err(HostProblem::Unauthorized);
        }
        let reply_channel = reply_channel.map(channel_name).transpose()?;
        change(self, parent.as_str(), |state| {
            let key = token_key(&token);
            if state.children.contains_key(&key) {
                return Err(HostProblem::IdempotencyConflict);
            }
            if state.children.len() == MAX_CHILDREN {
                return Err(HostProblem::ResourceExhausted);
            }
            state.children.insert(
                key,
                Child {
                    token,
                    reply_channel: reply_channel.clone(),
                    completion: None,
                    abcode: None,
                    fetched: false,
                    channel_fetched: false,
                    freed: false,
                },
            );
            Ok(())
        })
    }

    /// The provider holds this authenticated run outside the live-run map
    /// while dispatching RUN TRANSID. It writes the same child-ownership row.
    pub(in crate::service) fn register_bts_child_inflight(
        &self,
        run: &Run,
        token: [u8; 16],
        reply_channel: Option<&str>,
    ) -> Result<(), HostProblem> {
        if token == [0; 16] {
            return Err(HostProblem::Unauthorized);
        }
        let reply_channel = reply_channel.map(channel_name).transpose()?;
        change(self, run.invocation.run_unit_id.as_str(), |state| {
            let key = token_key(&token);
            if state.children.contains_key(&key) {
                return Err(HostProblem::IdempotencyConflict);
            }
            if state.children.len() == MAX_CHILDREN {
                return Err(HostProblem::ResourceExhausted);
            }
            state.children.insert(
                key,
                Child {
                    token,
                    reply_channel: reply_channel.clone(),
                    completion: None,
                    abcode: None,
                    fetched: false,
                    channel_fetched: false,
                    freed: false,
                },
            );
            Ok(())
        })
    }

    /// Record terminal child outcome from the shared BTS lifecycle authority.
    pub fn complete_bts_child(
        &self,
        parent: &RunUnitId,
        token: [u8; 16],
        completion: CicsBtsChildCompletion,
        abcode: Option<&str>,
    ) -> Result<(), HostProblem> {
        let abcode = match completion {
            CicsBtsChildCompletion::Abend => {
                Some(valid_abcode(abcode.ok_or(HostProblem::Malformed)?)?)
            }
            _ if abcode.is_none() => None,
            _ => return Err(HostProblem::Malformed),
        };
        change(self, parent.as_str(), |state| {
            let child = state
                .children
                .get_mut(&token_key(&token))
                .ok_or_else(invalid_child)?;
            if child.completion.is_some() {
                return if child.completion == Some(completion) && child.abcode == abcode {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            child.completion = Some(completion);
            child.abcode = abcode.clone();
            if child.freed {
                child.reply_channel = None;
            }
            Ok(())
        })
    }
    /// Reconcile a crash after registration using the sibling row's exact token.
    #[allow(dead_code)] // Consumed by the RUN TRANSID outbox route in the next feature.
    pub(crate) fn bts_child_registered(
        &self,
        parent: &RunUnitId,
        token: [u8; 16],
        reply_channel: Option<&str>,
    ) -> Result<bool, HostProblem> {
        let reply_channel = reply_channel.map(channel_name).transpose()?;
        Ok(load(self, parent.as_str())?
            .children
            .get(&token_key(&token))
            .is_some_and(|child| {
                child.reply_channel == reply_channel || child.freed && child.reply_channel.is_none()
            }))
    }

    /// Read the sibling token outcome for crash-gap reconciliation.
    pub(in crate::service) fn bts_child_outcome(
        &self,
        parent: &RunUnitId,
        token: [u8; 16],
    ) -> Result<Option<(CicsBtsChildCompletion, Option<String>)>, HostProblem> {
        let state = load(self, parent.as_str())?;
        let child = state
            .children
            .get(&token_key(&token))
            .ok_or(HostProblem::NotFound)?;
        Ok(child
            .completion
            .map(|completion| (completion, child.abcode.clone())))
    }
}

pub(super) fn invoke_child(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    super::bts_live(service, run, retention_tick)?;
    let parent = run.invocation.run_unit_id.as_str().to_string();
    let token = if request.operation == CicsOperation::FetchAny {
        None
    } else {
        let bytes = request
            .arguments
            .get("CHILD")
            .ok_or(HostProblem::Malformed)?
            .bytes();
        Some(<[u8; 16]>::try_from(bytes).map_err(|_| invalid_child())?)
    };
    let timeout = if let Some(value) = request.arguments.get("TIMEOUT") {
        let number = std::str::from_utf8(value.bytes())
            .ok()
            .and_then(|text| text.parse::<i64>().ok())
            .ok_or_else(invalid_timeout)?;
        let maximum = if request.operation == CicsOperation::FetchAny {
            40_800_000
        } else {
            4_080_000
        };
        if !(0..=maximum).contains(&number) {
            return Err(invalid_timeout());
        }
        number as u64
    } else {
        0
    };
    let wait_id = request
        .arguments
        .get("FETCH.ID")
        .map(|value| {
            if value.schema() != "mainframe-env.cics.fetch-id@1" {
                return Err(HostProblem::Malformed);
            }
            let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
            let suffix = text
                .strip_prefix(&format!("{parent}:"))
                .ok_or(HostProblem::Malformed)?;
            if suffix.is_empty()
                || !suffix.bytes().all(|byte| byte.is_ascii_digit())
                || text.len() > 256
            {
                return Err(HostProblem::Malformed);
            }
            Ok(text.to_string())
        })
        .transpose()?;
    if timeout > 0 && wait_id.is_none() {
        return Err(HostProblem::Malformed);
    }
    let now_tick = if timeout > 0 {
        super::event_control::clock_millis(service, run)?
    } else {
        0
    };
    service.authorize(
        run,
        "BTSCHILD",
        &format!("CICS.BTS.CHILD.{:x}", Sha256::digest(parent.as_bytes())),
        AccessIntent::Update,
    )?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    for _ in 0..CAS_ATTEMPTS {
        let mut state = load(service, &parent)?;
        if let Some(replay) = state.replays.get(mutation.idempotency_key.as_str()) {
            if replay.request_digest != digest
                || replay.run_unit != parent
                || replay.execution != run.invocation.execution_id.as_str()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            return if replay.response == 0 {
                respond(service, run, &replay.outputs)
            } else {
                Err(condition(
                    &replay.condition,
                    replay.response,
                    replay.response2,
                ))
            };
        }
        if state.replays.len() == MAX_REPLAYS {
            return Err(HostProblem::ResourceExhausted);
        }
        let selected = match request.operation {
            CicsOperation::FetchAny => {
                if state.children.is_empty() {
                    return Err(invreq(52));
                }
                let mut eligible = state
                    .children
                    .iter()
                    .filter(|(_, child)| !child.freed && !child.fetched);
                let first = eligible
                    .next()
                    .map(|(key, _)| key.clone())
                    .ok_or_else(|| condition("NOTFND", 13, 1))?;
                state
                    .children
                    .iter()
                    .find(|(_, child)| !child.freed && !child.fetched && child.completion.is_some())
                    .map(|(key, _)| key.clone())
                    .or(Some(first))
            }
            CicsOperation::FetchChild | CicsOperation::FreeChild => {
                let key = token_key(&token.expect("validated child token"));
                if !state.children.get(&key).is_some_and(|child| !child.freed) {
                    return Err(invalid_child());
                }
                Some(key)
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        let key = selected.expect("validated selection");
        if request.operation != CicsOperation::FreeChild {
            let pending = state
                .children
                .get(&key)
                .is_some_and(|child| child.completion.is_none());
            if pending {
                if request.arguments.contains_key("OPTION.NOSUSPEND") {
                    return Err(condition("NOTFINISHED", 113, 52));
                }
                if timeout > 0 {
                    let wait_key = wait_id.clone().expect("validated timeout identity");
                    let started = *state.waits.entry(wait_key).or_insert(now_tick);
                    if now_tick.saturating_sub(started) >= timeout {
                        state
                            .waits
                            .remove(wait_id.as_deref().expect("validated timeout identity"));
                        state.replays.insert(
                            mutation.idempotency_key.as_str().into(),
                            Replay {
                                run_unit: parent.clone(),
                                execution: run.invocation.execution_id.as_str().into(),
                                request_digest: digest,
                                condition: "NOTFINISHED".into(),
                                response: 113,
                                response2: 53,
                                outputs: BTreeMap::new(),
                            },
                        );
                        match persist(service, &parent, &mut state) {
                            Ok(()) => return Err(condition("NOTFINISHED", 113, 53)),
                            Err(HostProblem::IdempotencyConflict) => continue,
                            Err(problem) => return Err(problem),
                        }
                    }
                    match persist(service, &parent, &mut state) {
                        Ok(()) => {}
                        Err(HostProblem::IdempotencyConflict) => continue,
                        Err(problem) => return Err(problem),
                    }
                }
                return service.response(
                    run,
                    CicsDisposition::Suspended,
                    "NORMAL",
                    0,
                    0,
                    None,
                    None,
                    Vec::new(),
                );
            }
        }
        let child = state.children.get_mut(&key).expect("selected child");
        let mut outputs = BTreeMap::new();
        if request.operation == CicsOperation::FreeChild {
            child.freed = true;
            if child.completion.is_some() {
                child.reply_channel = None;
            }
        } else {
            if request.operation == CicsOperation::FetchChild
                && request.arguments.contains_key("CHANNEL")
                && child.channel_fetched
            {
                return Err(invreq(51));
            }
            let completion = child.completion.expect("completed child");
            child.fetched = true;
            if request.arguments.contains_key("CHANNEL") {
                child.channel_fetched = true;
            }
            if request.operation == CicsOperation::FetchAny {
                outputs.insert("ANY".into(), child.token.to_vec());
            }
            outputs.insert(
                "COMPSTATUS".into(),
                match completion {
                    CicsBtsChildCompletion::Normal => b"NORMAL".to_vec(),
                    CicsBtsChildCompletion::Abend => b"ABEND".to_vec(),
                    CicsBtsChildCompletion::SecurityError => b"SECERROR".to_vec(),
                },
            );
            if request.arguments.contains_key("CHANNEL") {
                outputs.insert(
                    "CHANNEL".into(),
                    format!("{:<16}", child.reply_channel.as_deref().unwrap_or("")).into_bytes(),
                );
            }
            if request.arguments.contains_key("ABCODE") {
                outputs.insert(
                    "ABCODE".into(),
                    format!("{:<4}", child.abcode.as_deref().unwrap_or("")).into_bytes(),
                );
            }
        }
        let response = respond(service, run, &outputs)?;
        state.replays.insert(
            mutation.idempotency_key.as_str().into(),
            Replay {
                run_unit: parent.clone(),
                execution: run.invocation.execution_id.as_str().into(),
                request_digest: digest,
                condition: "NORMAL".into(),
                response: 0,
                response2: 0,
                outputs,
            },
        );
        if let Some(id) = &wait_id {
            state.waits.remove(id);
        }
        match persist(service, &parent, &mut state) {
            Ok(()) => return Ok(response),
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if super::selected_link_return(run) {
        return Ok(());
    }
    let parent = run.invocation.run_unit_id.as_str();
    if service
        .store
        .get_provider_state(NAMESPACE, parent)
        .map_err(store_error)?
        .is_none()
    {
        return Ok(());
    }
    change(service, parent, |state| {
        for child in state.children.values_mut() {
            child.freed = true;
            child.reply_channel = None;
        }
        state.waits.clear();
        Ok(())
    })
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let is_fetch = request.operation != CicsOperation::FreeChild;
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "CHILD" if request.operation != CicsOperation::FetchAny => {
                matches!(
                    value.schema(),
                    "mainframe-env.cics.argument@1"
                        | "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                ) && value.bytes().len() == 16
            }
            "ANY" if request.operation == CicsOperation::FetchAny => {
                value.schema() == "mainframe-env.cics.argument@1"
            }
            "COMPSTATUS" | "CHANNEL" | "ABCODE" if is_fetch => {
                value.schema() == "mainframe-env.cics.argument@1"
            }
            "TIMEOUT" if is_fetch => value.schema() == "mainframe-env.cics.decimal@1",
            "FETCH.ID" if is_fetch => value.schema() == "mainframe-env.cics.fetch-id@1",
            "OPTION.NOSUSPEND" if is_fetch => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    if is_fetch
        && (!request.arguments.contains_key("COMPSTATUS")
            || request.operation == CicsOperation::FetchAny
                && !request.arguments.contains_key("ANY")
            || request.arguments.contains_key("TIMEOUT")
                && request.arguments.contains_key("OPTION.NOSUSPEND"))
        || request.operation != CicsOperation::FetchAny && !request.arguments.contains_key("CHILD")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn change(
    service: &CicsService,
    parent: &str,
    mut transition: impl FnMut(&mut State) -> Result<(), HostProblem>,
) -> Result<(), HostProblem> {
    for _ in 0..CAS_ATTEMPTS {
        let mut state = load(service, parent)?;
        transition(&mut state)?;
        match persist(service, parent, &mut state) {
            Ok(()) => return Ok(()),
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn load(service: &CicsService, parent: &str) -> Result<State, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(NAMESPACE, parent)
        .map_err(store_error)?
    else {
        return Ok(State::default());
    };
    if row.version == 0 || row.payload.len() > MAX_BYTES {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut state: State =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if state.children.len() > MAX_CHILDREN
        || state.replays.len() > MAX_REPLAYS
        || state.waits.len() > MAX_REPLAYS
        || state
            .children
            .iter()
            .any(|(key, child)| key != &token_key(&child.token))
        || state
            .replays
            .values()
            .any(|replay| replay.run_unit != parent)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    state.version = row.version;
    Ok(state)
}

fn persist(service: &CicsService, parent: &str, state: &mut State) -> Result<(), HostProblem> {
    let payload = serde_json::to_vec(state).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > MAX_BYTES {
        return Err(HostProblem::ResourceExhausted);
    }
    let next = state
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: parent.into(),
                version: next,
                payload,
            },
            (state.version != 0).then_some(state.version),
        )
        .map_err(|error| match error {
            StoreError::AlreadyExists | StoreError::Conflict => HostProblem::IdempotencyConflict,
            other => store_error(other),
        })?;
    state.version = next;
    Ok(())
}

fn respond(
    service: &CicsService,
    run: &Run,
    outputs: &BTreeMap<String, Vec<u8>>,
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    for (name, bytes) in outputs {
        response.outputs.insert(
            name.clone(),
            BoundedPayload::new(
                if name == "COMPSTATUS" {
                    "mainframe-env.cics.cvda@1"
                } else {
                    "mainframe-env.cics.payload@1"
                },
                bytes.clone(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
}

fn token_key(token: &[u8; 16]) -> String {
    token.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn normal_condition() -> String {
    "NORMAL".into()
}
fn channel_name(value: &str) -> Result<String, HostProblem> {
    let value = value.trim_end_matches(' ');
    if value.is_empty()
        || value.chars().count() > 16
        || value.chars().any(|character| {
            !(character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '$' | '@'
                        | '#'
                        | '.'
                        | '/'
                        | '-'
                        | '_'
                        | '%'
                        | '&'
                        | '?'
                        | '!'
                        | ':'
                        | '|'
                        | '"'
                        | '='
                        | '¬'
                        | ','
                        | ';'
                        | '<'
                        | '>'
                ))
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(value.into())
}

fn valid_abcode(value: &str) -> Result<String, HostProblem> {
    if value.len() != 4 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(HostProblem::Malformed);
    }
    Ok(value.into())
}
fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
fn invalid_child() -> HostProblem {
    condition("INVREQ", 16, 50)
}
fn invalid_timeout() -> HostProblem {
    condition("INVREQ", 16, 241)
}
fn invreq(response2: i32) -> HostProblem {
    condition("INVREQ", 16, response2)
}
