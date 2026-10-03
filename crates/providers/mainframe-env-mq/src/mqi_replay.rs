//! Private strict typed-result storage conversion, not a receipt or dispatch authority.
//! Historical tokens have no live registry authority. No namespace selection,
//! owner minting or status calculation.

use crate::delivery::replay::{ReplayDescriptor, ReplayMessage, ReplayProperty};
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_object_route::{
    MQ_ROUTE_NAME_BYTES, MqDynamicQueueOpenResult, MqRouteDynamicKind, MqRouteName,
};
use mainframe_env_host_api::mq_status::{MqReviewedStatus, MqStatusProblem};
use mainframe_env_host_api::*;
use serde::{Deserialize, Serialize};

mod budget;
mod full_message;
mod handles;
mod producer;
mod property;
mod rfh2;
mod shape;
use shape::{StoredOutcome, StoredResult};

pub(crate) const SCHEMA: &str = "mainframe-env.mq-mqi-result-storage@1";
// Same codec/authority, distinct admitted output vocabulary; old bytes stay @1.
pub(crate) const FULL_SCHEMA: &str = "mainframe-env.mq-mqi-result-storage@2";
pub(crate) const PROPERTY_SCHEMA: &str = "mainframe-env.mq-mqi-result-storage@3";
// Distinct admitted classes share this sole codec; older bytes retain their versions.
pub(crate) const RFH2_SCHEMA: &str = "mainframe-env.mq-mqi-result-storage@4";
pub(crate) const PRODUCER_SCHEMA: &str = "mainframe-env.mq-mqi-result-storage@5";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplayPending {
    HistoricalSpecialConnection,
}
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ReplayError {
    Bounds,
    Malformed,
    UnsupportedSchema,
    DigestMismatch,
    Unsupported(ReplayPending),
    Host(HostProblem),
    Mqi(MqMqiProblem),
    Status(MqStatusProblem),
    MessageProjection,
    Handle(MqHandleProblem),
}
impl From<MqHandleProblem> for ReplayError {
    fn from(value: MqHandleProblem) -> Self {
        Self::Handle(value)
    }
}
impl From<crate::MqDeliveryError> for ReplayError {
    fn from(_: crate::MqDeliveryError) -> Self {
        Self::MessageProjection
    }
}

/// Limits are trusted codec inputs; exact MQI limits also belong to the original
/// full host result identity. A different profile cannot silently relabel it.
pub(crate) fn encode(
    result: &MqMqiResult,
    host: HostLimits,
    mqi: MqMqiLimits,
    byte_ceiling: usize,
) -> Result<Vec<u8>, ReplayError> {
    limits(host, mqi, byte_ceiling)?;
    refuse_special_connections(result)?;
    let digest = validate_and_digest(result, host, mqi)?;
    let stored = StoredResult {
        schema_version: if producer_output(result) {
            PRODUCER_SCHEMA
        } else if rfh2_output(result) {
            RFH2_SCHEMA
        } else if property_output(result) {
            PROPERTY_SCHEMA
        } else if full_output(result) {
            FULL_SCHEMA
        } else {
            SCHEMA
        }
        .into(),
        call: result.call.label().into(),
        outcome: StoredOutcome::from_result(result)?,
        host_result_digest: digest,
    };
    let mut sink = budget::Sink::new(byte_ceiling);
    if serde_json::to_writer(&mut sink, &stored).is_err() {
        return Err(ReplayError::Bounds);
    }
    let bytes = sink.into_bytes();
    budget::preflight(&bytes, host, mqi)?;
    Ok(bytes)
}

pub(crate) fn decode(
    bytes: &[u8],
    host: HostLimits,
    mqi: MqMqiLimits,
    byte_ceiling: usize,
) -> Result<MqMqiResult, ReplayError> {
    limits(host, mqi, byte_ceiling)?;
    if bytes.len() > byte_ceiling {
        return Err(ReplayError::Bounds);
    }
    // Streaming traversal without a value tree BEFORE typed vectors/strings.
    // Bounds every collection, nesting, map/key set and catches duplicate keys.
    budget::preflight(bytes, host, mqi)?;
    let stored: StoredResult = serde_json::from_slice(bytes).map_err(|_| ReplayError::Malformed)?;
    if !matches!(
        stored.schema_version.as_str(),
        SCHEMA | FULL_SCHEMA | PROPERTY_SCHEMA | RFH2_SCHEMA | PRODUCER_SCHEMA
    ) || (stored.schema_version == FULL_SCHEMA) != stored.outcome.is_full()
        || (stored.schema_version == PROPERTY_SCHEMA) != stored.outcome.is_property()
        || (stored.schema_version == PRODUCER_SCHEMA) != stored.outcome.is_producer()
        || (stored.schema_version == RFH2_SCHEMA) != stored.outcome.is_rfh2()
    {
        return Err(ReplayError::UnsupportedSchema);
    }
    let call = MqMqiCall::ALL
        .into_iter()
        .find(|c| c.label() == stored.call)
        .ok_or(ReplayError::Malformed)?;
    let result = MqMqiResult {
        call,
        outcome: stored.outcome.into_outcome(call)?,
    };
    if validate_and_digest(&result, host, mqi)? != stored.host_result_digest {
        return Err(ReplayError::DigestMismatch);
    }
    Ok(result)
}

fn rfh2_output(value: &MqMqiResult) -> bool {
    matches!(&value.outcome, MqMqiOutcome::ReviewedOutput { output, .. }
        if matches!(output, MqMqiOutput::Rfh2Observation(_)))
}
fn property_output(value: &MqMqiResult) -> bool {
    matches!(&value.outcome,MqMqiOutcome::Completed {output,..}|MqMqiOutcome::StatusPending {output}|MqMqiOutcome::ReviewedOutput {output,..}
        if matches!(output,MqMqiOutput::PropertyObservation(_)))
}
fn producer_output(value: &MqMqiResult) -> bool {
    matches!(&value.outcome, MqMqiOutcome::Completed { output, .. }
        | MqMqiOutcome::StatusPending { output } | MqMqiOutcome::ReviewedOutput { output, .. }
        if matches!(output, MqMqiOutput::Produced(_)))
}
fn full_output(value: &MqMqiResult) -> bool {
    matches!(&value.outcome, MqMqiOutcome::Completed { output, .. }
        | MqMqiOutcome::StatusPending { output } | MqMqiOutcome::ReviewedOutput { output, .. }
        if matches!(output, MqMqiOutput::FullPut {..} | MqMqiOutput::FullGot {..}))
}

fn limits(host: HostLimits, mqi: MqMqiLimits, bytes: usize) -> Result<(), ReplayError> {
    let max = HostLimits::default();
    for (n, cap) in [
        (host.max_name_bytes, max.max_name_bytes),
        (host.max_record_bytes, max.max_record_bytes),
        (host.max_records, max.max_records),
        (host.max_fields, max.max_fields),
        (host.max_audit_fields, max.max_audit_fields),
        (host.max_state_bytes, max.max_state_bytes),
        (bytes, MAX_CANONICAL_EFFECT_BYTES),
    ] {
        if n == 0 || n > cap {
            return Err(ReplayError::Bounds);
        }
    }
    mqi.validate().map_err(ReplayError::Mqi)
}

fn refuse_special_connections(value: &MqMqiResult) -> Result<(), ReplayError> {
    if let MqMqiOutcome::Completed { output, .. }
    | MqMqiOutcome::StatusPending { output }
    | MqMqiOutcome::ReviewedOutput { output, .. } = &value.outcome
        && matches!(
            output,
            MqMqiOutput::Connected(MqHconn::Default | MqHconn::Unassociated)
        )
    {
        return Err(ReplayError::Unsupported(
            ReplayPending::HistoricalSpecialConnection,
        ));
    }
    Ok(())
}

fn validate_and_digest(
    value: &MqMqiResult,
    host: HostLimits,
    mqi: MqMqiLimits,
) -> Result<[u8; 32], ReplayError> {
    value.validate(mqi).map_err(ReplayError::Mqi)?;
    // Persisted local UOW/cursor identities cannot overflow SQL adapters. This
    // is storage representability, not new MQ numeric legality or ownership.
    if let MqMqiOutcome::Completed { output, .. }
    | MqMqiOutcome::StatusPending { output }
    | MqMqiOutcome::ReviewedOutput { output, .. } = &value.outcome
    {
        let id = match output {
            MqMqiOutput::UnitOfWork { unit } => Some(*unit),
            MqMqiOutput::Got { cursor, .. } | MqMqiOutput::FullGot { cursor, .. } => *cursor,
            _ => None,
        };
        if id.is_some_and(|n| n == 0 || n > i64::MAX as u64) {
            return Err(ReplayError::Bounds);
        }
    }
    let wrapper = MqMqiHostResult {
        result: value.clone(),
        limits: mqi,
    };
    if value.call != MqMqiCall::CallbackFunction {
        wrapper.validate(host).map_err(ReplayError::Host)?;
    }
    // Callback notifications are private stored values only: the frozen
    // standalone validator allows them; public host-effect validation rejects
    // them. Counting/hashing does not admit them as executable host effects.
    let full = Ok(HostResult::MqMqi(wrapper));
    canonical_result_size(&full, mqi.canonical_bytes).map_err(ReplayError::Host)?;
    canonical_result_digest(&full).map_err(ReplayError::Host)
}

#[cfg(test)]
mod tests;
