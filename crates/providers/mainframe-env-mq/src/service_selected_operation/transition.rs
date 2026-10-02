//! Selected ordinary-batch transitions use the existing delivery/handle owners.
//! No publication or adoption is permitted by these staged candidates alone.

use super::*;
use mainframe_env_host_api::mq_object_route::*;
use mainframe_env_host_api::{
    MqDeliveryOutcome, MqGetMode, MqHandle, MqHandleKind, MqHandleSharing, MqHconn, MqHobj,
    MqPersistence,
};

#[path = "transition/connection_warning.rs"]
mod connection_warning;
#[path = "transition/full_get.rs"]
mod full_get;

#[derive(Clone)]
pub(super) struct ConnectionBinding {
    pub(super) connection: MqHconn,
    pub(super) key: String,
    pub(super) unit: u64,
}
#[derive(Clone)]
pub(super) struct ObjectBinding {
    object: MqHobj,
    connection: MqHconn,
    queue: crate::MqObjectName,
    path: Vec<String>,
    access: Vec<MqRouteOpenAccess>,
}
pub(super) enum HandleAction {
    None,
    NewConnection(MqHconn),
    NewObject(MqHconn, MqHobj),
    Close(MqHconn, MqHobj),
    Disconnect(MqHconn),
}

pub(super) struct Candidate {
    pub(super) delivery: crate::MqDeliveryKernel,
    pub(super) control: Control,
    pub(super) units: BTreeMap<u64, ownership::UnitOwner>,
    pub(super) connections: Vec<ConnectionBinding>,
    pub(super) objects: Vec<ObjectBinding>,
    pub(super) output: MqMqiOutput,
    pub(super) reviewed_status: Option<MqReviewedStatus>,
    pub(super) handle: HandleAction,
    pub(super) unit_dependencies: Vec<u64>,
    pub(super) property: Option<MqPropertyRequest>,
}

fn handle_error(_: mainframe_env_host_api::MqHandleProblem) -> HostProblem {
    HostProblem::Malformed
}
fn delivery_error(error: crate::MqDeliveryError) -> HostProblem {
    match error {
        crate::MqDeliveryError::ResourceExhausted | crate::MqDeliveryError::InvalidLimits => {
            HostProblem::ResourceExhausted
        }
        crate::MqDeliveryError::Unsupported => HostProblem::Unsupported,
        crate::MqDeliveryError::UnknownQueue => HostProblem::NotFound,
        _ => HostProblem::Malformed,
    }
}

pub(super) fn prepare(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    invocation: &Invocation,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    request: &MqMqiRequest,
    key: &str,
    now: u64,
    authorizer: &dyn EnterpriseAuthorizer,
    limits: MqLimits,
) -> Result<Candidate, HostProblem> {
    if logical.owner() != owner {
        return Err(HostProblem::Unauthorized);
    }
    let mut next = Candidate {
        delivery: state.delivery.clone(),
        control: runtime.control.clone(),
        units: state.ownership.units.clone(),
        connections: runtime.connections.clone(),
        objects: runtime.objects.clone(),
        output: MqMqiOutput::NoOutput,
        reviewed_status: None,
        handle: HandleAction::None,
        unit_dependencies: Vec::new(),
        property: None,
    };
    let connection = match request {
        MqMqiRequest::Property(request) => Some(request.connection()),
        MqMqiRequest::Open(open) => Some(open.connection()),
        MqMqiRequest::Put { connection, .. }
        | MqMqiRequest::PutOne { connection, .. }
        | MqMqiRequest::Commit { connection, .. }
        | MqMqiRequest::Back { connection, .. }
        | MqMqiRequest::Disconnect { connection } => Some(*connection),
        MqMqiRequest::Get(get) => Some(get.connection),
        MqMqiRequest::FullGet(get) => Some(get.connection),
        MqMqiRequest::Close(close) => Some(close.connection()),
        _ => None,
    };
    if let Some(connection) = connection {
        require_connection(runtime, owner, connection)?;
        let binding = next
            .connections
            .iter()
            .find(|b| b.connection == connection)
            .ok_or(HostProblem::Malformed)?;
        next.units
            .get(&binding.unit)
            .ok_or(HostProblem::Malformed)?
            .require_owner(logical, &binding.key, &next.control, binding.unit)?;
        next.unit_dependencies.push(binding.unit);
    }
    if let MqMqiRequest::Connect(connect) | MqMqiRequest::ConnectExtended(connect) = request {
        connection_warning::validate_profile(state, connect)?;
        if let Some(binding) = connection_warning::prior(state, runtime, logical, owner)? {
            authorize(
                authorizer,
                invocation,
                &[resource(
                    EnterpriseResourceClass::MqUnitOfWork,
                    "CURRENT",
                    AccessIntent::Execute,
                )?],
            )?;
            next.unit_dependencies.push(binding.unit);
            next.output = MqMqiOutput::Connected(binding.connection);
            next.reviewed_status = Some(
                MqReviewedStatus::from_symbols(
                    request.call(),
                    "MQCC_WARNING",
                    "MQRC_ALREADY_CONNECTED",
                )
                .map_err(|_| HostProblem::Unsupported)?,
            );
            // Reuse is not queue activity or expiry. Publish only the original
            // occurrence plus existing control/UOW/catalog/marker dependencies.
            return Ok(next);
        }
    }
    if let MqMqiRequest::FullGet(get) = request {
        // Complete profile/payload preflight occurs before clock, pending or
        // cursor changes can become a publication candidate.
        full_get::prepare(
            state, runtime, invocation, logical, owner, get, now, authorizer, &mut next,
        )?;
        return Ok(next);
    }
    // Actual clock expiry changes only the candidate, never the current queues.
    next.delivery.advance_tick(now).map_err(delivery_error)?;
    match request {
        MqMqiRequest::Property(request) => {
            super::property::prepare(runtime, invocation, owner, request, authorizer, &mut next)?
        }
        MqMqiRequest::Connect(connect) | MqMqiRequest::ConnectExtended(connect) => {
            // A checked SAME TASK child can establish this processing unit's
            // first connection. Registry ownership comes from the directory;
            // durable provenance retains its logical origin and this caller's
            // original CONNECT key. Ordinary CALL return does not end that task.
            authorize(
                authorizer,
                invocation,
                &[resource(
                    EnterpriseResourceClass::MqUnitOfWork,
                    "CURRENT",
                    AccessIntent::Execute,
                )?],
            )?;
            if next.units.len() >= limits.max_pending_units {
                return Err(HostProblem::ResourceExhausted);
            }
            let unit = next.control.allocate(logical, key)?;
            let connection = runtime
                .handles
                .handles_mut()
                .connect(owner, connect.sharing)
                .map_err(handle_error)?;
            next.connections.push(ConnectionBinding {
                connection,
                key: key.into(),
                unit: unit.unit,
            });
            next.units.insert(unit.unit, unit);
            next.output = MqMqiOutput::Connected(connection);
            next.handle = HandleAction::NewConnection(connection);
        }
        MqMqiRequest::Open(open) => {
            require_connection(runtime, owner, open.connection())?;
            if open.modifiers() != &MqRouteOpenModifiers::default() {
                return Err(HostProblem::Unsupported);
            }
            let mut target = None;
            let mut path = Vec::new();
            let mut resources = Vec::new();
            for access in open.access() {
                let (capability, intent) = match access {
                    MqRouteOpenAccess::InputShared => {
                        (crate::MqObjectCapability::Input, AccessIntent::Read)
                    }
                    MqRouteOpenAccess::Output => {
                        (crate::MqObjectCapability::Output, AccessIntent::Update)
                    }
                    MqRouteOpenAccess::Browse => {
                        (crate::MqObjectCapability::Browse, AccessIntent::Read)
                    }
                    _ => return Err(HostProblem::Unsupported),
                };
                let (queue, resolved) = resolve(&state.catalog, open.lookup(), capability)?;
                if target.as_ref().is_some_and(|current| current != &queue) {
                    return Err(HostProblem::Malformed);
                }
                target = Some(queue);
                path = resolved;
                resources.extend(
                    path.iter()
                        .map(|name| resource(EnterpriseResourceClass::MqQueue, name, intent))
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            authorize(authorizer, invocation, &resources)?;
            let object = runtime
                .handles
                .handles_mut()
                .create_object(owner, open.connection())
                .map_err(handle_error)?;
            next.objects.push(ObjectBinding {
                object,
                connection: open.connection(),
                queue: target.ok_or(HostProblem::Malformed)?,
                path,
                access: open.access().to_vec(),
            });
            next.output = MqMqiOutput::Opened {
                object,
                dynamic: None,
            };
            next.handle = HandleAction::NewObject(open.connection(), object);
        }
        MqMqiRequest::Put {
            connection,
            object,
            put,
        } => {
            let binding = require_object(
                runtime,
                owner,
                *connection,
                *object,
                MqRouteOpenAccess::Output,
            )?;
            authorize_path(authorizer, invocation, &binding.path, AccessIntent::Update)?;
            put_message(
                &mut next,
                logical,
                *connection,
                put,
                &binding.queue,
                &state.catalog,
            )?;
        }
        MqMqiRequest::PutOne {
            connection,
            lookup,
            alternate_user,
            put,
        } => {
            // PUT1 omits HOBJ, NEVER its actual connection admission.
            require_connection(runtime, owner, *connection)?;
            if alternate_user.is_some() {
                return Err(HostProblem::Unsupported);
            }
            let (queue, path) = resolve(&state.catalog, lookup, crate::MqObjectCapability::Output)?;
            authorize_path(authorizer, invocation, &path, AccessIntent::Update)?;
            put_message(&mut next, logical, *connection, put, &queue, &state.catalog)?;
        }
        MqMqiRequest::Get(get) => {
            if get.message_handle.is_some()
                || get.get.wait != mainframe_env_host_api::MqWait::NoWait
            {
                return Err(HostProblem::Unsupported);
            }
            let access = if matches!(
                get.get.mode,
                MqGetMode::BrowseFirst | MqGetMode::BrowseNext { .. }
            ) {
                MqRouteOpenAccess::Browse
            } else {
                MqRouteOpenAccess::InputShared
            };
            let binding = require_object(runtime, owner, get.connection, get.object, access)?;
            authorize_path(authorizer, invocation, &binding.path, AccessIntent::Read)?;
            let unit = resolve_unit(&next, logical, get.connection, get.unit)?;
            let got = next
                .delivery
                .get(&state.catalog, &binding.queue, &get.get, unit)
                .map_err(delivery_error)?;
            // Keep every descriptor/copied byte/required length/cursor for the
            // original-request reviewed output constructor before publication.
            if let Some(unit) = unit
                && matches!(next.delivery.unit_outcome(unit), MqDeliveryOutcome::Pending)
            {
                next.units
                    .get_mut(&unit)
                    .ok_or(HostProblem::Malformed)?
                    .touch_queue(binding.queue.as_str())?;
            }
            next.output = MqMqiOutput::Got {
                disposition: got.disposition,
                message: got.message,
                cursor: got.cursor,
            };
        }
        MqMqiRequest::Commit { connection, unit } | MqMqiRequest::Back { connection, unit } => {
            require_connection(runtime, owner, *connection)?;
            resolve_unit(
                &next,
                logical,
                *connection,
                MqMqiUnitOfWork::Local { unit: *unit },
            )?;
            let current = next.units.get(unit).ok_or(HostProblem::Malformed)?;
            let mut resources = vec![resource(
                EnterpriseResourceClass::MqUnitOfWork,
                "CURRENT",
                AccessIntent::Update,
            )?];
            resources.extend(
                current
                    .queues
                    .iter()
                    .map(|q| resource(EnterpriseResourceClass::MqQueue, q, AccessIntent::Update))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            authorize(authorizer, invocation, &resources)?;
            let commit = matches!(request, MqMqiRequest::Commit { .. });
            if matches!(
                next.delivery.unit_outcome(*unit),
                MqDeliveryOutcome::Pending
            ) {
                let outcome = if commit {
                    next.delivery.commit(*unit)
                } else if owner.environment == MqHostEnvironment::ZosBatch {
                    // Actual original ordinary batch admission and current
                    // logical unit/SAF checks above precede this live policy.
                    // No cold/task-end/HardenGetBackout inference is permitted.
                    next.delivery.backout_complete_zos(*unit)
                } else {
                    next.delivery.backout(*unit)
                }
                .map_err(delivery_error)?;
                if (commit && outcome != MqDeliveryOutcome::Accepted)
                    || (!commit && outcome != MqDeliveryOutcome::Rejected)
                {
                    return Err(HostProblem::UnknownOutcome);
                }
            }
            next.units
                .get_mut(unit)
                .ok_or(HostProblem::Malformed)?
                .finalize(commit)?;
            let binding = next
                .connections
                .iter_mut()
                .find(|b| b.connection == *connection)
                .ok_or(HostProblem::Malformed)?;
            let fresh = next.control.allocate(logical, &binding.key)?;
            binding.unit = fresh.unit;
            next.units.insert(fresh.unit, fresh);
            next.output = MqMqiOutput::UnitOfWork { unit: *unit };
        }
        MqMqiRequest::Close(close) => {
            let MqRouteCloseTarget::Object {
                handle,
                lifecycle: MqRouteCloseLifecycle::Predefined,
            } = close.target()
            else {
                return Err(HostProblem::Unsupported);
            };
            if close.mode() != MqRouteCloseMode::None {
                return Err(HostProblem::Unsupported);
            }
            require_connection(runtime, owner, close.connection())?;
            runtime
                .handles
                .handles_mut()
                .validate(
                    owner,
                    close.connection(),
                    handle.into(),
                    MqHandleKind::Object,
                )
                .map_err(handle_error)?;
            let binding = runtime
                .objects
                .iter()
                .find(|b| b.object == handle && b.connection == close.connection())
                .ok_or(HostProblem::Malformed)?;
            authorize_path(authorizer, invocation, &binding.path, AccessIntent::Execute)?;
            next.objects.retain(|b| b.object != handle);
            next.handle = HandleAction::Close(close.connection(), handle);
        }
        MqMqiRequest::Disconnect { connection } => {
            require_connection(runtime, owner, *connection)?;
            let binding = next
                .connections
                .iter()
                .find(|b| b.connection == *connection)
                .ok_or(HostProblem::Malformed)?;
            let unit = next
                .units
                .get(&binding.unit)
                .ok_or(HostProblem::Malformed)?;
            unit.require_owner(logical, &binding.key, &next.control, binding.unit)?;
            // Source-pinned ordinary batch MQDISC commits the local connection
            // UOW. Its exact owner and all affected resources remain mandatory.
            let mut resources = vec![resource(
                EnterpriseResourceClass::MqUnitOfWork,
                "CURRENT",
                AccessIntent::Execute,
            )?];
            resources.extend(
                unit.queues
                    .iter()
                    .map(|q| resource(EnterpriseResourceClass::MqQueue, q, AccessIntent::Update))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            authorize(authorizer, invocation, &resources)?;
            let id = binding.unit;
            if next.delivery.unit_outcome(id) == MqDeliveryOutcome::Pending
                && next.delivery.commit(id).map_err(delivery_error)? != MqDeliveryOutcome::Accepted
            {
                return Err(HostProblem::UnknownOutcome);
            }
            next.units
                .get_mut(&id)
                .ok_or(HostProblem::Malformed)?
                .finalize(true)?;
            next.connections.retain(|b| b.connection != *connection);
            next.objects.retain(|b| b.connection != *connection);
            next.handle = HandleAction::Disconnect(*connection);
        }
        _ => return Err(HostProblem::Unsupported),
    }
    Ok(next)
}

pub(super) fn require_connection(
    runtime: &mut SelectedRuntime,
    owner: MqHandleOwner,
    connection: MqHconn,
) -> Result<(), HostProblem> {
    runtime
        .handles
        .handles_mut()
        .validate_connection(owner, connection)
        .map_err(handle_error)?;
    if runtime
        .connections
        .iter()
        .any(|b| b.connection == connection)
    {
        Ok(())
    } else {
        Err(HostProblem::Malformed)
    }
}
fn require_object(
    runtime: &mut SelectedRuntime,
    owner: MqHandleOwner,
    connection: MqHconn,
    object: MqHobj,
    access: MqRouteOpenAccess,
) -> Result<ObjectBinding, HostProblem> {
    require_connection(runtime, owner, connection)?;
    runtime
        .handles
        .handles_mut()
        .validate(
            owner,
            connection,
            MqHandle::Object(object),
            MqHandleKind::Object,
        )
        .map_err(handle_error)?;
    runtime
        .objects
        .iter()
        .find(|b| b.connection == connection && b.object == object && b.access.contains(&access))
        .cloned()
        .ok_or(HostProblem::Unauthorized)
}
fn resolve(
    catalog: &MqObjectCatalog,
    lookup: &MqRouteLookup,
    capability: crate::MqObjectCapability,
) -> Result<(crate::MqObjectName, Vec<String>), HostProblem> {
    let MqRouteLookup::Queue {
        name,
        manager: None,
        dynamic_pattern: None,
    } = lookup
    else {
        return Err(HostProblem::Unsupported);
    };
    let resolution = catalog
        .resolve(
            &crate::MqObjectLookup::Queue(
                crate::MqObjectName::new(name.as_str()).map_err(|_| HostProblem::Malformed)?,
            ),
            capability,
        )
        .map_err(crate::object_service::object_problem)?;
    let crate::MqResolvedTarget::Queue { name, .. } = resolution.target else {
        return Err(HostProblem::Unsupported);
    };
    Ok((
        name,
        resolution
            .path
            .into_iter()
            .map(|i| i.name.as_str().into())
            .collect(),
    ))
}
fn resolve_unit(
    candidate: &Candidate,
    logical: &LogicalBatchOwner,
    connection: MqHconn,
    unit: MqMqiUnitOfWork,
) -> Result<Option<u64>, HostProblem> {
    match unit {
        MqMqiUnitOfWork::NoSyncpoint => Ok(None),
        MqMqiUnitOfWork::ExternalPending { .. } => Err(HostProblem::Unsupported),
        MqMqiUnitOfWork::Local { unit } => {
            let binding = candidate
                .connections
                .iter()
                .find(|b| b.connection == connection)
                .ok_or(HostProblem::Malformed)?;
            if binding.unit != unit {
                return Err(HostProblem::IdempotencyConflict);
            }
            candidate
                .units
                .get(&unit)
                .ok_or(HostProblem::Malformed)?
                .require_owner(logical, &binding.key, &candidate.control, unit)?;
            Ok(Some(unit))
        }
    }
}
fn put_message(
    candidate: &mut Candidate,
    logical: &LogicalBatchOwner,
    connection: MqHconn,
    put: &MqMqiPut,
    queue: &crate::MqObjectName,
    catalog: &MqObjectCatalog,
) -> Result<(), HostProblem> {
    if put.message_handle.is_some()
        || put.context != MqMqiMessageContext::Default
        || put.options != MqMqiOptions::ContractDefault
        || put.message.descriptor.identifiers.message_id.is_none()
        || !matches!(
            put.message.descriptor.persistence,
            MqPersistence::Persistent | MqPersistence::NonPersistent
        )
    {
        // Kernel-generated descriptor fields need an owned return hook; copying
        // the input as a supposedly generated descriptor would lose output.
        return Err(HostProblem::Unsupported);
    }
    let unit = resolve_unit(candidate, logical, connection, put.unit)?;
    let outcome = candidate
        .delivery
        .put_one(catalog, queue, put.message.clone(), unit)
        .map_err(delivery_error)?;
    if let Some(unit) = unit {
        candidate
            .units
            .get_mut(&unit)
            .ok_or(HostProblem::Malformed)?
            .touch_queue(queue.as_str())?;
    }
    candidate.output = MqMqiOutput::Put {
        descriptor: put.message.descriptor.clone(),
        outcome,
    };
    Ok(())
}
fn resource(
    class: EnterpriseResourceClass,
    name: &str,
    intent: AccessIntent,
) -> Result<EnterpriseResource, HostProblem> {
    EnterpriseResource::new(class, name, intent)
}
fn authorize(
    authorizer: &dyn EnterpriseAuthorizer,
    invocation: &Invocation,
    resources: &[EnterpriseResource],
) -> Result<(), HostProblem> {
    for resource in resources {
        authorizer.authorize(invocation.principal.id(), resource)?;
    }
    Ok(())
}
fn authorize_path(
    authorizer: &dyn EnterpriseAuthorizer,
    invocation: &Invocation,
    path: &[String],
    intent: AccessIntent,
) -> Result<(), HostProblem> {
    authorize(
        authorizer,
        invocation,
        &path
            .iter()
            .map(|name| resource(EnterpriseResourceClass::MqQueue, name, intent))
            .collect::<Result<Vec<_>, _>>()?,
    )
}

pub(super) fn discard(
    runtime: &mut SelectedRuntime,
    owner: MqHandleOwner,
    candidate: &mut Candidate,
) -> Result<(), HostProblem> {
    let handle = std::mem::replace(&mut candidate.handle, HandleAction::None);
    match handle {
        HandleAction::NewConnection(connection) => runtime
            .handles
            .disconnect(owner, connection)
            .map_err(|_| HostProblem::UnknownOutcome),
        HandleAction::NewObject(connection, object) => runtime
            .handles
            .handles_mut()
            .release(owner, connection, object.into(), MqHandleKind::Object)
            .map_err(|_| HostProblem::UnknownOutcome),
        _ => Ok(()),
    }
}
pub(super) fn adopt(
    runtime: &mut SelectedRuntime,
    owner: MqHandleOwner,
    candidate: &mut Candidate,
) -> Result<(), HostProblem> {
    let handle = std::mem::replace(&mut candidate.handle, HandleAction::None);
    match handle {
        HandleAction::Close(connection, object) => runtime
            .handles
            .handles_mut()
            .release(owner, connection, object.into(), MqHandleKind::Object)
            .map_err(|_| HostProblem::UnknownOutcome),
        HandleAction::Disconnect(connection) => runtime
            .handles
            .disconnect(owner, connection)
            .map_err(|_| HostProblem::UnknownOutcome),
        _ => Ok(()),
    }
}
pub(super) fn has_handle_reply(reply: &EffectResult) -> bool {
    matches!(
        &reply.outcome,
        Ok(HostResult::MqMqi(MqMqiHostResult {
            result: MqMqiResult {
                outcome: MqMqiOutcome::Completed {
                    output: MqMqiOutput::Connected(_)
                        | MqMqiOutput::Opened { .. }
                        | MqMqiOutput::MessageHandle(_)
                        | MqMqiOutput::Subscribed { .. },
                    ..
                } | MqMqiOutcome::ReviewedOutput {
                    output: MqMqiOutput::Connected(_)
                        | MqMqiOutput::Opened { .. }
                        | MqMqiOutput::MessageHandle(_)
                        | MqMqiOutput::Subscribed { .. },
                    ..
                } | MqMqiOutcome::StatusPending {
                    output: MqMqiOutput::Connected(_)
                        | MqMqiOutput::Opened { .. }
                        | MqMqiOutput::MessageHandle(_)
                        | MqMqiOutput::Subscribed { .. }
                },
                ..
            },
            ..
        }))
    )
}

pub(super) fn resolve_reply(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    request: &MqMqiRequest,
    reply: &mut EffectResult,
    invocation: &Invocation,
    authorizer: &dyn EnterpriseAuthorizer,
) -> Result<(), HostProblem> {
    let Ok(HostResult::MqMqi(reply)) = &mut reply.outcome else {
        return Err(HostProblem::UnknownOutcome);
    };
    if let MqMqiRequest::Property(request) = request {
        return super::property::replay(state, runtime, logical, owner, request, reply);
    }
    match (&mut reply.result.outcome, request) {
        (_, MqMqiRequest::FullGet(get)) => {
            full_get::require_replay(state, runtime, logical, owner, get, invocation, authorizer)
        }
        (
            MqMqiOutcome::Completed {
                output: MqMqiOutput::Connected(connection),
                ..
            }
            | MqMqiOutcome::ReviewedOutput {
                output: MqMqiOutput::Connected(connection),
                ..
            },
            MqMqiRequest::Connect(_) | MqMqiRequest::ConnectExtended(_),
        ) => {
            let observation =
                mainframe_env_host_api::MqHandleObservation::capture_connection(*connection)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
            let live = runtime
                .handles
                .handles_mut()
                .resolve_observed_connection(owner, observation)
                .map_err(|_| HostProblem::UnknownOutcome)?;
            require_connection(runtime, owner, live).map_err(|_| HostProblem::UnknownOutcome)?;
            let prior = connection_warning::prior(state, runtime, logical, owner)
                .map_err(|_| HostProblem::UnknownOutcome)?
                .ok_or(HostProblem::UnknownOutcome)?;
            if prior.connection != live {
                return Err(HostProblem::UnknownOutcome);
            }
            *connection = live;
            Ok(())
        }
        (
            MqMqiOutcome::Completed {
                output:
                    MqMqiOutput::Opened {
                        object,
                        dynamic: None,
                    },
                ..
            }
            | MqMqiOutcome::ReviewedOutput {
                output:
                    MqMqiOutput::Opened {
                        object,
                        dynamic: None,
                    },
                ..
            },
            MqMqiRequest::Open(open),
        ) => {
            require_connection(runtime, owner, open.connection())
                .map_err(|_| HostProblem::UnknownOutcome)?;
            let observation =
                mainframe_env_host_api::MqHandleObservation::from(MqHandle::Object(*object));
            let live = runtime
                .handles
                .handles_mut()
                .resolve_observed_handle(
                    owner,
                    open.connection(),
                    observation,
                    MqHandleKind::Object,
                )
                .map_err(|_| HostProblem::UnknownOutcome)?;
            let MqHandle::Object(live) = live else {
                return Err(HostProblem::UnknownOutcome);
            };
            if !runtime
                .objects
                .iter()
                .any(|b| b.object == live && b.connection == open.connection())
            {
                return Err(HostProblem::UnknownOutcome);
            }
            *object = live;
            Ok(())
        }
        (
            MqMqiOutcome::Completed {
                output:
                    MqMqiOutput::Connected(_)
                    | MqMqiOutput::Opened { .. }
                    | MqMqiOutput::MessageHandle(_)
                    | MqMqiOutput::Subscribed { .. },
                ..
            }
            | MqMqiOutcome::ReviewedOutput {
                output:
                    MqMqiOutput::Connected(_)
                    | MqMqiOutput::Opened { .. }
                    | MqMqiOutput::MessageHandle(_)
                    | MqMqiOutput::Subscribed { .. },
                ..
            }
            | MqMqiOutcome::StatusPending {
                output:
                    MqMqiOutput::Connected(_)
                    | MqMqiOutput::Opened { .. }
                    | MqMqiOutput::MessageHandle(_)
                    | MqMqiOutput::Subscribed { .. },
            },
            _,
        ) => Err(HostProblem::Unsupported),
        _ => Ok(()),
    }
}
