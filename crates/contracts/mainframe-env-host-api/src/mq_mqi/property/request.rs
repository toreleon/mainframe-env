use super::*;

/// Finite checked source profile for five existing call identities. Connections
/// and message handles remain registry-issued opaque identities; this value
/// supplies no host/lifecycle/current-unit, grant, SAF or mutation permission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqPropertyRequest {
    /// Create an initially empty property handle and associated default MQMD1.
    Create {
        /// Actual independently retained ordinary issued connection.
        connection: MqHconn,
        /// Reviewed MQCMHO1 default validation, never ContractDefault inference.
        options: MqPropertyOptions,
    },
    /// Set/replace one exact ordinary property or supported descriptor field.
    Set {
        /// Must equal the retained connection used to create this HMSG.
        connection: MqHconn,
        /// Actual live message identity; historical observations fail at dispatch.
        handle: MqHmsg,
        /// Reviewed MQSMPO1 SET_FIRST; append/cursor forms are unsupported.
        options: MqPropertyOptions,
        /// Exact validated name, including descriptor-field case.
        name: MqPropertyName,
        /// Complete MQPD1 input; no missing field is synthesized.
        descriptor: MqPropertyDescriptor,
        /// Entire as-set value and declared encoding/CCSID, not a partial prefix.
        value: MqPropertyData,
    },
    /// Inquire one exact property as set, retaining required and copied lengths.
    Inquire {
        /// Must equal the actual creating connection.
        connection: MqHconn,
        /// Actual live HMSG; not reconstructed from a number or receipt.
        handle: MqHmsg,
        /// Reviewed MQIMPO1 INQ_FIRST with no conversion.
        options: MqPropertyOptions,
        /// Exact query; aliases, wildcard and cursor-relative selection are pending.
        name: MqPropertyName,
        /// Exact MQTYPE_AS_SET identity; requested conversion types are unsupported.
        requested_type: i32,
        /// Actual returned-name buffer bytes, including zero.
        name_capacity: usize,
        /// Actual returned-value buffer bytes, including zero.
        value_capacity: usize,
    },
    /// Delete an exact property, or reset a descriptor field to its source default.
    Delete {
        /// Actual creating connection, not a special/default token.
        connection: MqHconn,
        /// Actual live HMSG.
        handle: MqHmsg,
        /// Reviewed MQDMPO1 DEL_FIRST; wildcard/under-cursor is pending.
        options: MqPropertyOptions,
        /// Exact property or descriptor field identity.
        name: MqPropertyName,
    },
    /// Retire one message handle after known physical publication.
    DeleteHandle {
        /// Actual creating connection.
        connection: MqHconn,
        /// Live non-in-use identity to invalidate, never a numeric handle.
        handle: MqHmsg,
        /// Reviewed MQDMHO1 NONE.
        options: MqPropertyOptions,
    },
}
impl MqPropertyRequest {
    /// The exact original MQI call; this is not an executable dispatch registry.
    pub const fn call(&self) -> MqMqiCall {
        match self {
            Self::Create { .. } => MqMqiCall::CreateMessageHandle,
            Self::Set { .. } => MqMqiCall::SetProperty,
            Self::Inquire { .. } => MqMqiCall::InquireProperty,
            Self::Delete { .. } => MqMqiCall::DeleteProperty,
            Self::DeleteHandle { .. } => MqMqiCall::DeleteMessageHandle,
        }
    }
    /// Borrow the application's token assertion; live authority must be independent.
    pub const fn connection(&self) -> MqHconn {
        match self {
            Self::Create { connection, .. }
            | Self::Set { connection, .. }
            | Self::Inquire { connection, .. }
            | Self::Delete { connection, .. }
            | Self::DeleteHandle { connection, .. } => *connection,
        }
    }
    /// The asserted existing HMSG; creation supplies none.
    pub const fn handle(&self) -> Option<MqHmsg> {
        match self {
            Self::Create { .. } => None,
            Self::Set { handle, .. }
            | Self::Inquire { handle, .. }
            | Self::Delete { handle, .. }
            | Self::DeleteHandle { handle, .. } => Some(*handle),
        }
    }
    /// Exact name where meaningful; no inferred default name.
    pub fn name(&self) -> Option<&MqPropertyName> {
        match self {
            Self::Set { name, .. } | Self::Inquire { name, .. } | Self::Delete { name, .. } => {
                Some(name)
            }
            _ => None,
        }
    }
    /// Checks source profile and product bounds, not execution/SAF/state permission.
    pub fn validate(&self, limits: MqMqiLimits) -> Result<(), MqPropertyProblem> {
        limits.validate().map_err(|_| MqPropertyProblem::Capacity)?;
        if !matches!(self.connection(), MqHconn::Issued(_)) {
            return Err(MqPropertyProblem::Unsupported);
        }
        let options = match self {
            Self::Create { options, .. }
            | Self::Set { options, .. }
            | Self::Inquire { options, .. }
            | Self::Delete { options, .. }
            | Self::DeleteHandle { options, .. } => *options,
        };
        if options.call != self.call() {
            return Err(MqPropertyProblem::Call);
        }
        MqPropertyOptions::checked(options.call, options.version, options.options)?;
        if let Some(name) = self.name() {
            MqPropertyName::checked(name.0.clone(), limits.message)?;
        }
        match self {
            Self::Set {
                name,
                descriptor,
                value,
                ..
            } => {
                descriptor.validate_input()?;
                value.validate(limits.message)?;
                if let Some(field) = name.descriptor_field() {
                    let definition = super::mq_property_md_fields()
                        .iter()
                        .find(|d| d.name == field)
                        .ok_or(MqPropertyProblem::Unsupported)?;
                    if value.kind != definition.kind || value.bytes.len() != definition.width {
                        return Err(MqPropertyProblem::Value);
                    }
                }
            }
            Self::Inquire {
                requested_type,
                name_capacity,
                value_capacity,
                ..
            } => {
                if *requested_type != generated::MQTYPE_AS_SET {
                    return Err(MqPropertyProblem::Unsupported);
                }
                if *name_capacity > limits.message.property_name_bytes
                    || *value_capacity > limits.message.property_value_bytes
                    || *value_capacity > limits.buffer_bytes
                {
                    return Err(MqPropertyProblem::Capacity);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
