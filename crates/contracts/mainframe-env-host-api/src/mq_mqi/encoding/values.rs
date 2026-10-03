use super::*;
use crate::*;

variants!(MqHostEnvironment {
    ZosBatch,
    ZosImsBatchDli,
    ZosCics,
    ZosIms,
    MqiClient,
    OtherBindings
});
variants!(MqSyncpointOwner {
    QueueManager,
    HostCoordinator
});
variants!(MqHandleSharing {
    NonShared,
    SharedBlock,
    SharedNoBlock
});
object!(MqHandleOwner {
    environment,
    host_id,
    process_id,
    syncpoint_epoch,
    task_id,
    thread_id
});
object!(MqMessageLimits {
    body_bytes,
    destination_bytes,
    distribution_items,
    format_bytes,
    identifier_bytes,
    properties,
    property_name_bytes,
    property_total_bytes,
    property_value_bytes,
    wait_ticks
});
object!(MqMessageIdentifiers {
    correlation_id,
    group_id,
    message_id
});
object!(MqMessageOrdering {
    group_sequence,
    last_in_group,
    last_segment,
    segment_offset,
    segmentation_allowed
});
object!(MqMessageDescriptor {
    expiry,
    format,
    identifiers,
    ordering,
    persistence,
    priority
});
object!(MqMessageProperty { kind, name, value });
object!(MqMessage {
    body,
    descriptor,
    properties
});
object!(MqDistributionItemResult {
    destination,
    outcome
});
object!(MqDistributionResult { items });
object!(MqMessageMatch { identifiers });
object!(MqGetContract {
    buffer_capacity,
    mode,
    selection,
    truncation,
    wait
});
variants!(MqPersistence {
    QueueDefault,
    Persistent,
    NonPersistent,
    PendingSource
});
variants!(MqPropertyType {
    Boolean,
    ByteString,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    String,
    Null
});
variants!(MqTruncation { Reject, Accept });
variants!(MqGetMode { Remove, BrowseFirst, BrowseNext { cursor }, RemoveUnderCursor { cursor } });
variants!(MqTruncationDisposition { Complete { length }, RejectedRetained { copied, required },
    AcceptedRemoved { copied, required }, AcceptedBrowsed { copied, required } });
variants!(MqDeliveryOutcome {
    Pending,
    Accepted,
    Rejected,
    DuplicatePossible,
    UnknownOutcome
});

impl Canonical for MqExpiry {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Unlimited => out.variant("MqExpiry", "Unlimited", 0),
            Self::PendingSource => out.variant("MqExpiry", "PendingSource", 0),
            Self::RelativeHostTicks(value) => {
                out.variant("MqExpiry", "RelativeHostTicks", 1)?;
                out.text("0")?;
                value.encode(out)
            }
        }
    }
}
impl Canonical for MqPriority {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::QueueDefault => out.variant("MqPriority", "QueueDefault", 0),
            Self::PendingNumeric(value) => {
                out.variant("MqPriority", "PendingNumeric", 1)?;
                out.text("0")?;
                value.encode(out)
            }
        }
    }
}
impl Canonical for MqWait {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::NoWait => out.variant("MqWait", "NoWait", 0),
            Self::BoundedHostTicks(value) => {
                out.variant("MqWait", "BoundedHostTicks", 1)?;
                out.text("0")?;
                value.encode(out)
            }
        }
    }
}
impl Canonical for MqPropertyQuery {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let (variant, value) = match self {
            Self::Exact(v) => ("Exact", v),
            Self::Prefix(v) => ("Prefix", v),
        };
        out.variant("MqPropertyQuery", variant, 1)?;
        out.text("0")?;
        value.encode(out)
    }
}
impl Canonical for MqGetDisposition {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Message(value) => {
                out.variant("MqGetDisposition", "Message", 1)?;
                out.text("0")?;
                value.encode(out)
            }
            Self::NoMessage => out.variant("MqGetDisposition", "NoMessage", 0),
            Self::WaitExpired => out.variant("MqGetDisposition", "WaitExpired", 0),
            Self::UnknownOutcome => out.variant("MqGetDisposition", "UnknownOutcome", 0),
        }
    }
}

macro_rules! text_newtype {
    ($($type:ident),+ $(,)?) => {$(
        impl Canonical for $type {
            fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
                out.tag(0x42)?; out.text(stringify!($type))?; out.text(self.as_str())
            }
        }
    )+};
}
text_newtype!(
    MqRouteName,
    MqRouteDynamicPattern,
    MqRouteTopicString,
    MqRouteAlternateUser
);
variants!(MqRouteOpenAccess {
    InputAsQueueDefault,
    InputShared,
    InputExclusive,
    Browse,
    Output,
    Inquire,
    Set
});
variants!(MqRouteContextOutput {
    None,
    PassIdentity,
    PassAll,
    SetIdentity,
    SetAll
});
object!(MqRouteContextIntent {
    output,
    save_all_from_input
});
object!(MqRouteOpenModifiers {
    alternate_user,
    context,
    cooperative_browse,
    no_multicast,
    resolve_local_queue,
    resolve_local_topic
});
variants!(MqRouteCloseMode {
    None,
    Delete,
    DeletePurge,
    KeepSubscription,
    RemoveSubscription,
    Immediate,
    Quiesce
});
variants!(MqRouteCloseLifecycle {
    Unknown,
    Predefined,
    TemporaryDynamic,
    PermanentDynamic,
    ManagedDestination
});
variants!(MqRouteDynamicKind {
    Temporary,
    Permanent
});
variants!(MqRouteCloseTarget { Object { handle, lifecycle }, Subscription { durable, handle } });
object!(MqDynamicQueueOpenResult {
    handle,
    kind,
    model,
    name
});

impl Canonical for MqRouteLookup {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Queue {
                name,
                manager,
                dynamic_pattern,
            } => {
                out.variant("MqRouteLookup", "Queue", 3)?;
                out.text("dynamic_pattern")?;
                dynamic_pattern.encode(out)?;
                out.text("manager")?;
                manager.encode(out)?;
                out.text("name")?;
                name.encode(out)
            }
            Self::Topic {
                name,
                object_string,
            } => {
                out.variant("MqRouteLookup", "Topic", 2)?;
                out.text("name")?;
                name.encode(out)?;
                out.text("object_string")?;
                object_string.encode(out)
            }
            Self::Process(value) | Self::Namelist(value) => {
                let variant = match self {
                    Self::Process(_) => "Process",
                    Self::Namelist(_) => "Namelist",
                    _ => unreachable!(),
                };
                out.variant("MqRouteLookup", variant, 1)?;
                out.text("0")?;
                value.encode(out)
            }
            Self::QueueManager => out.variant("MqRouteLookup", "QueueManager", 0),
            Self::DistributionList => out.variant("MqRouteLookup", "DistributionList", 0),
        }
    }
}
impl Canonical for MqObjectOpenRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("MqObjectOpenRequest", 4)?;
        out.text("access")?;
        MqRouteOpenAccess::sequence(self.access(), out)?;
        out.text("connection")?;
        self.connection().encode(out)?;
        out.text("lookup")?;
        self.lookup().encode(out)?;
        out.text("modifiers")?;
        self.modifiers().encode(out)
    }
}
impl Canonical for MqObjectCloseRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("MqObjectCloseRequest", 3)?;
        out.text("connection")?;
        self.connection().encode(out)?;
        out.text("mode")?;
        self.mode().encode(out)?;
        out.text("target")?;
        self.target().encode(out)
    }
}

impl Canonical for MqHconn {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Default => out.variant("MqHconn", "Default", 0),
            Self::Unassociated => out.variant("MqHconn", "Unassociated", 0),
            Self::Issued(value) => {
                out.variant("MqHconn", "Issued", 1)?;
                out.text("0")?;
                value.encode(out)
            }
        }
    }
}
macro_rules! issued_handle {
    ($($type:ident),+) => {$(
        impl Canonical for $type {
            fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
                let (registry, slot, generation, epoch) = self.canonical_parts();
                out.object(stringify!($type), 4)?;
                out.text("epoch")?; epoch.encode(out)?;
                out.text("generation")?; generation.encode(out)?;
                out.text("registry")?; registry.encode(out)?;
                out.text("slot")?; slot.encode(out)
            }
        }
    )+};
}
issued_handle!(MqConnectionId, MqHobj, MqHsub, MqHmsg);
