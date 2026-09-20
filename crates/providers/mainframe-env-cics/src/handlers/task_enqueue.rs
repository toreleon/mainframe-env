use super::super::{
    CicsEffectReplay, CicsService, Run, cics_effect_replay_binding_digest,
    encode_cics_effect_replay, store_error,
};
use mainframe_env_host_api::{
    CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, HostResult, canonical_request_digest, canonical_result_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

mod model;
pub use model::CicsEnqueueModelDefinition;
pub(crate) use model::load_enqueue_models;
use model::{enqueue_model_matches, model_character, validate_active_catalog};

const LOCK_NAMESPACE: &str = "cics-enqueue-v1";
const CATALOG_NAMESPACE: &str = "cics-enqueue-catalog-v1";
const CATALOG_KEY: &str = "locks";
const LOCK_MAGIC: &[u8; 8] = b"MECENQ02";
const SCOPED_LOCK_MAGIC: &[u8; 8] = b"MECENQ03";
const CATALOG_MAGIC: &[u8; 8] = b"MECENQC1";
const MAX_CAS_ATTEMPTS: usize = 16;
const TASK_CVDA: i64 = 233;
const UOW_CVDA: i64 = 246;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lifetime {
    Uow,
    Task,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LockRecord {
    scope: LockScope,
    owner_execution: String,
    owner_run_unit: String,
    uow_count: u32,
    task_count: u32,
    grant_pending: bool,
    waiters: Vec<Waiter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LockScope {
    LegacyLocal,
    Region { applid: String, sysid: String },
    Global(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Waiter {
    owner_execution: String,
    owner_run_unit: String,
    lifetime: Lifetime,
}

struct EnqueueIdentity {
    resource_key: String,
    lifetime: Lifetime,
    scope: LockScope,
    disabled: bool,
}

enum ApplyError {
    Store(StoreError),
    Problem(HostProblem),
}

impl From<StoreError> for ApplyError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<HostProblem> for ApplyError {
    fn from(problem: HostProblem) -> Self {
        Self::Problem(problem)
    }
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if !matches!(request.operation, CicsOperation::Deq | CicsOperation::Enq) {
        return Err(HostProblem::InfrastructureFailure);
    }
    request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    validate_request_shape(request)?;
    let models = service.lock()?.enqueue_models.clone();
    validate_active_catalog(service.store.as_ref(), &models)?;
    let models = models.values().cloned().collect::<Vec<_>>();
    let identity = request_identity(request, run, &models)?;
    if identity.disabled && request.operation == CicsOperation::Enq {
        super::release_task_state(service, run)?;
        return service.response(
            run,
            CicsDisposition::Abended,
            "ERROR",
            1,
            0,
            None,
            None,
            Vec::new(),
        );
    }
    for _ in 0..MAX_CAS_ATTEMPTS {
        let current = service
            .store
            .get_provider_state(LOCK_NAMESPACE, &identity.resource_key)
            .map_err(store_error)?;
        let applied = match request.operation {
            CicsOperation::Enq => acquire(
                service,
                run,
                request,
                retention_tick,
                &identity.resource_key,
                identity.lifetime,
                &identity.scope,
                current,
            ),
            CicsOperation::Deq => release(
                service,
                run,
                request,
                retention_tick,
                &identity.resource_key,
                identity.lifetime,
                &identity.scope,
                current,
            ),
            _ => unreachable!(),
        };
        match applied {
            Ok(response) => return Ok(response),
            Err(ApplyError::Store(StoreError::AlreadyExists | StoreError::Conflict)) => continue,
            Err(ApplyError::Store(error)) => return Err(store_error(error)),
            Err(ApplyError::Problem(problem)) => return Err(problem),
        }
    }
    Err(HostProblem::ResourceExhausted)
}

fn validate_request_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = match request.operation {
        CicsOperation::Deq => [
            "LENGTH",
            "MAXLIFETIME",
            "OPTION.NOHANDLE",
            "OPTION.TASK",
            "OPTION.UOW",
            "RESOURCE",
            "RESP",
            "RESP2",
        ]
        .as_slice(),
        CicsOperation::Enq => [
            "LENGTH",
            "MAXLIFETIME",
            "OPTION.NOHANDLE",
            "OPTION.NOSUSPEND",
            "OPTION.TASK",
            "OPTION.UOW",
            "RESOURCE",
            "RESP",
            "RESP2",
        ]
        .as_slice(),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if request
        .arguments
        .keys()
        .any(|name| !allowed.contains(&name.as_str()))
        || request
            .arguments
            .iter()
            .any(|(name, value)| name.starts_with("OPTION.") && !value.bytes().is_empty())
        || ["LENGTH", "MAXLIFETIME"].iter().any(|name| {
            request
                .arguments
                .get(*name)
                .is_some_and(|value| value.schema() != "mainframe-env.cics.decimal@1")
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(crate) fn release_uow(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    release_lifetime(service, run, Lifetime::Uow)
}

pub(crate) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    release_lifetime(service, run, Lifetime::Task)?;
    release_lifetime(service, run, Lifetime::Uow)
}

pub(crate) fn validate_store(
    store: &dyn ProviderStateStore,
    limits: super::super::CicsLimits,
) -> Result<(), HostProblem> {
    let maximum = limits
        .max_queue_records
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = store
        .list_provider_state(LOCK_NAMESPACE, maximum)
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in &rows {
        if row.version == 0
            || row.key.len() != 64
            || !row
                .key
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        decode_lock(&row.payload, limits)?;
    }
    let catalog = store
        .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
        .map_err(store_error)?;
    let count = match catalog {
        Some(record) if record.version > 0 => decode_catalog(&record.payload)?,
        Some(_) => return Err(HostProblem::InfrastructureFailure),
        None if rows.is_empty() => 0,
        None => return Err(HostProblem::InfrastructureFailure),
    };
    if usize::try_from(count).ok() != Some(rows.len()) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn release_lifetime(
    service: &CicsService,
    run: &Run,
    lifetime: Lifetime,
) -> Result<(), HostProblem> {
    let maximum = service
        .limits
        .max_queue_records
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = service
        .store
        .list_provider_state(LOCK_NAMESPACE, maximum)
        .map_err(store_error)?;
    if rows.len() > service.limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in rows {
        let mut settled = false;
        for _ in 0..MAX_CAS_ATTEMPTS {
            let Some(current) = service
                .store
                .get_provider_state(LOCK_NAMESPACE, &row.key)
                .map_err(store_error)?
            else {
                settled = true;
                break;
            };
            let mut lock = decode_lock(&current.payload, service.limits)?;
            let owner_matches = lock.owner_execution == run.invocation.execution_id.as_str()
                && lock.owner_run_unit == run.invocation.run_unit_id.as_str();
            let waiter_count = lock.waiters.len();
            if lifetime == Lifetime::Task {
                lock.waiters.retain(|waiter| {
                    waiter.owner_execution != run.invocation.execution_id.as_str()
                        || waiter.owner_run_unit != run.invocation.run_unit_id.as_str()
                });
            }
            if owner_matches {
                match lifetime {
                    Lifetime::Uow => lock.uow_count = 0,
                    Lifetime::Task => lock.task_count = 0,
                }
            }
            if !owner_matches && waiter_count == lock.waiters.len() {
                settled = true;
                break;
            }
            if lock.uow_count == 0 && lock.task_count == 0 {
                promote_waiter(&mut lock);
            }
            let result = if lock.uow_count == 0 && lock.task_count == 0 {
                let catalog = catalog_write(service, false)?;
                service.store.mutate_provider_states_atomic(vec![
                    ProviderStateMutation::Delete {
                        namespace: LOCK_NAMESPACE.into(),
                        key: current.key,
                        expected_version: current.version,
                    },
                    ProviderStateMutation::Put(catalog),
                ])
            } else {
                service.store.put_provider_state(
                    ProviderStateRecord {
                        namespace: LOCK_NAMESPACE.into(),
                        key: current.key,
                        version: current
                            .version
                            .checked_add(1)
                            .ok_or(HostProblem::ResourceExhausted)?,
                        payload: encode_lock(&lock, service.limits)?,
                    },
                    Some(current.version),
                )
            };
            match result {
                Ok(()) => {
                    settled = true;
                    break;
                }
                Err(StoreError::Conflict | StoreError::NotFound) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        if !settled {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn acquire(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    resource_key: &str,
    lifetime: Lifetime,
    scope: &LockScope,
    current: Option<ProviderStateRecord>,
) -> Result<CicsResponse, ApplyError> {
    let owner_execution = run.invocation.execution_id.as_str();
    let owner_run_unit = run.invocation.run_unit_id.as_str();
    let (mut lock, expected_version) = match current {
        Some(record) => (
            decode_lock(&record.payload, service.limits)?,
            Some(record.version),
        ),
        None => (
            LockRecord {
                scope: scope.clone(),
                owner_execution: owner_execution.into(),
                owner_run_unit: owner_run_unit.into(),
                uow_count: 0,
                task_count: 0,
                grant_pending: false,
                waiters: Vec::new(),
            },
            None,
        ),
    };
    if &lock.scope != scope {
        return Err(HostProblem::InfrastructureFailure.into());
    }
    if lock.owner_execution != owner_execution || lock.owner_run_unit != owner_run_unit {
        let active_handle = matches!(request.condition_policy, CicsConditionPolicy::Default)
            && run.handlers.contains_key("ENQBUSY");
        if request.arguments.contains_key("OPTION.NOSUSPEND") || active_handle {
            let response = super::condition::respond(
                service,
                run,
                &request.condition_policy,
                HostProblem::Condition {
                    name: "ENQBUSY".into(),
                    response: 55,
                    response2: 0,
                },
            )?;
            service.store.put_provider_state(
                replay_write(run, request, retention_tick, &response)?.record,
                None,
            )?;
            return Ok(response);
        }
        let response = service.response(
            run,
            CicsDisposition::Suspended,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?;
        let matching_waiter = lock.waiters.iter().find(|waiter| {
            waiter.owner_execution == owner_execution && waiter.owner_run_unit == owner_run_unit
        });
        if let Some(waiter) = matching_waiter {
            if waiter.lifetime != lifetime {
                return Err(HostProblem::IdempotencyConflict.into());
            }
            service.store.put_provider_state(
                replay_write(run, request, retention_tick, &response)?.record,
                None,
            )?;
        } else {
            if lock.waiters.len() >= service.limits.max_runs {
                return Err(HostProblem::ResourceExhausted.into());
            }
            lock.waiters.push(Waiter {
                owner_execution: owner_execution.into(),
                owner_run_unit: owner_run_unit.into(),
                lifetime,
            });
            service.store.put_provider_states_atomic(vec![
                ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: LOCK_NAMESPACE.into(),
                        key: resource_key.into(),
                        version: expected_version
                            .and_then(|version| version.checked_add(1))
                            .ok_or(StoreError::CapacityExceeded)?,
                        payload: encode_lock(&lock, service.limits)?,
                    },
                    expected_version,
                },
                replay_write(run, request, retention_tick, &response)?,
            ])?;
        }
        return Ok(response);
    }
    if lock.grant_pending {
        let granted = match lifetime {
            Lifetime::Uow => lock.uow_count,
            Lifetime::Task => lock.task_count,
        };
        if granted != 1 || lock.uow_count.checked_add(lock.task_count) != Some(1) {
            return Err(HostProblem::IdempotencyConflict.into());
        }
        lock.grant_pending = false;
    } else {
        match lifetime {
            Lifetime::Uow => {
                lock.uow_count = lock
                    .uow_count
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?
            }
            Lifetime::Task => {
                lock.task_count = lock
                    .task_count
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?
            }
        }
    }
    let next_version = match expected_version {
        Some(value) => value.checked_add(1).ok_or(StoreError::CapacityExceeded)?,
        None => 1,
    };
    let response = normal_response(service, run)?;
    let mut writes = vec![ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: LOCK_NAMESPACE.into(),
            key: resource_key.into(),
            version: next_version,
            payload: encode_lock(&lock, service.limits)?,
        },
        expected_version,
    }];
    if expected_version.is_none() {
        writes.push(catalog_write(service, true)?);
    }
    writes.push(replay_write(run, request, retention_tick, &response)?);
    service.store.put_provider_states_atomic(writes)?;
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn release(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    resource_key: &str,
    lifetime: Lifetime,
    scope: &LockScope,
    current: Option<ProviderStateRecord>,
) -> Result<CicsResponse, ApplyError> {
    let response = normal_response(service, run)?;
    let Some(record) = current else {
        service.store.put_provider_state(
            replay_write(run, request, retention_tick, &response)?.record,
            None,
        )?;
        return Ok(response);
    };
    let mut lock = decode_lock(&record.payload, service.limits)?;
    if &lock.scope != scope {
        return Err(HostProblem::InfrastructureFailure.into());
    }
    if lock.owner_execution != run.invocation.execution_id.as_str()
        || lock.owner_run_unit != run.invocation.run_unit_id.as_str()
    {
        service.store.put_provider_state(
            replay_write(run, request, retention_tick, &response)?.record,
            None,
        )?;
        return Ok(response);
    }
    let count = match lifetime {
        Lifetime::Uow => &mut lock.uow_count,
        Lifetime::Task => &mut lock.task_count,
    };
    if *count == 0 {
        service.store.put_provider_state(
            replay_write(run, request, retention_tick, &response)?.record,
            None,
        )?;
        return Ok(response);
    }
    *count -= 1;
    if lock.uow_count == 0 && lock.task_count == 0 {
        promote_waiter(&mut lock);
    }
    let deleting = lock.uow_count == 0 && lock.task_count == 0;
    let lock_mutation = if deleting {
        ProviderStateMutation::Delete {
            namespace: LOCK_NAMESPACE.into(),
            key: resource_key.into(),
            expected_version: record.version,
        }
    } else {
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: LOCK_NAMESPACE.into(),
                key: resource_key.into(),
                version: record
                    .version
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?,
                payload: encode_lock(&lock, service.limits)?,
            },
            expected_version: Some(record.version),
        })
    };
    let mut mutations = vec![lock_mutation];
    if deleting {
        mutations.push(ProviderStateMutation::Put(catalog_write(service, false)?));
    }
    mutations.push(ProviderStateMutation::Put(replay_write(
        run,
        request,
        retention_tick,
        &response,
    )?));
    service.store.mutate_provider_states_atomic(mutations)?;
    Ok(response)
}

fn normal_response(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
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

fn replay_write(
    run: &Run,
    request: &CicsRequest,
    retention_tick: u64,
    response: &CicsResponse,
) -> Result<ProviderStateWrite, HostProblem> {
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let result_digest = canonical_result_digest(&Ok(HostResult::Cics(response.clone())))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut replay = CicsEffectReplay {
        effect_key: Some(mutation.idempotency_key.as_str().into()),
        owner_execution: Some(run.invocation.execution_id.as_str().into()),
        owner_run_unit: Some(run.invocation.run_unit_id.as_str().into()),
        sequence: Some(mutation.sequence),
        deadline_tick: Some(retention_tick),
        resolution_tick: None,
        request_digest,
        result_digest: Some(result_digest),
        binding_digest: None,
        response: response.clone(),
    };
    replay.binding_digest = Some(cics_effect_replay_binding_digest(&replay));
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: "cics-effect-replay-v1".into(),
            key: mutation.idempotency_key.as_str().into(),
            version: 1,
            payload: encode_cics_effect_replay(&replay)?,
        },
        expected_version: None,
    })
}

fn catalog_write(service: &CicsService, increase: bool) -> Result<ProviderStateWrite, HostProblem> {
    let current = service
        .store
        .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
        .map_err(store_error)?;
    let (count, expected_version) = match current {
        Some(record) => (decode_catalog(&record.payload)?, Some(record.version)),
        None => (0, None),
    };
    let maximum = u64::try_from(service.limits.max_queue_records)
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let next_count = if increase {
        if count >= maximum {
            return Err(HostProblem::ResourceExhausted);
        }
        count.checked_add(1).ok_or(HostProblem::ResourceExhausted)?
    } else {
        count
            .checked_sub(1)
            .ok_or(HostProblem::InfrastructureFailure)?
    };
    let version = expected_version
        .unwrap_or_default()
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let mut payload = CATALOG_MAGIC.to_vec();
    payload.extend_from_slice(&next_count.to_be_bytes());
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: CATALOG_NAMESPACE.into(),
            key: CATALOG_KEY.into(),
            version,
            payload,
        },
        expected_version,
    })
}

fn decode_catalog(payload: &[u8]) -> Result<u64, HostProblem> {
    if payload.len() != CATALOG_MAGIC.len() + 8 || !payload.starts_with(CATALOG_MAGIC) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(u64::from_be_bytes(
        payload[CATALOG_MAGIC.len()..]
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
}

fn request_identity(
    request: &CicsRequest,
    run: &Run,
    models: &[CicsEnqueueModelDefinition],
) -> Result<EnqueueIdentity, HostProblem> {
    let resource = request
        .arguments
        .get("RESOURCE")
        .ok_or(HostProblem::Malformed)?;
    let length = request
        .arguments
        .get("LENGTH")
        .map(|value| parse_integer(value.bytes()))
        .transpose()?;
    let (mode, identity) = if let Some(length) = length {
        if !(1..=255).contains(&length) {
            return Err(HostProblem::Condition {
                name: "LENGERR".into(),
                response: 22,
                response2: 1,
            });
        }
        let length = usize::try_from(length).map_err(|_| HostProblem::Malformed)?;
        if resource.schema() != "mainframe-env.cics.storage-value@1"
            || resource.bytes().len() < length
        {
            return Err(HostProblem::Malformed);
        }
        (b'C', &resource.bytes()[..length])
    } else {
        if resource.schema() != "mainframe-env.cics.storage-identity@1"
            || resource.bytes().is_empty()
            || resource.bytes().len() > 1024
        {
            return Err(HostProblem::Malformed);
        }
        (b'A', resource.bytes())
    };
    let direct = usize::from(request.arguments.contains_key("OPTION.TASK"))
        + usize::from(request.arguments.contains_key("OPTION.UOW"));
    if direct > 1 || (direct == 1 && request.arguments.contains_key("MAXLIFETIME")) {
        return Err(invalid_lifetime());
    }
    let lifetime = if request.arguments.contains_key("OPTION.TASK") {
        Lifetime::Task
    } else if request.arguments.contains_key("OPTION.UOW") {
        Lifetime::Uow
    } else if let Some(value) = request.arguments.get("MAXLIFETIME") {
        match parse_integer(value.bytes())? {
            TASK_CVDA => Lifetime::Task,
            UOW_CVDA => Lifetime::Uow,
            _ => return Err(invalid_lifetime()),
        }
    } else {
        Lifetime::Uow
    };
    let model = (mode == b'C')
        .then(|| {
            models
                .iter()
                .find(|model| enqueue_model_matches(&model.enqueue_name, identity))
        })
        .flatten();
    let scope = if models.is_empty() {
        LockScope::LegacyLocal
    } else if let Some(scope) = model.and_then(|model| model.enqueue_scope.clone()) {
        LockScope::Global(scope)
    } else {
        LockScope::Region {
            applid: run.applid.clone(),
            sysid: run.sysid.clone(),
        }
    };
    let mut hash = Sha256::new();
    match &scope {
        LockScope::LegacyLocal => hash.update(b"mainframe-env.cics.enqueue-resource@1\0"),
        LockScope::Region { applid, sysid } => {
            hash.update(b"mainframe-env.cics.enqueue-resource@2\0L");
            hash_framed(&mut hash, applid.as_bytes());
            hash_framed(&mut hash, sysid.as_bytes());
        }
        LockScope::Global(scope) => {
            hash.update(b"mainframe-env.cics.enqueue-resource@2\0G");
            hash_framed(&mut hash, scope.as_bytes());
        }
    }
    hash.update([mode]);
    hash_framed(&mut hash, identity);
    Ok(EnqueueIdentity {
        resource_key: hex(&hash.finalize()),
        lifetime,
        scope,
        disabled: model.is_some_and(|model| !model.enabled),
    })
}

fn hash_framed(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

fn parse_integer(value: &[u8]) -> Result<i64, HostProblem> {
    std::str::from_utf8(value)
        .map_err(|_| HostProblem::Malformed)?
        .parse()
        .map_err(|_| HostProblem::Malformed)
}

fn invalid_lifetime() -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: 2,
    }
}

fn promote_waiter(lock: &mut LockRecord) {
    let Some(waiter) = lock.waiters.first().cloned() else {
        lock.grant_pending = false;
        return;
    };
    lock.waiters.remove(0);
    lock.owner_execution = waiter.owner_execution;
    lock.owner_run_unit = waiter.owner_run_unit;
    lock.uow_count = u32::from(waiter.lifetime == Lifetime::Uow);
    lock.task_count = u32::from(waiter.lifetime == Lifetime::Task);
    lock.grant_pending = true;
}

fn encode_lock(
    lock: &LockRecord,
    limits: super::super::CicsLimits,
) -> Result<Vec<u8>, HostProblem> {
    if lock.waiters.len() > limits.max_runs {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut out = match &lock.scope {
        LockScope::LegacyLocal => LOCK_MAGIC.to_vec(),
        LockScope::Region { applid, sysid } => {
            let mut out = SCOPED_LOCK_MAGIC.to_vec();
            out.push(0);
            field(&mut out, applid.as_bytes())?;
            field(&mut out, sysid.as_bytes())?;
            out
        }
        LockScope::Global(scope) => {
            let mut out = SCOPED_LOCK_MAGIC.to_vec();
            out.push(1);
            field(&mut out, scope.as_bytes())?;
            out
        }
    };
    field(&mut out, lock.owner_execution.as_bytes())?;
    field(&mut out, lock.owner_run_unit.as_bytes())?;
    out.extend_from_slice(&lock.uow_count.to_be_bytes());
    out.extend_from_slice(&lock.task_count.to_be_bytes());
    out.push(u8::from(lock.grant_pending));
    out.extend_from_slice(
        &u32::try_from(lock.waiters.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for waiter in &lock.waiters {
        field(&mut out, waiter.owner_execution.as_bytes())?;
        field(&mut out, waiter.owner_run_unit.as_bytes())?;
        out.push(match waiter.lifetime {
            Lifetime::Uow => 0,
            Lifetime::Task => 1,
        });
    }
    if out.len() > limits.max_queue_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(out)
}

fn decode_lock(bytes: &[u8], limits: super::super::CicsLimits) -> Result<LockRecord, HostProblem> {
    if bytes.len() > limits.max_queue_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let (mut reader, scope) = if bytes.starts_with(LOCK_MAGIC) {
        (CodecReader::new(bytes, LOCK_MAGIC)?, LockScope::LegacyLocal)
    } else {
        let mut reader = CodecReader::new(bytes, SCOPED_LOCK_MAGIC)?;
        let scope = match reader.byte()? {
            0 => LockScope::Region {
                applid: reader.text(128)?,
                sysid: reader.text(128)?,
            },
            1 => LockScope::Global(reader.text(16)?),
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        (reader, scope)
    };
    let owner_execution = reader.text(256)?;
    let owner_run_unit = reader.text(256)?;
    let uow_count = reader.u32()?;
    let task_count = reader.u32()?;
    let grant_pending = match reader.byte()? {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let waiter_count =
        usize::try_from(reader.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    if waiter_count > limits.max_runs {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut waiters = Vec::with_capacity(waiter_count);
    for _ in 0..waiter_count {
        let owner_execution = reader.text(256)?;
        let owner_run_unit = reader.text(256)?;
        let lifetime = match reader.byte()? {
            0 => Lifetime::Uow,
            1 => Lifetime::Task,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        waiters.push(Waiter {
            owner_execution,
            owner_run_unit,
            lifetime,
        });
    }
    reader.finish()?;
    let unique_waiters = waiters
        .iter()
        .map(|waiter| (&waiter.owner_execution, &waiter.owner_run_unit))
        .collect::<BTreeSet<_>>();
    if owner_execution.is_empty()
        || owner_run_unit.is_empty()
        || match &scope {
            LockScope::LegacyLocal => false,
            LockScope::Region { applid, sysid } => applid.is_empty() || sysid.is_empty(),
            LockScope::Global(scope) => {
                scope.chars().count() != 4
                    || !scope
                        .chars()
                        .all(|character| model_character(character, false))
            }
        }
        || uow_count
            .checked_add(task_count)
            .is_none_or(|count| count == 0)
        || (grant_pending && uow_count.checked_add(task_count) != Some(1))
        || waiters.iter().any(|waiter| {
            waiter.owner_execution.is_empty()
                || waiter.owner_run_unit.is_empty()
                || (waiter.owner_execution == owner_execution
                    && waiter.owner_run_unit == owner_run_unit)
        })
        || unique_waiters.len() != waiters.len()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(LockRecord {
        scope,
        owner_execution,
        owner_run_unit,
        uow_count,
        task_count,
        grant_pending,
        waiters,
    })
}

fn field(out: &mut Vec<u8>, value: &[u8]) -> Result<(), HostProblem> {
    out.extend_from_slice(
        &u16::try_from(value.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    out.extend_from_slice(value);
    Ok(())
}

struct CodecReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> CodecReader<'a> {
    fn new(bytes: &'a [u8], magic: &[u8]) -> Result<Self, HostProblem> {
        if !bytes.starts_with(magic) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Self {
            bytes,
            at: magic.len(),
        })
    }

    fn take(&mut self, amount: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(amount)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, HostProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
    }

    fn text(&mut self, maximum: usize) -> Result<String, HostProblem> {
        let length = usize::from(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ));
        if length > maximum {
            return Err(HostProblem::ResourceExhausted);
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn finish(&self) -> Result<(), HostProblem> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(HostProblem::InfrastructureFailure)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    value
}
