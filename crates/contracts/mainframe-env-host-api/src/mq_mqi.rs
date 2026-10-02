//! Frozen additive MQI vocabulary, independent of `HostRequest` registration.
//!
//! IBM MQ 9.4 baseline `ibm-mq-9.4-mqi-2026-08-31`, official rows 0001–0026.
//! `MqMqiCall::source` joins the existing registry, including MQMHBUF positions
//! 18 and 25. All 26 call pages and the call-list page were read offline against
//! the committed topic hashes (MQINQ uses the issue #337 re-pin).
//!
//! Values express host intent, never native pointers, IBM wire layouts, trusted
//! dispatch, SAF permission, or executable/licensed credit. A provider must bind
//! caller context to its trusted invocation and validate registry lifetimes,
//! authorization, finite controls, effect identity and store CAS before mutation.
//! Unknown outcomes require fenced reconciliation, never automatic redispatch.

mod encoding;
mod reviewed_output;
#[cfg(test)]
mod tests;
mod validation;

pub use encoding::{
    MQ_MD_VALUE_DOMAIN, MQ_MD_VALUE_MAX_BYTES, MQ_MD_VALUE_SCHEMA, mq_md_value_bytes,
    mq_md_value_decode, mq_md_value_digest, mq_md_value_size, mq_mqi_request_bytes,
    mq_mqi_request_digest, mq_mqi_request_size, mq_mqi_result_bytes, mq_mqi_result_digest,
    mq_mqi_result_size,
};

use crate::mq_object_route::*;
use crate::{
    MqDeliveryOutcome, MqGetContract, MqGetDisposition, MqHandleOwner, MqHandleSharing, MqHconn,
    MqHmsg, MqHobj, MqHsub, MqMessage, MqMessageDescriptor, MqMessageLimits, MqMessageProperty,
    MqMqiCallIdentityDescriptor, MqPropertyQuery, MqPropertyType, MqSyncpointOwner,
    mq_mqi_call_identity_by_label,
};

/// Uses the existing canonical value schema, in a distinct additive domain.
pub const MQ_MQI_BOUNDARY_SCHEMA: &str = "mainframe-env.mq-mqi-boundary@1";
pub const MQ_MQI_REQUEST_DOMAIN: &[u8] = b"mainframe-env.mq-mqi-request@1\0";
pub const MQ_MQI_RESULT_DOMAIN: &[u8] = b"mainframe-env.mq-mqi-result@1\0";

macro_rules! calls {
    ($($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum MqMqiCall { $($variant),+ }
        impl MqMqiCall {
            pub const ALL: [Self; 26] = [$(Self::$variant),+];
            #[must_use]
            pub const fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }
            /// Registry-owned source and 27-row provenance; not a handler lookup.
            #[must_use]
            pub fn source(self) -> &'static MqMqiCallIdentityDescriptor {
                mq_mqi_call_identity_by_label(self.label()).expect("frozen MQI registry identity")
            }
        }
    };
}
calls! {
    Back => "MQBACK", Begin => "MQBEGIN", BufferToHandle => "MQBUFMH",
    Callback => "MQCB", CallbackFunction => "MQCB_FUNCTION", Close => "MQCLOSE",
    Commit => "MQCMIT", Connect => "MQCONN", ConnectExtended => "MQCONNX",
    CreateMessageHandle => "MQCRTMH", Control => "MQCTL", Disconnect => "MQDISC",
    DeleteMessageHandle => "MQDLTMH", DeleteProperty => "MQDLTMP", Get => "MQGET",
    Inquire => "MQINQ", InquireProperty => "MQINQMP", HandleToBuffer => "MQMHBUF",
    Open => "MQOPEN", Put => "MQPUT", PutOne => "MQPUT1", Set => "MQSET",
    SetProperty => "MQSETMP", Stat => "MQSTAT", Subscribe => "MQSUB",
    SubscriptionRequest => "MQSUBRQ",
}

/// No free-form option bags: absence of a reviewed structure is explicit.
/// `ContractDefault` matches private kernel defaults, not an IBM option integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiOptions {
    ContractDefault,
    PendingStructure { requested_version: Option<i32> },
}

/// Unit identity mirrors existing kernel `Option<u64>` local staging. External
/// coordination is pending; the caller cannot select a trusted coordinator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiUnitOfWork {
    NoSyncpoint,
    Local { unit: u64 },
    ExternalPending { unit: u64 },
}

/// Only default message context is represented by the current delivery kernel.
/// Passing/setting context stays typed and pending, including its source handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiMessageContext {
    Default,
    PassIdentityPending { source: MqHobj },
    PassAllPending { source: MqHobj },
    SetIdentityPending { user: MqRouteAlternateUser },
    SetAllPending { user: MqRouteAlternateUser },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiConnect {
    /// None requests the configured default queue manager, not an empty name.
    pub manager: Option<MqRouteName>,
    pub sharing: MqHandleSharing,
    pub options: MqMqiOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiGet {
    pub connection: MqHconn,
    pub object: MqHobj,
    pub get: MqGetContract,
    pub message_handle: Option<MqHmsg>,
    pub options: MqMqiOptions,
    pub unit: MqMqiUnitOfWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiPut {
    pub message: MqMessage,
    pub message_handle: Option<MqHmsg>,
    pub context: MqMqiMessageContext,
    pub options: MqMqiOptions,
    pub unit: MqMqiUnitOfWork,
}

/// Numeric identities are preserved as requests for later review, never
/// accepted selector constants. The inquiry kernel/adapter owns their mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiSelector {
    PendingInteger(i32),
    PendingCharacter(i32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiInquiry {
    pub connection: MqHconn,
    pub object: MqHobj,
    /// Order and duplicates are meaningful to MQINQ/MQSET and are preserved.
    pub selectors: Vec<MqMqiSelector>,
    pub integer_capacity: usize,
    pub character_capacity: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiSet {
    pub connection: MqHconn,
    pub object: MqHobj,
    pub selectors: Vec<MqMqiSelector>,
    pub integers: Vec<i32>,
    /// Character field widths/selector legality remain pending.
    pub characters: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiPropertyInquiry {
    pub connection: MqHconn,
    pub handle: MqHmsg,
    pub query: MqPropertyQuery,
    /// Private kernel continuation, not an inferred MQIMPO cursor layout.
    pub after: Option<String>,
    pub requested_type: Option<MqPropertyType>,
    pub value_capacity: usize,
    pub name_capacity: usize,
    pub options: MqMqiOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiBufferFormat {
    /// Existing private `MqBufferCodec::KernelV1`; never MQRFH2 equivalence.
    KernelV1,
    MqRfh2Pending,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiBuffer {
    pub connection: MqHconn,
    pub handle: MqHmsg,
    pub descriptor: MqMessageDescriptor,
    pub query: MqPropertyQuery,
    pub buffer: Vec<u8>,
    pub capacity: usize,
    pub strip_properties: bool,
    pub format: MqMqiBufferFormat,
    pub options: MqMqiOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiCallbackOperation {
    Register {
        callback_id: u64,
        get: MqGetContract,
        suspended: bool,
    },
    Deregister,
    Suspend,
    Resume,
    EventHandlerPending,
}

/// Mirrors existing pub/sub control actions. Quiesce is a private kernel
/// action; no MQOP wire value is inferred. StartWait stays explicitly pending.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiControl {
    Start,
    Stop,
    Quiesce,
    Suspend,
    Resume,
    StartWaitPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiSubscriptionMode {
    Create { publications_on_request: bool },
    Resume,
    AlterPending,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiSubscriptionDestination {
    /// Use the package's catalog definition, as the current kernel does.
    Catalog,
    ManagedPending,
    ProvidedPending(MqHobj),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiSubscribe {
    pub connection: MqHconn,
    pub name: MqRouteName,
    pub mode: MqMqiSubscriptionMode,
    pub destination: MqMqiSubscriptionDestination,
    pub options: MqMqiOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiStatType {
    AsyncError,
    Reconnection,
    ReconnectionError,
}

/// One meaningful typed payload per distinct pinned call. The callback-function
/// variant describes provider-to-consumer input; it is not an MQI entry point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiRequest {
    Back {
        connection: MqHconn,
        unit: u64,
    },
    Begin {
        connection: MqHconn,
        unit: u64,
        options: MqMqiOptions,
    },
    BufferToHandle(MqMqiBuffer),
    Callback {
        connection: MqHconn,
        object: MqHobj,
        operation: MqMqiCallbackOperation,
        options: MqMqiOptions,
    },
    CallbackFunction {
        connection: MqHconn,
        callback_id: u64,
        message: Option<MqMessage>,
        get: Option<MqGetContract>,
        context: MqMqiOptions,
    },
    Close(MqObjectCloseRequest),
    Commit {
        connection: MqHconn,
        unit: u64,
    },
    Connect(MqMqiConnect),
    ConnectExtended(MqMqiConnect),
    CreateMessageHandle {
        connection: MqHconn,
        options: MqMqiOptions,
    },
    Control {
        connection: MqHconn,
        operation: MqMqiControl,
        options: MqMqiOptions,
    },
    Disconnect {
        connection: MqHconn,
    },
    DeleteMessageHandle {
        connection: MqHconn,
        handle: MqHmsg,
        options: MqMqiOptions,
    },
    DeleteProperty {
        connection: MqHconn,
        handle: MqHmsg,
        query: MqPropertyQuery,
        options: MqMqiOptions,
    },
    Get(MqMqiGet),
    Inquire(MqMqiInquiry),
    InquireProperty(MqMqiPropertyInquiry),
    HandleToBuffer(MqMqiBuffer),
    Open(MqObjectOpenRequest),
    Put {
        connection: MqHconn,
        object: MqHobj,
        put: MqMqiPut,
    },
    PutOne {
        connection: MqHconn,
        lookup: MqRouteLookup,
        alternate_user: Option<MqRouteAlternateUser>,
        put: MqMqiPut,
    },
    Set(MqMqiSet),
    SetProperty {
        connection: MqHconn,
        handle: MqHmsg,
        property: MqMessageProperty,
        options: MqMqiOptions,
    },
    Stat {
        connection: MqHconn,
        kind: MqMqiStatType,
        options: MqMqiOptions,
    },
    Subscribe(MqMqiSubscribe),
    /// The only pinned action is MQSR_ACTION_PUBLICATION; other actions cannot
    /// be represented as a supposedly legal integer.
    SubscriptionRequest {
        connection: MqHconn,
        subscription: MqHsub,
        options: MqMqiOptions,
        unit: MqMqiUnitOfWork,
    },
}

impl MqMqiRequest {
    #[must_use]
    pub const fn call(&self) -> MqMqiCall {
        match self {
            Self::Back { .. } => MqMqiCall::Back,
            Self::Begin { .. } => MqMqiCall::Begin,
            Self::BufferToHandle(_) => MqMqiCall::BufferToHandle,
            Self::Callback { .. } => MqMqiCall::Callback,
            Self::CallbackFunction { .. } => MqMqiCall::CallbackFunction,
            Self::Close(_) => MqMqiCall::Close,
            Self::Commit { .. } => MqMqiCall::Commit,
            Self::Connect(_) => MqMqiCall::Connect,
            Self::ConnectExtended(_) => MqMqiCall::ConnectExtended,
            Self::CreateMessageHandle { .. } => MqMqiCall::CreateMessageHandle,
            Self::Control { .. } => MqMqiCall::Control,
            Self::Disconnect { .. } => MqMqiCall::Disconnect,
            Self::DeleteMessageHandle { .. } => MqMqiCall::DeleteMessageHandle,
            Self::DeleteProperty { .. } => MqMqiCall::DeleteProperty,
            Self::Get(_) => MqMqiCall::Get,
            Self::Inquire(_) => MqMqiCall::Inquire,
            Self::InquireProperty(_) => MqMqiCall::InquireProperty,
            Self::HandleToBuffer(_) => MqMqiCall::HandleToBuffer,
            Self::Open(_) => MqMqiCall::Open,
            Self::Put { .. } => MqMqiCall::Put,
            Self::PutOne { .. } => MqMqiCall::PutOne,
            Self::Set(_) => MqMqiCall::Set,
            Self::SetProperty { .. } => MqMqiCall::SetProperty,
            Self::Stat { .. } => MqMqiCall::Stat,
            Self::Subscribe(_) => MqMqiCall::Subscribe,
            Self::SubscriptionRequest { .. } => MqMqiCall::SubscriptionRequest,
        }
    }
}

/// Caller assertions are hashed but must be compared with trusted dispatch.
/// No principal, SAF permit, or effect-origin attestation can be supplied here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiContext {
    pub owner: MqHandleOwner,
    pub syncpoint_owner: MqSyncpointOwner,
}

/// Product ceilings, not MQ numeric constants. Smaller limits are semantic
/// inputs and participate in the canonical identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MqMqiLimits {
    pub message: MqMessageLimits,
    pub selectors: usize,
    pub attribute_bytes: usize,
    pub buffer_bytes: usize,
    pub canonical_bytes: usize,
}

impl Default for MqMqiLimits {
    fn default() -> Self {
        Self {
            message: MqMessageLimits::default(),
            selectors: 256,
            attribute_bytes: 1024 * 1024,
            buffer_bytes: 4 * 1024 * 1024,
            canonical_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiRequestEnvelope {
    pub context: MqMqiContext,
    pub limits: MqMqiLimits,
    pub request: MqMqiRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiPending {
    PublicDispatch,
    StructureAndWireMapping,
    SelectorAndAttributeMapping,
    StatusMapping,
    TrustedContextAndAuthorization,
    ExternalUnitOfWork,
    CallbackContext,
}

/// Closed source-pinned pairs only, with no guessed numeric conversion.
/// MQCC_OK/MQRC_NONE are stated in every non-callback call page. The failed
/// environment pair is the existing source-bound direct-syncpoint contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiStatus {
    OkNone,
    FailedEnvironment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiOutput {
    Connected(MqHconn),
    Opened {
        object: MqHobj,
        dynamic: Option<MqDynamicQueueOpenResult>,
    },
    MessageHandle(MqHmsg),
    Subscribed {
        object: MqHobj,
        subscription: MqHsub,
    },
    Got {
        disposition: MqGetDisposition,
        message: Option<MqMessage>,
        cursor: Option<u64>,
    },
    Put {
        descriptor: MqMessageDescriptor,
        outcome: MqDeliveryOutcome,
    },
    /// Existing delivery-kernel per-destination observations; wire completion
    /// mapping remains pending when any item is rejected or uncertain.
    Distribution(crate::MqDistributionResult),
    Property(MqMessageProperty),
    Buffer {
        descriptor: MqMessageDescriptor,
        bytes: Vec<u8>,
        data_length: usize,
    },
    /// Attributes use the exact order of the inquiry's selector arrays.
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
    /// Only calls with no payload output may use this variant.
    NoOutput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqMqiOutcome {
    /// A reviewed call return with its bounded typed observations. This is not
    /// state/handle authority; providers must also validate the original request.
    ReviewedOutput {
        status: crate::mq_status::MqReviewedStatus,
        output: MqMqiOutput,
    },
    /// A source-reviewed observed return identity. This carries no operation
    /// output and does not calculate or assert provider execution success.
    ReviewedStatus {
        status: crate::mq_status::MqReviewedStatus,
    },
    Completed {
        status: MqMqiStatus,
        output: MqMqiOutput,
    },
    /// A typed kernel observation whose completion/reason mapping is not yet
    /// source-reviewed. This preserves data without claiming MQI completion.
    StatusPending {
        output: MqMqiOutput,
    },
    /// MQCB_FUNCTION has no completion/reason pair or MQI entry point.
    CallbackReturned {
        context: MqMqiOptions,
    },
    Pending(MqMqiPending),
    UnknownOutcome,
    DuplicatePossible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiResult {
    pub call: MqMqiCall,
    pub outcome: MqMqiOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqMqiProblem {
    Limits,
    Context,
    Connection,
    Unit,
    Message(crate::MqMessageProblem),
    SelectorCount,
    AttributeCount,
    Buffer,
    Callback,
    OutputCallMismatch,
    StatusCallMismatch,
    CanonicalLimit,
    Allocation,
}
