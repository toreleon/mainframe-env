use super::*;

impl MqMqiLimits {
    pub fn validate(self) -> Result<(), MqMqiProblem> {
        self.message.validate().map_err(MqMqiProblem::Message)?;
        let max = Self::default();
        if [
            (self.selectors, max.selectors),
            (self.attribute_bytes, max.attribute_bytes),
            (self.buffer_bytes, max.buffer_bytes),
            (self.canonical_bytes, max.canonical_bytes),
        ]
        .iter()
        .any(|(value, ceiling)| *value == 0 || value > ceiling)
        {
            return Err(MqMqiProblem::Limits);
        }
        Ok(())
    }
}

fn unit(value: MqMqiUnitOfWork) -> Result<(), MqMqiProblem> {
    match value {
        MqMqiUnitOfWork::NoSyncpoint => Ok(()),
        MqMqiUnitOfWork::Local { unit } | MqMqiUnitOfWork::ExternalPending { unit }
            if unit != 0 =>
        {
            Ok(())
        }
        _ => Err(MqMqiProblem::Unit),
    }
}

pub(super) fn descriptor(
    value: &MqMessageDescriptor,
    limits: MqMessageLimits,
) -> Result<(), MqMqiProblem> {
    // Preflight every allocating field before using the existing validator.
    if [
        value.identifiers.message_id.as_deref(),
        value.identifiers.correlation_id.as_deref(),
        value.identifiers.group_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|bytes| bytes.len() > limits.identifier_bytes)
        || value
            .format
            .as_ref()
            .is_some_and(|text| text.len() > limits.format_bytes)
    {
        return Err(MqMqiProblem::Message(crate::MqMessageProblem::Identifier));
    }
    MqMessage {
        descriptor: value.clone(),
        body: Vec::new(),
        properties: Vec::new(),
    }
    .validate(limits)
    .map_err(MqMqiProblem::Message)
}

pub(super) fn property(
    value: &MqMessageProperty,
    limits: MqMessageLimits,
) -> Result<(), MqMqiProblem> {
    if value.name.len() > limits.property_name_bytes
        || value.value.len() > limits.property_value_bytes
    {
        return Err(MqMqiProblem::Message(
            crate::MqMessageProblem::PropertyValueLength,
        ));
    }
    // Reuse the sole message/property validator, after bounded preflight.
    MqMessage {
        descriptor: MqMessageDescriptor {
            identifiers: Default::default(),
            format: None,
            expiry: crate::MqExpiry::Unlimited,
            persistence: crate::MqPersistence::QueueDefault,
            priority: crate::MqPriority::QueueDefault,
            ordering: Default::default(),
        },
        body: Vec::new(),
        properties: vec![value.clone()],
    }
    .validate(limits)
    .map_err(MqMqiProblem::Message)
}

impl MqMqiRequestEnvelope {
    /// Structural/budget validation only. This never validates a live token,
    /// grants SAF authority or changes the trusted host-context binding.
    pub fn validate(&self) -> Result<(), MqMqiProblem> {
        self.limits.validate()?;
        let owner = self.context.owner;
        if [
            owner.host_id,
            owner.process_id,
            owner.thread_id,
            owner.task_id,
            owner.syncpoint_epoch,
        ]
        .contains(&0)
        {
            return Err(MqMqiProblem::Context);
        }
        let limits = self.limits;
        let message = limits.message;
        use MqMqiRequest as R;
        let connection = match &self.request {
            R::Connect(value) | R::ConnectExtended(value) => {
                if matches!(self.request, R::Connect(_))
                    && (value.sharing != MqHandleSharing::NonShared
                        || value.options != MqMqiOptions::ContractDefault)
                {
                    return Err(MqMqiProblem::Connection);
                }
                None
            }
            R::Open(value) => Some(value.connection()),
            R::Close(value) => Some(value.connection()),
            R::Get(value) => {
                value.get.validate(message).map_err(MqMqiProblem::Message)?;
                unit(value.unit)?;
                Some(value.connection)
            }
            R::FullGet(value) | R::QualifiedFullGet(value) => {
                super::full_message::descriptor(&value.descriptor, message)?;
                value
                    .controls()
                    .validate(message)
                    .map_err(MqMqiProblem::Message)?;
                super::full_message::unit(value.unit)?;
                Some(value.connection)
            }
            R::FullPut {
                connection, put, ..
            }
            | R::FullPutOne {
                connection, put, ..
            } => {
                put.message.validate(message)?;
                super::full_message::unit(put.unit)?;
                if matches!(
                    &self.request,
                    R::FullPutOne {
                        lookup: MqRouteLookup::Queue {
                            dynamic_pattern: Some(_),
                            ..
                        },
                        ..
                    }
                ) {
                    return Err(MqMqiProblem::Connection);
                }
                Some(*connection)
            }
            R::Put {
                connection, put, ..
            }
            | R::PutOne {
                connection, put, ..
            } => {
                put.message
                    .validate(message)
                    .map_err(MqMqiProblem::Message)?;
                unit(put.unit)?;
                if let R::PutOne {
                    lookup:
                        MqRouteLookup::Queue {
                            dynamic_pattern: Some(_),
                            ..
                        },
                    ..
                } = &self.request
                {
                    return Err(MqMqiProblem::Connection);
                }
                Some(*connection)
            }
            R::Back { connection, unit }
            | R::Commit { connection, unit }
            | R::Begin {
                connection, unit, ..
            } => {
                if *unit == 0 {
                    return Err(MqMqiProblem::Unit);
                }
                Some(*connection)
            }
            R::Inquire(value) => {
                if value.selectors.len() > limits.selectors {
                    return Err(MqMqiProblem::SelectorCount);
                }
                if value.integer_capacity > limits.selectors
                    || value.character_capacity > limits.attribute_bytes
                {
                    return Err(MqMqiProblem::AttributeCount);
                }
                // Smaller output arrays remain representable for source-defined
                // partial/error outcomes. Do not invent successful completion.
                Some(value.connection)
            }
            R::Set(value) => {
                if value.selectors.len() > limits.selectors {
                    return Err(MqMqiProblem::SelectorCount);
                }
                let integers = value
                    .selectors
                    .iter()
                    .filter(|s| matches!(s, MqMqiSelector::PendingInteger(_)))
                    .count();
                if value.integers.len() > limits.selectors
                    || value.integers.len() < integers
                    || value.characters.len() > limits.attribute_bytes
                {
                    return Err(MqMqiProblem::AttributeCount);
                }
                Some(value.connection)
            }
            R::InquireProperty(value) => {
                value
                    .query
                    .validate(message)
                    .map_err(MqMqiProblem::Message)?;
                if let Some(after) = &value.after {
                    if after.len() > message.property_name_bytes {
                        return Err(MqMqiProblem::Buffer);
                    }
                    MqPropertyQuery::Exact(after.clone())
                        .validate(message)
                        .map_err(MqMqiProblem::Message)?;
                }
                if value.value_capacity > message.property_value_bytes
                    || value.name_capacity > message.property_name_bytes
                {
                    return Err(MqMqiProblem::Buffer);
                }
                Some(value.connection)
            }
            R::DeleteProperty {
                connection, query, ..
            } => {
                query.validate(message).map_err(MqMqiProblem::Message)?;
                if !matches!(query, MqPropertyQuery::Exact(_)) {
                    return Err(MqMqiProblem::Message(crate::MqMessageProblem::PropertyName));
                }
                Some(*connection)
            }
            R::SetProperty {
                connection,
                property: value,
                ..
            } => {
                property(value, message)?;
                Some(*connection)
            }
            R::BufferToHandle(value) | R::HandleToBuffer(value) => {
                if value.capacity > limits.buffer_bytes || value.buffer.len() > value.capacity {
                    return Err(MqMqiProblem::Buffer);
                }
                descriptor(&value.descriptor, message)?;
                value
                    .query
                    .validate(message)
                    .map_err(MqMqiProblem::Message)?;
                Some(value.connection)
            }
            R::Callback {
                connection,
                operation,
                ..
            } => {
                if let MqMqiCallbackOperation::Register {
                    callback_id, get, ..
                } = operation
                {
                    if *callback_id == 0 {
                        return Err(MqMqiProblem::Callback);
                    }
                    get.validate(message).map_err(MqMqiProblem::Message)?;
                }
                Some(*connection)
            }
            R::CallbackFunction {
                connection,
                callback_id,
                message: value,
                get,
                ..
            } => {
                if *callback_id == 0 {
                    return Err(MqMqiProblem::Callback);
                }
                if let Some(value) = value {
                    value.validate(message).map_err(MqMqiProblem::Message)?;
                }
                if let Some(get) = get {
                    get.validate(message).map_err(MqMqiProblem::Message)?;
                }
                Some(*connection)
            }
            R::Subscribe(value) => Some(value.connection),
            R::SubscriptionRequest {
                connection,
                unit: value,
                ..
            } => {
                unit(*value)?;
                Some(*connection)
            }
            R::Rfh2(value) => {
                value
                    .validate(self.limits)
                    .map_err(MqMqiProblem::Property)?;
                if owner.environment != crate::MqHostEnvironment::ZosBatch
                    || self.context.syncpoint_owner != MqSyncpointOwner::QueueManager
                {
                    return Err(MqMqiProblem::Context);
                }
                Some(value.connection())
            }
            R::Property(value) => {
                value
                    .validate(self.limits)
                    .map_err(MqMqiProblem::Property)?;
                if owner.environment != crate::MqHostEnvironment::ZosBatch
                    || self.context.syncpoint_owner != MqSyncpointOwner::QueueManager
                {
                    return Err(MqMqiProblem::Context);
                }
                Some(value.connection())
            }
            R::CreateMessageHandle { connection, .. }
            | R::DeleteMessageHandle { connection, .. }
            | R::Disconnect { connection }
            | R::Control { connection, .. }
            | R::Stat { connection, .. } => Some(*connection),
        };
        if connection == Some(MqHconn::Unassociated)
            && !matches!(
                self.request,
                R::CreateMessageHandle { .. } | R::DeleteMessageHandle { .. }
            )
        {
            return Err(MqMqiProblem::Connection);
        }
        // Default CICS connection is a typed special identity. Its authority is
        // still established by the registry and trusted context at dispatch.
        if connection == Some(MqHconn::Default)
            && owner.environment != crate::MqHostEnvironment::ZosCics
        {
            return Err(MqMqiProblem::Connection);
        }
        Ok(())
    }

    /// Every request remains non-executable here, even when its shape is valid.
    pub fn review(&self) -> Result<MqMqiPending, MqMqiProblem> {
        self.validate()?;
        match &self.request {
            MqMqiRequest::FullPut { put, .. } | MqMqiRequest::FullPutOne { put, .. } => {
                if matches!(put.unit, MqMqiUnitOfWork::ExternalPending { .. }) {
                    return Ok(MqMqiPending::ExternalUnitOfWork);
                }
                if put.context != MqMqiMessageContext::Default
                    || matches!(
                        &self.request,
                        MqMqiRequest::FullPutOne {
                            alternate_user: Some(_),
                            ..
                        }
                    )
                {
                    return Ok(MqMqiPending::TrustedContextAndAuthorization);
                }
                return Ok(MqMqiPending::StructureAndWireMapping);
            }
            MqMqiRequest::FullGet(get) | MqMqiRequest::QualifiedFullGet(get) => {
                return Ok(
                    if matches!(get.unit, MqMqiUnitOfWork::ExternalPending { .. }) {
                        MqMqiPending::ExternalUnitOfWork
                    } else {
                        MqMqiPending::StructureAndWireMapping
                    },
                );
            }
            _ => {}
        }
        Ok(MqMqiPending::PublicDispatch)
    }
}

impl MqMqiResult {
    /// Checks standalone result shape. Dispatch must additionally compare the
    /// result to the original request (capacities, UOW and returned lifetimes).
    pub fn validate(&self, limits: MqMqiLimits) -> Result<(), MqMqiProblem> {
        limits.validate()?;
        let (status, output) = match &self.outcome {
            MqMqiOutcome::ReviewedOutput { status, output } => {
                return super::reviewed_output::validate(self.call, *status, output, limits);
            }
            MqMqiOutcome::ReviewedStatus { status } => {
                return if status.call() == self.call {
                    Ok(())
                } else {
                    Err(MqMqiProblem::StatusCallMismatch)
                };
            }
            MqMqiOutcome::Completed { status, output } => (Some(*status), output),
            MqMqiOutcome::StatusPending { output } => (None, output),
            MqMqiOutcome::CallbackReturned { .. } if self.call != MqMqiCall::CallbackFunction => {
                return Err(MqMqiProblem::OutputCallMismatch);
            }
            _ => return Ok(()),
        };
        validate_output(self.call, status, output, limits)
    }
}

pub(super) fn validate_output(
    call: MqMqiCall,
    status: Option<MqMqiStatus>,
    output: &MqMqiOutput,
    limits: MqMqiLimits,
) -> Result<(), MqMqiProblem> {
    use MqMqiCall as C;
    use MqMqiOutput as O;
    if status == Some(MqMqiStatus::FailedEnvironment) {
        if !matches!(call, C::Back | C::Begin | C::Commit) || !matches!(output, O::NoOutput) {
            return Err(MqMqiProblem::StatusCallMismatch);
        }
        return Ok(());
    }
    let matched = match (call, output) {
        (C::Get, O::QualifiedFullGot(value)) => {
            value.validate(limits.message)?;
            status.is_none()
                || matches!(
                    value.disposition,
                    MqGetDisposition::Message(crate::MqTruncationDisposition::Complete { .. })
                )
        }
        (C::Put | C::PutOne, O::Produced(value)) => {
            value.validate(limits.message)?;
            status.is_none()
                || matches!(
                    value.outcome,
                    MqDeliveryOutcome::Accepted | MqDeliveryOutcome::Pending
                )
        }
        (_, O::Rfh2Observation(value)) => {
            value
                .validate(call, limits)
                .map_err(MqMqiProblem::Property)?;
            status.is_none()
        }
        (_, O::PropertyObservation(value)) => {
            value
                .validate(call, limits)
                .map_err(MqMqiProblem::Property)?;
            // Exact reviewed pairing is mandatory for the new output vocabulary.
            status.is_none()
        }
        (
            C::Get,
            O::FullGot {
                disposition,
                message,
                data_length,
                cursor,
            },
        ) => {
            super::full_message::got(
                *disposition,
                message.as_ref(),
                *data_length,
                *cursor,
                limits.message,
            )?;
            status.is_none()
                || matches!(
                    disposition,
                    MqGetDisposition::Message(crate::MqTruncationDisposition::Complete { .. })
                )
        }
        (
            C::Put | C::PutOne,
            O::FullPut {
                descriptor,
                outcome,
            },
        ) => {
            super::full_message::descriptor(descriptor, limits.message)?;
            status.is_none()
                || matches!(
                    outcome,
                    MqDeliveryOutcome::Accepted | MqDeliveryOutcome::Pending
                )
        }
        (C::Connect | C::ConnectExtended, O::Connected(connection)) => {
            *connection != MqHconn::Unassociated
        }
        (C::Open, O::Opened { object, dynamic }) => dynamic
            .as_ref()
            .is_none_or(|d| d.handle == *object && d.model != d.name),
        (C::CreateMessageHandle, O::MessageHandle(_)) => true,
        (C::Subscribe, O::Subscribed { .. }) => true,
        (
            C::Get,
            O::Got {
                disposition,
                message,
                cursor,
            },
        ) => {
            if let Some(message) = message {
                message
                    .validate(limits.message)
                    .map_err(MqMqiProblem::Message)?;
            }
            if *cursor == Some(0) {
                return Err(MqMqiProblem::Buffer);
            }
            match disposition {
                MqGetDisposition::Message(crate::MqTruncationDisposition::Complete { length }) => {
                    message.as_ref().is_some_and(|m| m.body.len() == *length)
                }
                MqGetDisposition::Message(truncation) if status.is_none() => {
                    let (required, copied) = match truncation {
                        crate::MqTruncationDisposition::RejectedRetained { required, copied }
                        | crate::MqTruncationDisposition::AcceptedRemoved { required, copied }
                        | crate::MqTruncationDisposition::AcceptedBrowsed { required, copied } => {
                            (*required, *copied)
                        }
                        crate::MqTruncationDisposition::Complete { .. } => unreachable!(),
                    };
                    required <= limits.message.body_bytes
                        && copied < required
                        && message.as_ref().is_some_and(|m| m.body.len() == copied)
                }
                MqGetDisposition::NoMessage
                | MqGetDisposition::WaitExpired
                | MqGetDisposition::UnknownOutcome => {
                    status.is_none() && message.is_none() && cursor.is_none()
                }
                _ => false,
            }
        }
        (
            C::Put | C::PutOne,
            O::Put {
                descriptor: value,
                outcome,
            },
        ) => {
            descriptor(value, limits.message)?;
            status.is_none()
                || matches!(
                    outcome,
                    MqDeliveryOutcome::Accepted | MqDeliveryOutcome::Pending
                )
        }
        (C::Put | C::PutOne, O::Distribution(value)) => {
            value
                .validate(limits.message)
                .map_err(MqMqiProblem::Message)?;
            status.is_none()
                || value.items.iter().all(|item| {
                    matches!(
                        item.outcome,
                        MqDeliveryOutcome::Accepted | MqDeliveryOutcome::Pending
                    )
                })
        }
        (C::InquireProperty, O::Property(value)) => {
            property(value, limits.message)?;
            true
        }
        (
            C::BufferToHandle | C::HandleToBuffer,
            O::Buffer {
                descriptor: value,
                bytes,
                data_length,
            },
        ) => {
            descriptor(value, limits.message)?;
            bytes.len() <= limits.buffer_bytes && *data_length == bytes.len()
        }
        (
            C::Inquire,
            O::Attributes {
                integers,
                characters,
            },
        ) => integers.len() <= limits.selectors && characters.len() <= limits.attribute_bytes,
        (C::Back | C::Begin | C::Commit, O::UnitOfWork { unit }) => *unit != 0,
        (C::SubscriptionRequest, O::PublicationsRequested { count }) => {
            *count <= limits.message.distribution_items
        }
        (
            C::Callback
            | C::Close
            | C::Control
            | C::Disconnect
            | C::DeleteMessageHandle
            | C::DeleteProperty
            | C::Set
            | C::SetProperty,
            O::NoOutput,
        ) => true,
        _ => false,
    };
    if !matched {
        return Err(MqMqiProblem::OutputCallMismatch);
    }
    Ok(())
}
