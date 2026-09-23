//! Strict, bounded message and reply records shared by CICS dispatch and the console gateway.

use crate::service::{CicsLimits, store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const NAMESPACE: &str = "cics-operator-v1";
const SCHEMA: &str = "mainframe-env.cics-operator@1";
const MAX_CAS_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(in crate::service::handlers) enum MessageState {
    Complete,
    Waiting,
    Replied,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::service::handlers) struct OperatorMessage {
    pub schema: String,
    pub key: String,
    pub execution: String,
    pub run_unit: String,
    pub principal: String,
    pub text: Vec<u8>,
    pub console: Option<String>,
    pub routes: Vec<u8>,
    pub action: Option<u8>,
    pub maximum_reply: Option<usize>,
    pub deadline_tick: Option<u64>,
    pub reply: Option<Vec<u8>>,
    pub state: MessageState,
    pub effect_key: String,
    pub request_digest: [u8; 32],
    pub version: u64,
}

impl OperatorMessage {
    pub(super) fn new(
        execution: &str,
        run_unit: &str,
        principal: &str,
        text: Vec<u8>,
        console: Option<String>,
        routes: Vec<u8>,
        action: Option<u8>,
        maximum_reply: Option<usize>,
        deadline_tick: Option<u64>,
        effect_key: &str,
        request_digest: [u8; 32],
    ) -> Result<Self, HostProblem> {
        let hash = Sha256::digest([effect_key.as_bytes(), &request_digest].concat());
        let record = Self {
            schema: SCHEMA.into(),
            key: format!("cics-operator:{hash:x}"),
            execution: execution.into(),
            run_unit: run_unit.into(),
            principal: principal.into(),
            text,
            console,
            routes,
            action,
            maximum_reply,
            deadline_tick,
            reply: None,
            state: if maximum_reply.is_some() {
                MessageState::Waiting
            } else {
                MessageState::Complete
            },
            effect_key: effect_key.into(),
            request_digest,
            version: 1,
        };
        validate(&record)?;
        Ok(record)
    }
}

pub(super) fn put(
    store: &dyn ProviderStateStore,
    record: &OperatorMessage,
) -> Result<(), HostProblem> {
    let payload = encode(record)?;
    match store.put_provider_state(
        ProviderStateRecord {
            namespace: NAMESPACE.into(),
            key: record.key.clone(),
            version: 1,
            payload,
        },
        None,
    ) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict) => {
            let existing = read(store, &record.key)?.ok_or(HostProblem::InfrastructureFailure)?;
            if existing.effect_key == record.effect_key
                && existing.request_digest == record.request_digest
                && existing.execution == record.execution
                && existing.run_unit == record.run_unit
            {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            }
        }
        Err(error) => Err(store_error(error)),
    }
}

pub(super) fn read(
    store: &dyn ProviderStateStore,
    key: &str,
) -> Result<Option<OperatorMessage>, HostProblem> {
    store
        .get_provider_state(NAMESPACE, key)
        .map_err(store_error)?
        .map(|row| decode(&row))
        .transpose()
}

pub(in crate::service::handlers) fn list(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<Vec<OperatorMessage>, HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut bytes = 0usize;
    rows.into_iter()
        .map(|row| {
            bytes = bytes
                .checked_add(row.payload.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            if bytes > limits.max_queue_bytes {
                return Err(HostProblem::ResourceExhausted);
            }
            decode(&row)
        })
        .collect()
}

pub(super) fn answer(
    store: &dyn ProviderStateStore,
    key: &str,
    reply: &[u8],
    now_tick: u64,
) -> Result<OperatorMessage, HostProblem> {
    for _ in 0..MAX_CAS_ATTEMPTS {
        let row = store
            .get_provider_state(NAMESPACE, key)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let mut record = decode(&row)?;
        if record.state == MessageState::Replied {
            return if record.reply.as_deref() == Some(reply) {
                Ok(record)
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        if record.state != MessageState::Waiting {
            return Err(HostProblem::NotFound);
        }
        if now_tick == 0
            || record
                .deadline_tick
                .is_some_and(|deadline| now_tick >= deadline)
        {
            return Err(HostProblem::TimedOut);
        }
        if reply.len() > 119 {
            return Err(HostProblem::ResourceExhausted);
        }
        record.reply = Some(reply.to_vec());
        record.state = MessageState::Replied;
        record.version = record
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let payload = encode(&record)?;
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: key.into(),
                version: record.version,
                payload,
            },
            Some(row.version),
        ) {
            Ok(()) => return Ok(record),
            Err(StoreError::Conflict | StoreError::NotFound) => continue,
            Err(error) => return Err(store_error(error)),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

pub(super) fn expire(
    store: &dyn ProviderStateStore,
    key: &str,
    now_tick: u64,
) -> Result<OperatorMessage, HostProblem> {
    if now_tick == 0 {
        return Err(HostProblem::Malformed);
    }
    for _ in 0..MAX_CAS_ATTEMPTS {
        let row = store
            .get_provider_state(NAMESPACE, key)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let mut record = decode(&row)?;
        if record.state != MessageState::Waiting {
            return Ok(record);
        }
        if record
            .deadline_tick
            .is_none_or(|deadline| now_tick < deadline)
        {
            return Ok(record);
        }
        record.state = MessageState::Expired;
        record.version = record
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let payload = encode(&record)?;
        match store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: key.into(),
                version: record.version,
                payload,
            },
            Some(row.version),
        ) {
            Ok(()) => return Ok(record),
            Err(StoreError::Conflict | StoreError::NotFound) => continue,
            Err(error) => return Err(store_error(error)),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn encode(record: &OperatorMessage) -> Result<Vec<u8>, HostProblem> {
    validate(record)?;
    serde_json::to_vec(record).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(row: &ProviderStateRecord) -> Result<OperatorMessage, HostProblem> {
    if row.namespace != NAMESPACE || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record: OperatorMessage =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    validate(&record)?;
    if row.key != record.key || row.version != record.version || encode(&record)? != row.payload {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}

fn validate(record: &OperatorMessage) -> Result<(), HostProblem> {
    let expected_key = format!(
        "cics-operator:{:x}",
        Sha256::digest([record.effect_key.as_bytes(), &record.request_digest].concat())
    );
    if record.schema != SCHEMA
        || record.key != expected_key
        || record.execution.is_empty()
        || record.run_unit.is_empty()
        || record.principal.is_empty()
        || record.text.len() > 690
        || record.maximum_reply.is_some() && record.text.len() > 121
        || record.console.as_ref().is_some_and(|name| {
            !(2..=8).contains(&name.len())
                || !name.bytes().all(|byte| {
                    byte.is_ascii_uppercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'@' | b'#' | b'$')
                })
        })
        || !record.routes.is_empty() && record.console.is_some()
        || record.routes.len() > 28
        || record.routes.iter().any(|code| !(1..=28).contains(code))
        || record
            .action
            .is_some_and(|code| !matches!(code, 2 | 3 | 11))
        || record
            .maximum_reply
            .is_some_and(|length| !(1..=119).contains(&length))
        || record.maximum_reply.is_some() != record.deadline_tick.is_some()
        || record.deadline_tick == Some(0)
        || record.effect_key.is_empty()
        || record.version == 0
        || !matches!(
            (record.state, record.maximum_reply, record.reply.as_ref()),
            (MessageState::Complete, None, None)
                | (MessageState::Waiting, Some(_), None)
                | (MessageState::Expired, Some(_), None)
                | (MessageState::Replied, Some(_), Some(_))
        )
        || record.reply.as_ref().is_some_and(|value| value.len() > 119)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};

    fn message(maximum_reply: Option<usize>) -> OperatorMessage {
        OperatorMessage::new(
            "execution-1",
            "run-1",
            "ALICE",
            b"READY FOR OPERATOR".to_vec(),
            Some("OPER".into()),
            vec![],
            Some(2),
            maximum_reply,
            maximum_reply.map(|_| 50),
            "effect-1",
            [7; 32],
        )
        .unwrap()
    }

    #[test]
    fn durable_reply_is_canonical_bounded_and_replayable() {
        let store = MemoryStore::new(Default::default());
        let pending = message(Some(8));
        put(&store, &pending).unwrap();
        assert_eq!(read(&store, &pending.key).unwrap(), Some(pending.clone()));
        assert_eq!(
            list(&store, CicsLimits::default()).unwrap(),
            vec![pending.clone()]
        );

        let replied = answer(&store, &pending.key, b"CONTINUE", 49).unwrap();
        assert_eq!(replied.state, MessageState::Replied);
        assert_eq!(replied.reply.as_deref(), Some(b"CONTINUE".as_slice()));
        assert_eq!(answer(&store, &pending.key, b"CONTINUE", 49), Ok(replied));
        assert_eq!(
            answer(&store, &pending.key, b"DIFFERENT", 49),
            Err(HostProblem::IdempotencyConflict)
        );
        put(&store, &pending).unwrap();
    }

    #[test]
    fn deadline_and_corruption_reject_before_reply_mutation() {
        let store = MemoryStore::new(Default::default());
        let pending = message(Some(4));
        put(&store, &pending).unwrap();
        assert_eq!(
            answer(&store, &pending.key, b"LATE", 50),
            Err(HostProblem::TimedOut)
        );
        assert_eq!(read(&store, &pending.key).unwrap(), Some(pending.clone()));

        let row = store
            .get_provider_state(NAMESPACE, &pending.key)
            .unwrap()
            .unwrap();
        store
            .delete_provider_state(NAMESPACE, &pending.key, row.version)
            .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    payload: b"{\"schema\":\"truncated\"}".to_vec(),
                    ..row
                },
                None,
            )
            .unwrap();
        assert_eq!(
            list(&store, CicsLimits::default()),
            Err(HostProblem::InfrastructureFailure)
        );
    }

    #[test]
    fn sqlite_reopen_recovers_reply_and_deadline_transition() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-operator-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let pending = message(Some(8));
        {
            let store = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
            put(&store, &pending).unwrap();
        }
        {
            let store = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
            assert_eq!(read(&store, &pending.key).unwrap(), Some(pending.clone()));
            assert_eq!(expire(&store, &pending.key, 49).unwrap(), pending);
            let expired = expire(&store, &pending.key, 50).unwrap();
            assert_eq!(expired.state, MessageState::Expired);
            assert_eq!(
                answer(&store, &pending.key, b"LATE", 50),
                Err(HostProblem::NotFound)
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
