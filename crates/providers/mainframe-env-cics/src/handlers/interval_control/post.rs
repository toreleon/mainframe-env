//! Durable task-owned timer-event control areas for EXEC CICS POST.

use super::{clock_millis_since_midnight, name_argument, optional_decimal};
use crate::service::{CicsLimits, CicsService, Run, bounded, store_error};
use crate::{CicsIntervalMode, CicsIntervalTime};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, ExecutionId, InvocationLimits, Selector,
};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, ClockRequest,
    HostProblem, HostRequest, HostResult, canonical_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const NAMESPACE: &str = "cics-post-v1";
const SCHEMA: &str = "mainframe-env.cics-post@1";
const MAX_CAS_ATTEMPTS: usize = 8;
/// Durable work-generation identity used to post one task timer on expiry.
pub const CICS_POST_WORK_GENERATION: &str = "cics-post-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PostState {
    Pending,
    Ready,
    Superseded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PostRecord {
    schema: String,
    run_unit: String,
    execution: String,
    session: String,
    transaction: String,
    request_id: String,
    address: [u8; 4],
    expiration_tick: u64,
    work_id: String,
    producer_effect_key: String,
    producer_request_digest: [u8; 32],
    state: PostState,
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
    for row in rows {
        retained = retained
            .checked_add(row.payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if retained > limits.max_queue_bytes {
            return Err(HostProblem::ResourceExhausted);
        }
        decode(&row)?;
    }
    Ok(())
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let supplied_id = request
        .arguments
        .get("REQID")
        .map(|_| name_argument(request, "REQID", 8))
        .transpose()?;
    let request_id = supplied_id.clone().unwrap_or_else(|| {
        let hash = Sha256::digest(
            [
                mutation.idempotency_key.as_str().as_bytes(),
                &request_digest,
            ]
            .concat(),
        );
        format!(
            "{:08X}",
            u32::from_be_bytes(hash[..4].try_into().expect("digest prefix"))
        )
    });
    let schedule = schedule(request).map_err(|error| HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: error.start_response2(),
    })?;
    let local_millis = if schedule.mode() == CicsIntervalMode::Absolute {
        let timestamp = match service.nested(run, HostRequest::Clock(ClockRequest::UtcTimestamp))? {
            HostResult::Clock(value) => value,
            _ => return Err(HostProblem::ProviderFailure),
        };
        clock_millis_since_midnight(&timestamp)?
    } else {
        0
    };
    let now_tick = service.durable_tick()?;
    let delay = schedule
        .delay_millis(local_millis)
        .ok_or(HostProblem::ResourceExhausted)?;
    if schedule.mode() == CicsIntervalMode::Absolute && delay == 0 {
        return Err(HostProblem::Condition {
            name: "EXPIRED".into(),
            response: 31,
            response2: 0,
        });
    }
    let expiration_tick = now_tick
        .checked_add(delay)
        .ok_or(HostProblem::ResourceExhausted)?;
    let address: [u8; 4] = request.arguments["POST.SET.ADDRESS"]
        .bytes()
        .try_into()
        .map_err(|_| HostProblem::Malformed)?;
    if address == [0; 4] {
        return Err(HostProblem::Malformed);
    }
    let key = run.invocation.run_unit_id.as_str();
    let record = for_record(
        service,
        run,
        request_id,
        address,
        expiration_tick,
        mutation.idempotency_key.as_str(),
        request_digest,
    )?;
    ensure_unique_name(service, &record)?;
    for _ in 0..MAX_CAS_ATTEMPTS {
        let current = service
            .store
            .get_provider_state(NAMESPACE, key)
            .map_err(store_error)?;
        if let Some(current) = &current {
            let previous = decode(current)?;
            if previous.producer_effect_key == record.producer_effect_key
                && previous.producer_request_digest == record.producer_request_digest
            {
                enqueue_work(service, &previous, run.invocation.priority)?;
                return complete(service, run, &previous, supplied_id.is_none());
            }
        }
        let mut next = record.clone();
        next.version = current
            .as_ref()
            .map_or(1, |row| row.version.saturating_add(1));
        if next.version == 0 {
            return Err(HostProblem::ResourceExhausted);
        }
        let persisted = service.store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: key.into(),
                version: next.version,
                payload: encode(&next)?,
            },
            current.as_ref().map(|row| row.version),
        );
        match persisted {
            Ok(()) => {
                enqueue_work(service, &next, run.invocation.priority)?;
                return complete(service, run, &next, supplied_id.is_none());
            }
            Err(StoreError::AlreadyExists | StoreError::Conflict | StoreError::NotFound) => {
                continue;
            }
            Err(error) => return Err(store_error(error)),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "HOURS",
        "INTERVAL",
        "MINUTES",
        "OPTION.AFTER",
        "OPTION.AT",
        "OPTION.NOHANDLE",
        "POST.SET.ADDRESS",
        "REQID",
        "RESP",
        "RESP2",
        "SECONDS",
        "SET",
        "SET.MAXLENGTH",
        "TIME",
    ];
    if request.operation != CicsOperation::Post
        || !request.arguments.contains_key("SET")
        || !request.arguments.contains_key("SET.MAXLENGTH")
        || !request.arguments.contains_key("POST.SET.ADDRESS")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "HOURS" | "INTERVAL" | "MINUTES" | "SECONDS" | "SET.MAXLENGTH" | "TIME" => {
                        value.schema() != "mainframe-env.cics.decimal@1"
                    }
                    "POST.SET.ADDRESS" => {
                        value.schema() != "mainframe-env.cics.virtual-address@1"
                            || value.bytes().len() != 4
                    }
                    "OPTION.AFTER" | "OPTION.AT" | "OPTION.NOHANDLE" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "SET" | "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                    "REQID" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
                    _ => true,
                }
        })
    {
        return Err(HostProblem::Malformed);
    }
    let maximum = optional_decimal(request, "SET.MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
    if maximum < 4 {
        return Err(HostProblem::ResourceExhausted);
    }
    let after = request.arguments.contains_key("OPTION.AFTER");
    let at = request.arguments.contains_key("OPTION.AT");
    let units = ["HOURS", "MINUTES", "SECONDS"]
        .into_iter()
        .any(|name| request.arguments.contains_key(name));
    let schedules = usize::from(request.arguments.contains_key("INTERVAL"))
        + usize::from(request.arguments.contains_key("TIME"))
        + usize::from(after)
        + usize::from(at);
    if schedules > 1 || (after || at) != units {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn schedule(request: &CicsRequest) -> Result<CicsIntervalTime, crate::CicsIntervalError> {
    let component = |name| {
        optional_decimal(request, name).map_err(|_| crate::CicsIntervalError::InvalidRequest)
    };
    if let Some(value) = component("INTERVAL")? {
        CicsIntervalTime::from_hhmmss(CicsIntervalMode::Relative, value)
    } else if let Some(value) = component("TIME")? {
        CicsIntervalTime::from_hhmmss(CicsIntervalMode::Absolute, value)
    } else if request.arguments.contains_key("OPTION.AFTER") {
        CicsIntervalTime::from_components(
            CicsIntervalMode::Relative,
            component("HOURS")?,
            component("MINUTES")?,
            component("SECONDS")?,
        )
    } else if request.arguments.contains_key("OPTION.AT") {
        CicsIntervalTime::from_components(
            CicsIntervalMode::Absolute,
            component("HOURS")?,
            component("MINUTES")?,
            component("SECONDS")?,
        )
    } else {
        Ok(CicsIntervalTime::immediate())
    }
}

fn for_record(
    service: &CicsService,
    run: &Run,
    request_id: String,
    address: [u8; 4],
    expiration_tick: u64,
    producer_effect_key: &str,
    producer_request_digest: [u8; 32],
) -> Result<PostRecord, HostProblem> {
    let hash = Sha256::digest([producer_effect_key.as_bytes(), &producer_request_digest].concat());
    let work_id = format!("cics-post:{:x}", hash);
    if service.work_store.is_none() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(PostRecord {
        schema: SCHEMA.into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        execution: run.invocation.execution_id.as_str().into(),
        session: run.session.clone(),
        transaction: run.transaction.clone(),
        request_id,
        address,
        expiration_tick,
        work_id,
        producer_effect_key: producer_effect_key.into(),
        producer_request_digest,
        state: PostState::Pending,
        version: 1,
    })
}

fn complete(
    service: &CicsService,
    run: &Run,
    record: &PostRecord,
    generated: bool,
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
    response.outputs.remove("POST.EVENT");
    response.outputs.insert("SET".into(), bounded(vec![0; 4])?);
    if generated {
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

fn ensure_unique_name(service: &CicsService, record: &PostRecord) -> Result<(), HostProblem> {
    let rows = service
        .store
        .list_provider_state(
            NAMESPACE,
            service.limits.max_queue_records.saturating_add(1),
        )
        .map_err(store_error)?;
    if rows.len() > service.limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in rows {
        let current = decode(&row)?;
        if current.run_unit != record.run_unit
            && current.state != PostState::Superseded
            && current.request_id == record.request_id
        {
            return Err(HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: 0,
            });
        }
    }
    Ok(())
}

fn encode(record: &PostRecord) -> Result<Vec<u8>, HostProblem> {
    validate_record(record)?;
    serde_json::to_vec(record).map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode(row: &ProviderStateRecord) -> Result<PostRecord, HostProblem> {
    if row.namespace != NAMESPACE || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record: PostRecord =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    validate_record(&record)?;
    if record.version != row.version
        || record.run_unit != row.key
        || encode(&record)? != row.payload
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(record)
}

fn validate_record(record: &PostRecord) -> Result<(), HostProblem> {
    if record.schema != SCHEMA
        || record.run_unit.is_empty()
        || record.execution.is_empty()
        || record.session.is_empty()
        || record.transaction.is_empty()
        || !matches!(record.request_id.len(), 1..=8)
        || !record
            .request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
        || record.address == [0; 4]
        || record.expiration_tick == 0
        || !record.work_id.starts_with("cics-post:")
        || record.producer_effect_key.is_empty()
        || record.version == 0
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn enqueue_work(
    service: &CicsService,
    record: &PostRecord,
    priority: u8,
) -> Result<(), HostProblem> {
    let work_store = service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let limits = InvocationLimits::default();
    let work = WorkRecord {
        work_id: record.work_id.clone(),
        execution_id: ExecutionId::new(format!("cics-post-{}", &record.work_id[10..34]), limits)
            .map_err(|_| HostProblem::ResourceExhausted)?,
        required_selector: Selector::new("cics:post", limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        required_generation: CICS_POST_WORK_GENERATION.into(),
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
        payload: format!("{}:0", record.run_unit).into_bytes(),
    };
    match work_store.enqueue(work.clone()) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict)
            if work_store
                .get_work(&work.work_id)
                .map_err(store_error)?
                .as_ref()
                .is_some_and(|existing| {
                    existing.work_id == work.work_id
                        && existing.execution_id == work.execution_id
                        && existing.required_generation == work.required_generation
                        && existing.required_selector == work.required_selector
                        && existing.available_tick == work.available_tick
                        && existing.payload == work.payload
                }) =>
        {
            Ok(())
        }
        Err(error) => Err(store_error(error)),
    }
}

impl CicsService {
    /// Promote a claimed timer work item, waking any task already waiting on its ECB.
    pub fn promote_post_work(&self, work: &WorkRecord, now_tick: u64) -> Result<bool, HostProblem> {
        if work.required_generation != CICS_POST_WORK_GENERATION
            || work.required_selector.as_str() != "cics:post"
            || work.artifact.as_str() != "artifact:none"
            || work.state != WorkState::Claimed
            || work.lease_id.is_none()
            || work.lease_epoch == 0
            || now_tick < work.available_tick
        {
            return Err(HostProblem::Malformed);
        }
        let payload = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let run_unit = payload
            .strip_suffix(":0")
            .filter(|value| !value.is_empty())
            .ok_or(HostProblem::Malformed)?;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(row) = self
                .store
                .get_provider_state(NAMESPACE, run_unit)
                .map_err(store_error)?
            else {
                return Ok(false);
            };
            let mut record = decode(&row)?;
            if record.work_id != work.work_id
                || record.expiration_tick != work.available_tick
                || record.state == PostState::Superseded
            {
                return Ok(false);
            }
            if record.state == PostState::Ready {
                return super::super::task_wait::post_timer_event(
                    self,
                    &record.run_unit,
                    record.address,
                );
            }
            record.state = PostState::Ready;
            record.version = record
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: run_unit.into(),
                    version: record.version,
                    payload: encode(&record)?,
                },
                Some(row.version),
            ) {
                Ok(()) => {
                    return super::super::task_wait::post_timer_event(
                        self,
                        &record.run_unit,
                        record.address,
                    );
                }
                Err(StoreError::Conflict | StoreError::NotFound) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }
}

pub(in crate::service) fn ready_event(
    service: &CicsService,
    run: &Run,
) -> Result<Option<Vec<u8>>, HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    let Some(row) = service
        .store
        .get_provider_state(NAMESPACE, key)
        .map_err(store_error)?
    else {
        return Ok(None);
    };
    let record = decode(&row)?;
    if record.execution != run.invocation.execution_id.as_str() || record.session != run.session {
        return Err(HostProblem::IdempotencyConflict);
    }
    if record.state != PostState::Ready {
        return Ok(None);
    }
    let mut value = record.address.to_vec();
    value.extend_from_slice(&[0x40, 0, 0x80, 0]);
    Ok(Some(value))
}

pub(super) fn supersede_for_run(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    for _ in 0..MAX_CAS_ATTEMPTS {
        let Some(row) = service
            .store
            .get_provider_state(NAMESPACE, key)
            .map_err(store_error)?
        else {
            return Ok(());
        };
        let mut record = decode(&row)?;
        if record.execution != run.invocation.execution_id.as_str() {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.state == PostState::Superseded {
            return Ok(());
        }
        record.state = PostState::Superseded;
        record.version = record
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        match service.store.put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: key.into(),
                version: record.version,
                payload: encode(&record)?,
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

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    let Some(row) = service
        .store
        .get_provider_state(NAMESPACE, key)
        .map_err(store_error)?
    else {
        return Ok(());
    };
    let record = decode(&row)?;
    if record.execution != run.invocation.execution_id.as_str() {
        return Err(HostProblem::IdempotencyConflict);
    }
    match service
        .store
        .delete_provider_state(NAMESPACE, key, row.version)
    {
        Ok(()) | Err(StoreError::NotFound) => Ok(()),
        Err(error) => Err(store_error(error)),
    }
}

pub(super) fn cancel_named(
    service: &CicsService,
    run: &mut Run,
    request_id: &str,
    selected_transaction: Option<&str>,
    effect_key: &str,
    _request_digest: [u8; 32],
) -> Result<Option<CicsResponse>, HostProblem> {
    let rows = service
        .store
        .list_provider_state(
            NAMESPACE,
            service.limits.max_queue_records.saturating_add(1),
        )
        .map_err(store_error)?;
    if rows.len() > service.limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in rows {
        let record = decode(&row)?;
        if record.request_id != request_id || record.state == PostState::Superseded {
            continue;
        }
        if record.state == PostState::Ready || service.durable_tick()? >= record.expiration_tick {
            return Err(HostProblem::Condition {
                name: "NOTFND".into(),
                response: 13,
                response2: 0,
            });
        }
        let transaction = selected_transaction.unwrap_or(&record.transaction);
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
            })?;
        let mut next = record;
        next.state = PostState::Ready;
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        service
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: row.key,
                    version: next.version,
                    payload: encode(&next)?,
                },
                Some(row.version),
            )
            .map_err(store_error)?;
        super::super::task_wait::post_timer_event(service, &next.run_unit, next.address)?;
        let _ = effect_key;
        return service
            .response(
                run,
                CicsDisposition::Complete,
                "NORMAL",
                0,
                0,
                None,
                None,
                Vec::new(),
            )
            .map(Some);
    }
    Ok(None)
}
