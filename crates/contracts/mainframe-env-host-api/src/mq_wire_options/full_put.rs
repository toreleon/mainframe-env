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
    if sync.count_ones() != 1 || context.count_ones() != 1 || bits & MQPMO_SYNC_RESPONSE == 0 {
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
