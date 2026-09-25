//! Exact task-owned cursor transitions backed by the shared provider store.

use super::{BrowseBook, BrowseItem, BrowseKind, BrowseScope};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const NAMESPACE: &str = "cics-bts-browse-v1";
const SCHEMA: &str = "mainframe-env.cics.bts-browse@1";
const MAX_ROW_BYTES: usize = 4_194_304;
const MAX_REPLAYS: usize = 512;
const MAX_CAS_ATTEMPTS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseOwner {
    pub run_unit: String,
    pub execution: String,
    pub principal: String,
}

impl BrowseOwner {
    pub fn new(run_unit: &str, execution: &str, principal: &str) -> Result<Self, HostProblem> {
        if [run_unit, execution, principal]
            .iter()
            .any(|part| part.is_empty() || part.len() > 256)
        {
            return Err(HostProblem::Malformed);
        }
        Ok(Self {
            run_unit: run_unit.into(),
            execution: execution.into(),
            principal: principal.into(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowseEffect {
    Start {
        scope: BrowseScope,
        items: Vec<BrowseItem>,
    },
    Next {
        token: u32,
        kind: BrowseKind,
        live_epoch: u64,
        expected: BrowseItem,
    },
    End {
        token: u32,
        kind: BrowseKind,
    },
    /// Task end or rollback leaves a durable tombstone to fence late retries.
    Close,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BrowseOutcome {
    Token(u32),
    Item(BrowseItem),
    Ended,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Replay {
    digest: [u8; 32],
    generation: u64,
    outcome: BrowseOutcome,
    scope: Option<BrowseScope>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: String,
    run_unit: String,
    execution: String,
    principal: String,
    closed: bool,
    generation: u64,
    book: BrowseBook,
    replays: BTreeMap<String, Replay>,
}

impl State {
    fn new(owner: &BrowseOwner) -> Self {
        Self {
            schema_version: SCHEMA.into(),
            run_unit: owner.run_unit.clone(),
            execution: owner.execution.clone(),
            principal: owner.principal.clone(),
            closed: false,
            generation: 1,
            book: BrowseBook::default(),
            replays: BTreeMap::new(),
        }
    }

    fn validate(&self, owner: &BrowseOwner) -> Result<(), HostProblem> {
        if self.schema_version != SCHEMA
            || self.run_unit != owner.run_unit
            || self.execution.is_empty()
            || self.principal.is_empty()
            || self.generation == 0
            || self.replays.len() > MAX_REPLAYS
            || self.replays.values().any(|replay| {
                replay.generation == 0
                    || replay.generation > self.generation
                    || replay
                        .scope
                        .as_ref()
                        .is_some_and(|scope| scope.validate().is_err())
            })
            || self
                .replays
                .keys()
                .any(|key| key.is_empty() || key.len() > 256)
            || self.closed && !self.book.cursors.is_empty()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if self.execution != owner.execution || self.principal != owner.principal {
            return Err(HostProblem::Unauthorized);
        }
        self.book.validate()
    }

    fn transition(&mut self, effect: &BrowseEffect) -> Result<BrowseOutcome, HostProblem> {
        if self.closed {
            return Err(HostProblem::NotFound);
        }
        match effect {
            BrowseEffect::Start { scope, items } => self
                .book
                .start(scope.clone(), items.clone())
                .map(BrowseOutcome::Token),
            BrowseEffect::Next {
                token,
                kind,
                live_epoch,
                expected,
            } => self
                .book
                .next(*token, *kind, *live_epoch, expected)
                .map(BrowseOutcome::Item),
            BrowseEffect::End { token, kind } => {
                self.book.end(*token, *kind)?;
                Ok(BrowseOutcome::Ended)
            }
            BrowseEffect::Close => {
                self.book.clear();
                self.closed = true;
                Ok(BrowseOutcome::Closed)
            }
        }
    }
}

pub struct BtsBrowseStore<'a> {
    store: &'a dyn ProviderStateStore,
}

impl<'a> BtsBrowseStore<'a> {
    pub fn new(store: &'a dyn ProviderStateStore) -> Self {
        Self { store }
    }

    pub fn peek(
        &self,
        owner: &BrowseOwner,
        token: u32,
        kind: BrowseKind,
    ) -> Result<BrowseItem, HostProblem> {
        let state = self.load_state(owner)?;
        if state.closed {
            return Err(HostProblem::NotFound);
        }
        state.book.peek(token, kind)
    }

    pub fn scope(
        &self,
        owner: &BrowseOwner,
        token: u32,
        kind: BrowseKind,
    ) -> Result<BrowseScope, HostProblem> {
        let state = self.load_state(owner)?;
        if state.closed {
            return Err(HostProblem::NotFound);
        }
        state.book.scope(token, kind)
    }

    fn load_state(&self, owner: &BrowseOwner) -> Result<State, HostProblem> {
        BrowseOwner::new(&owner.run_unit, &owner.execution, &owner.principal)?;
        let row = self
            .store
            .get_provider_state(NAMESPACE, &owner.run_unit)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
            return Err(HostProblem::InfrastructureFailure);
        }
        let state: State =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        state.validate(owner)?;
        Ok(state)
    }

    pub fn replay(
        &self,
        owner: &BrowseOwner,
        effect_key: &str,
        digest: [u8; 32],
    ) -> Result<Option<(BrowseOutcome, BrowseScope)>, HostProblem> {
        if effect_key.is_empty() || effect_key.len() > 256 {
            return Err(HostProblem::Malformed);
        }
        let state = match self.load_state(owner) {
            Ok(state) => state,
            Err(HostProblem::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let Some(replay) = state.replays.get(effect_key) else {
            return Ok(None);
        };
        if state.closed || replay.generation != state.generation {
            return Err(HostProblem::NotFound);
        }
        if replay.digest != digest {
            return Err(HostProblem::IdempotencyConflict);
        }
        let scope = replay
            .scope
            .clone()
            .ok_or(HostProblem::InfrastructureFailure)?;
        Ok(Some((replay.outcome.clone(), scope)))
    }

    /// Remove the task's cursors after rollback, or close its book at task end.
    /// An absent book needs no tombstone.
    pub fn clear_existing(&self, owner: &BrowseOwner, close: bool) -> Result<(), HostProblem> {
        BrowseOwner::new(&owner.run_unit, &owner.execution, &owner.principal)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(row) = self
                .store
                .get_provider_state(NAMESPACE, &owner.run_unit)
                .map_err(store_error)?
            else {
                return Ok(());
            };
            if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
                return Err(HostProblem::InfrastructureFailure);
            }
            let mut state: State = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            state.validate(owner)?;
            if state.closed {
                return Ok(());
            }
            state.book.clear();
            state.generation = state
                .generation
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            state.closed = close;
            let payload =
                serde_json::to_vec(&state).map_err(|_| HostProblem::InfrastructureFailure)?;
            if payload.len() > MAX_ROW_BYTES {
                return Err(HostProblem::ResourceExhausted);
            }
            let version = row
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: owner.run_unit.clone(),
                    version,
                    payload,
                },
                Some(row.version),
            ) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    pub fn apply(
        &self,
        owner: &BrowseOwner,
        effect_key: &str,
        digest: [u8; 32],
        effect: &BrowseEffect,
    ) -> Result<BrowseOutcome, HostProblem> {
        BrowseOwner::new(&owner.run_unit, &owner.execution, &owner.principal)?;
        if effect_key.is_empty() || effect_key.len() > 256 {
            return Err(HostProblem::Malformed);
        }
        for _ in 0..MAX_CAS_ATTEMPTS {
            let prior = self
                .store
                .get_provider_state(NAMESPACE, &owner.run_unit)
                .map_err(store_error)?;
            let (mut state, expected) = match prior {
                Some(row) => {
                    if row.version == 0 || row.payload.len() > MAX_ROW_BYTES {
                        return Err(HostProblem::InfrastructureFailure);
                    }
                    let state: State = serde_json::from_slice(&row.payload)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    state.validate(owner)?;
                    (state, Some(row.version))
                }
                None => (State::new(owner), None),
            };
            if let Some(replay) = state.replays.get(effect_key) {
                return if state.closed || replay.generation != state.generation {
                    Err(HostProblem::NotFound)
                } else if replay.digest == digest {
                    Ok(replay.outcome.clone())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                };
            }
            if state.replays.len() >= MAX_REPLAYS
                || state.replays.len() == MAX_REPLAYS - 1 && !matches!(effect, BrowseEffect::Close)
            {
                return Err(HostProblem::ResourceExhausted);
            }
            let scope = match effect {
                BrowseEffect::Start { scope, .. } => Some(scope.clone()),
                BrowseEffect::Next { token, kind, .. } | BrowseEffect::End { token, kind } => {
                    Some(state.book.scope(*token, *kind)?)
                }
                BrowseEffect::Close => None,
            };
            let outcome = state.transition(effect)?;
            state.replays.insert(
                effect_key.into(),
                Replay {
                    digest,
                    generation: state.generation,
                    outcome: outcome.clone(),
                    scope,
                },
            );
            let payload =
                serde_json::to_vec(&state).map_err(|_| HostProblem::InfrastructureFailure)?;
            if payload.len() > MAX_ROW_BYTES {
                return Err(HostProblem::ResourceExhausted);
            }
            let version = expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let row = ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: owner.run_unit.clone(),
                version,
                payload,
            };
            match self.store.put_provider_state(row, expected) {
                Ok(()) => return Ok(outcome),
                Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }
}

fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        StoreError::Conflict | StoreError::AlreadyExists => HostProblem::IdempotencyConflict,
        _ => HostProblem::InfrastructureFailure,
    }
}
