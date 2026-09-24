//! RUN TRANSID child-token port, byte-compatible with the sibling FETCH/FREE row.

use super::super::{CicsService, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const NAMESPACE: &str = "cics-bts-child-ownership-v1";
const MAX_CHILDREN: usize = 256;
const MAX_REPLAYS: usize = 512;
const MAX_BYTES: usize = 1_048_576;
const CAS_ATTEMPTS: usize = 8;

/// Terminal child status shared with the sibling FETCH/FREE contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CicsBtsChildCompletion {
    Normal,
    Abend,
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
    /// Register the 16-byte token issued by RUN TRANSID for its live parent.
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

    /// Retain a terminal child outcome for FETCH CHILD or FETCH ANY.
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
                .ok_or(HostProblem::NotFound)?;
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
            .is_some_and(|child| child.reply_channel == reply_channel))
    }
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
    Err(HostProblem::UnknownOutcome)
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
