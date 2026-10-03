//! Checked numeric option-to-intent translation, never dispatch or SAF authority.
//!
//! Numeric facts are generated from the existing structure catalog's supplemental
//! projection. The first family represents local queue MQOD1/GMO1/PMO1 with basic
//! MQMD1/2. This consumes already decoded typed fields, not wire memory or pointers.
//! A successful request still requires live registry, coordinator and service admission.

mod full_get;
mod full_put;
mod generated;
pub use full_get::{MqWireFullGet, get_full};
pub use full_put::{MqWireFullPut, put_full, put_full_for_target};
#[cfg(test)]
mod tests;

use crate::mq_mqi::{
    MqMqiGet, MqMqiMessageContext, MqMqiOptions, MqMqiPut, MqMqiRequest, MqMqiUnitOfWork,
};
use crate::mq_object_route::*;
use crate::{
    MqGetContract, MqGetMode, MqHconn, MqHobj, MqMessage, MqMessageLimits, MqMessageMatch,
    MqMessageOrdering, MqMessageProblem, MqTruncation, MqWait,
};
use generated::*;

/// Environment of the queue manager, not that of the application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqWireQueueManagerPlatform {
    Zos,
    Distributed,
}

/// Trusted integration port. Implement from independently admitted service/UOW,
/// registry cursor and configured clock authority, never from application integers.
/// Returned intent is not an authorization capability; service admission rechecks it.
/// Queries must observe existing admission/configuration; they must not create a
/// unit/cursor, reserve delivery, dispatch a provider or mutate a queue.
pub trait MqWireBindings {
    /// Independently verified local queue defaults: no cluster/read-ahead policy,
    /// no unrepresented property mode or asynchronous put response. Fail closed
    /// if configuration/catalog cannot establish this subset. Never a SAF grant.
    fn queue_defaults_are_represented(
        &self,
        connection: MqHconn,
        object: Option<MqHobj>,
        lookup: Option<&MqRouteLookup>,
    ) -> bool;
    fn queue_manager_platform(&self) -> MqWireQueueManagerPlatform;
    fn admitted_unit(&self, connection: MqHconn) -> Option<MqMqiUnitOfWork>;
    fn existing_cursor(&self, connection: MqHconn, object: MqHobj) -> Option<u64>;
    fn milliseconds_to_ticks(&self, milliseconds: u32) -> Option<u64>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqWireFamily {
    Open,
    Close,
    Get,
    Put,
    ObjectDescriptor,
    MessageDescriptor,
}

/// Product errors, not invented MQCC/MQRC outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqWireProblem {
    MqlongRange,
    NegativeOptions,
    UnknownBits { family: MqWireFamily, bits: i32 },
    PendingOptions { family: MqWireFamily, bits: i32 },
    PendingVersion { family: MqWireFamily, version: i32 },
    UnreviewedVersion { family: MqWireFamily, version: i32 },
    IllegalCombination,
    PendingFields,
    MissingUnit,
    PendingExternalUnit,
    MissingCursor,
    PendingWait,
    InvalidConnection,
    Object(MqObjectRouteError),
    Message(MqMessageProblem),
}

fn mqlong(value: i64) -> Result<i32, MqWireProblem> {
    i32::try_from(value).map_err(|_| MqWireProblem::MqlongRange)
}

fn options(value: i64, family: MqWireFamily, known: i32) -> Result<i32, MqWireProblem> {
    let value = mqlong(value)?;
    if value < 0 {
        return Err(MqWireProblem::NegativeOptions);
    }
    let bits = value & !known;
    if bits != 0 {
        return Err(MqWireProblem::UnknownBits { family, bits });
    }
    Ok(value)
}

fn represented(value: i32, family: MqWireFamily, supported: i32) -> Result<(), MqWireProblem> {
    let bits = value & !supported;
    if bits != 0 {
        return Err(MqWireProblem::PendingOptions { family, bits });
    }
    Ok(())
}

fn version(
    value: i64,
    family: MqWireFamily,
    supported: i32,
    reviewed: &[i32],
) -> Result<(), MqWireProblem> {
    let value = mqlong(value)?;
    if value == supported {
        return Ok(());
    }
    if reviewed.contains(&value) {
        return Err(MqWireProblem::PendingVersion {
            family,
            version: value,
        });
    }
    Err(MqWireProblem::UnreviewedVersion {
        family,
        version: value,
    })
}

fn md_version(value: i64) -> Result<(), MqWireProblem> {
    let value = mqlong(value)?;
    if [MQMD_VERSION_1, MQMD_VERSION_2].contains(&value) {
        return Ok(());
    }
    Err(MqWireProblem::UnreviewedVersion {
        family: MqWireFamily::MessageDescriptor,
        version: value,
    })
}

fn connection(value: MqHconn) -> Result<(), MqWireProblem> {
    if value == MqHconn::Unassociated {
        Err(MqWireProblem::InvalidConnection)
    } else {
        Ok(())
    }
}

fn queue(lookup: &MqRouteLookup) -> Result<(), MqWireProblem> {
    if matches!(
        lookup,
        MqRouteLookup::Queue {
            manager: None,
            dynamic_pattern: None,
            ..
        }
    ) {
        Ok(())
    } else {
        Err(MqWireProblem::PendingFields)
    }
}

/// Uses existing opaque handles and bounded typed names. MQOD extensions and all
/// unrepresented modifiers are explicit pending, including zero-valued defaults
/// that rely on queue policy; the service must enforce that policy independently.
pub fn open(
    connection: MqHconn,
    lookup: MqRouteLookup,
    od_version: i64,
    wire_options: i64,
    bindings: &impl MqWireBindings,
) -> Result<MqObjectOpenRequest, MqWireProblem> {
    version(
        od_version,
        MqWireFamily::ObjectDescriptor,
        MQOD_VERSION_1,
        &[],
    )?;
    queue(&lookup)?;
    let value = options(wire_options, MqWireFamily::Open, MQOO_KNOWN)?;
    if (value & (MQOO_INPUT_AS_Q_DEF | MQOO_INPUT_SHARED | MQOO_INPUT_EXCLUSIVE)).count_ones() > 1 {
        return Err(MqWireProblem::IllegalCombination);
    }
    represented(
        value,
        MqWireFamily::Open,
        MQOO_INPUT_AS_Q_DEF
            | MQOO_INPUT_SHARED
            | MQOO_INPUT_EXCLUSIVE
            | MQOO_BROWSE
            | MQOO_OUTPUT
            | MQOO_INQUIRE
            | MQOO_SET,
    )?;
    if !bindings.queue_defaults_are_represented(connection, None, Some(&lookup)) {
        return Err(MqWireProblem::PendingFields);
    }
    let access: Vec<_> = [
        (MQOO_INPUT_AS_Q_DEF, MqRouteOpenAccess::InputAsQueueDefault),
        (MQOO_INPUT_SHARED, MqRouteOpenAccess::InputShared),
        (MQOO_INPUT_EXCLUSIVE, MqRouteOpenAccess::InputExclusive),
        (MQOO_BROWSE, MqRouteOpenAccess::Browse),
        (MQOO_OUTPUT, MqRouteOpenAccess::Output),
        (MQOO_INQUIRE, MqRouteOpenAccess::Inquire),
        (MQOO_SET, MqRouteOpenAccess::Set),
    ]
    .into_iter()
    .filter_map(|(bit, access)| (value & bit != 0).then_some(access))
    .collect();
    MqObjectOpenRequest::new(connection, lookup, &access, Default::default())
        .map_err(MqWireProblem::Object)
}

/// Zero means NONE for this local object subset. It does not assert equivalence
/// to client read-ahead IMMEDIATE. Lifecycle is an expectation, never permission.
pub fn close(
    connection: MqHconn,
    target: MqRouteCloseTarget,
    wire_options: i64,
    bindings: &impl MqWireBindings,
) -> Result<MqObjectCloseRequest, MqWireProblem> {
    connection_check(connection)?;
    let value = options(wire_options, MqWireFamily::Close, MQCO_KNOWN)?;
    if value.count_ones() > 1 {
        return Err(MqWireProblem::IllegalCombination);
    }
    let MqRouteCloseTarget::Object { handle, .. } = target else {
        return Err(MqWireProblem::PendingFields);
    };
    represented(value, MqWireFamily::Close, MQCO_DELETE | MQCO_DELETE_PURGE)?;
    if !bindings.queue_defaults_are_represented(connection, Some(handle), None) {
        return Err(MqWireProblem::PendingFields);
    }
    let mode = match value {
        MQCO_NONE => MqRouteCloseMode::None,
        MQCO_DELETE => MqRouteCloseMode::Delete,
        MQCO_DELETE_PURGE => MqRouteCloseMode::DeletePurge,
        _ => unreachable!("represented close modes"),
    };
    MqObjectCloseRequest::new(connection, target, mode).map_err(MqWireProblem::Object)
}

fn unit(
    bindings: &impl MqWireBindings,
    connection: MqHconn,
    sync: bool,
    no_sync: bool,
    browse: bool,
) -> Result<MqMqiUnitOfWork, MqWireProblem> {
    let needs_unit = !browse
        && (sync
            || (!no_sync && bindings.queue_manager_platform() == MqWireQueueManagerPlatform::Zos));
    if !needs_unit {
        return Ok(MqMqiUnitOfWork::NoSyncpoint);
    }
    match bindings.admitted_unit(connection) {
        Some(MqMqiUnitOfWork::Local { unit }) if unit != 0 => Ok(MqMqiUnitOfWork::Local { unit }),
        Some(MqMqiUnitOfWork::ExternalPending { .. }) => Err(MqWireProblem::PendingExternalUnit),
        _ => Err(MqWireProblem::MissingUnit),
    }
}

/// MQGMO1 has no MatchOptions extension. MsgId/CorrelId matching is supplied as
/// decoded typed identifiers; numeric MQMO constants are not inferred here.
pub struct MqWireGet {
    pub connection: MqHconn,
    pub object: MqHobj,
    pub md_version: i64,
    pub gmo_version: i64,
    pub options: i64,
    pub wait_milliseconds: i64,
    pub selection: MqMessageMatch,
    pub buffer_capacity: i64,
}

pub fn get(
    input: MqWireGet,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiGet, MqWireProblem> {
    connection(input.connection)?;
    md_version(input.md_version)?;
    version(
        input.gmo_version,
        MqWireFamily::Get,
        MQGMO_VERSION_1,
        &[MQGMO_VERSION_2, MQGMO_VERSION_3, MQGMO_VERSION_4],
    )?;
    let value = options(input.options, MqWireFamily::Get, MQGMO_KNOWN)?;
    let sync = value & (MQGMO_SYNCPOINT | MQGMO_NO_SYNCPOINT);
    let mode = value & (MQGMO_BROWSE_FIRST | MQGMO_BROWSE_NEXT | MQGMO_MSG_UNDER_CURSOR);
    let browse = mode == MQGMO_BROWSE_FIRST || mode == MQGMO_BROWSE_NEXT;
    let sync_modes = value & (MQGMO_SYNCPOINT | MQGMO_NO_SYNCPOINT | MQGMO_SYNCPOINT_IF_PERSISTENT);
    if sync_modes.count_ones() > 1
        || mode.count_ones() > 1
        || (browse && value & (MQGMO_SYNCPOINT | MQGMO_SYNCPOINT_IF_PERSISTENT) != 0)
    {
        return Err(MqWireProblem::IllegalCombination);
    }
    represented(
        value,
        MqWireFamily::Get,
        MQGMO_WAIT
            | MQGMO_SYNCPOINT
            | MQGMO_NO_SYNCPOINT
            | MQGMO_BROWSE_FIRST
            | MQGMO_BROWSE_NEXT
            | MQGMO_MSG_UNDER_CURSOR
            | MQGMO_ACCEPT_TRUNCATED_MSG,
    )?;
    if !bindings.queue_defaults_are_represented(input.connection, Some(input.object), None) {
        return Err(MqWireProblem::PendingFields);
    }
    if input.selection.identifiers.group_id.is_some() {
        return Err(MqWireProblem::PendingFields);
    }
    // The source ignores MatchOptions under cursor. The current kernel does not;
    // admit only empty selection rather than silently changing caller intent.
    if mode == MQGMO_MSG_UNDER_CURSOR && input.selection != MqMessageMatch::default() {
        return Err(MqWireProblem::PendingFields);
    }
    let mode = match mode {
        MQGMO_BROWSE_FIRST => MqGetMode::BrowseFirst,
        MQGMO_BROWSE_NEXT | MQGMO_MSG_UNDER_CURSOR => {
            let cursor = bindings
                .existing_cursor(input.connection, input.object)
                .filter(|cursor| *cursor != 0)
                .ok_or(MqWireProblem::MissingCursor)?;
            if mode == MQGMO_BROWSE_NEXT {
                MqGetMode::BrowseNext { cursor }
            } else {
                MqGetMode::RemoveUnderCursor { cursor }
            }
        }
        _ => MqGetMode::Remove,
    };
    let wait_ms = mqlong(input.wait_milliseconds)?;
    let wait = if value & MQGMO_WAIT != 0 {
        if wait_ms < 0 {
            return Err(MqWireProblem::PendingWait);
        }
        if wait_ms == 0 {
            MqWait::NoWait
        } else {
            MqWait::BoundedHostTicks(
                bindings
                    .milliseconds_to_ticks(wait_ms as u32)
                    .filter(|ticks| *ticks != 0 && *ticks <= limits.wait_ticks)
                    .ok_or(MqWireProblem::PendingWait)?,
            )
        }
    } else {
        // Nonzero ignored fields are outside this strict constructor subset.
        if wait_ms != 0 {
            return Err(MqWireProblem::PendingFields);
        }
        MqWait::NoWait
    };
    let capacity = mqlong(input.buffer_capacity)?;
    let buffer_capacity = usize::try_from(capacity).map_err(|_| MqWireProblem::MqlongRange)?;
    let get = MqGetContract {
        selection: input.selection,
        mode,
        wait,
        truncation: if value & MQGMO_ACCEPT_TRUNCATED_MSG != 0 {
            MqTruncation::Accept
        } else {
            MqTruncation::Reject
        },
        buffer_capacity,
    };
    get.validate(limits).map_err(MqWireProblem::Message)?;
    Ok(MqMqiGet {
        connection: input.connection,
        object: input.object,
        get,
        message_handle: None,
        options: MqMqiOptions::ContractDefault,
        unit: unit(
            bindings,
            input.connection,
            sync & MQGMO_SYNCPOINT != 0,
            sync & MQGMO_NO_SYNCPOINT != 0,
            browse,
        )?,
    })
}

pub struct MqWirePut {
    pub md_version: i64,
    pub pmo_version: i64,
    pub options: i64,
    pub message: MqMessage,
}

/// Preserves typed message fields; NEW_MSG_ID selects existing generator intent
/// by clearing the requested ID. No ID or status is generated by this adapter.
pub fn put(
    connection: MqHconn,
    object: MqHobj,
    input: MqWirePut,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiPut, MqWireProblem> {
    if !bindings.queue_defaults_are_represented(connection, Some(object), None) {
        return Err(MqWireProblem::PendingFields);
    }
    put_fields(connection, input, bindings, limits)
}

fn put_fields(
    connection: MqHconn,
    mut input: MqWirePut,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiPut, MqWireProblem> {
    connection_check(connection)?;
    md_version(input.md_version)?;
    version(
        input.pmo_version,
        MqWireFamily::Put,
        MQPMO_VERSION_1,
        &[MQPMO_VERSION_2, MQPMO_VERSION_3],
    )?;
    let value = options(input.options, MqWireFamily::Put, MQPMO_KNOWN)?;
    let sync = value & (MQPMO_SYNCPOINT | MQPMO_NO_SYNCPOINT);
    let context = value
        & (MQPMO_DEFAULT_CONTEXT
            | MQPMO_NO_CONTEXT
            | MQPMO_PASS_IDENTITY_CONTEXT
            | MQPMO_PASS_ALL_CONTEXT
            | MQPMO_SET_IDENTITY_CONTEXT
            | MQPMO_SET_ALL_CONTEXT);
    let response = value & (MQPMO_ASYNC_RESPONSE | MQPMO_SYNC_RESPONSE);
    if sync.count_ones() > 1 || context.count_ones() > 1 || response.count_ones() > 1 {
        return Err(MqWireProblem::IllegalCombination);
    }
    represented(
        value,
        MqWireFamily::Put,
        MQPMO_SYNCPOINT | MQPMO_NO_SYNCPOINT | MQPMO_DEFAULT_CONTEXT | MQPMO_NEW_MSG_ID,
    )?;
    if !input.message.properties.is_empty()
        || input.message.descriptor.identifiers.group_id.is_some()
        || input.message.descriptor.ordering != MqMessageOrdering::default()
        || matches!(
            input.message.descriptor.expiry,
            crate::MqExpiry::PendingSource
        )
        || matches!(
            input.message.descriptor.persistence,
            crate::MqPersistence::PendingSource
        )
        || matches!(
            input.message.descriptor.priority,
            crate::MqPriority::PendingNumeric(_)
        )
    {
        return Err(MqWireProblem::PendingFields);
    }
    input
        .message
        .validate(limits)
        .map_err(MqWireProblem::Message)?;
    if value & MQPMO_NEW_MSG_ID != 0 {
        input.message.descriptor.identifiers.message_id = None;
    }
    Ok(MqMqiPut {
        message: input.message,
        message_handle: None,
        context: MqMqiMessageContext::Default,
        options: MqMqiOptions::ContractDefault,
        unit: unit(
            bindings,
            connection,
            sync & MQPMO_SYNCPOINT != 0,
            sync & MQPMO_NO_SYNCPOINT != 0,
            false,
        )?,
    })
}

fn connection_check(value: MqHconn) -> Result<(), MqWireProblem> {
    connection(value)
}

/// PUT1 still requires actual HCONN even though it has no HOBJ.
pub fn put_one(
    connection: MqHconn,
    lookup: MqRouteLookup,
    od_version: i64,
    input: MqWirePut,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiRequest, MqWireProblem> {
    version(
        od_version,
        MqWireFamily::ObjectDescriptor,
        MQOD_VERSION_1,
        &[],
    )?;
    queue(&lookup)?;
    if !bindings.queue_defaults_are_represented(connection, None, Some(&lookup)) {
        return Err(MqWireProblem::PendingFields);
    }
    Ok(MqMqiRequest::PutOne {
        connection,
        lookup,
        alternate_user: None,
        put: put_fields(connection, input, bindings, limits)?,
    })
}

/// Digest of the exact reviewed numeric/source projection, not execution evidence.
#[must_use]
pub const fn source_projection_sha256() -> &'static str {
    PROJECTION_SHA256
}

/// Reviewed numeric identities, including pending forms and contextual aliases.
/// Presence here never admits an option or constructs a status/handle.
#[must_use]
pub const fn numeric_identities() -> &'static [(&'static str, i32)] {
    NUMERIC_IDENTITIES
}
