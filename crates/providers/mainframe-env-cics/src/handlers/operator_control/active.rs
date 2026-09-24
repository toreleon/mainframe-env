//! Durable command-position pointer for a suspending WRITE OPERATOR instruction.

use super::authority;
use crate::service::{CicsLimits, store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};

const NAMESPACE: &str = "cics-operator-active-v1";
const SCHEMA: &str = "mainframe-env.cics-operator-active@1";
const MAX_CAS_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ActiveState {
    Waiting,
    Consumed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActiveOperator {
    schema: String,
    pub id: String,
    pub message_key: String,
    pub semantic_digest: [u8; 32],
    state: ActiveState,
    pub consumer_effect_key: Option<String>,
    pub consumer_request_digest: Option<[u8; 32]>,
    pub version: u64,
}

impl ActiveOperator {
    pub(super) fn new(id: &str, message_key: &str, semantic_digest: [u8; 32]) -> Self {
        Self {
            schema: SCHEMA.into(),
            id: id.into(),
            message_key: message_key.into(),
            semantic_digest,
            state: ActiveState::Waiting,
            consumer_effect_key: None,
            consumer_request_digest: None,
            version: 1,
        }
    }

    pub(super) fn waiting(&self) -> bool {
        self.state == ActiveState::Waiting
    }

    pub(super) fn consumed_by(&self, effect_key: &str, digest: [u8; 32]) -> bool {
        self.state == ActiveState::Consumed
            && self.consumer_effect_key.as_deref() == Some(effect_key)
            && self.consumer_request_digest == Some(digest)
    }
}

pub(in crate::service::handlers) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bytes = 0usize;
    for row in rows {
        bytes = bytes
            .checked_add(row.payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let pointer = decode(&row)?;
        let message = authority::read(store, &pointer.message_key)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let run_unit = pointer.id.rsplit_once(':').map(|(run, _)| run);
        if run_unit != Some(message.run_unit.as_str())
            || pointer.waiting() && message.state == authority::MessageState::Complete
            || !pointer.waiting() && message.state == authority::MessageState::Waiting
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

pub(super) fn read(
    store: &dyn ProviderStateStore,
    id: &str,
) -> Result<Option<ActiveOperator>, HostProblem> {
    store
        .get_provider_state(NAMESPACE, id)
        .map_err(store_error)?
        .map(|row| decode(&row))
        .transpose()
}

pub(super) fn pending_for_message(
    store: &dyn ProviderStateStore,
    key: &str,
    limits: CicsLimits,
) -> Result<bool, HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut found = None;
    let mut bytes = 0usize;
    for row in rows {
        bytes = bytes
            .checked_add(row.payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        let pointer = decode(&row)?;
        if pointer.message_key == key && found.replace(pointer.waiting()).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(found.unwrap_or(false))
}

pub(super) fn staged_replace(
    mut next: ActiveOperator,
    current: Option<&ActiveOperator>,
) -> Result<(ActiveOperator, ProviderStateWrite), HostProblem> {
    next.version = current.map_or(1, |record| record.version.saturating_add(1));
    if next.version == 0 {
        return Err(HostProblem::ResourceExhausted);
    }
    let payload = encode(&next)?;
    let write = ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: NAMESPACE.into(),
            key: next.id.clone(),
            version: next.version,
            payload,
        },
        expected_version: current.map(|record| record.version),
    };
    Ok((next, write))
}

pub(super) fn consume(
    store: &dyn ProviderStateStore,
    id: &str,
    message_key: &str,
    effect_key: &str,
    digest: [u8; 32],
) -> Result<(), HostProblem> {
    for _ in 0..MAX_CAS_ATTEMPTS {
        let row = store
            .get_provider_state(NAMESPACE, id)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let mut record = decode(&row)?;
        if record.message_key != message_key {
            return Err(HostProblem::IdempotencyConflict);
        }
        if !record.waiting() {
            return if record.consumed_by(effect_key, digest) {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        record.state = ActiveState::Consumed;
        record.consumer_effect_key = Some(effect_key.into());
        record.consumer_request_digest = Some(digest);
        record.version = record
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let payload = encode(&record)?;
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: id.into(),
                version: record.version,
                payload,
            },
            Some(row.version),
        ) {
            Ok(()) => return Ok(()),
            Err(StoreError::Conflict | StoreError::NotFound) => continue,
            Err(error) => return Err(store_error(error)),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn encode(record: &ActiveOperator) -> Result<Vec<u8>, HostProblem> {
    validate(record)?;
    serde_json::to_vec(record).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(row: &ProviderStateRecord) -> Result<ActiveOperator, HostProblem> {
    if row.namespace != NAMESPACE || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record: ActiveOperator =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    validate(&record)?;
    if record.id != row.key || record.version != row.version || encode(&record)? != row.payload {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}

fn validate(record: &ActiveOperator) -> Result<(), HostProblem> {
    let suffix = record
        .id
        .rsplit_once(':')
        .map(|(run, position)| (!run.is_empty(), position));
    if record.schema != SCHEMA
        || record.id.len() > 256
        || !suffix.is_some_and(|(run, position)| {
            run && !position.is_empty() && position.bytes().all(|byte| byte.is_ascii_digit())
        })
        || !record.message_key.starts_with("cics-operator:")
        || record.message_key.len() != "cics-operator:".len() + 64
        || record.version == 0
        || !matches!(
            (
                &record.state,
                &record.consumer_effect_key,
                &record.consumer_request_digest
            ),
            (ActiveState::Waiting, None, None) | (ActiveState::Consumed, Some(_), Some(_))
        )
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}
