use super::*;

object!(MqMqiConnect {
    manager,
    options,
    sharing
});
object!(MqMqiGet {
    connection,
    get,
    message_handle,
    object,
    options,
    unit
});
object!(MqMqiPut {
    context,
    message,
    message_handle,
    options,
    unit
});
object!(MqMqiInquiry {
    character_capacity,
    connection,
    integer_capacity,
    object,
    selectors
});
object!(MqMqiSet {
    characters,
    connection,
    integers,
    object,
    selectors
});
object!(MqMqiPropertyInquiry {
    after,
    connection,
    handle,
    name_capacity,
    options,
    query,
    requested_type,
    value_capacity
});
object!(MqMqiBuffer {
    buffer,
    capacity,
    connection,
    descriptor,
    format,
    handle,
    options,
    query,
    strip_properties
});
object!(MqMqiSubscribe {
    connection,
    destination,
    mode,
    name,
    options
});
object!(MqMqiContext {
    owner,
    syncpoint_owner
});
object!(MqMqiLimits {
    attribute_bytes,
    buffer_bytes,
    canonical_bytes,
    message,
    selectors
});
impl Canonical for MqMqiRequestEnvelope {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            context,
            limits,
            request,
        } = self;
        out.object("MqMqiRequestEnvelope", 4)?;
        out.text("call")?;
        request.call().encode(out)?;
        out.text("context")?;
        context.encode(out)?;
        out.text("limits")?;
        limits.encode(out)?;
        out.text("request")?;
        request.encode(out)
    }
}
object!(MqMqiResult { call, outcome });
variants!(MqMqiOptions { ContractDefault, PendingStructure { requested_version } });
variants!(MqMqiUnitOfWork { NoSyncpoint, Local { unit }, ExternalPending { unit } });
variants!(MqMqiMessageContext { Default, PassIdentityPending { source }, PassAllPending { source },
    SetIdentityPending { user }, SetAllPending { user } });
variants!(MqMqiBufferFormat {
    KernelV1,
    MqRfh2Pending
});
variants!(MqMqiCallbackOperation { Register { callback_id, get, suspended }, Deregister, Suspend,
    Resume, EventHandlerPending });
variants!(MqMqiControl {
    Start,
    Stop,
    Quiesce,
    Suspend,
    Resume,
    StartWaitPending
});
variants!(MqMqiSubscriptionMode { Create { publications_on_request }, Resume, AlterPending });
variants!(MqMqiStatType {
    AsyncError,
    Reconnection,
    ReconnectionError
});
variants!(MqMqiStatus {
    OkNone,
    FailedEnvironment
});
variants!(MqMqiPending {
    PublicDispatch,
    StructureAndWireMapping,
    SelectorAndAttributeMapping,
    StatusMapping,
    TrustedContextAndAuthorization,
    ExternalUnitOfWork,
    CallbackContext
});

impl Canonical for MqMqiCall {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let source = self.source();
        out.object("MqMqiCall", 5)?;
        out.text("label")?;
        out.text(source.label)?;
        out.text("official_row")?;
        out.text(source.official_row)?;
        out.text("source_positions")?;
        u16::sequence(source.source_positions, out)?;
        out.text("topic_path")?;
        out.text(source.topic_path)?;
        out.text("topic_sha256")?;
        out.text(source.topic_sha256)
    }
}
impl Canonical for MqMqiSelector {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let (variant, value) = match self {
            Self::PendingInteger(v) => ("PendingInteger", v),
            Self::PendingCharacter(v) => ("PendingCharacter", v),
        };
        out.variant("MqMqiSelector", variant, 1)?;
        out.text("0")?;
        value.encode(out)
    }
}
impl Canonical for MqMqiSubscriptionDestination {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Catalog => out.variant("MqMqiSubscriptionDestination", "Catalog", 0),
            Self::ManagedPending => {
                out.variant("MqMqiSubscriptionDestination", "ManagedPending", 0)
            }
            Self::ProvidedPending(v) => {
                out.variant("MqMqiSubscriptionDestination", "ProvidedPending", 1)?;
                out.text("0")?;
                v.encode(out)
            }
        }
    }
}

// Exhaustive mixed tuple/named enum encoder. Named fields remain sorted. The
// match patterns include every field: adding one is a compilation failure.
macro_rules! payload_enum {
    ($type:ident {
        tuples { $($tuple:ident),* $(,)? }
        named { $($variant:ident { $($field:ident),* }),* $(,)? }
        units { $($unit:ident),* $(,)? }
    }) => {
        impl Canonical for $type {
            fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
                match self {
                    $(Self::$tuple(v) => {
                        out.variant(stringify!($type), stringify!($tuple), 1)?;
                        out.text("0")?; v.encode(out)
                    },)*
                    $(Self::$variant { $($field),* } => {
                        out.variant(stringify!($type), stringify!($variant), [$(stringify!($field)),*].len())?;
                        $(out.text(stringify!($field))?; $field.encode(out)?;)*
                        Ok(())
                    },)*
                    $(Self::$unit => out.variant(stringify!($type), stringify!($unit), 0),)*
                }
            }
        }
    };
}

payload_enum!(MqMqiRequest {
    tuples { BufferToHandle, Close, Connect, ConnectExtended, Get, Inquire, InquireProperty,
        HandleToBuffer, Open, Set, Subscribe }
    named {
        Back { connection, unit },
        Begin { connection, options, unit },
        Callback { connection, object, operation, options },
        CallbackFunction { callback_id, connection, context, get, message },
        Commit { connection, unit },
        CreateMessageHandle { connection, options },
        Control { connection, operation, options },
        Disconnect { connection },
        DeleteMessageHandle { connection, handle, options },
        DeleteProperty { connection, handle, options, query },
        Put { connection, object, put },
        PutOne { alternate_user, connection, lookup, put },
        SetProperty { connection, handle, options, property },
        Stat { connection, kind, options },
        SubscriptionRequest { connection, options, subscription, unit }
    }
    units {}
});
payload_enum!(MqMqiOutput {
    tuples { Connected, MessageHandle, Property, Distribution }
    named {
        Opened { dynamic, object },
        Subscribed { object, subscription },
        Got { cursor, disposition, message },
        Put { descriptor, outcome },
        Buffer { bytes, data_length, descriptor },
        Attributes { characters, integers },
        UnitOfWork { unit },
        PublicationsRequested { count }
    }
    units { NoOutput }
});
payload_enum!(MqMqiOutcome {
    tuples { Pending }
    named { Completed { output, status }, StatusPending { output }, CallbackReturned { context },
        ReviewedStatus { status } }
    units { UnknownOutcome, DuplicatePossible }
});
