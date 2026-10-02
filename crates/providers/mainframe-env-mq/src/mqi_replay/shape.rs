//! Reference-free storage shapes; construction reuses the frozen public validators.
use super::*;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredResult {
    pub(super) schema_version: String,
    pub(super) call: String,
    pub(super) outcome: StoredOutcome,
    pub(super) host_result_digest: [u8; 32],
}

pub(super) fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

macro_rules! simple {
    ($local:ident, $host:ident { $($variant:ident),+ }) => {
        #[derive(Serialize)]
        pub(super) enum $local { $($variant),+ }
        impl<'de> Deserialize<'de> for $local {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let value = String::deserialize(d)?;
                match value.as_str() {
                    $(stringify!($variant) => Ok(Self::$variant),)+
                    _ => Err(serde::de::Error::custom("unknown stored enum symbol")),
                }
            }
        }
        impl From<$host> for $local {
            fn from(value: $host) -> Self {
                match value { $($host::$variant => Self::$variant),+ }
            }
        }
        impl From<$local> for $host {
            fn from(value: $local) -> Self {
                match value { $($local::$variant => Self::$variant),+ }
            }
        }
    }
}
simple!(
    Completion,
    MqMqiStatus {
        OkNone,
        FailedEnvironment
    }
);
simple!(
    Delivery,
    MqDeliveryOutcome {
        Pending,
        Accepted,
        Rejected,
        DuplicatePossible,
        UnknownOutcome
    }
);
simple!(
    Pending,
    MqMqiPending {
        PublicDispatch,
        StructureAndWireMapping,
        SelectorAndAttributeMapping,
        StatusMapping,
        TrustedContextAndAuthorization,
        ExternalUnitOfWork,
        CallbackContext
    }
);

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Options {
    ContractDefault {},
    PendingStructure {
        #[serde(deserialize_with = "required_option")]
        requested_version: Option<i32>,
    },
}
impl From<MqMqiOptions> for Options {
    fn from(value: MqMqiOptions) -> Self {
        match value {
            MqMqiOptions::ContractDefault => Self::ContractDefault {},
            MqMqiOptions::PendingStructure { requested_version } => {
                Self::PendingStructure { requested_version }
            }
        }
    }
}
impl From<Options> for MqMqiOptions {
    fn from(value: Options) -> Self {
        match value {
            Options::ContractDefault {} => Self::ContractDefault,
            Options::PendingStructure { requested_version } => {
                Self::PendingStructure { requested_version }
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Get {
    Complete { length: usize },
    RejectedRetained { required: usize, copied: usize },
    AcceptedRemoved { required: usize, copied: usize },
    AcceptedBrowsed { required: usize, copied: usize },
    NoMessage {},
    WaitExpired {},
    UnknownOutcome {},
}
impl From<MqGetDisposition> for Get {
    fn from(value: MqGetDisposition) -> Self {
        match value {
            MqGetDisposition::Message(t) => match t {
                MqTruncationDisposition::Complete { length } => Self::Complete { length },
                MqTruncationDisposition::RejectedRetained { required, copied } => {
                    Self::RejectedRetained { required, copied }
                }
                MqTruncationDisposition::AcceptedRemoved { required, copied } => {
                    Self::AcceptedRemoved { required, copied }
                }
                MqTruncationDisposition::AcceptedBrowsed { required, copied } => {
                    Self::AcceptedBrowsed { required, copied }
                }
            },
            MqGetDisposition::NoMessage => Self::NoMessage {},
            MqGetDisposition::WaitExpired => Self::WaitExpired {},
            MqGetDisposition::UnknownOutcome => Self::UnknownOutcome {},
        }
    }
}
impl From<Get> for MqGetDisposition {
    fn from(value: Get) -> Self {
        match value {
            Get::Complete { length } => Self::Message(MqTruncationDisposition::Complete { length }),
            Get::RejectedRetained { required, copied } => {
                Self::Message(MqTruncationDisposition::RejectedRetained { required, copied })
            }
            Get::AcceptedRemoved { required, copied } => {
                Self::Message(MqTruncationDisposition::AcceptedRemoved { required, copied })
            }
            Get::AcceptedBrowsed { required, copied } => {
                Self::Message(MqTruncationDisposition::AcceptedBrowsed { required, copied })
            }
            Get::NoMessage {} => Self::NoMessage,
            Get::WaitExpired {} => Self::WaitExpired,
            Get::UnknownOutcome {} => Self::UnknownOutcome,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Item {
    destination: String,
    outcome: Delivery,
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Output {
    PropertyObservation {
        observation: super::property::Observation,
    },
    FullPut {
        md_value: Vec<u8>,
        outcome: Delivery,
    },
    FullGot {
        disposition: Get,
        #[serde(deserialize_with = "required_option")]
        message: Option<super::full_message::Message>,
        #[serde(deserialize_with = "required_option")]
        data_length: Option<i32>,
        #[serde(deserialize_with = "required_option")]
        cursor: Option<u64>,
    },
    Put {
        descriptor: ReplayDescriptor,
        outcome: Delivery,
    },
    Got {
        disposition: Get,
        #[serde(deserialize_with = "required_option")]
        message: Option<ReplayMessage>,
        #[serde(deserialize_with = "required_option")]
        cursor: Option<u64>,
    },
    Distribution {
        items: Vec<Item>,
    },
    Property {
        property: ReplayProperty,
    },
    Buffer {
        descriptor: ReplayDescriptor,
        bytes: Vec<u8>,
        data_length: usize,
    },
    Attributes {
        integers: Vec<i32>,
        characters: Vec<u8>,
    },
    UnitOfWork {
        unit: u64,
    },
    PublicationsRequested {
        count: usize,
    },
    NoOutput {},
    Connected {
        connection: MqHandleObservation,
    },
    Opened {
        object: MqHandleObservation,
        #[serde(deserialize_with = "required_option")]
        dynamic: Option<super::handles::Dynamic>,
    },
    MessageHandle {
        message: MqHandleObservation,
    },
    Subscribed {
        object: MqHandleObservation,
        subscription: MqHandleObservation,
    },
}
impl Output {
    fn from_output(value: &MqMqiOutput) -> Result<Self, ReplayError> {
        Ok(match value {
            MqMqiOutput::PropertyObservation(v) => Self::PropertyObservation {
                observation: super::property::Observation::capture(v),
            },
            MqMqiOutput::FullPut {
                descriptor,
                outcome,
            } => Self::FullPut {
                md_value: super::full_message::capture_md(descriptor)?,
                outcome: outcome.clone().into(),
            },
            MqMqiOutput::FullGot {
                disposition,
                message,
                data_length,
                cursor,
            } => Self::FullGot {
                disposition: (*disposition).into(),
                message: message
                    .as_ref()
                    .map(super::full_message::Message::capture)
                    .transpose()?,
                data_length: *data_length,
                cursor: *cursor,
            },
            MqMqiOutput::Put {
                descriptor,
                outcome,
            } => Self::Put {
                descriptor: ReplayDescriptor::from_descriptor(descriptor)?,
                outcome: outcome.clone().into(),
            },
            MqMqiOutput::Got {
                disposition,
                message,
                cursor,
            } => Self::Got {
                disposition: (*disposition).into(),
                message: message
                    .as_ref()
                    .map(ReplayMessage::from_message)
                    .transpose()?,
                cursor: *cursor,
            },
            MqMqiOutput::Distribution(value) => Self::Distribution {
                items: value
                    .items
                    .iter()
                    .map(|i| Item {
                        destination: i.destination.clone(),
                        outcome: i.outcome.clone().into(),
                    })
                    .collect(),
            },
            MqMqiOutput::Property(value) => Self::Property {
                property: ReplayProperty::from_property(value),
            },
            MqMqiOutput::Buffer {
                descriptor,
                bytes,
                data_length,
            } => Self::Buffer {
                descriptor: ReplayDescriptor::from_descriptor(descriptor)?,
                bytes: bytes.clone(),
                data_length: *data_length,
            },
            MqMqiOutput::Attributes {
                integers,
                characters,
            } => Self::Attributes {
                integers: integers.clone(),
                characters: characters.clone(),
            },
            MqMqiOutput::UnitOfWork { unit } => Self::UnitOfWork { unit: *unit },
            MqMqiOutput::PublicationsRequested { count } => {
                Self::PublicationsRequested { count: *count }
            }
            MqMqiOutput::NoOutput => Self::NoOutput {},
            MqMqiOutput::Connected(value) => Self::Connected {
                connection: MqHandleObservation::capture_connection(*value)?,
            },
            MqMqiOutput::Opened { object, dynamic } => Self::Opened {
                object: MqHandleObservation::from(MqHandle::Object(*object)),
                dynamic: dynamic.as_ref().map(super::handles::Dynamic::capture),
            },
            MqMqiOutput::MessageHandle(value) => Self::MessageHandle {
                message: MqHandleObservation::from(MqHandle::Message(*value)),
            },
            MqMqiOutput::Subscribed {
                object,
                subscription,
            } => Self::Subscribed {
                object: MqHandleObservation::from(MqHandle::Object(*object)),
                subscription: MqHandleObservation::from(MqHandle::Subscription(*subscription)),
            },
        })
    }
    fn into_output(self) -> Result<MqMqiOutput, ReplayError> {
        Ok(match self {
            Self::PropertyObservation { observation } => {
                MqMqiOutput::PropertyObservation(observation.restore()?)
            }
            Self::FullPut { md_value, outcome } => MqMqiOutput::FullPut {
                descriptor: super::full_message::restore_md(&md_value)?,
                outcome: outcome.into(),
            },
            Self::FullGot {
                disposition,
                message,
                data_length,
                cursor,
            } => MqMqiOutput::FullGot {
                disposition: disposition.into(),
                message: message
                    .map(super::full_message::Message::restore)
                    .transpose()?,
                data_length,
                cursor,
            },
            Self::Put {
                descriptor,
                outcome,
            } => MqMqiOutput::Put {
                descriptor: descriptor.into_descriptor()?,
                outcome: outcome.into(),
            },
            Self::Got {
                disposition,
                message,
                cursor,
            } => MqMqiOutput::Got {
                disposition: disposition.into(),
                message: message.map(ReplayMessage::into_message).transpose()?,
                cursor,
            },
            Self::Distribution { items } => MqMqiOutput::Distribution(MqDistributionResult {
                items: items
                    .into_iter()
                    .map(|i| MqDistributionItemResult {
                        destination: i.destination,
                        outcome: i.outcome.into(),
                    })
                    .collect(),
            }),
            Self::Property { property } => MqMqiOutput::Property(property.into_property()?),
            Self::Buffer {
                descriptor,
                bytes,
                data_length,
            } => MqMqiOutput::Buffer {
                descriptor: descriptor.into_descriptor()?,
                bytes,
                data_length,
            },
            Self::Attributes {
                integers,
                characters,
            } => MqMqiOutput::Attributes {
                integers,
                characters,
            },
            Self::UnitOfWork { unit } => MqMqiOutput::UnitOfWork { unit },
            Self::PublicationsRequested { count } => MqMqiOutput::PublicationsRequested { count },
            Self::NoOutput {} => MqMqiOutput::NoOutput,
            Self::Connected { connection } => {
                MqMqiOutput::Connected(connection.historical_connection()?)
            }
            Self::Opened { object, dynamic } => MqMqiOutput::Opened {
                object: object.historical_object()?,
                dynamic: dynamic.map(super::handles::Dynamic::restore).transpose()?,
            },
            Self::MessageHandle { message } => {
                MqMqiOutput::MessageHandle(message.historical_message()?)
            }
            Self::Subscribed {
                object,
                subscription,
            } => MqMqiOutput::Subscribed {
                object: object.historical_object()?,
                subscription: subscription.historical_subscription()?,
            },
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum StoredOutcome {
    Completed {
        status: Completion,
        output: Output,
    },
    StatusPending {
        output: Output,
    },
    ReviewedStatus {
        completion: String,
        reason: String,
    },
    ReviewedOutput {
        completion: String,
        reason: String,
        output: Output,
    },
    CallbackReturned {
        context: Options,
    },
    Pending {
        reason: Pending,
    },
    UnknownOutcome {},
    DuplicatePossible {},
}
impl StoredOutcome {
    pub(super) fn is_property(&self) -> bool {
        matches!(self,Self::Completed {output,..}|Self::StatusPending {output}|Self::ReviewedOutput {output,..}
            if matches!(output,Output::PropertyObservation {..}))
    }
    pub(super) fn is_full(&self) -> bool {
        matches!(self, Self::Completed { output, .. } | Self::StatusPending { output }
            | Self::ReviewedOutput { output, .. } if matches!(output, Output::FullPut {..} | Output::FullGot {..}))
    }
    pub(super) fn from_result(value: &MqMqiResult) -> Result<Self, ReplayError> {
        Ok(match &value.outcome {
            MqMqiOutcome::Completed { status, output } => Self::Completed {
                status: (*status).into(),
                output: Output::from_output(output)?,
            },
            MqMqiOutcome::StatusPending { output } => Self::StatusPending {
                output: Output::from_output(output)?,
            },
            MqMqiOutcome::ReviewedStatus { status } => Self::ReviewedStatus {
                completion: status.completion().symbol().into(),
                reason: status.reason_symbol().into(),
            },
            MqMqiOutcome::ReviewedOutput { status, output } => Self::ReviewedOutput {
                completion: status.completion().symbol().into(),
                reason: status.reason_symbol().into(),
                output: Output::from_output(output)?,
            },
            MqMqiOutcome::CallbackReturned { context } => Self::CallbackReturned {
                context: (*context).into(),
            },
            MqMqiOutcome::Pending(reason) => Self::Pending {
                reason: (*reason).into(),
            },
            MqMqiOutcome::UnknownOutcome => Self::UnknownOutcome {},
            MqMqiOutcome::DuplicatePossible => Self::DuplicatePossible {},
        })
    }
    pub(super) fn into_outcome(self, call: MqMqiCall) -> Result<MqMqiOutcome, ReplayError> {
        Ok(match self {
            Self::Completed { status, output } => MqMqiOutcome::Completed {
                status: status.into(),
                output: output.into_output()?,
            },
            Self::StatusPending { output } => MqMqiOutcome::StatusPending {
                output: output.into_output()?,
            },
            Self::ReviewedStatus { completion, reason } => MqMqiOutcome::ReviewedStatus {
                status: MqReviewedStatus::from_symbols(call, &completion, &reason)
                    .map_err(ReplayError::Status)?,
            },
            Self::ReviewedOutput {
                completion,
                reason,
                output,
            } => MqMqiOutcome::ReviewedOutput {
                status: MqReviewedStatus::from_symbols(call, &completion, &reason)
                    .map_err(ReplayError::Status)?,
                output: output.into_output()?,
            },
            Self::CallbackReturned { context } => MqMqiOutcome::CallbackReturned {
                context: context.into(),
            },
            Self::Pending { reason } => MqMqiOutcome::Pending(reason.into()),
            Self::UnknownOutcome {} => MqMqiOutcome::UnknownOutcome,
            Self::DuplicatePossible {} => MqMqiOutcome::DuplicatePossible,
        })
    }
}
