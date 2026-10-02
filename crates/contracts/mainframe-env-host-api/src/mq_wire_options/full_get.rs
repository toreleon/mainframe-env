//! Complete descriptor adapter over the existing numeric GMO control authority.
//! MQGET row0015; supplemental q096715_1269–1381/q097395_1389–1482.
use super::*;
use crate::MqMessageIdentifiers;
use crate::mq_md_value::MqMdValue;
use crate::mq_mqi::MqMqiFullGet;

/// Complete MQMD observation and numeric GMO1 inputs. Handles and policy facts
/// must come from independently admitted bindings, never numeric handle bits.
pub struct MqWireFullGet {
    /// Registry-issued or historical observation; execution rechecks authority.
    pub connection: MqHconn,
    /// Existing object observation, not a numeric ABI alias constructor.
    pub object: MqHobj,
    /// Exact complete descriptor; no partial descriptor projection is made.
    pub descriptor: MqMdValue,
    /// Numeric GMO version, independent of the descriptor's version.
    pub gmo_version: i64,
    /// Reviewed numeric option bits, not an execution permission.
    pub options: i64,
    /// Observed wait interval; the first complete adapter requires zero/no-wait.
    pub wait_milliseconds: i64,
    /// Caller capacity in bytes, checked against MQLONG and retained limits.
    pub buffer_capacity: i64,
}

/// Decode finite remove/no-wait GMO1 controls while retaining every MQMD1/2
/// field and its explicit structure character encoding. Uses the SAME numeric
/// option decoder, binding checks and local-unit rules as the old adapter.
/// GMO1 selects both nonzero binary MsgId and CorrelId; GroupId/sequence/offset
/// do not become additional selectors. Nonmatching diagnostic descriptor fields
/// remain exact observations, not evidence of per-call legality. The selected
/// provider still checks its finite descriptor, queue, SAF and lifecycle policy.
/// This function neither dispatches nor mutates a queue or original descriptor.
pub fn get_full(
    input: MqWireFullGet,
    bindings: &impl MqWireBindings,
    limits: MqMessageLimits,
) -> Result<MqMqiFullGet, MqWireProblem> {
    input
        .descriptor
        .validate_representation()
        .map_err(|_| MqWireProblem::PendingFields)?;
    if limits.identifier_bytes < 24 || limits.format_bytes < 8 {
        return Err(MqWireProblem::Message(MqMessageProblem::Limits));
    }
    let flags = options(input.options, MqWireFamily::Get, MQGMO_KNOWN)?;
    represented(
        flags,
        MqWireFamily::Get,
        MQGMO_SYNCPOINT | MQGMO_NO_SYNCPOINT | MQGMO_ACCEPT_TRUNCATED_MSG,
    )?;
    let fields = input.descriptor.fields();
    let controls = get(
        MqWireGet {
            connection: input.connection,
            object: input.object,
            md_version: i64::from(input.descriptor.version()),
            gmo_version: input.gmo_version,
            options: input.options,
            wait_milliseconds: input.wait_milliseconds,
            selection: MqMessageMatch {
                identifiers: MqMessageIdentifiers {
                    message_id: (fields.msg_id != [0; 24]).then(|| fields.msg_id.to_vec()),
                    correlation_id: (fields.correl_id != [0; 24])
                        .then(|| fields.correl_id.to_vec()),
                    group_id: None,
                },
            },
            buffer_capacity: input.buffer_capacity,
        },
        bindings,
        limits,
    )?;
    Ok(MqMqiFullGet {
        connection: controls.connection,
        object: controls.object,
        descriptor: input.descriptor,
        mode: controls.get.mode,
        wait: controls.get.wait,
        truncation: controls.get.truncation,
        buffer_capacity: controls.get.buffer_capacity,
        message_handle: controls.message_handle,
        options: controls.options,
        unit: controls.unit,
    })
}
