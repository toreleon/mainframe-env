//! BTS container commands, staged within the owning unit of work.

use super::{
    command,
    scope::OwnerIdentity,
    state::{self, ContainerDatatype, ContainerOwner, ContainerValue},
};
use crate::retention::ContainerReplay as Replay;
use crate::service::handlers::bts_lifecycle::{self, BtsLifecycleStore, BtsProcess};
use crate::service::{CicsService, Run};
use mainframe_env_execution_api::BoundedPayload;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PENDING_NAMESPACE: &str = "cics-bts-container-pending-v1";
const REPLAY_NAMESPACE: &str = "cics-container-replay-v1";
const CAPACITY_NAMESPACE: &str = "cics-container-capacity-v1";
const MAX_ATTEMPTS: usize = 8;
const MAX_PENDING: usize = 16_384;

#[derive(Clone, Copy)]
enum Selector<'a> {
    Current,
    Child(&'a str),
    CurrentProcess,
    AcquiredActivity,
    AcquiredProcess,
}

#[derive(Clone)]
struct Scope {
    owner: ContainerOwner,
    process_type: String,
    process_name: String,
    process_version: u64,
    resource: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    schema_version: u8,
    run_unit: String,
    owner: ContainerOwner,
    name: String,
    original_version: Option<u64>,
    value: Option<ContainerValue>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capacity {
    schema_version: u8,
    channels: usize,
    containers: usize,
    replays: usize,
}

fn store_problem(_: StoreError) -> HostProblem {
    HostProblem::InfrastructureFailure
}
fn next_version(version: Option<u64>) -> Result<u64, HostProblem> {
    version
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn pending_key(run_unit: &str, owner: &ContainerOwner, name: &str) -> Result<String, HostProblem> {
    let bytes = serde_json::to_vec(&(run_unit, owner, name))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(hex(&Sha256::digest(bytes)))
}

fn active_scope(
    store: &dyn ProviderStateStore,
    identity: OwnerIdentity<'_>,
    selector: Selector<'_>,
    allow_self: bool,
    read_only: bool,
) -> Result<Scope, HostProblem> {
    let lifecycle = BtsLifecycleStore::new(store);
    let context = lifecycle
        .active_context(identity.run_unit, identity.execution, identity.principal)?
        .ok_or_else(|| command::condition("INVREQ", 16, 1))?;
    let process = lifecycle
        .load_process(&context.process_type, &context.process_name)?
        .ok_or(HostProblem::NotFound)?;
    if !process.visible_to(identity.run_unit) {
        return Err(HostProblem::Unauthorized);
    }
    let owner = match selector {
        Selector::CurrentProcess => {
            if context.activity_id != process.root_id && !read_only {
                return Err(command::condition("CONTAINERERR", 110, 2));
            }
            ContainerOwner::Process {
                process_type: context.process_type.clone(),
                process_name: context.process_name.clone(),
                root_activity_id: process.root_id.clone(),
            }
        }
        Selector::Current | Selector::Child(_) => {
            let activity_id = match selector {
                Selector::Current => context.activity_id.clone(),
                Selector::Child(name) => {
                    if !state::valid_name(name, 16) {
                        return Err(command::condition("ACTIVITYERR", 109, 1));
                    }
                    if allow_self
                        && process
                            .activities
                            .get(&context.activity_id)
                            .is_some_and(|activity| activity.name == name)
                    {
                        context.activity_id.clone()
                    } else {
                        process
                            .child(&context.activity_id, name)
                            .ok_or_else(|| command::condition("ACTIVITYERR", 109, 1))?
                            .id
                            .clone()
                    }
                }
                _ => unreachable!(),
            };
            let activity = process
                .activities
                .get(&activity_id)
                .ok_or(HostProblem::NotFound)?;
            if activity
                .pending_uow
                .as_deref()
                .is_some_and(|uow| uow != identity.run_unit)
            {
                return Err(command::condition("LOCKED", 100, 0));
            }
            ContainerOwner::Activity {
                process_type: context.process_type.clone(),
                process_name: context.process_name.clone(),
                root_activity_id: process.root_id.clone(),
                activity_id,
            }
        }
        _ => return Err(HostProblem::Unsupported),
    };
    Ok(Scope {
        owner,
        process_type: context.process_type.clone(),
        process_name: context.process_name.clone(),
        process_version: process.row_version,
        resource: BtsLifecycleStore::saf_resource(&context.process_type, &context.process_name)?,
    })
}

fn acquired_scope(
    store: &dyn ProviderStateStore,
    identity: OwnerIdentity<'_>,
    selector: Selector<'_>,
) -> Result<Scope, HostProblem> {
    let lifecycle = BtsLifecycleStore::new(store);
    let held = lifecycle
        .acquired_process_container_scope(
            identity.run_unit,
            identity.execution,
            identity.principal,
        )?
        .ok_or_else(|| command::condition("INVREQ", 16, 1))?;
    if matches!(selector, Selector::AcquiredProcess) && !held.permits_acqprocess() {
        // The pinned GET topic does not establish ACQPROCESS for a descendant acquisition.
        return Err(HostProblem::Unsupported);
    }
    let process = lifecycle
        .load_process(&held.process_type, &held.process_name)?
        .ok_or(HostProblem::NotFound)?;
    let owner = if matches!(selector, Selector::AcquiredProcess) {
        ContainerOwner::Process {
            process_type: held.process_type.clone(),
            process_name: held.process_name.clone(),
            root_activity_id: held.root_activity_id,
        }
    } else {
        ContainerOwner::Activity {
            process_type: held.process_type.clone(),
            process_name: held.process_name.clone(),
            root_activity_id: held.root_activity_id,
            activity_id: held.acquired_activity_id,
        }
    };
    Ok(Scope {
        owner,
        process_type: held.process_type.clone(),
        process_name: held.process_name.clone(),
        process_version: process.row_version,
        resource: BtsLifecycleStore::saf_resource(&held.process_type, &held.process_name)?,
    })
}

fn resolve(
    store: &dyn ProviderStateStore,
    identity: OwnerIdentity<'_>,
    selector: Selector<'_>,
    allow_self: bool,
    read_only: bool,
) -> Result<Scope, HostProblem> {
    match selector {
        Selector::AcquiredActivity | Selector::AcquiredProcess => {
            acquired_scope(store, identity, selector)
        }
        _ => active_scope(store, identity, selector, allow_self, read_only),
    }
}

fn single_selector(request: &CicsRequest) -> Result<Selector<'_>, HostProblem> {
    let flags = ["OPTION.PROCESS", "OPTION.ACQPROCESS", "OPTION.ACQACTIVITY"];
    let count = usize::from(request.arguments.contains_key("ACTIVITY"))
        + flags
            .iter()
            .filter(|key| request.arguments.contains_key(**key))
            .count();
    if count > 1 || request.arguments.contains_key("CHANNEL") {
        return Err(HostProblem::Unsupported);
    }
    if let Some(name) = request.arguments.get("ACTIVITY") {
        let value = selector_text(name)?;
        return Ok(Selector::Child(value));
    }
    if request.arguments.contains_key("OPTION.PROCESS") {
        Ok(Selector::CurrentProcess)
    } else if request.arguments.contains_key("OPTION.ACQPROCESS") {
        Ok(Selector::AcquiredProcess)
    } else if request.arguments.contains_key("OPTION.ACQACTIVITY") {
        Ok(Selector::AcquiredActivity)
    } else {
        Ok(Selector::Current)
    }
}

fn selector_text(value: &BoundedPayload) -> Result<&str, HostProblem> {
    if !matches!(
        value.schema(),
        "mainframe-env.cics.argument@1"
            | "mainframe-env.cics.literal@1"
            | "mainframe-env.cics.storage-value@1"
    ) {
        return Err(HostProblem::Malformed);
    }
    std::str::from_utf8(value.bytes())
        .map(|text| text.trim_end_matches(' '))
        .map_err(|_| HostProblem::Malformed)
}

fn move_selector(request: &CicsRequest, source: bool) -> Result<Selector<'_>, HostProblem> {
    let (activity, process) = if source {
        ("FROMACTIVITY", "OPTION.FROMPROCESS")
    } else {
        ("TOACTIVITY", "OPTION.TOPROCESS")
    };
    if request.arguments.contains_key(activity) && request.arguments.contains_key(process)
        || request.arguments.contains_key("CHANNEL")
        || request.arguments.contains_key("TOCHANNEL")
    {
        return Err(HostProblem::Unsupported);
    }
    if let Some(name) = request.arguments.get(activity) {
        let value = selector_text(name)?;
        Ok(Selector::Child(value))
    } else if request.arguments.contains_key(process) {
        Ok(Selector::CurrentProcess)
    } else {
        Ok(Selector::Current)
    }
}

fn read_pending(
    store: &dyn ProviderStateStore,
    run_unit: &str,
    owner: &ContainerOwner,
    name: &str,
) -> Result<Option<(ProviderStateRecord, Pending)>, HostProblem> {
    let key = pending_key(run_unit, owner, name)?;
    store
        .get_provider_state(PENDING_NAMESPACE, &key)
        .map_err(store_problem)?
        .map(|row| {
            let pending: Pending = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if row.version == 0
                || pending.schema_version != 1
                || pending.run_unit != run_unit
                || pending.owner != *owner
                || pending.name != name
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok((row, pending))
        })
        .transpose()
}

fn read_value(
    store: &dyn ProviderStateStore,
    identity: OwnerIdentity<'_>,
    scope: &Scope,
    name: &str,
) -> Result<Option<ContainerValue>, HostProblem> {
    if !state::valid_name(name, 16) {
        return Err(command::condition("CONTAINERERR", 110, 1));
    }
    if let Some((_, pending)) = read_pending(store, identity.run_unit, &scope.owner, name)? {
        return Ok(pending.value);
    }
    let row = store
        .get_provider_state(&state::container_namespace(&scope.owner)?, name)
        .map_err(store_problem)?;
    row.map(|row| state::decode_container(&row, &scope.owner))
        .transpose()
}

fn read_guarded(
    service: &CicsService,
    run: &mut Run,
    identity: OwnerIdentity<'_>,
    selector: Selector<'_>,
    name: &str,
) -> Result<ContainerValue, HostProblem> {
    let before = resolve(service.store.as_ref(), identity, selector, false, true)?;
    service.authorize(run, "BTSLIFE", &before.resource, AccessIntent::Read)?;
    let value = read_value(service.store.as_ref(), identity, &before, name)?;
    let after = resolve(service.store.as_ref(), identity, selector, false, true)?;
    if before.owner != after.owner || before.process_version != after.process_version {
        return Err(HostProblem::UnknownOutcome);
    }
    value.ok_or_else(|| command::condition("CONTAINERERR", 110, 1))
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    use CicsOperation as O;
    let operation = request.operation;
    let allowed: &[&str] = match operation {
        O::DeleteContainer => &[
            "CONTAINER",
            "ACTIVITY",
            "OPTION.PROCESS",
            "OPTION.ACQPROCESS",
            "OPTION.ACQACTIVITY",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        O::GetContainer => &[
            "CONTAINER",
            "ACTIVITY",
            "OPTION.PROCESS",
            "OPTION.ACQPROCESS",
            "OPTION.ACQACTIVITY",
            "INTO",
            "INTO.MAXLENGTH",
            "SET",
            "SET.MAXLENGTH",
            "FLENGTH",
            "OPTION.NODATA",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        O::MoveContainer => &[
            "CONTAINER",
            "AS",
            "FROMACTIVITY",
            "TOACTIVITY",
            "OPTION.FROMPROCESS",
            "OPTION.TOPROCESS",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        O::PutContainer => &[
            "CONTAINER",
            "ACTIVITY",
            "OPTION.PROCESS",
            "OPTION.ACQPROCESS",
            "OPTION.ACQACTIVITY",
            "FROM",
            "FLENGTH",
            "OPTION.APPEND",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
        ],
        _ => return Err(HostProblem::Unsupported),
    };
    if request.arguments.iter().any(|(key, value)| {
        !allowed.contains(&key.as_str())
            || key.starts_with("OPTION.")
                && (value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty())
    }) {
        return Err(HostProblem::Unsupported);
    }
    let run_unit = run.invocation.run_unit_id.as_str().to_owned();
    let execution = run.invocation.execution_id.as_str().to_owned();
    let principal = run.invocation.principal.id().as_str().to_owned();
    let identity = OwnerIdentity {
        run_unit: &run_unit,
        execution: &execution,
        principal: &principal,
    };
    let name = command::name(request, "CONTAINER")?.ok_or(HostProblem::Malformed)?;
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
    match operation {
        O::GetContainer => {
            let value = read_guarded(service, run, identity, single_selector(request)?, &name)?;
            let into = request.arguments.contains_key("INTO");
            let set = request.arguments.contains_key("SET");
            let nodata = request.arguments.contains_key("OPTION.NODATA");
            if usize::from(into) + usize::from(set) + usize::from(nodata) != 1
                || (set || nodata) && !request.arguments.contains_key("FLENGTH")
            {
                return Err(command::condition("INVREQ", 16, 1));
            }
            if into {
                // Only INTO reads FLENGTH. SET and NODATA return its actual
                // value regardless of the receiving field's incoming bytes.
                let maximum = command::number(request, "FLENGTH")?
                    .map(|value| {
                        usize::try_from(value.max(0))
                            .map_err(|_| command::condition("LENGERR", 22, 11))
                    })
                    .transpose()?;
                let capacity = command::number(request, "INTO.MAXLENGTH")?
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(HostProblem::Malformed)?;
                let maximum = maximum.unwrap_or(capacity);
                let copied = value.bytes.len().min(capacity).min(maximum);
                if maximum != value.bytes.len() || copied < value.bytes.len() {
                    response.condition = "LENGERR".into();
                    response.response = 22;
                    response.response2 = 11;
                }
                command::output(
                    &mut response,
                    "INTO",
                    "mainframe-env.cics.payload@1",
                    value.bytes[..copied].to_vec(),
                )?;
            }
            if set {
                let capacity = command::number(request, "SET.MAXLENGTH")?
                    .and_then(|value| usize::try_from(value).ok())
                    .ok_or(HostProblem::Malformed)?;
                if value.bytes.len() > capacity {
                    return Err(HostProblem::ResourceExhausted);
                }
                command::output(
                    &mut response,
                    "SET",
                    "mainframe-env.cics.payload@1",
                    value.bytes.clone(),
                )?;
            }
            if request.arguments.contains_key("FLENGTH") {
                command::output(
                    &mut response,
                    "FLENGTH",
                    "mainframe-env.cics.decimal@1",
                    value.bytes.len().to_string().into_bytes(),
                )?;
            }
        }
        O::PutContainer => {
            let from = request
                .arguments
                .get("FROM")
                .ok_or(HostProblem::Malformed)?;
            if !matches!(
                from.schema(),
                "mainframe-env.cics.storage-value@1" | "mainframe-env.cics.literal@1"
            ) {
                return Err(HostProblem::Malformed);
            }
            let length = usize::try_from(
                command::number(request, "FLENGTH")?.unwrap_or(from.bytes().len() as i64),
            )
            .map_err(|_| command::condition("LENGERR", 22, 1))?;
            if length > from.bytes().len() {
                return Err(command::condition("LENGERR", 22, 1));
            }
            mutate(
                service,
                run,
                identity,
                request,
                single_selector(request)?,
                None,
                &name,
                None,
                Some(&from.bytes()[..length]),
                request.arguments.contains_key("OPTION.APPEND"),
            )?;
        }
        O::DeleteContainer => mutate(
            service,
            run,
            identity,
            request,
            single_selector(request)?,
            None,
            &name,
            None,
            None,
            false,
        )?,
        O::MoveContainer => {
            let as_name = command::name(request, "AS")?.ok_or(HostProblem::Malformed)?;
            mutate(
                service,
                run,
                identity,
                request,
                move_selector(request, true)?,
                Some(move_selector(request, false)?),
                &name,
                Some(&as_name),
                None,
                false,
            )?;
        }
        _ => unreachable!(),
    }
    Ok(response)
}

fn request_digest(request: &CicsRequest) -> String {
    let mut digest = Sha256::new();
    digest.update(request.operation.runtime_name().as_bytes());
    for (key, value) in &request.arguments {
        digest.update((key.len() as u64).to_be_bytes());
        digest.update(key.as_bytes());
        digest.update(value.schema().as_bytes());
        digest.update((value.bytes().len() as u64).to_be_bytes());
        digest.update(value.bytes());
    }
    hex(&digest.finalize())
}

fn pending_record(
    pending: &Pending,
    old: Option<&ProviderStateRecord>,
) -> Result<ProviderStateMutation, HostProblem> {
    let key = pending_key(&pending.run_unit, &pending.owner, &pending.name)?;
    let expected = old.map(|row| row.version);
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: PENDING_NAMESPACE.into(),
            key,
            version: next_version(expected)?,
            payload: serde_json::to_vec(pending).map_err(|_| HostProblem::ResourceExhausted)?,
        },
        expected_version: expected,
    }))
}

fn committed(
    store: &dyn ProviderStateStore,
    owner: &ContainerOwner,
    name: &str,
) -> Result<Option<(ProviderStateRecord, ContainerValue)>, HostProblem> {
    store
        .get_provider_state(&state::container_namespace(owner)?, name)
        .map_err(store_problem)?
        .map(|row| {
            let value = state::decode_container(&row, owner)?;
            Ok((row, value))
        })
        .transpose()
}

fn effective_count(
    store: &dyn ProviderStateStore,
    run_unit: &str,
    owner: &ContainerOwner,
) -> Result<usize, HostProblem> {
    let namespace = state::container_namespace(owner)?;
    let rows = store
        .list_provider_state(&namespace, state::MAX_CONTAINERS + 1)
        .map_err(store_problem)?;
    if rows.len() > state::MAX_CONTAINERS {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        state::decode_container(&row, owner)?;
        names.insert(row.key);
    }
    let pending_rows = store
        .list_provider_state(PENDING_NAMESPACE, MAX_PENDING + 1)
        .map_err(store_problem)?;
    if pending_rows.len() > MAX_PENDING {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in pending_rows {
        let pending: Pending =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        if pending.schema_version != 1 {
            return Err(HostProblem::InfrastructureFailure);
        }
        if pending.run_unit == run_unit && pending.owner == *owner {
            if pending.value.is_some() {
                names.insert(pending.name);
            } else {
                names.remove(&pending.name);
            }
        }
    }
    Ok(names.len())
}

#[allow(clippy::too_many_arguments)]
fn mutate(
    service: &CicsService,
    run: &mut Run,
    identity: OwnerIdentity<'_>,
    request: &CicsRequest,
    source_selector: Selector<'_>,
    target_selector: Option<Selector<'_>>,
    name: &str,
    as_name: Option<&str>,
    bytes: Option<&[u8]>,
    append: bool,
) -> Result<(), HostProblem> {
    let replay_key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    if replay_key.is_empty() || replay_key.len() > 256 {
        return Err(HostProblem::MissingIdempotency);
    }
    let digest = request_digest(request);
    for _ in 0..MAX_ATTEMPTS {
        let source = resolve(
            service.store.as_ref(),
            identity,
            source_selector,
            target_selector.is_some(),
            false,
        )?;
        let target = target_selector
            .map(|selector| resolve(service.store.as_ref(), identity, selector, true, false))
            .transpose()?;
        if target.as_ref().is_some_and(|other| {
            other.process_type != source.process_type || other.process_name != source.process_name
        }) {
            return Err(HostProblem::Unsupported);
        }
        service.authorize(run, "BTSLIFE", &source.resource, AccessIntent::Update)?;
        if let Some(target) = &target {
            service.authorize(run, "BTSLIFE", &target.resource, AccessIntent::Update)?;
        }
        if let Some(row) = service
            .store
            .get_provider_state(REPLAY_NAMESPACE, replay_key)
            .map_err(store_problem)?
        {
            let replay = Replay::decode(&row).map_err(|_| HostProblem::InfrastructureFailure)?;
            return if replay.owner_execution == identity.execution
                && replay.owner_principal == identity.principal
                && replay.owner_run_unit == identity.run_unit
                && replay.digest == digest
            {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        let old_process = BtsLifecycleStore::new(service.store.as_ref())
            .load_process(&source.process_type, &source.process_name)?
            .ok_or(HostProblem::NotFound)?;
        if old_process.row_version != source.process_version
            || target
                .as_ref()
                .is_some_and(|other| other.process_version != source.process_version)
        {
            continue;
        }
        let capacity_old = service
            .store
            .get_provider_state(CAPACITY_NAMESPACE, "global")
            .map_err(store_problem)?;
        let mut capacity = capacity_old
            .as_ref()
            .map(|row| {
                serde_json::from_slice::<Capacity>(&row.payload)
                    .map_err(|_| HostProblem::InfrastructureFailure)
            })
            .transpose()?
            .unwrap_or(Capacity {
                schema_version: 1,
                channels: 0,
                containers: 0,
                replays: 0,
            });
        if capacity.schema_version != 1
            || capacity.channels > 4096
            || capacity.containers > 16384
            || capacity.replays >= 16384
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let source_pending = read_pending(
            service.store.as_ref(),
            identity.run_unit,
            &source.owner,
            name,
        )?;
        let source_committed = committed(service.store.as_ref(), &source.owner, name)?;
        let source_value = source_pending
            .as_ref()
            .map(|(_, pending)| pending.value.clone())
            .unwrap_or_else(|| source_committed.as_ref().map(|(_, value)| value.clone()));
        let source_original = source_pending
            .as_ref()
            .map(|(_, pending)| pending.original_version)
            .unwrap_or_else(|| source_committed.as_ref().map(|(row, _)| row.version));
        let mut writes = Vec::new();
        match request.operation {
            CicsOperation::PutContainer => {
                if source_value.as_ref().is_some_and(|value| value.read_only) {
                    return Err(command::condition("CONTAINERERR", 110, 2));
                }
                if source_value.is_none()
                    && effective_count(service.store.as_ref(), identity.run_unit, &source.owner)?
                        >= state::MAX_CONTAINERS
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                let mut value = source_value.unwrap_or(ContainerValue {
                    datatype: ContainerDatatype::Bit,
                    ccsid: None,
                    read_only: false,
                    bytes: Vec::new(),
                });
                if append {
                    value
                        .bytes
                        .extend_from_slice(bytes.ok_or(HostProblem::Malformed)?);
                } else {
                    value.bytes = bytes.ok_or(HostProblem::Malformed)?.to_vec();
                }
                state::container_record(&source.owner, name, &value)?;
                writes.push(pending_record(
                    &Pending {
                        schema_version: 1,
                        run_unit: identity.run_unit.into(),
                        owner: source.owner.clone(),
                        name: name.into(),
                        original_version: source_original,
                        value: Some(value),
                    },
                    source_pending.as_ref().map(|(row, _)| row),
                )?);
            }
            CicsOperation::DeleteContainer => {
                let value =
                    source_value.ok_or_else(|| command::condition("CONTAINERERR", 110, 1))?;
                if value.read_only {
                    return Err(command::condition("CONTAINERERR", 110, 2));
                }
                writes.push(pending_record(
                    &Pending {
                        schema_version: 1,
                        run_unit: identity.run_unit.into(),
                        owner: source.owner.clone(),
                        name: name.into(),
                        original_version: source_original,
                        value: None,
                    },
                    source_pending.as_ref().map(|(row, _)| row),
                )?);
            }
            CicsOperation::MoveContainer => {
                let target = target.as_ref().ok_or(HostProblem::Malformed)?;
                let as_name = as_name.ok_or(HostProblem::Malformed)?;
                if !state::valid_name(as_name, 16) {
                    return Err(command::condition("CONTAINERERR", 110, 1));
                }
                let value =
                    source_value.ok_or_else(|| command::condition("CONTAINERERR", 110, 1))?;
                if value.read_only {
                    return Err(command::condition("CONTAINERERR", 110, 2));
                }
                if source.owner != target.owner || name != as_name {
                    let target_pending = read_pending(
                        service.store.as_ref(),
                        identity.run_unit,
                        &target.owner,
                        as_name,
                    )?;
                    let target_committed =
                        committed(service.store.as_ref(), &target.owner, as_name)?;
                    let target_value = target_pending
                        .as_ref()
                        .map(|(_, pending)| pending.value.clone())
                        .unwrap_or_else(|| {
                            target_committed.as_ref().map(|(_, value)| value.clone())
                        });
                    if target_value.as_ref().is_some_and(|value| value.read_only) {
                        return Err(command::condition("CONTAINERERR", 110, 2));
                    }
                    if target_value.is_none()
                        && effective_count(
                            service.store.as_ref(),
                            identity.run_unit,
                            &target.owner,
                        )? >= state::MAX_CONTAINERS
                    {
                        return Err(HostProblem::ResourceExhausted);
                    }
                    let original = target_pending
                        .as_ref()
                        .map(|(_, pending)| pending.original_version)
                        .unwrap_or_else(|| target_committed.as_ref().map(|(row, _)| row.version));
                    writes.push(pending_record(
                        &Pending {
                            schema_version: 1,
                            run_unit: identity.run_unit.into(),
                            owner: source.owner.clone(),
                            name: name.into(),
                            original_version: source_original,
                            value: None,
                        },
                        source_pending.as_ref().map(|(row, _)| row),
                    )?);
                    writes.push(pending_record(
                        &Pending {
                            schema_version: 1,
                            run_unit: identity.run_unit.into(),
                            owner: target.owner.clone(),
                            name: as_name.into(),
                            original_version: original,
                            value: Some(value),
                        },
                        target_pending.as_ref().map(|(row, _)| row),
                    )?);
                }
            }
            _ => return Err(HostProblem::Unsupported),
        }
        let mut process = old_process.clone();
        process.epoch = process
            .epoch
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = BtsLifecycleStore::process_key(&source.process_type, &source.process_name)?;
        writes.push(bts_lifecycle::put_process(
            &key,
            &process,
            Some(old_process.row_version),
        )?);
        capacity.replays += 1;
        let expected = capacity_old.as_ref().map(|row| row.version);
        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: CAPACITY_NAMESPACE.into(),
                key: "global".into(),
                version: next_version(expected)?,
                payload: serde_json::to_vec(&capacity)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
            },
            expected_version: expected,
        }));
        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: REPLAY_NAMESPACE.into(),
                key: replay_key.into(),
                version: 1,
                payload: serde_json::to_vec(&Replay {
                    schema_version: 1,
                    owner_execution: identity.execution.into(),
                    owner_principal: identity.principal.into(),
                    owner_run_unit: identity.run_unit.into(),
                    digest: digest.clone(),
                })
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            },
            expected_version: None,
        }));
        match service.store.mutate_provider_states_atomic(writes) {
            Ok(()) => return Ok(()),
            Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
            Err(_) => return Err(HostProblem::UnknownOutcome),
        }
    }
    Err(HostProblem::UnknownOutcome)
}

pub(in crate::service::handlers) fn settle_uow(
    store: &dyn ProviderStateStore,
    run_unit: &str,
    commit: bool,
) -> Result<(), HostProblem> {
    for _ in 0..MAX_ATTEMPTS {
        let rows = store
            .list_provider_state(PENDING_NAMESPACE, MAX_PENDING + 1)
            .map_err(store_problem)?;
        if rows.len() > MAX_PENDING {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        let mut delta: isize = 0;
        let mut processes = std::collections::BTreeMap::<(String, String), BtsProcess>::new();
        for row in rows {
            let pending: Pending = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if row.version == 0
                || pending.schema_version != 1
                || pending_key(&pending.run_unit, &pending.owner, &pending.name)? != row.key
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            if pending.run_unit != run_unit {
                continue;
            }
            let (process_type, process_name) = match &pending.owner {
                ContainerOwner::Process {
                    process_type,
                    process_name,
                    ..
                }
                | ContainerOwner::Activity {
                    process_type,
                    process_name,
                    ..
                } => (process_type, process_name),
                ContainerOwner::Channel { .. } => return Err(HostProblem::InfrastructureFailure),
            };
            if let std::collections::btree_map::Entry::Vacant(entry) =
                processes.entry((process_type.clone(), process_name.clone()))
            {
                let process = BtsLifecycleStore::new(store)
                    .load_process(process_type, process_name)?
                    .ok_or(HostProblem::NotFound)?;
                entry.insert(process);
            }
            if commit {
                let prior = committed(store, &pending.owner, &pending.name)?;
                if prior.as_ref().map(|(row, _)| row.version) != pending.original_version {
                    return Err(HostProblem::UnknownOutcome);
                }
                match pending.value {
                    Some(value) => {
                        let mut record =
                            state::container_record(&pending.owner, &pending.name, &value)?;
                        record.version = next_version(pending.original_version)?;
                        writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                            record,
                            expected_version: pending.original_version,
                        }));
                        if prior.is_none() {
                            delta += 1;
                        }
                    }
                    None if let Some((prior, _)) = prior => {
                        writes.push(ProviderStateMutation::Delete {
                            namespace: prior.namespace,
                            key: prior.key,
                            expected_version: prior.version,
                        });
                        delta -= 1;
                    }
                    None => {}
                }
            }
            writes.push(ProviderStateMutation::Delete {
                namespace: PENDING_NAMESPACE.into(),
                key: row.key,
                expected_version: row.version,
            });
        }
        if writes.is_empty() {
            return Ok(());
        }
        for ((process_type, process_name), mut process) in processes {
            let expected = process.row_version;
            process.epoch = process
                .epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(bts_lifecycle::put_process(
                &BtsLifecycleStore::process_key(&process_type, &process_name)?,
                &process,
                Some(expected),
            )?);
        }
        if commit && delta != 0 {
            let prior = store
                .get_provider_state(CAPACITY_NAMESPACE, "global")
                .map_err(store_problem)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let mut capacity: Capacity = serde_json::from_slice(&prior.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            capacity.containers = capacity
                .containers
                .checked_add_signed(delta)
                .ok_or(HostProblem::ResourceExhausted)?;
            if capacity.containers > 16384 {
                return Err(HostProblem::ResourceExhausted);
            }
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: CAPACITY_NAMESPACE.into(),
                    key: "global".into(),
                    version: next_version(Some(prior.version))?,
                    payload: serde_json::to_vec(&capacity)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: Some(prior.version),
            }));
        }
        match store.mutate_provider_states_atomic(writes) {
            Ok(()) => return Ok(()),
            Err(StoreError::Conflict | StoreError::AlreadyExists) => continue,
            Err(_) => return Err(HostProblem::UnknownOutcome),
        }
    }
    Err(HostProblem::UnknownOutcome)
}

/// Plan command-data removal in the caller's process CAS batch. The caller
/// supplies exactly the activity IDs leaving the lifecycle tree.
pub(in crate::service::handlers) fn cleanup_removed(
    store: &dyn ProviderStateStore,
    process: &BtsProcess,
    activity_ids: &[String],
    include_process: bool,
) -> Result<Vec<ProviderStateMutation>, HostProblem> {
    let mut owners = activity_ids
        .iter()
        .map(|activity_id| ContainerOwner::Activity {
            process_type: process.process_type.clone(),
            process_name: process.name.clone(),
            root_activity_id: process.root_id.clone(),
            activity_id: activity_id.clone(),
        })
        .collect::<Vec<_>>();
    if include_process {
        owners.push(ContainerOwner::Process {
            process_type: process.process_type.clone(),
            process_name: process.name.clone(),
            root_activity_id: process.root_id.clone(),
        });
    }
    let mut writes = Vec::new();
    let mut deleted = 0usize;
    for owner in &owners {
        let namespace = state::container_namespace(owner)?;
        let rows = store
            .list_provider_state(&namespace, state::MAX_CONTAINERS + 1)
            .map_err(store_problem)?;
        if rows.len() > state::MAX_CONTAINERS {
            return Err(HostProblem::ResourceExhausted);
        }
        for row in rows {
            state::decode_container(&row, owner)?;
            writes.push(ProviderStateMutation::Delete {
                namespace: row.namespace,
                key: row.key,
                expected_version: row.version,
            });
            deleted += 1;
        }
    }
    let pending_rows = store
        .list_provider_state(PENDING_NAMESPACE, MAX_PENDING + 1)
        .map_err(store_problem)?;
    if pending_rows.len() > MAX_PENDING {
        return Err(HostProblem::ResourceExhausted);
    }
    for row in pending_rows {
        let pending: Pending =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
        if pending.schema_version != 1
            || row.key != pending_key(&pending.run_unit, &pending.owner, &pending.name)?
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        if owners.contains(&pending.owner) {
            writes.push(ProviderStateMutation::Delete {
                namespace: row.namespace,
                key: row.key,
                expected_version: row.version,
            });
        }
    }
    if deleted != 0 {
        let prior = store
            .get_provider_state(CAPACITY_NAMESPACE, "global")
            .map_err(store_problem)?;
        if let Some(prior) = prior {
            let mut capacity: Capacity = serde_json::from_slice(&prior.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if capacity.schema_version != 1 {
                return Err(HostProblem::InfrastructureFailure);
            }
            capacity.containers = capacity.containers.saturating_sub(deleted);
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: CAPACITY_NAMESPACE.into(),
                    key: "global".into(),
                    version: next_version(Some(prior.version))?,
                    payload: serde_json::to_vec(&capacity)
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: Some(prior.version),
            }));
        }
    }
    Ok(writes)
}
