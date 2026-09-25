use super::{CounterKey, normalize_name, normalize_pool};
use crate::service::{CicsLimits, CicsService, mutation_problem, store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const NAMESPACE: &str = "cics-counter-control-v1";
const KEY: &str = "state";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CounterKind {
    Fullword,
    Doubleword,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CounterRecord {
    pub kind: CounterKind,
    pub current: u64,
    pub at_limit: bool,
    pub minimum: u64,
    pub maximum: u64,
}

impl CounterRecord {
    pub fn current_value(&self) -> u64 {
        if self.at_limit {
            self.maximum.wrapping_add(1)
        } else {
            self.current
        }
    }

    pub fn set_current(&mut self, value: u64) {
        self.current = value;
        self.at_limit = value == self.maximum.wrapping_add(1) && value != self.minimum;
    }

    pub fn reach_limit(&mut self) {
        self.current = self.maximum.wrapping_add(1);
        self.at_limit = true;
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CounterReply {
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub value: Option<u64>,
    pub minimum: Option<u64>,
    pub maximum: Option<u64>,
}

impl CounterReply {
    pub fn normal() -> Self {
        Self {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            value: None,
            minimum: None,
            maximum: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CounterReplay {
    pub owner_execution: String,
    pub owner_run_unit: String,
    pub owner_principal: String,
    pub request_digest: [u8; 32],
    pub reply: CounterReply,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CounterState {
    schema_version: u8,
    pub version: u64,
    pub records: BTreeMap<String, CounterRecord>,
    pub replays: BTreeMap<String, CounterReplay>,
    pub rebuilding: BTreeSet<String>,
}

impl Default for CounterState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            version: 0,
            records: BTreeMap::new(),
            replays: BTreeMap::new(),
            rebuilding: BTreeSet::new(),
        }
    }
}

impl CounterState {
    pub fn replay(
        &self,
        key: &str,
        execution: &str,
        run_unit: &str,
        principal: &str,
        digest: [u8; 32],
    ) -> Result<Option<CounterReply>, HostProblem> {
        let Some(saved) = self.replays.get(key) else {
            return Ok(None);
        };
        if saved.request_digest != digest
            || saved.owner_execution != execution
            || saved.owner_run_unit != run_unit
            || saved.owner_principal != principal
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(Some(saved.reply.clone()))
    }

    pub fn record_replay(
        &mut self,
        key: String,
        replay: CounterReplay,
        limits: CicsLimits,
    ) -> Result<(), HostProblem> {
        if self.replays.len() >= limits.max_queue_records || self.replays.contains_key(&key) {
            return Err(HostProblem::ResourceExhausted);
        }
        self.replays.insert(key, replay);
        Ok(())
    }

    fn validate(&self, limits: CicsLimits) -> Result<(), HostProblem> {
        if self.schema_version != 1
            || self.records.len() > limits.max_queue_records
            || self.replays.len() > limits.max_queue_records
            || self.rebuilding.len() > limits.max_queue_records
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        for (key, record) in &self.records {
            let Some((pool, name)) = key.split_once('/') else {
                return Err(HostProblem::InfrastructureFailure);
            };
            if CounterKey::new(pool, name).as_str() != key
                || normalize_pool(pool.as_bytes()).as_deref() != Ok(pool)
                || normalize_name(name.as_bytes()).as_deref() != Ok(name)
                || record.minimum > record.maximum
                || (!record.at_limit
                    && (record.current < record.minimum || record.current > record.maximum))
                || (record.at_limit && record.current != record.maximum.wrapping_add(1))
                || record.kind == CounterKind::Fullword && record.maximum > i32::MAX as u64
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        for pool in &self.rebuilding {
            if normalize_pool(pool.as_bytes()).as_deref() != Ok(pool.as_str()) {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        for (key, replay) in &self.replays {
            if key.is_empty()
                || key.len() > 256
                || replay.owner_execution.is_empty()
                || replay.owner_execution.len() > 256
                || replay.owner_run_unit.is_empty()
                || replay.owner_run_unit.len() > 256
                || replay.owner_principal.is_empty()
                || replay.owner_principal.len() > 256
                || !matches!(replay.reply.condition.as_str(), "NORMAL" | "LENGERR")
                || replay.reply.response != 0 && replay.reply.response != 22
                || replay.reply.response == 0 && replay.reply.response2 != 0
                || replay.reply.response == 22 && !(1..=3).contains(&replay.reply.response2)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
        }
        Ok(())
    }

    fn encode(&self, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
        self.validate(limits)?;
        let payload = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if payload.len() > limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(payload)
    }
}

pub(super) fn load(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<CounterState, HostProblem> {
    let Some(row) = store
        .get_provider_state(NAMESPACE, KEY)
        .map_err(store_error)?
    else {
        return Ok(CounterState::default());
    };
    if row.payload.len() > limits.max_queue_bytes || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let state: CounterState =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if state.version != row.version || state.encode(limits)? != row.payload {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(state)
}

pub(super) fn persist(
    service: &CicsService,
    current: &CounterState,
    next: &mut CounterState,
) -> Result<bool, HostProblem> {
    next.version = current
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let payload = next.encode(service.limits)?;
    match service.store.put_provider_state(
        ProviderStateRecord {
            namespace: NAMESPACE.into(),
            key: KEY.into(),
            version: next.version,
            payload,
        },
        (current.version != 0).then_some(current.version),
    ) {
        Ok(()) => Ok(true),
        Err(StoreError::Conflict) => Ok(false),
        Err(error) => Err(mutation_problem(store_error(error))),
    }
}
