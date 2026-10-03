//! Original selected finite MQPUT/PUT1; same candidate and audited publication.
use super::super::producer;
use super::*;
use crate::delivery::full_message::QueueProfile;
use mainframe_env_host_api::mq_md_value::MqMdValue;

fn fields(request: &MqMqiRequest) -> Result<(MqHconn, &MqMqiFullPut), HostProblem> {
    match request {
        MqMqiRequest::FullPut {
            connection, put, ..
        }
        | MqMqiRequest::FullPutOne {
            connection,
            put,
            alternate_user: None,
            ..
        } if matches!(connection, MqHconn::Issued(_)) => {
            put.validate_producer_profile()
                .map_err(|_| HostProblem::Unsupported)?;
            Ok((*connection, put))
        }
        _ => Err(HostProblem::Unsupported),
    }
}
fn target(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    owner: MqHandleOwner,
    request: &MqMqiRequest,
) -> Result<(crate::MqObjectName, Vec<String>), HostProblem> {
    let (connection, _) = fields(request)?;
    require_connection(runtime, owner, connection)?;
    let (queue, path) = match request {
        MqMqiRequest::FullPut { object, .. } => {
            let b = require_object(
                runtime,
                owner,
                connection,
                *object,
                MqRouteOpenAccess::Output,
            )?;
            (b.queue, b.path)
        }
        MqMqiRequest::FullPutOne {
            lookup:
                lookup @ MqRouteLookup::Queue {
                    name,
                    manager: None,
                    dynamic_pattern: None,
                },
            ..
        } => {
            // First producer requires the actual predefined normal definition,
            // no aliases/remote/model/default resolution inferred from a name.
            let q = crate::MqObjectName::new(name.as_str()).map_err(|_| HostProblem::Malformed)?;
            if !state.catalog.definitions().any(|d| matches!(d,
                crate::MqObjectDefinition::LocalQueue { name, usage: crate::MqLocalQueueUsage::Normal, .. } if name == &q)) {
                return Err(HostProblem::Unsupported);
            }
            resolve(&state.catalog, lookup, crate::MqObjectCapability::Output)?
        }
        _ => return Err(HostProblem::Unsupported),
    };
    let (_, put) = fields(request)?;
    let attrs = state
        .catalog
        .native_attributes()
        .ok_or(HostProblem::Unsupported)?;
    let q = attrs
        .queues
        .iter()
        .find(|a| a.name == queue)
        .ok_or(HostProblem::Unsupported)?;
    if q.delivery_sequence != crate::object::MqNativeDeliverySequence::Fifo
        || attrs.characters.md() != put.message.descriptor.characters()
        || state
            .delivery
            .full_queue_profile(&queue)
            .map_err(delivery_error)?
            != (QueueProfile::Complete {
                version: put.message.descriptor.version(),
                characters: attrs.characters.md(),
            })
    {
        return Err(HostProblem::Unsupported);
    }
    if put.message.body.len() > attrs.max_msg_length.min(q.max_msg_length) as usize {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok((queue, path))
}
pub(super) fn prepare(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    invocation: &Invocation,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    request: &MqMqiRequest,
    now: u64,
    authorizer: &dyn EnterpriseAuthorizer,
    next: &mut Candidate,
    service: &MqService,
    frame: FrameLease,
    admitted: &crate::mqi_admission::MqMqiAdmitted<'_>,
) -> Result<(), HostProblem> {
    if owner.environment != MqHostEnvironment::ZosBatch {
        return Err(HostProblem::Unsupported);
    }
    let (connection, put) = fields(request)?;
    let (queue, path) = target(state, runtime, owner, request)?;
    let unit = resolve_unit(next, logical, connection, put.unit)?;
    authorize_path(authorizer, invocation, &path, AccessIntent::Update)?;
    if unit.is_some() {
        authorize(
            authorizer,
            invocation,
            &[resource(
                EnterpriseResourceClass::MqUnitOfWork,
                "CURRENT",
                AccessIntent::Update,
            )?],
        )?;
    }
    producer::recheck(
        service,
        frame,
        invocation,
        admitted,
        &runtime.directory,
        now,
    )?;
    let mut returned = put.message.descriptor.clone();
    let returned_fields = match &mut returned {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields,
    };
    producer::context(
        service,
        invocation,
        &put.context,
        put.message.descriptor.characters(),
        returned_fields,
    )?;
    producer::recheck(
        service,
        frame,
        invocation,
        admitted,
        &runtime.directory,
        service
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?,
    )?;
    let mut stored = put.message.clone();
    stored.descriptor = returned.clone();
    match &mut stored.descriptor {
        MqMdValue::V1 { fields, .. } | MqMdValue::V2 { fields, .. } => fields.backout_count = 0,
    }
    next.delivery.advance_tick(now).map_err(delivery_error)?;
    let outcome = next
        .delivery
        .put_full(&state.catalog, &queue, stored, unit)
        .map_err(delivery_error)?;
    if let Some(unit) = unit {
        next.units
            .get_mut(&unit)
            .ok_or(HostProblem::Malformed)?
            .touch_queue(queue.as_str())?;
    }
    next.output = MqMqiOutput::Produced(MqMqiProduced {
        descriptor: returned,
        outcome,
        resolved_queue: producer::fixed(
            service,
            queue.as_str(),
            put.message.descriptor.characters(),
        )?,
        resolved_manager: producer::fixed(
            service,
            state.catalog.queue_manager().name.as_str(),
            put.message.descriptor.characters(),
        )?,
        known_dest_count: MqMqiDestinationCount::UndefinedZos,
        unknown_dest_count: MqMqiDestinationCount::UndefinedZos,
        invalid_dest_count: MqMqiDestinationCount::UndefinedZos,
        backout_count: MqMqiIgnoredCounter::PreservedIgnoredInput,
    });
    next.reviewed_status = Some(
        MqReviewedStatus::from_symbols(request.call(), "MQCC_OK", "MQRC_NONE")
            .map_err(|_| HostProblem::Unsupported)?,
    );
    Ok(())
}
pub(super) fn require_replay(
    state: &rich_state::RichStoredState,
    runtime: &mut SelectedRuntime,
    logical: &LogicalBatchOwner,
    owner: MqHandleOwner,
    request: &MqMqiRequest,
    invocation: &Invocation,
    authorizer: &dyn EnterpriseAuthorizer,
) -> Result<(), HostProblem> {
    let (connection, put) = fields(request).map_err(|_| HostProblem::UnknownOutcome)?;
    let (_, path) =
        target(state, runtime, owner, request).map_err(|_| HostProblem::UnknownOutcome)?;
    let b = runtime
        .connections
        .iter()
        .find(|b| b.connection == connection)
        .ok_or(HostProblem::UnknownOutcome)?;
    state
        .ownership
        .units
        .get(&b.unit)
        .ok_or(HostProblem::UnknownOutcome)?
        .require_owner(logical, &b.key, &runtime.control, b.unit)
        .map_err(|_| HostProblem::UnknownOutcome)?;
    if matches!(put.unit, MqMqiUnitOfWork::Local {unit} if unit != b.unit) {
        return Err(HostProblem::UnknownOutcome);
    }
    authorize_path(authorizer, invocation, &path, AccessIntent::Update)?;
    if matches!(put.unit, MqMqiUnitOfWork::Local { .. }) {
        authorize(
            authorizer,
            invocation,
            &[resource(
                EnterpriseResourceClass::MqUnitOfWork,
                "CURRENT",
                AccessIntent::Update,
            )?],
        )?;
    }
    Ok(())
}
