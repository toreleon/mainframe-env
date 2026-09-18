//! Durable producer/consumer state for CICS interval START records.
//!
//! The typed local-data route connects these transitions to shared work admission
//! and RETRIEVE storage bindings. Explicit syncpoint owns protected-record release;
//! bounded due scans and general protected task cleanup remain later slices.

mod cancel;
mod delay;
mod protect;
mod retrieve;

pub use delay::CICS_DELAY_WORK_GENERATION;
pub(super) use protect::discard_records as discard_protected_start_records;
pub(super) use protect::discard_run as discard_protected_starts;
pub(super) use protect::finish_syncpoint as finish_protected_starts;

use super::super::{CicsLimits, CicsReplayClock, CicsService, Run, field, store_error};
use crate::{CicsIntervalMode, CicsIntervalTime};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, IdempotencyKey, InvocationLimits, PrincipalId,
    Selector,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, ClockRequest,
    HostProblem, HostRequest, HostResult, ScopedHostService, SecurityDecision, SecurityRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState, WorkStore,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;

const NAMESPACE: &str = "cics-interval-start-v1";
const MAGIC: &[u8; 7] = b"MECISR1";

/// Durable work-generation identity used by interval START records.
pub const CICS_START_WORK_GENERATION: &str = "cics-start-v1";

impl CicsService {
    /// Open with the shared durable clock and work authority used by interval START.
    pub fn open_with_runtime(
        host: Arc<ScopedHostService>,
        store: Arc<dyn ProviderStateStore>,
        work_store: Arc<dyn WorkStore>,
        limits: CicsLimits,
        replay_clock: Arc<dyn CicsReplayClock>,
    ) -> Result<Arc<Self>, HostProblem> {
        delay::validate_store(store.as_ref(), limits)?;
        Self::open_inner(host, store, limits, Some(replay_clock), Some(work_store))
    }

    fn durable_tick(&self) -> Result<u64, HostProblem> {
        let tick = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?
            .now_tick()?;
        if tick == 0 {
            Err(HostProblem::InfrastructureFailure)
        } else {
            Ok(tick)
        }
    }

    fn enqueue_interval_work(
        &self,
        record: &IntervalStartRecord,
        priority: u8,
    ) -> Result<(), HostProblem> {
        let work_store = self
            .work_store
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        let limits = InvocationLimits::default();
        let identity = format!(
            "{:x}",
            Sha256::digest(
                [
                    record.request_id.as_bytes(),
                    &record.producer_request_digest,
                ]
                .concat(),
            ),
        );
        let work = WorkRecord {
            work_id: format!("cics-start:{}", record.request_id),
            execution_id: ExecutionId::new(format!("cics-start-{}", &identity[..24]), limits)
                .map_err(|_| HostProblem::ResourceExhausted)?,
            required_selector: Selector::new("cics:start", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            required_generation: CICS_START_WORK_GENERATION.into(),
            artifact: ArtifactRef::new("artifact:none", limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            state: WorkState::Queued,
            priority,
            attempt: 0,
            max_attempts: 3,
            available_tick: record.expiration_tick,
            deadline_tick: record
                .expiration_tick
                .checked_add(86_400_000)
                .ok_or(HostProblem::ResourceExhausted)?,
            cancellation_requested: false,
            worker_id: None,
            lease_id: None,
            lease_epoch: 0,
            lease_expiry_tick: None,
            heartbeat_tick: None,
            terminal_tick: None,
            checkpoint_id: None,
            effect_sequence: 0,
            payload: record.request_id.as_bytes().to_vec(),
        };
        match work_store.enqueue(work.clone()) {
            Ok(()) => Ok(()),
            Err(StoreError::AlreadyExists | StoreError::Conflict)
                if work_store
                    .get_work(&work.work_id)
                    .map_err(store_error)?
                    .as_ref()
                    .is_some_and(|existing| same_interval_work(existing, &work)) =>
            {
                Ok(())
            }
            Err(problem) => Err(store_error(problem)),
        }
    }

    /// Promote one claimed START work item into the ready-record authority.
    pub fn promote_start_work(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<super::CicsStartTask, HostProblem> {
        if work.required_generation != CICS_START_WORK_GENERATION
            || work.required_selector.as_str() != "cics:start"
            || work.artifact.as_str() != "artifact:none"
            || work.state != WorkState::Claimed
            || work.lease_id.is_none()
            || work.lease_epoch == 0
            || now_tick < work.available_tick
        {
            return Err(HostProblem::Malformed);
        }
        let request_id = std::str::from_utf8(&work.payload)
            .map_err(|_| HostProblem::Malformed)?
            .to_string();
        if work.work_id != format!("cics-start:{request_id}") {
            return Err(HostProblem::Malformed);
        }
        let mut state = self.lock()?;
        promote_request(
            self.store.as_ref(),
            &mut state.interval_records,
            &request_id,
            now_tick,
            self.limits,
        )?;
        state
            .interval_records
            .get(&request_id)
            .map(super::start_task::from_interval_record)
            .ok_or(HostProblem::InfrastructureFailure)
    }
}

fn same_interval_work(existing: &WorkRecord, expected: &WorkRecord) -> bool {
    existing.work_id == expected.work_id
        && existing.execution_id == expected.execution_id
        && existing.required_selector == expected.required_selector
        && existing.required_generation == expected.required_generation
        && existing.artifact == expected.artifact
        && existing.priority == expected.priority
        && existing.max_attempts == expected.max_attempts
        && existing.available_tick == expected.available_tick
        && existing.deadline_tick == expected.deadline_tick
        && existing.payload == expected.payload
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    match request.operation {
        CicsOperation::Cancel => cancel::invoke(service, run, request),
        CicsOperation::Delay => delay::invoke(service, run, request),
        CicsOperation::Start => start(service, run, request),
        CicsOperation::Retrieve => retrieve::invoke(service, run, request),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    delay::release_task(service, run)
}

fn start(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_start_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let transaction = name_argument(request, "TRANSID", 4)?;
    let producer_request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let supplied_request_id = request
        .arguments
        .get("REQID")
        .map(|_| name_argument(request, "REQID", 8))
        .transpose()?;
    let request_id_was_generated = supplied_request_id.is_none();
    let request_id = supplied_request_id.unwrap_or_else(|| {
        generated_request_id(mutation.idempotency_key.as_str(), &producer_request_digest)
    });
    let return_transaction = optional_name_argument(request, "RTRANSID", 4)?;
    let return_terminal = optional_name_argument(request, "RTERMID", 4)?;
    let queue = optional_name_argument(request, "QUEUE", 8)?;
    let execution_user = optional_name_argument(request, "USERID", 8)?;
    if queue.as_deref() == Some(request_id.as_str()) {
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 0,
        });
    }
    let source = request
        .arguments
        .get("FROM")
        .map(BoundedPayload::bytes)
        .unwrap_or_default();
    let length = optional_decimal(request, "LENGTH")?
        .unwrap_or(i64::try_from(source.len()).map_err(|_| HostProblem::ResourceExhausted)?);
    let length = usize::try_from(length).map_err(|_| HostProblem::Condition {
        name: "LENGERR".into(),
        response: 22,
        response2: 0,
    })?;
    if (request.arguments.contains_key("FROM") && length == 0) || length > source.len() {
        return Err(HostProblem::Condition {
            name: "LENGERR".into(),
            response: 22,
            response2: 0,
        });
    }
    service
        .authorize(
            run,
            "TCICSTRN",
            &format!("CICS.{transaction}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => HostProblem::Condition {
                name: "NOTAUTH".into(),
                response: 70,
                response2: 7,
            },
            other => other,
        })?;
    if let Some(user) = execution_user.as_deref() {
        validate_start_principal(service, run, user)?;
        service
            .authorize(
                run,
                "SURROGAT",
                &format!("{user}.DFHSTART"),
                AccessIntent::Read,
            )
            .map_err(|problem| match problem {
                HostProblem::Unauthorized => HostProblem::Condition {
                    name: "NOTAUTH".into(),
                    response: 70,
                    response2: 9,
                },
                other => other,
            })?;
    }
    let time = if let Some(value) = optional_decimal(request, "INTERVAL")? {
        CicsIntervalTime::from_hhmmss(CicsIntervalMode::Relative, value)
    } else if let Some(value) = optional_decimal(request, "TIME")? {
        CicsIntervalTime::from_hhmmss(CicsIntervalMode::Absolute, value)
    } else if request.arguments.contains_key("OPTION.AFTER") {
        explicit_start_time(request, CicsIntervalMode::Relative)
    } else if request.arguments.contains_key("OPTION.AT") {
        explicit_start_time(request, CicsIntervalMode::Absolute)
    } else {
        Ok(CicsIntervalTime::immediate())
    }
    .map_err(|problem| HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: problem.start_response2(),
    })?;
    let timestamp = match service.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
        HostResult::Clock(value) => value,
        _ => return Err(HostProblem::ProviderFailure),
    };
    let local_millis = clock_millis_since_midnight(&timestamp)?;
    let expiration_tick = time
        .deadline_tick(service.durable_tick()?, local_millis)
        .ok_or(HostProblem::ResourceExhausted)?;
    let record = IntervalStartRecord {
        request_id,
        transaction,
        principal: execution_user.unwrap_or_else(|| run.invocation.principal.id().as_str().into()),
        originating_run_unit: run.invocation.run_unit_id.as_str().into(),
        expiration_tick,
        terminal: None,
        data: source[..length].to_vec(),
        return_transaction,
        return_terminal,
        queue,
        fmh: request.arguments.contains_key("OPTION.FMH"),
        state: if request.arguments.contains_key("OPTION.PROTECT") {
            IntervalStartState::ProtectedPending
        } else {
            IntervalStartState::Pending
        },
        producer_effect_key: mutation.idempotency_key.as_str().into(),
        producer_request_digest,
        consumer_effect_key: None,
        consumer_request_digest: None,
        version: 1,
    };
    let outcome = {
        let mut state = service.lock()?;
        schedule(
            service.store.as_ref(),
            &mut state.interval_records,
            record.clone(),
            service.limits,
        )?
    };
    if outcome == IntervalScheduleOutcome::DuplicateRequestId {
        return Err(HostProblem::Condition {
            name: "IOERR".into(),
            response: 17,
            response2: 0,
        });
    }
    if record.state == IntervalStartState::Pending {
        service.enqueue_interval_work(&record, run.invocation.priority)?;
    }
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
    if request_id_was_generated && !request.arguments.contains_key("OPTION.NOCHECK") {
        response.outputs.insert(
            "EIBREQID".into(),
            BoundedPayload::new(
                "mainframe-env.cics.reqid@1",
                record.request_id.as_bytes().to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
}

fn validate_start_principal(
    service: &CicsService,
    run: &mut Run,
    user: &str,
) -> Result<(), HostProblem> {
    let principal =
        PrincipalId::new(user, InvocationLimits::default()).map_err(|_| HostProblem::Malformed)?;
    let decision = match service.nested(
        run,
        HostRequest::Security(SecurityRequest::ValidatePrincipal { principal }),
    )? {
        HostResult::Security(decision) => decision,
        _ => return Err(HostProblem::ProviderFailure),
    };
    match decision {
        SecurityDecision::Allow | SecurityDecision::Expired => Ok(()),
        SecurityDecision::NotFound | SecurityDecision::InvalidCredentials => {
            Err(userid_condition(8))
        }
        SecurityDecision::Revoked => Err(userid_condition(19)),
        SecurityDecision::Locked => Err(userid_condition(10)),
        SecurityDecision::Deny => Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: 18,
        }),
    }
}

fn userid_condition(response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: "USERIDERR".into(),
        response: 69,
        response2,
    }
}

fn explicit_start_time(
    request: &CicsRequest,
    mode: CicsIntervalMode,
) -> Result<CicsIntervalTime, crate::CicsIntervalError> {
    let component = |name| {
        optional_decimal(request, name).map_err(|_| crate::CicsIntervalError::InvalidRequest)
    };
    CicsIntervalTime::from_components(
        mode,
        component("HOURS")?,
        component("MINUTES")?,
        component("SECONDS")?,
    )
}

fn validate_start_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "FROM",
        "HOURS",
        "INTERVAL",
        "LENGTH",
        "MINUTES",
        "OPTION.AFTER",
        "OPTION.AT",
        "OPTION.FMH",
        "OPTION.NOCHECK",
        "OPTION.NOHANDLE",
        "OPTION.PROTECT",
        "QUEUE",
        "REQID",
        "RESP",
        "RESP2",
        "RTERMID",
        "RTRANSID",
        "SECONDS",
        "TIME",
        "TRANSID",
        "USERID",
    ];
    let schedule_selectors = ["INTERVAL", "TIME", "OPTION.AFTER", "OPTION.AT"]
        .into_iter()
        .filter(|name| request.arguments.contains_key(*name))
        .count();
    let explicit_mode = request.arguments.contains_key("OPTION.AFTER")
        || request.arguments.contains_key("OPTION.AT");
    let explicit_components = ["HOURS", "MINUTES", "SECONDS"]
        .into_iter()
        .any(|name| request.arguments.contains_key(name));
    if !request.arguments.contains_key("TRANSID")
        || schedule_selectors > 1
        || explicit_mode != explicit_components
        || request.arguments.contains_key("LENGTH") && !request.arguments.contains_key("FROM")
        || request.arguments.contains_key("OPTION.FMH") && !request.arguments.contains_key("FROM")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || if matches!(
                    name.as_str(),
                    "HOURS" | "INTERVAL" | "LENGTH" | "MINUTES" | "SECONDS" | "TIME"
                ) {
                    value.schema() != "mainframe-env.cics.decimal@1"
                } else if matches!(
                    name.as_str(),
                    "OPTION.AFTER"
                        | "OPTION.AT"
                        | "OPTION.FMH"
                        | "OPTION.NOCHECK"
                        | "OPTION.NOHANDLE"
                        | "OPTION.PROTECT"
                ) {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                } else {
                    !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1"
                            | "mainframe-env.cics.storage-value@1"
                            | "mainframe-env.cics.argument@1"
                    )
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn name_argument(request: &CicsRequest, name: &str, max: usize) -> Result<String, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim()
        .to_ascii_uppercase();
    if valid_name(&text, max) {
        Ok(text)
    } else {
        Err(HostProblem::Malformed)
    }
}

fn optional_name_argument(
    request: &CicsRequest,
    name: &str,
    max: usize,
) -> Result<Option<String>, HostProblem> {
    request
        .arguments
        .contains_key(name)
        .then(|| name_argument(request, name, max))
        .transpose()
}

fn optional_decimal(request: &CicsRequest, name: &str) -> Result<Option<i64>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            std::str::from_utf8(value.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<i64>()
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

fn generated_request_id(effect_key: &str, request_digest: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"mainframe-env.cics.start.generated-reqid@1\0");
    hasher.update(effect_key.as_bytes());
    hasher.update(request_digest);
    format!("{:X}", hasher.finalize())[..8].into()
}

pub(super) fn clock_millis_since_midnight(timestamp: &str) -> Result<u64, HostProblem> {
    if timestamp.len() != 17 || !timestamp.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HostProblem::ProviderFailure);
    }
    let part = |range: std::ops::Range<usize>| {
        timestamp[range]
            .parse::<u64>()
            .map_err(|_| HostProblem::ProviderFailure)
    };
    let (hour, minute, second, millis) =
        (part(8..10)?, part(10..12)?, part(12..14)?, part(14..17)?);
    if hour > 23 || minute > 59 || second > 59 || millis > 999 {
        return Err(HostProblem::ProviderFailure);
    }
    Ok(hour * 3_600_000 + minute * 60_000 + second * 1_000 + millis)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::service) enum IntervalStartState {
    Pending,
    ProtectedPending,
    Ready,
    Consumed,
    Cancelled,
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
    pub(in crate::service) return_transaction: bool,
    pub(in crate::service) return_terminal: bool,
    pub(in crate::service) queue: bool,
    pub(in crate::service) max_data_length: Option<usize>,
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

#[allow(dead_code, reason = "reserved for the bounded due-scan worker slice")]
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

pub(in crate::service) fn promote_request(
    store: &dyn ProviderStateStore,
    records: &mut BTreeMap<String, IntervalStartRecord>,
    request_id: &str,
    now_tick: u64,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let current = records.get(request_id).ok_or(HostProblem::NotFound)?;
    if matches!(
        current.state,
        IntervalStartState::Ready | IntervalStartState::Consumed
    ) && current.expiration_tick <= now_tick
    {
        return Ok(());
    }
    if current.state != IntervalStartState::Pending || current.expiration_tick > now_tick {
        return Err(HostProblem::InfrastructureFailure);
    }
    replace_state(
        store,
        records,
        request_id,
        IntervalStartState::Ready,
        None,
        None,
        limits,
    )
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
    let selected = records
        .get(&request_id)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if request.return_transaction && selected.return_transaction.is_none()
        || request.return_terminal && selected.return_terminal.is_none()
        || request.queue && selected.queue.is_none()
    {
        return Err(HostProblem::Condition {
            name: "ENVDEFERR".into(),
            response: 56,
            response2: 0,
        });
    }
    if request
        .max_data_length
        .is_some_and(|maximum| selected.data.len() > maximum)
    {
        return Err(HostProblem::ResourceExhausted);
    }
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
        (IntervalStartState::Consumed | IntervalStartState::Cancelled, Some(key), Some(_)) => {
            checked_effect_key(key)
        }
        (IntervalStartState::Consumed | IntervalStartState::Cancelled, _, _) => {
            Err(HostProblem::Malformed)
        }
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
        IntervalStartState::Cancelled => 5,
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
            5 => IntervalStartState::Cancelled,
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
            return_transaction: false,
            return_terminal: false,
            queue: false,
            max_data_length: None,
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
                    return_transaction: false,
                    return_terminal: false,
                    queue: false,
                    max_data_length: None,
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
    fn missing_requested_metadata_returns_envdeferr_without_consuming() {
        let store = Arc::new(MemoryStore::new(Default::default()));
        let limits = CicsLimits::default();
        let mut records = BTreeMap::new();
        let mut missing = record("METALESS", 100, "producer-metadata");
        missing.queue = None;
        schedule(store.as_ref(), &mut records, missing, limits).unwrap();
        promote_due(store.as_ref(), &mut records, 100, 8, limits).unwrap();
        let requested = IntervalConsumeRequest {
            transaction: "NEXT",
            terminal: None,
            now_tick: 100,
            effect_key: "consumer-metadata",
            request_digest: [5; 32],
            return_transaction: false,
            return_terminal: false,
            queue: true,
            max_data_length: None,
        };
        assert_eq!(
            consume_next(store.as_ref(), &mut records, requested.clone(), limits),
            Err(HostProblem::Condition {
                name: "ENVDEFERR".into(),
                response: 56,
                response2: 0,
            })
        );
        assert_eq!(records["METALESS"].state, IntervalStartState::Ready);
        assert!(
            consume_next(
                store.as_ref(),
                &mut records,
                IntervalConsumeRequest {
                    queue: false,
                    ..requested
                },
                limits,
            )
            .unwrap()
            .is_some()
        );
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
            protect::release(store.as_ref(), &mut records, "RUN-1", limits).unwrap(),
            std::collections::BTreeSet::from(["SAFE".into()])
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
        let mut protected = record("PROTSQL", 100, "producer-protect-sql");
        protected.state = IntervalStartState::ProtectedPending;
        schedule(first.as_ref(), &mut records, protected, limits).unwrap();
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
                return_transaction: false,
                return_terminal: false,
                queue: false,
                max_data_length: None,
            },
            limits,
        )
        .unwrap();
        drop(first);

        let second = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
        let mut reopened = load(&second, limits).unwrap();
        assert_eq!(reopened, records);
        assert_eq!(
            protect::release(&second, &mut reopened, "RUN-1", limits).unwrap(),
            std::collections::BTreeSet::from(["PROTSQL".into()])
        );
        assert_eq!(load(&second, limits).unwrap(), reopened);
        drop(second);
        std::fs::remove_dir_all(root).unwrap();
    }
}
