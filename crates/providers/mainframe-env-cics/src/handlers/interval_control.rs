//! Durable producer/consumer state for CICS interval START records.
//!
//! This module owns only the versioned record transition. Command lowering,
//! shared work admission, task creation, and RETRIEVE storage bindings remain
//! separate slices. Keeping the state authority internal prevents incomplete
//! interval behavior from becoming an advertised route.

#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "records-core is intentionally unreachable until START and RETRIEVE seal"
    )
)]

use super::super::{CicsLimits, field, store_error};
use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::BTreeMap;

const NAMESPACE: &str = "cics-interval-start-v1";
const MAGIC: &[u8; 7] = b"MECISR1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::service) enum IntervalStartState {
    Pending,
    ProtectedPending,
    Ready,
    Consumed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct IntervalStartRecord {
    pub(in crate::service) request_id: String,
    pub(in crate::service) transaction: String,
    pub(in crate::service) principal: String,
    pub(in crate::service) originating_run_unit: String,
    pub(in crate::service) expiration_tick: u64,
    pub(in crate::service) terminal: Option<String>,
    pub(in crate::service) data: Vec<u8>,
    pub(in crate::service) return_transaction: Option<String>,
    pub(in crate::service) return_terminal: Option<String>,
    pub(in crate::service) queue: Option<String>,
    pub(in crate::service) fmh: bool,
    pub(in crate::service) state: IntervalStartState,
    pub(in crate::service) producer_effect_key: String,
    pub(in crate::service) producer_request_digest: [u8; 32],
    pub(in crate::service) consumer_effect_key: Option<String>,
    pub(in crate::service) consumer_request_digest: Option<[u8; 32]>,
    pub(in crate::service) version: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::service) enum IntervalScheduleOutcome {
    Created,
    Replayed,
    DuplicateRequestId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct IntervalConsumeRequest<'a> {
    pub(in crate::service) transaction: &'a str,
    pub(in crate::service) terminal: Option<&'a str>,
    pub(in crate::service) now_tick: u64,
    pub(in crate::service) effect_key: &'a str,
    pub(in crate::service) request_digest: [u8; 32],
}

pub(in crate::service) fn load(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<BTreeMap<String, IntervalStartRecord>, HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total_bytes = 0usize;
    let mut records = BTreeMap::new();
    for row in rows {
        let record = decode(&row.payload, row.version, limits)?;
        total_bytes = total_bytes
            .checked_add(record.retained_bytes())
            .ok_or(HostProblem::ResourceExhausted)?;
        if total_bytes > limits.max_queue_bytes
            || row.key != record.request_id
            || records.insert(row.key, record).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(records)
}

pub(in crate::service) fn schedule(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    record: IntervalStartRecord,
    limits: CicsLimits,
) -> Result<IntervalScheduleOutcome, HostProblem> {
    validate(&record, limits)?;
    if let Some(existing) = records.get(&record.request_id) {
        return Ok(if same_producer(existing, &record) {
            IntervalScheduleOutcome::Replayed
        } else {
            IntervalScheduleOutcome::DuplicateRequestId
        });
    }
    if records.len() >= limits.max_queue_records
        || records
            .values()
            .try_fold(record.retained_bytes(), |total, value| {
                total.checked_add(value.retained_bytes())
            })
            .is_none_or(|total| total > limits.max_queue_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: record.request_id.clone(),
                version: 1,
                payload: encode(&record, limits)?,
            },
            None,
        )
        .map_err(store_error)?;
    records.insert(record.request_id.clone(), record);
    Ok(IntervalScheduleOutcome::Created)
}

pub(in crate::service) fn promote_due(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    now_tick: u64,
    max: usize,
    limits: CicsLimits,
) -> Result<usize, HostProblem> {
    if now_tick == 0 || max == 0 {
        return Err(HostProblem::Malformed);
    }
    let selected = records
        .values()
        .filter(|record| {
            record.state == IntervalStartState::Pending && record.expiration_tick <= now_tick
        })
        .map(|record| (record.expiration_tick, record.request_id.clone()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(max)
        .map(|(_, request_id)| request_id)
        .collect::<Vec<_>>();
    for request_id in &selected {
        replace_state(
            store,
            records,
            request_id,
            IntervalStartState::Ready,
            None,
            None,
            limits,
        )?;
    }
    Ok(selected.len())
}

pub(in crate::service) fn release_protected(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    producer_effect_key: &str,
    limits: CicsLimits,
) -> Result<usize, HostProblem> {
    checked_effect_key(producer_effect_key)?;
    let selected = records
        .values()
        .filter(|record| {
            record.state == IntervalStartState::ProtectedPending
                && record.producer_effect_key == producer_effect_key
        })
        .map(|record| record.request_id.clone())
        .collect::<Vec<_>>();
    for request_id in &selected {
        replace_state(
            store,
            records,
            request_id,
            IntervalStartState::Pending,
            None,
            None,
            limits,
        )?;
    }
    Ok(selected.len())
}

pub(in crate::service) fn consume_next(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    request: IntervalConsumeRequest<'_>,
    limits: CicsLimits,
) -> Result<Option<IntervalStartRecord>, HostProblem> {
    checked_effect_key(request.effect_key)?;
    if request.now_tick == 0 || !valid_name(request.transaction, 4) {
        return Err(HostProblem::Malformed);
    }
    if request.terminal.is_some_and(|value| !valid_name(value, 4)) {
        return Err(HostProblem::Malformed);
    }
    if let Some(record) = records
        .values()
        .find(|record| record.consumer_effect_key.as_deref() == Some(request.effect_key))
    {
        return if record.consumer_request_digest == Some(request.request_digest)
            && matches_context(record, request.transaction, request.terminal)
        {
            Ok(Some(record.clone()))
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    let candidate = records
        .values()
        .filter(|record| {
            record.state == IntervalStartState::Ready
                && record.expiration_tick <= request.now_tick
                && matches_context(record, request.transaction, request.terminal)
        })
        .map(|record| (record.expiration_tick, record.request_id.clone()))
        .min()
        .map(|(_, request_id)| request_id);
    let Some(request_id) = candidate else {
        return Ok(None);
    };
    replace_state(
        store,
        records,
        &request_id,
        IntervalStartState::Consumed,
        Some(request.effect_key.into()),
        Some(request.request_digest),
        limits,
    )?;
    Ok(records.get(&request_id).cloned())
}

fn replace_state(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    request_id: &str,
    state: IntervalStartState,
    consumer_effect_key: Option<String>,
    consumer_request_digest: Option<[u8; 32]>,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let current = records
        .get(request_id)
        .cloned()
        .ok_or(HostProblem::NotFound)?;
    let mut next = current.clone();
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    next.state = state;
    next.consumer_effect_key = consumer_effect_key;
    next.consumer_request_digest = consumer_request_digest;
    validate(&next, limits)?;
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: request_id.into(),
                version: next.version,
                payload: encode(&next, limits)?,
            },
            Some(current.version),
        )
        .map_err(store_error)?;
    records.insert(request_id.into(), next);
    Ok(())
}

fn same_producer(left: &IntervalStartRecord, right: &IntervalStartRecord) -> bool {
    left.producer_effect_key == right.producer_effect_key
        && left.producer_request_digest == right.producer_request_digest
        && left.request_id == right.request_id
        && left.transaction == right.transaction
        && left.principal == right.principal
        && left.originating_run_unit == right.originating_run_unit
        && left.expiration_tick == right.expiration_tick
        && left.terminal == right.terminal
        && left.data == right.data
        && left.return_transaction == right.return_transaction
        && left.return_terminal == right.return_terminal
        && left.queue == right.queue
        && left.fmh == right.fmh
}

fn matches_context(
    record: &IntervalStartRecord,
    transaction: &str,
    terminal: Option<&str>,
) -> bool {
    record.transaction == transaction.to_ascii_uppercase()
        && record.terminal.as_deref() == terminal.map(str::to_ascii_uppercase).as_deref()
}

fn validate(record: &IntervalStartRecord, limits: CicsLimits) -> Result<(), HostProblem> {
    if !valid_name(&record.request_id, 8)
        || !valid_name(&record.transaction, 4)
        || record
            .terminal
            .as_deref()
            .is_some_and(|value| !valid_name(value, 4))
        || record
            .return_transaction
            .as_deref()
            .is_some_and(|value| !valid_name(value, 4))
        || record
            .return_terminal
            .as_deref()
            .is_some_and(|value| !valid_name(value, 4))
        || record
            .queue
            .as_deref()
            .is_some_and(|value| !valid_name(value, 8))
        || record.principal.is_empty()
        || record.originating_run_unit.is_empty()
        || record.expiration_tick == 0
        || record.version == 0
        || record.data.len() > limits.max_queue_bytes
    {
        return Err(HostProblem::Malformed);
    }
    checked_effect_key(&record.producer_effect_key)?;
    match (
        record.state,
        record.consumer_effect_key.as_deref(),
        record.consumer_request_digest,
    ) {
        (IntervalStartState::Consumed, Some(key), Some(_)) => checked_effect_key(key),
        (IntervalStartState::Consumed, _, _) => Err(HostProblem::Malformed),
        (_, None, None) => Ok(()),
        _ => Err(HostProblem::Malformed),
    }
}

fn checked_effect_key(value: &str) -> Result<(), HostProblem> {
    IdempotencyKey::new(value, InvocationLimits::default())
        .map(|_| ())
        .map_err(|_| HostProblem::Malformed)
}

fn valid_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b'$' | b'#' | b'@')
        })
}

impl IntervalStartRecord {
    fn retained_bytes(&self) -> usize {
        self.request_id.len()
            + self.transaction.len()
            + self.principal.len()
            + self.originating_run_unit.len()
            + self.terminal.as_ref().map_or(0, String::len)
            + self.data.len()
            + self.return_transaction.as_ref().map_or(0, String::len)
            + self.return_terminal.as_ref().map_or(0, String::len)
            + self.queue.as_ref().map_or(0, String::len)
            + self.producer_effect_key.len()
            + self.consumer_effect_key.as_ref().map_or(0, String::len)
            + 96
    }
}

fn encode(record: &IntervalStartRecord, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
    validate(record, limits)?;
    let mut out = MAGIC.to_vec();
    field(&mut out, record.request_id.as_bytes())?;
    field(&mut out, record.transaction.as_bytes())?;
    field(&mut out, record.principal.as_bytes())?;
    field(&mut out, record.originating_run_unit.as_bytes())?;
    out.extend_from_slice(&record.expiration_tick.to_be_bytes());
    optional_field(&mut out, record.terminal.as_deref())?;
    field(&mut out, &record.data)?;
    optional_field(&mut out, record.return_transaction.as_deref())?;
    optional_field(&mut out, record.return_terminal.as_deref())?;
    optional_field(&mut out, record.queue.as_deref())?;
    out.push(u8::from(record.fmh));
    out.push(match record.state {
        IntervalStartState::Pending => 1,
        IntervalStartState::ProtectedPending => 2,
        IntervalStartState::Ready => 3,
        IntervalStartState::Consumed => 4,
    });
    field(&mut out, record.producer_effect_key.as_bytes())?;
    out.extend_from_slice(&record.producer_request_digest);
    optional_field(&mut out, record.consumer_effect_key.as_deref())?;
    match record.consumer_request_digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(&digest);
        }
        None => out.push(0),
    }
    Ok(out)
}

fn decode(
    payload: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<IntervalStartRecord, HostProblem> {
    let mut reader = RecordReader::new(payload);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record = IntervalStartRecord {
        request_id: reader.text(8)?,
        transaction: reader.text(4)?,
        principal: reader.text(256)?,
        originating_run_unit: reader.text(256)?,
        expiration_tick: reader.u64()?,
        terminal: reader.optional_text(4)?,
        data: reader.field(limits.max_queue_bytes)?,
        return_transaction: reader.optional_text(4)?,
        return_terminal: reader.optional_text(4)?,
        queue: reader.optional_text(8)?,
        fmh: reader.boolean()?,
        state: match reader.byte()? {
            1 => IntervalStartState::Pending,
            2 => IntervalStartState::ProtectedPending,
            3 => IntervalStartState::Ready,
            4 => IntervalStartState::Consumed,
            _ => return Err(HostProblem::InfrastructureFailure),
        },
        producer_effect_key: reader.text(InvocationLimits::default().max_binding_bytes)?,
        producer_request_digest: reader.digest()?,
        consumer_effect_key: reader.optional_text(InvocationLimits::default().max_binding_bytes)?,
        consumer_request_digest: match reader.byte()? {
            0 => None,
            1 => Some(reader.digest()?),
            _ => return Err(HostProblem::InfrastructureFailure),
        },
        version,
    };
    if !reader.done() {
        return Err(HostProblem::InfrastructureFailure);
    }
    validate(&record, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(record)
}

fn optional_field(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), HostProblem> {
    match value {
        Some(value) => {
            out.push(1);
            field(out, value.as_bytes())
        }
        None => {
            out.push(0);
            Ok(())
        }
    }
}

struct RecordReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> RecordReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
    }

    fn boolean(&mut self) -> Result<bool, HostProblem> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn u64(&mut self) -> Result<u64, HostProblem> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn field(&mut self, max: usize) -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::ResourceExhausted)?;
        if length > max {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(length)?.to_vec())
    }

    fn text(&mut self, max: usize) -> Result<String, HostProblem> {
        String::from_utf8(self.field(max)?).map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn optional_text(&mut self, max: usize) -> Result<Option<String>, HostProblem> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.text(max).map(Some),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn digest(&mut self) -> Result<[u8; 32], HostProblem> {
        self.take(32)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store::{MemoryStore, SqliteStateStore};
    use std::sync::Arc;

    fn record(request_id: &str, expiration_tick: u64, producer: &str) -> IntervalStartRecord {
        IntervalStartRecord {
            request_id: request_id.into(),
            transaction: "NEXT".into(),
            principal: "IBMUSER".into(),
            originating_run_unit: "RUN-1".into(),
            expiration_tick,
            terminal: None,
            data: format!("DATA-{request_id}").into_bytes(),
            return_transaction: Some("BACK".into()),
            return_terminal: Some("T001".into()),
            queue: Some("QUEUE1".into()),
            fmh: false,
            state: IntervalStartState::Pending,
            producer_effect_key: producer.into(),
            producer_request_digest: [expiration_tick as u8; 32],
            consumer_effect_key: None,
            consumer_request_digest: None,
            version: 1,
        }
    }

    #[test]
    fn duplicate_request_id_distinguishes_replay_from_conflict() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let limits = CicsLimits::default();
        let mut records = load(store.as_ref(), limits).unwrap();
        let first = record("REQ1", 100, "producer-1");
        assert_eq!(
            schedule(store.as_ref(), &mut records, first.clone(), limits).unwrap(),
            IntervalScheduleOutcome::Created
        );
        assert_eq!(
            schedule(store.as_ref(), &mut records, first, limits).unwrap(),
            IntervalScheduleOutcome::Replayed
        );
        assert_eq!(
            schedule(
                store.as_ref(),
                &mut records,
                record("REQ1", 101, "producer-2"),
                limits,
            )
            .unwrap(),
            IntervalScheduleOutcome::DuplicateRequestId
        );
    }

    #[test]
    fn expiry_order_and_consumption_are_durable_and_replay_safe() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let limits = CicsLimits::default();
        let mut records = BTreeMap::new();
        schedule(
            store.as_ref(),
            &mut records,
            record("LATE", 200, "producer-1"),
            limits,
        )
        .unwrap();
        schedule(
            store.as_ref(),
            &mut records,
            record("EARLY", 100, "producer-2"),
            limits,
        )
        .unwrap();
        assert_eq!(
            promote_due(store.as_ref(), &mut records, 150, 8, limits).unwrap(),
            1
        );
        let request = IntervalConsumeRequest {
            transaction: "NEXT",
            terminal: None,
            now_tick: 150,
            effect_key: "consumer-1",
            request_digest: [9; 32],
        };
        let first = consume_next(store.as_ref(), &mut records, request.clone(), limits)
            .unwrap()
            .unwrap();
        assert_eq!(first.request_id, "EARLY");
        assert_eq!(first.state, IntervalStartState::Consumed);
        assert_eq!(
            consume_next(store.as_ref(), &mut records, request, limits)
                .unwrap()
                .unwrap(),
            first
        );
        assert!(
            consume_next(
                store.as_ref(),
                &mut records,
                IntervalConsumeRequest {
                    transaction: "NEXT",
                    terminal: None,
                    now_tick: 150,
                    effect_key: "consumer-2",
                    request_digest: [8; 32],
                },
                limits,
            )
            .unwrap()
            .is_none()
        );

        let reopened = load(store.as_ref(), limits).unwrap();
        assert_eq!(reopened, records);
    }

    #[test]
    fn protected_record_is_invisible_until_its_owner_releases_it() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let limits = CicsLimits::default();
        let mut records = BTreeMap::new();
        let mut protected = record("SAFE", 100, "producer-safe");
        protected.state = IntervalStartState::ProtectedPending;
        schedule(store.as_ref(), &mut records, protected, limits).unwrap();
        assert_eq!(
            promote_due(store.as_ref(), &mut records, 200, 8, limits).unwrap(),
            0
        );
        assert_eq!(
            release_protected(store.as_ref(), &mut records, "producer-safe", limits).unwrap(),
            1
        );
        assert_eq!(
            promote_due(store.as_ref(), &mut records, 200, 8, limits).unwrap(),
            1
        );
    }

    #[test]
    fn sqlite_reopen_preserves_ready_and_consumed_records() {
        let root = std::env::temp_dir().join(format!(
            "mainframe-env-cics-interval-records-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let url = format!("sqlite://{}?mode=rwc", root.join("state.db").display());
        let limits = CicsLimits::default();
        let first = Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
        let mut records = load(first.as_ref(), limits).unwrap();
        schedule(
            first.as_ref(),
            &mut records,
            record("REQSQL", 100, "producer-sql"),
            limits,
        )
        .unwrap();
        promote_due(first.as_ref(), &mut records, 100, 8, limits).unwrap();
        consume_next(
            first.as_ref(),
            &mut records,
            IntervalConsumeRequest {
                transaction: "NEXT",
                terminal: None,
                now_tick: 100,
                effect_key: "consumer-sql",
                request_digest: [7; 32],
            },
            limits,
        )
        .unwrap();
        drop(first);

        let second = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        let reopened = load(&second, limits).unwrap();
        assert_eq!(reopened, records);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
}
