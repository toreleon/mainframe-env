use super::{optional_decimal, valid_name};
use crate::service::{CicsLimits, CicsService, Run, field, store_error};
use crate::{CicsIntervalMode, CicsIntervalTime};
use mainframe_env_execution_api::{
    ArtifactRef, ExecutionId, IdempotencyKey, InvocationLimits, Selector,
};
use mainframe_env_host_api::{
    CicsDisposition, CicsRequest, CicsResponse, HostProblem, HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, StoreError, WorkRecord, WorkState,
};
use sha2::{Digest, Sha256};

const NAMESPACE: &str = "cics-delay-v1";
const MAGIC: &[u8; 7] = b"MECDLY1";

/// Durable work-generation identity used by positive DELAY requests.
pub const CICS_DELAY_WORK_GENERATION: &str = "cics-delay-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DelayState {
    Pending,
    Ready,
    Consumed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DelayRecord {
    delay_id: String,
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
        let record = decode(&row.payload, row.version, limits)?;
        retained = retained
            .checked_add(record.retained_bytes())
            .ok_or(HostProblem::ResourceExhausted)?;
        if row.key != record.delay_id || retained > limits.max_queue_bytes {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(())
}

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let interval = optional_decimal(request, "INTERVAL")?.unwrap_or(0);
    let time =
        CicsIntervalTime::from_hhmmss(CicsIntervalMode::Relative, interval).map_err(|problem| {
            HostProblem::Condition {
                name: "INVREQ".into(),
                response: 16,
                response2: problem.delay_response2(),
            }
        })?;
    if interval == 0 {
        return complete(service, run);
    }
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
            let now_tick = service.durable_tick()?;
            let record = new_record(
                &delay_id,
                run,
                interval,
                time.deadline_tick(now_tick, 0)
                    .ok_or(HostProblem::ResourceExhausted)?,
                mutation.idempotency_key.as_str(),
                digest,
                1,
            )?;
            persist(service.store.as_ref(), &record, None, service.limits)?;
            enqueue_work(service, &record)?;
            suspended(service, run)
        }
        Some(record) => {
            validate_context(&record, run, interval)?;
            match record.state {
                DelayState::Pending => {
                    enqueue_work(service, &record)?;
                    suspended(service, run)
                }
                DelayState::Ready => {
                    consume(service, &record, mutation.idempotency_key.as_str(), digest)?;
                    complete(service, run)
                }
                DelayState::Consumed
                    if record.consumer_effect_key.as_deref()
                        == Some(mutation.idempotency_key.as_str())
                        && record.consumer_request_digest == Some(digest) =>
                {
                    complete(service, run)
                }
                DelayState::Consumed => {
                    let now_tick = service.durable_tick()?;
                    let next = new_record(
                        &delay_id,
                        run,
                        interval,
                        time.deadline_tick(now_tick, 0)
                            .ok_or(HostProblem::ResourceExhausted)?,
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
            }
        }
    }
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
        if matches!(record.state, DelayState::Ready | DelayState::Consumed) {
            return Ok(());
        }
        record.state = DelayState::Ready;
        let expected = record.version;
        record.version = record
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        persist(self.store.as_ref(), &record, Some(expected), self.limits)
    }
}

fn new_record(
    delay_id: &str,
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
        version,
    })
}

fn validate_context(record: &DelayRecord, run: &Run, interval: i64) -> Result<(), HostProblem> {
    if record.run_unit != run.invocation.run_unit_id.as_str()
        || record.transaction != run.transaction
        || record.interval != interval
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
    let limits = InvocationLimits::default();
    let work = WorkRecord {
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
    };
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
    const ALLOWED: &[&str] = &["DELAY.ID", "INTERVAL", "OPTION.NOHANDLE", "RESP", "RESP2"];
    if request.arguments.iter().any(|(name, value)| {
        !ALLOWED.contains(&name.as_str())
            || match name.as_str() {
                "DELAY.ID" => value.schema() != "mainframe-env.cics.delay-id@1",
                "INTERVAL" => value.schema() != "mainframe-env.cics.decimal@1",
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => value.schema() != "mainframe-env.cics.argument@1",
            }
    }) {
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
    match (
        record.state,
        record.consumer_effect_key.as_deref(),
        record.consumer_request_digest,
    ) {
        (DelayState::Consumed, Some(key), Some(_)) => checked_key(key),
        (DelayState::Consumed, _, _) => Err(HostProblem::Malformed),
        (_, None, None) => Ok(()),
        _ => Err(HostProblem::Malformed),
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
            + self.run_unit.len()
            + self.transaction.len()
            + self.work_id.len()
            + self.producer_effect_key.len()
            + self.consumer_effect_key.as_ref().map_or(0, String::len)
            + 128
    }
}

fn encode(record: &DelayRecord, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
    validate(record, limits)?;
    let mut out = MAGIC.to_vec();
    field(&mut out, record.delay_id.as_bytes())?;
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

fn decode(payload: &[u8], version: u64, limits: CicsLimits) -> Result<DelayRecord, HostProblem> {
    let mut reader = Reader::new(payload);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record = DelayRecord {
        delay_id: reader.text(256)?,
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
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}
