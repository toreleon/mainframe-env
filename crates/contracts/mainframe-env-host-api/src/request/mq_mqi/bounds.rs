//! Host field ceilings in addition to the unchanged MQI structural validator.

use super::*;
use crate::mq_mqi::*;
use crate::mq_object_route::{MqRouteAlternateUser, MqRouteLookup};
use crate::{MqGetContract, MqMessage, MqMessageDescriptor, MqMessageProperty, MqPropertyQuery};

fn bound(value: usize, ceiling: usize) -> Result<(), HostProblem> {
    if value > ceiling {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(())
    }
}

fn name(value: &str, limits: HostLimits) -> Result<(), HostProblem> {
    bound(value.len(), limits.max_name_bytes)
}

fn descriptor(value: &MqMessageDescriptor, limits: HostLimits) -> Result<(), HostProblem> {
    for value in [
        value.identifiers.message_id.as_deref(),
        value.identifiers.correlation_id.as_deref(),
        value.identifiers.group_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        bound(value.len(), limits.max_record_bytes)?;
    }
    if let Some(format) = &value.format {
        name(format, limits)?;
    }
    Ok(())
}

fn property(value: &MqMessageProperty, limits: HostLimits) -> Result<(), HostProblem> {
    name(&value.name, limits)?;
    bound(value.value.len(), limits.max_record_bytes)
}

fn message(value: &MqMessage, limits: HostLimits) -> Result<(), HostProblem> {
    descriptor(&value.descriptor, limits)?;
    bound(value.body.len(), limits.max_record_bytes)?;
    bound(value.properties.len(), limits.max_fields)?;
    let mut total = 0usize;
    for value in &value.properties {
        property(value, limits)?;
        total = total
            .checked_add(value.name.len())
            .and_then(|total| total.checked_add(value.value.len()))
            .ok_or(HostProblem::ResourceExhausted)?;
        bound(total, limits.max_state_bytes)?;
    }
    Ok(())
}

fn get(value: &MqGetContract, limits: HostLimits) -> Result<(), HostProblem> {
    for value in [
        value.selection.identifiers.message_id.as_deref(),
        value.selection.identifiers.correlation_id.as_deref(),
        value.selection.identifiers.group_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        bound(value.len(), limits.max_record_bytes)?;
    }
    bound(value.buffer_capacity, limits.max_record_bytes)
}

fn query(value: &MqPropertyQuery, limits: HostLimits) -> Result<(), HostProblem> {
    match value {
        MqPropertyQuery::Exact(value) | MqPropertyQuery::Prefix(value) => name(value, limits),
    }
}

fn alternate(value: Option<&MqRouteAlternateUser>, limits: HostLimits) -> Result<(), HostProblem> {
    if let Some(value) = value {
        name(value.as_str(), limits)?;
    }
    Ok(())
}

fn lookup(value: &MqRouteLookup, limits: HostLimits) -> Result<(), HostProblem> {
    match value {
        MqRouteLookup::Queue {
            name: queue,
            manager,
            dynamic_pattern,
        } => {
            name(queue.as_str(), limits)?;
            if let Some(manager) = manager {
                name(manager.as_str(), limits)?;
            }
            if let Some(pattern) = dynamic_pattern {
                name(pattern.as_str(), limits)?;
            }
        }
        MqRouteLookup::Topic {
            name: topic,
            object_string,
        } => {
            name(topic.as_str(), limits)?;
            if let Some(text) = object_string {
                bound(text.as_str().len(), limits.max_record_bytes)?;
            }
        }
        MqRouteLookup::Process(value) | MqRouteLookup::Namelist(value) => {
            name(value.as_str(), limits)?
        }
        MqRouteLookup::QueueManager | MqRouteLookup::DistributionList => {}
    }
    Ok(())
}

fn put(value: &MqMqiPut, limits: HostLimits) -> Result<(), HostProblem> {
    message(&value.message, limits)?;
    match &value.context {
        MqMqiMessageContext::SetIdentityPending { user }
        | MqMqiMessageContext::SetAllPending { user } => alternate(Some(user), limits),
        MqMqiMessageContext::Default
        | MqMqiMessageContext::PassIdentityPending { .. }
        | MqMqiMessageContext::PassAllPending { .. } => Ok(()),
    }
}

pub(super) fn request(value: &MqMqiRequest, limits: HostLimits) -> Result<(), HostProblem> {
    use MqMqiRequest as R;
    match value {
        R::Connect(value) | R::ConnectExtended(value) => {
            if let Some(manager) = &value.manager {
                name(manager.as_str(), limits)?;
            }
            Ok(())
        }
        R::Open(value) => {
            bound(value.access().len(), limits.max_fields)?;
            lookup(value.lookup(), limits)?;
            alternate(value.modifiers().alternate_user.as_ref(), limits)
        }
        R::Get(value) => get(&value.get, limits),
        R::Put { put: value, .. } => put(value, limits),
        R::PutOne {
            lookup: route,
            alternate_user,
            put: value,
            ..
        } => {
            lookup(route, limits)?;
            alternate(alternate_user.as_ref(), limits)?;
            put(value, limits)
        }
        R::Inquire(value) => {
            bound(value.selectors.len(), limits.max_fields)?;
            bound(value.integer_capacity, limits.max_fields)?;
            bound(value.character_capacity, limits.max_record_bytes)
        }
        R::Set(value) => {
            bound(value.selectors.len(), limits.max_fields)?;
            bound(value.integers.len(), limits.max_fields)?;
            bound(value.characters.len(), limits.max_record_bytes)
        }
        R::InquireProperty(value) => {
            query(&value.query, limits)?;
            if let Some(after) = &value.after {
                name(after, limits)?;
            }
            bound(value.name_capacity, limits.max_name_bytes)?;
            bound(value.value_capacity, limits.max_record_bytes)
        }
        R::DeleteProperty { query: value, .. } => query(value, limits),
        R::SetProperty {
            property: value, ..
        } => property(value, limits),
        R::BufferToHandle(value) | R::HandleToBuffer(value) => {
            descriptor(&value.descriptor, limits)?;
            query(&value.query, limits)?;
            bound(value.capacity, limits.max_record_bytes)?;
            bound(value.buffer.len(), limits.max_record_bytes)
        }
        R::Callback { operation, .. } => match operation {
            MqMqiCallbackOperation::Register { get: value, .. } => get(value, limits),
            MqMqiCallbackOperation::Deregister
            | MqMqiCallbackOperation::Suspend
            | MqMqiCallbackOperation::Resume
            | MqMqiCallbackOperation::EventHandlerPending => Ok(()),
        },
        R::Subscribe(value) => name(value.name.as_str(), limits),
        R::CallbackFunction { .. } => Err(HostProblem::Malformed),
        R::Back { .. }
        | R::Begin { .. }
        | R::Commit { .. }
        | R::Close(_)
        | R::CreateMessageHandle { .. }
        | R::Control { .. }
        | R::Disconnect { .. }
        | R::DeleteMessageHandle { .. }
        | R::Stat { .. }
        | R::SubscriptionRequest { .. } => Ok(()),
    }
}

pub(super) fn result(value: &MqMqiResult, limits: HostLimits) -> Result<(), HostProblem> {
    use MqMqiOutcome as R;
    use MqMqiOutput as O;
    let output = match &value.outcome {
        R::Completed { output, .. } | R::StatusPending { output } => output,
        R::CallbackReturned { .. } => return Err(HostProblem::Malformed),
        R::ReviewedStatus { .. } | R::Pending(_) | R::UnknownOutcome | R::DuplicatePossible => {
            return Ok(());
        }
    };
    match output {
        O::Opened { dynamic, .. } => {
            if let Some(value) = dynamic {
                name(value.model.as_str(), limits)?;
                name(value.name.as_str(), limits)?;
            }
            Ok(())
        }
        O::Got {
            message: value,
            disposition,
            ..
        } => {
            if let crate::MqGetDisposition::Message(truncation) = disposition {
                use crate::MqTruncationDisposition as T;
                match truncation {
                    T::Complete { length } => bound(*length, limits.max_record_bytes)?,
                    T::RejectedRetained { required, copied }
                    | T::AcceptedRemoved { required, copied }
                    | T::AcceptedBrowsed { required, copied } => {
                        bound(*required, limits.max_record_bytes)?;
                        bound(*copied, limits.max_record_bytes)?;
                    }
                }
            }
            if let Some(value) = value {
                message(value, limits)?;
            }
            Ok(())
        }
        O::Put {
            descriptor: value, ..
        } => descriptor(value, limits),
        O::Distribution(value) => {
            bound(value.items.len(), limits.max_records)?;
            for value in &value.items {
                name(&value.destination, limits)?;
            }
            Ok(())
        }
        O::Property(value) => property(value, limits),
        O::Buffer {
            descriptor: value,
            bytes,
            data_length,
        } => {
            descriptor(value, limits)?;
            bound(bytes.len(), limits.max_record_bytes)?;
            bound(*data_length, limits.max_record_bytes)
        }
        O::Attributes {
            integers,
            characters,
        } => {
            bound(integers.len(), limits.max_fields)?;
            bound(characters.len(), limits.max_record_bytes)
        }
        O::PublicationsRequested { count } => bound(*count, limits.max_records),
        O::Connected(_)
        | O::MessageHandle(_)
        | O::Subscribed { .. }
        | O::UnitOfWork { .. }
        | O::NoOutput => Ok(()),
    }
}
