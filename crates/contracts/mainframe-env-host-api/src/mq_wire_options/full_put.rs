//! Complete PMO1 adapter over the single numeric authority; never dispatch.
use super::*;
use crate::mq_mqi::{MqFullMessage, MqMqiFullPut};

/// Complete input and finite PMO scalars, without pointer or numeric handle minting.
pub struct MqWireFullPut {
    /// Exact full message observation, without partial DTO conversion.
    pub message: MqFullMessage,
    /// PMO version; only version1 is represented by this producer intent.
    pub pmo_version: i64,
    /// Original numeric bits, checked through the existing generated table.
    pub options: i64,
}
/// Retain complete input and finite explicit sync/context controls. Independent
/// original core, live handles/unit, catalog, context, time and SAF remain required.
pub fn put_full(
    connection: MqHconn,
    input: MqWireFullPut,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiFullPut, MqWireProblem> {
    decode(connection, None, input, bindings, limits)
}

/// Decode the same finite producer intent with an exact retained object or
/// predefined PUT1 lookup. Only the existing trusted binding may establish a
/// synchronous queue default; caller fields do not attest it. PUT1 under
/// syncpoint/default response remains pending (q098655_336–341). No unit,
/// permission, queue transition or output is created by this adapter.
pub fn put_full_for_target(
    connection: MqHconn,
    object: Option<MqHobj>,
    lookup: Option<&MqRouteLookup>,
    input: MqWireFullPut,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiFullPut, MqWireProblem> {
    if object.is_some() == lookup.is_some() {
        return Err(MqWireProblem::IllegalCombination);
    }
    decode(connection, Some((object, lookup)), input, bindings, limits)
}

fn decode(
    connection: MqHconn,
    target: Option<(Option<MqHobj>, Option<&MqRouteLookup>)>,
    input: MqWireFullPut,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiFullPut, MqWireProblem> {
    connection_check(connection)?;
    version(
        input.pmo_version,
        MqWireFamily::Put,
        MQPMO_VERSION_1,
        &[MQPMO_VERSION_2, MQPMO_VERSION_3],
    )?;
    let bits = options(input.options, MqWireFamily::Put, MQPMO_KNOWN)?;
    let sync = bits & (MQPMO_SYNCPOINT | MQPMO_NO_SYNCPOINT);
    let context = bits & (MQPMO_DEFAULT_CONTEXT | MQPMO_NO_CONTEXT);
    if sync.count_ones() != 1 || context.count_ones() != 1 {
        return Err(MqWireProblem::IllegalCombination);
    }
    represented(
        bits,
        MqWireFamily::Put,
        MQPMO_SYNCPOINT
            | MQPMO_NO_SYNCPOINT
            | MQPMO_DEFAULT_CONTEXT
            | MQPMO_NO_CONTEXT
            | MQPMO_SYNC_RESPONSE,
    )?;
    if bits & MQPMO_SYNC_RESPONSE == MQPMO_RESPONSE_AS_Q_DEF {
        let (object, lookup) = target.ok_or(MqWireProblem::IllegalCombination)?;
        if bindings.queue_manager_platform() != MqWireQueueManagerPlatform::Zos
            || (lookup.is_some() && sync == MQPMO_SYNCPOINT)
            || !bindings.queue_defaults_are_represented(connection, object, lookup)
        {
            return Err(MqWireProblem::PendingOptions {
                family: MqWireFamily::Put,
                bits: MQPMO_RESPONSE_AS_Q_DEF,
            });
        }
    }
    let put = MqMqiFullPut {
        message: input.message,
        message_handle: None,
        context: if context == MQPMO_NO_CONTEXT {
            MqMqiMessageContext::NoContext
        } else {
            MqMqiMessageContext::Default
        },
        options: MqMqiOptions::PutV1Synchronous,
        unit: unit(
            bindings,
            connection,
            sync == MQPMO_SYNCPOINT,
            sync == MQPMO_NO_SYNCPOINT,
            false,
        )?,
    };
    put.message
        .validate(limits)
        .map_err(|_| MqWireProblem::PendingFields)?;
    put.validate_producer_profile()
        .map_err(|_| MqWireProblem::PendingFields)?;
    Ok(put)
}
