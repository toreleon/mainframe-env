use super::{clock_millis_since_midnight, name_argument, optional_decimal, valid_name};
use crate::service::{CicsLimits, CicsService, Run, field, store_error};
use crate::{CicsIntervalMode, CicsIntervalTime};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, InvocationLimits, Selector,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, ClockRequest, HostProblem,
    HostRequest, HostResult, canonical_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const NAMESPACE: &str = "cics-delay-v1";
const MAGIC_V1: &[u8; 7] = b"MECDLY1";
const MAGIC_V2: &[u8; 7] = b"MECDLY2";
const MAX_CAS_ATTEMPTS: usize = 8;

/// Durable work-generation identity used by positive DELAY requests.
pub const CICS_DELAY_WORK_GENERATION: &str = "cics-delay-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DelayState {
    Pending,
    Ready,
    Consumed,
    Abandoned,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DelayRecord {
    delay_id: String,
    request_id: Option<String>,
    run_unit: String,
    transaction: String,
    interval: i64,
    expiration_tick: u64,
    work_id: String,
    priority: u8,
    state: DelayState,
    producer_effect_key: String,
    producer_request_digest: [u8; 32],
    consumer_effect_key: Option<String>,
    consumer_request_digest: Option<[u8; 32]>,
    cancel_effect_key: Option<String>,
    cancel_request_digest: Option<[u8; 32]>,
    version: u64,
}

pub(super) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut retained = 0usize;
    let mut outstanding_names = BTreeMap::new();
    for row in rows {
        let record = decode(&row.payload, row.version, limits)?;
        retained = retained
            .checked_add(record.retained_bytes())
            .ok_or(HostProblem::ResourceExhausted)?;
        if row.key != record.delay_id || retained > limits.max_queue_bytes {
            return Err(HostProblem::InfrastructureFailure);
        }
        if let Some(request_id) = &record.request_id
            && outstanding_names
                .insert(request_id.clone(), record.delay_id.clone())
                .is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let request_id = request
        .arguments
        .get("REQID")
        .map(|_| name_argument(request, "REQID", 8))
        .transpose()?;
    let definition = delay_definition(request)?;
    let DelayDefinition::Scheduled { identity, time } = definition else {
        return match request_id {
            None => complete(service, run),
            Some(_) => Err(invalid_request()),
        };
    };
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let delay_id = delay_id(request, run)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let current = load_record(service.store.as_ref(), &delay_id, service.limits)?;
    match current {
        None => {
            let Some(expiration_tick) = expiration_tick(service, run, time)? else {
                return expired(service, run, request);
            };
            ensure_name_available(service, request_id.as_deref(), &delay_id)?;
            let record = new_record(
                &delay_id,
                request_id,
                run,
                identity,
                expiration_tick,
                mutation.idempotency_key.as_str(),
                digest,
                1,
            )?;
            persist(service.store.as_ref(), &record, None, service.limits)?;
            enqueue_work(service, &record)?;
            suspended(service, run)
        }
        Some(record) => {
            validate_context(&record, run, identity, request_id.as_deref())?;
            match record.state {
                DelayState::Pending => {
                    enqueue_work(service, &record)?;
                    suspended(service, run)
                }
                DelayState::Ready => {
                    consume(service, &record, mutation.idempotency_key.as_str(), digest)?;
                    complete_with_response2(service, run, cancellation_response2(&record))
                }
                DelayState::Consumed
                    if record.consumer_effect_key.as_deref()
                        == Some(mutation.idempotency_key.as_str())
                        && record.consumer_request_digest == Some(digest) =>
                {
                    complete_with_response2(service, run, cancellation_response2(&record))
                }
                DelayState::Consumed => {
                    let Some(expiration_tick) = expiration_tick(service, run, time)? else {
                        return expired(service, run, request);
                    };
                    ensure_name_available(service, request_id.as_deref(), &delay_id)?;
                    let next = new_record(
                        &delay_id,
                        request_id,
                        run,
                        identity,
                        expiration_tick,
                        mutation.idempotency_key.as_str(),
                        digest,
                        record
                            .version
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?,
                    )?;
                    persist(
                        service.store.as_ref(),
                        &next,
                        Some(record.version),
                        service.limits,
                    )?;
                    enqueue_work(service, &next)?;
                    suspended(service, run)
                }
                DelayState::Abandoned => Err(HostProblem::Cancelled),
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DelayDefinition {
    Immediate,
    Scheduled {
        identity: i64,
        time: CicsIntervalTime,
    },
}

fn delay_definition(request: &CicsRequest) -> Result<DelayDefinition, HostProblem> {
    let interval = optional_decimal(request, "INTERVAL")?;
    let relative = request.arguments.contains_key("OPTION.FOR");
    let absolute = request.arguments.contains_key("OPTION.UNTIL");
    let time = if let Some(value) = interval {
        CicsIntervalTime::from_hhmmss(CicsIntervalMode::Relative, value)
    } else if relative || absolute {
        CicsIntervalTime::from_components(
            if relative {
                CicsIntervalMode::Relative
            } else {
                CicsIntervalMode::Absolute
            },
            optional_decimal(request, "HOURS")?,
            optional_decimal(request, "MINUTES")?,
            optional_decimal(request, "SECONDS")?,
        )
    } else {
        Ok(CicsIntervalTime::immediate())
    }
    .map_err(|problem| HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: problem.delay_response2(),
    })?;
    if time.mode() == CicsIntervalMode::Relative && time.seconds() == 0 {
        return Ok(DelayDefinition::Immediate);
    }
    let identity = match (interval, time.mode()) {
        (Some(value), _) => value,
        (None, CicsIntervalMode::Relative) => 1_000_000 + i64::from(time.seconds()),
        (None, CicsIntervalMode::Absolute) => 2_000_000 + i64::from(time.seconds()),
    };
    Ok(DelayDefinition::Scheduled { identity, time })
}

fn expiration_tick(
    service: &CicsService,
    run: &mut Run,
    time: CicsIntervalTime,
) -> Result<Option<u64>, HostProblem> {
    let local_millis = if time.mode() == CicsIntervalMode::Absolute {
        let timestamp = match service.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
            HostResult::Clock(value) => value,
            _ => return Err(HostProblem::ProviderFailure),
        };
        clock_millis_since_midnight(&timestamp)?
    } else {
        0
    };
    let now_tick = service.durable_tick()?;
    let delay_millis = time
        .delay_millis(local_millis)
        .ok_or(HostProblem::ResourceExhausted)?;
    if time.mode() == CicsIntervalMode::Absolute && delay_millis == 0 {
        return Ok(None);
    }
    now_tick
        .checked_add(delay_millis)
        .ok_or(HostProblem::ResourceExhausted)
        .map(Some)
}

fn expired(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    super::super::condition::respond(
        service,
        run,
        &request.condition_policy,
        HostProblem::Condition {
            name: "EXPIRED".into(),
            response: 31,
            response2: 0,
        },
    )
}

impl CicsService {
    /// Promote one claimed positive DELAY item into its ready state.
    pub fn promote_delay_work(&self, work: &WorkRecord, now_tick: u64) -> Result<(), HostProblem> {
        if work.required_generation != CICS_DELAY_WORK_GENERATION
            || work.required_selector.as_str() != "cics:delay"
            || work.artifact.as_str() != "artifact:none"
            || work.state != WorkState::Claimed
            || work.lease_id.is_none()
            || work.lease_epoch == 0
            || now_tick < work.available_tick
        {
            return Err(HostProblem::Malformed);
        }
        let delay_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let mut record = load_record(self.store.as_ref(), delay_id, self.limits)?
            .ok_or(HostProblem::NotFound)?;
        if record.work_id != work.work_id || record.expiration_tick != work.available_tick {
            return Err(HostProblem::InfrastructureFailure);
        }
        if matches!(
            record.state,
            DelayState::Ready | DelayState::Consumed | DelayState::Abandoned
        ) {
            return Ok(());
        }
        record.state = DelayState::Ready;
        let expected = record.version;
        record.version = record
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        match persist(self.store.as_ref(), &record, Some(expected), self.limits) {
            Ok(()) => Ok(()),
            Err(HostProblem::IdempotencyConflict) => {
                let current = load_record(self.store.as_ref(), delay_id, self.limits)?
                    .ok_or(HostProblem::NotFound)?;
                if current.work_id == work.work_id
                    && current.expiration_tick == work.available_tick
                    && matches!(
                        current.state,
                        DelayState::Ready | DelayState::Consumed | DelayState::Abandoned
                    )
                {
                    Ok(())
                } else {
                    Err(HostProblem::IdempotencyConflict)
                }
            }
            Err(problem) => Err(problem),
        }
    }
}

pub(super) fn cancel_named(
    service: &CicsService,
    run: &mut Run,
    request_id: &str,
    selected_transaction: Option<&str>,
    effect_key: &str,
    request_digest: [u8; 32],
) -> Result<CicsResponse, HostProblem> {
    for _ in 0..MAX_CAS_ATTEMPTS {
        let current = find_named_record(service, request_id)?.ok_or_else(not_found)?;
        let replay = current.cancel_effect_key.as_deref() == Some(effect_key)
            && current.cancel_request_digest == Some(request_digest);
        if replay
            && matches!(
                current.state,
                DelayState::Ready | DelayState::Consumed | DelayState::Abandoned
            )
        {
            ensure_work_cancelled(service, &current)?;
            return complete(service, run);
        }
        if current.state != DelayState::Pending
            || current.run_unit == run.invocation.run_unit_id.as_str()
            || service.durable_tick()? >= current.expiration_tick
        {
            return Err(not_found());
        }
        authorize_cancel(
            service,
            run,
            selected_transaction.unwrap_or(&current.transaction),
        )?;
        let mut next = current.clone();
        next.state = DelayState::Ready;
        next.cancel_effect_key = Some(effect_key.into());
        next.cancel_request_digest = Some(request_digest);
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        match persist(
            service.store.as_ref(),
            &next,
            Some(current.version),
            service.limits,
        ) {
            Ok(()) => {
                ensure_work_cancelled(service, &next)?;
                return complete(service, run);
            }
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let delay_ids = list_records(service.store.as_ref(), service.limits)?
        .into_iter()
        .filter(|record| {
            record.run_unit == run.invocation.run_unit_id.as_str()
                && matches!(record.state, DelayState::Pending | DelayState::Ready)
        })
        .map(|record| record.delay_id)
        .collect::<Vec<_>>();
    for delay_id in delay_ids {
        abandon(service, &delay_id, run.invocation.run_unit_id.as_str())?;
    }
    Ok(())
}

fn abandon(service: &CicsService, delay_id: &str, run_unit: &str) -> Result<(), HostProblem> {
    for _ in 0..MAX_CAS_ATTEMPTS {
        let Some(current) = load_record(service.store.as_ref(), delay_id, service.limits)? else {
            return Ok(());
        };
        if current.run_unit != run_unit
            || !matches!(current.state, DelayState::Pending | DelayState::Ready)
        {
            return Ok(());
        }
        let mut next = current.clone();
        next.state = DelayState::Abandoned;
        next.consumer_effect_key = None;
        next.consumer_request_digest = None;
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        match persist(
            service.store.as_ref(),
            &next,
            Some(current.version),
            service.limits,
        ) {
            Ok(()) => return ensure_work_cancelled(service, &next),
            Err(HostProblem::IdempotencyConflict) => continue,
            Err(problem) => return Err(problem),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn authorize_cancel(
    service: &CicsService,
    run: &mut Run,
    transaction: &str,
) -> Result<(), HostProblem> {
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
                response2: 0,
            },
            other => other,
        })
}

fn new_record(
    delay_id: &str,
    request_id: Option<String>,
    run: &Run,
    interval: i64,
    expiration_tick: u64,
    producer_effect_key: &str,
    producer_request_digest: [u8; 32],
    version: u64,
) -> Result<DelayRecord, HostProblem> {
    let identity = format!(
        "{:x}",
        Sha256::digest([producer_effect_key.as_bytes(), &producer_request_digest].concat())
    );
    Ok(DelayRecord {
        delay_id: delay_id.into(),
        request_id,
        run_unit: run.invocation.run_unit_id.as_str().into(),
        transaction: run.transaction.clone(),
        interval,
        expiration_tick,
        work_id: format!("cics-delay:{}", &identity[..32]),
        priority: run.invocation.priority,
        state: DelayState::Pending,
        producer_effect_key: producer_effect_key.into(),
        producer_request_digest,
        consumer_effect_key: None,
        consumer_request_digest: None,
        cancel_effect_key: None,
        cancel_request_digest: None,
        version,
    })
}

fn validate_context(
    record: &DelayRecord,
    run: &Run,
    interval: i64,
    request_id: Option<&str>,
) -> Result<(), HostProblem> {
    if record.run_unit != run.invocation.run_unit_id.as_str()
        || record.transaction != run.transaction
        || record.interval != interval
        || record.request_id.as_deref() != request_id
    {
        Err(HostProblem::IdempotencyConflict)
    } else {
        Ok(())
    }
}

fn consume(
    service: &CicsService,
    current: &DelayRecord,
    effect_key: &str,
    request_digest: [u8; 32],
) -> Result<(), HostProblem> {
    let mut next = current.clone();
    next.state = DelayState::Consumed;
    next.consumer_effect_key = Some(effect_key.into());
    next.consumer_request_digest = Some(request_digest);
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    persist(
        service.store.as_ref(),
        &next,
        Some(current.version),
        service.limits,
    )
}

fn enqueue_work(service: &CicsService, record: &DelayRecord) -> Result<(), HostProblem> {
    let work = work_record(record)?;
    let work_store = service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    match work_store.enqueue(work.clone()) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict)
            if work_store
                .get_work(&work.work_id)
                .map_err(store_error)?
                .as_ref()
                .is_some_and(|existing| same_work(existing, &work)) =>
        {
            Ok(())
        }
        Err(problem) => Err(store_error(problem)),
    }
}

fn work_record(record: &DelayRecord) -> Result<WorkRecord, HostProblem> {
    let limits = InvocationLimits::default();
    Ok(WorkRecord {
        work_id: record.work_id.clone(),
        execution_id: ExecutionId::new(
            record.work_id.replacen("cics-delay:", "cics-delay-", 1),
            limits,
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
        required_selector: Selector::new("cics:delay", limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        required_generation: CICS_DELAY_WORK_GENERATION.into(),
        artifact: ArtifactRef::new("artifact:none", limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        state: WorkState::Queued,
        priority: record.priority,
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
        payload: record.delay_id.as_bytes().to_vec(),
    })
}

fn same_work(left: &WorkRecord, right: &WorkRecord) -> bool {
    left.work_id == right.work_id
        && left.execution_id == right.execution_id
        && left.required_selector == right.required_selector
        && left.required_generation == right.required_generation
        && left.artifact == right.artifact
        && left.priority == right.priority
        && left.max_attempts == right.max_attempts
        && left.available_tick == right.available_tick
        && left.deadline_tick == right.deadline_tick
        && left.payload == right.payload
}

fn load_record(
    store: &dyn ProviderStateStore,
    delay_id: &str,
    limits: CicsLimits,
) -> Result<Option<DelayRecord>, HostProblem> {
    store
        .get_provider_state(NAMESPACE, delay_id)
        .map_err(store_error)?
        .map(|record| decode(&record.payload, record.version, limits))
        .transpose()
}

fn list_records(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<Vec<DelayRecord>, HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    rows.into_iter()
        .map(|row| {
            let record = decode(&row.payload, row.version, limits)?;
            if row.key != record.delay_id {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(record)
        })
        .collect()
}

fn find_named_record(
    service: &CicsService,
    request_id: &str,
) -> Result<Option<DelayRecord>, HostProblem> {
    let mut matches = list_records(service.store.as_ref(), service.limits)?
        .into_iter()
        .filter(|record| record.request_id.as_deref() == Some(request_id));
    let found = matches.next();
    if matches.next().is_some() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(found)
}

fn ensure_name_available(
    service: &CicsService,
    request_id: Option<&str>,
    delay_id: &str,
) -> Result<(), HostProblem> {
    let Some(request_id) = request_id else {
        return Ok(());
    };
    if service.lock()?.interval_records.contains_key(request_id)
        || list_records(service.store.as_ref(), service.limits)?
            .into_iter()
            .any(|record| {
                record.delay_id != delay_id && record.request_id.as_deref() == Some(request_id)
            })
    {
        Err(invalid_request())
    } else {
        Ok(())
    }
}

fn ensure_work_cancelled(service: &CicsService, record: &DelayRecord) -> Result<(), HostProblem> {
    let work_store = service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let work = work_store
        .get_work(&record.work_id)
        .map_err(store_error)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    if !same_work(&work, &work_record(record)?) {
        return Err(HostProblem::InfrastructureFailure);
    }
    match work.state {
        WorkState::Queued | WorkState::Claimed => work_store
            .request_cancellation(&work.work_id)
            .map(|_| ())
            .map_err(store_error),
        WorkState::Completed | WorkState::Cancelled | WorkState::DeadLetter => Ok(()),
    }
}

fn persist(
    store: &dyn ProviderStateStore,
    record: &DelayRecord,
    expected: Option<u64>,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    validate(record, limits)?;
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: record.delay_id.clone(),
                version: record.version,
                payload: encode(record, limits)?,
            },
            expected,
        )
        .map_err(store_error)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "DELAY.ID",
        "HOURS",
        "INTERVAL",
        "MINUTES",
        "OPTION.FOR",
        "OPTION.NOHANDLE",
        "OPTION.UNTIL",
        "REQID",
        "RESP",
        "RESP2",
        "SECONDS",
    ];
    let components = ["HOURS", "MINUTES", "SECONDS"]
        .into_iter()
        .any(|name| request.arguments.contains_key(name));
    let modes = usize::from(request.arguments.contains_key("OPTION.FOR"))
        + usize::from(request.arguments.contains_key("OPTION.UNTIL"));
    let schedules = usize::from(request.arguments.contains_key("INTERVAL")) + modes;
    if schedules > 1
        || components != (modes == 1)
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "DELAY.ID" => value.schema() != "mainframe-env.cics.delay-id@1",
                    "HOURS" | "INTERVAL" | "MINUTES" | "SECONDS" => {
                        value.schema() != "mainframe-env.cics.decimal@1"
                    }
                    "REQID" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
                    "OPTION.FOR" | "OPTION.NOHANDLE" | "OPTION.UNTIL" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    _ => value.schema() != "mainframe-env.cics.argument@1",
                }
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn delay_id(request: &CicsRequest, run: &Run) -> Result<String, HostProblem> {
    let value = request
        .arguments
        .get("DELAY.ID")
        .ok_or(HostProblem::Malformed)?;
    let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
    let suffix = text
        .strip_prefix(run.invocation.run_unit_id.as_str())
        .and_then(|value| value.strip_prefix(':'));
    if text.len() > 256
        || !suffix.is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(text.into())
}

fn validate(record: &DelayRecord, limits: CicsLimits) -> Result<(), HostProblem> {
    if record.delay_id.is_empty()
        || record.delay_id.len() > 256
        || record.delay_id.chars().any(char::is_control)
        || record
            .request_id
            .as_deref()
            .is_some_and(|value| !valid_name(value, 8))
        || record.run_unit.is_empty()
        || !valid_name(&record.transaction, 4)
        || record.interval <= 0
        || record.expiration_tick == 0
        || record.work_id.len() > 128
        || record.version == 0
        || record.retained_bytes() > limits.max_queue_bytes
    {
        return Err(HostProblem::Malformed);
    }
    checked_key(&record.producer_effect_key)?;
    let consumer_valid = match (
        record.state,
        record.consumer_effect_key.as_deref(),
        record.consumer_request_digest,
    ) {
        (DelayState::Consumed, Some(key), Some(_)) => checked_key(key).is_ok(),
        (DelayState::Consumed, _, _) => false,
        (_, None, None) => true,
        _ => false,
    };
    let cancel_valid = match (
        record.cancel_effect_key.as_deref(),
        record.cancel_request_digest,
    ) {
        (Some(key), Some(_)) => {
            record.request_id.is_some()
                && matches!(
                    record.state,
                    DelayState::Ready | DelayState::Consumed | DelayState::Abandoned
                )
                && checked_key(key).is_ok()
        }
        (None, None) => true,
        _ => false,
    };
    if consumer_valid && cancel_valid {
        Ok(())
    } else {
        Err(HostProblem::Malformed)
    }
}

fn checked_key(value: &str) -> Result<(), HostProblem> {
    IdempotencyKey::new(value, InvocationLimits::default())
        .map(|_| ())
        .map_err(|_| HostProblem::Malformed)
}

impl DelayRecord {
    fn retained_bytes(&self) -> usize {
        self.delay_id.len()
            + self.request_id.as_ref().map_or(0, String::len)
            + self.run_unit.len()
            + self.transaction.len()
            + self.work_id.len()
            + self.producer_effect_key.len()
            + self.consumer_effect_key.as_ref().map_or(0, String::len)
            + self.cancel_effect_key.as_ref().map_or(0, String::len)
            + 192
    }
}

fn encode(record: &DelayRecord, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
    validate(record, limits)?;
    let mut out = MAGIC_V2.to_vec();
    field(&mut out, record.delay_id.as_bytes())?;
    optional_field(&mut out, record.request_id.as_deref())?;
    field(&mut out, record.run_unit.as_bytes())?;
    field(&mut out, record.transaction.as_bytes())?;
    out.extend_from_slice(&record.interval.to_be_bytes());
    out.extend_from_slice(&record.expiration_tick.to_be_bytes());
    field(&mut out, record.work_id.as_bytes())?;
    out.push(record.priority);
    out.push(match record.state {
        DelayState::Pending => 1,
        DelayState::Ready => 2,
        DelayState::Consumed => 3,
        DelayState::Abandoned => 4,
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
    optional_field(&mut out, record.cancel_effect_key.as_deref())?;
    optional_digest(&mut out, record.cancel_request_digest);
    Ok(out)
}

fn decode(payload: &[u8], version: u64, limits: CicsLimits) -> Result<DelayRecord, HostProblem> {
    let mut reader = Reader::new(payload);
    let magic = reader.take(MAGIC_V2.len())?;
    let record = if magic == MAGIC_V1 {
        decode_v1(&mut reader, version)?
    } else if magic == MAGIC_V2 {
        decode_v2(&mut reader, version)?
    } else {
        return Err(HostProblem::InfrastructureFailure);
    };
    if !reader.done() {
        return Err(HostProblem::InfrastructureFailure);
    }
    validate(&record, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(record)
}

fn decode_v1(reader: &mut Reader<'_>, version: u64) -> Result<DelayRecord, HostProblem> {
    Ok(DelayRecord {
        delay_id: reader.text(256)?,
        request_id: None,
        run_unit: reader.text(256)?,
        transaction: reader.text(4)?,
        interval: reader.i64()?,
        expiration_tick: reader.u64()?,
        work_id: reader.text(128)?,
        priority: reader.byte()?,
        state: match reader.byte()? {
            1 => DelayState::Pending,
            2 => DelayState::Ready,
            3 => DelayState::Consumed,
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
        cancel_effect_key: None,
        cancel_request_digest: None,
        version,
    })
}

fn decode_v2(reader: &mut Reader<'_>, version: u64) -> Result<DelayRecord, HostProblem> {
    Ok(DelayRecord {
        delay_id: reader.text(256)?,
        request_id: reader.optional_text(8)?,
        run_unit: reader.text(256)?,
        transaction: reader.text(4)?,
        interval: reader.i64()?,
        expiration_tick: reader.u64()?,
        work_id: reader.text(128)?,
        priority: reader.byte()?,
        state: match reader.byte()? {
            1 => DelayState::Pending,
            2 => DelayState::Ready,
            3 => DelayState::Consumed,
            4 => DelayState::Abandoned,
            _ => return Err(HostProblem::InfrastructureFailure),
        },
        producer_effect_key: reader.text(InvocationLimits::default().max_binding_bytes)?,
        producer_request_digest: reader.digest()?,
        consumer_effect_key: reader.optional_text(InvocationLimits::default().max_binding_bytes)?,
        consumer_request_digest: reader.optional_digest()?,
        cancel_effect_key: reader.optional_text(InvocationLimits::default().max_binding_bytes)?,
        cancel_request_digest: reader.optional_digest()?,
        version,
    })
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

fn optional_digest(out: &mut Vec<u8>, value: Option<[u8; 32]>) {
    match value {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(&digest);
        }
        None => out.push(0),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
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

    fn i64(&mut self) -> Result<i64, HostProblem> {
        Ok(i64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
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

    fn optional_digest(&mut self) -> Result<Option<[u8; 32]>, HostProblem> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.digest().map(Some),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

fn suspended(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Suspended,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn complete(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
    complete_with_response2(service, run, 0)
}

fn complete_with_response2(
    service: &CicsService,
    run: &Run,
    response2: i32,
) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        response2,
        None,
        None,
        Vec::new(),
    )
}

fn cancellation_response2(record: &DelayRecord) -> i32 {
    if record.cancel_effect_key.is_some() {
        23
    } else {
        0
    }
}

fn invalid_request() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 0,
    }
}

fn not_found() -> HostProblem {
    HostProblem::Condition {
        name: "NOTFND".into(),
        response: 13,
        response2: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> DelayRecord {
        DelayRecord {
            delay_id: "run:1".into(),
            request_id: None,
            run_unit: "run".into(),
            transaction: "MENU".into(),
            interval: 1,
            expiration_tick: 1_000,
            work_id: "cics-delay:0123456789abcdef0123456789abcdef".into(),
            priority: 0,
            state: DelayState::Pending,
            producer_effect_key: "outer-1".into(),
            producer_request_digest: [1; 32],
            consumer_effect_key: None,
            consumer_request_digest: None,
            cancel_effect_key: None,
            cancel_request_digest: None,
            version: 1,
        }
    }

    #[test]
    fn legacy_delay_rows_remain_readable_and_current_rows_roundtrip() {
        let original = record();
        let mut legacy = MAGIC_V1.to_vec();
        field(&mut legacy, original.delay_id.as_bytes()).unwrap();
        field(&mut legacy, original.run_unit.as_bytes()).unwrap();
        field(&mut legacy, original.transaction.as_bytes()).unwrap();
        legacy.extend_from_slice(&original.interval.to_be_bytes());
        legacy.extend_from_slice(&original.expiration_tick.to_be_bytes());
        field(&mut legacy, original.work_id.as_bytes()).unwrap();
        legacy.extend_from_slice(&[original.priority, 1]);
        field(&mut legacy, original.producer_effect_key.as_bytes()).unwrap();
        legacy.extend_from_slice(&original.producer_request_digest);
        legacy.extend_from_slice(&[0, 0]);
        assert_eq!(decode(&legacy, 1, CicsLimits::default()).unwrap(), original);

        let mut current = record();
        current.request_id = Some("WAIT0001".into());
        current.state = DelayState::Consumed;
        current.consumer_effect_key = Some("outer-2".into());
        current.consumer_request_digest = Some([2; 32]);
        current.cancel_effect_key = Some("outer-3".into());
        current.cancel_request_digest = Some([3; 32]);
        let encoded = encode(&current, CicsLimits::default()).unwrap();
        assert_eq!(
            decode(&encoded, current.version, CicsLimits::default()).unwrap(),
            current
        );
    }
}
