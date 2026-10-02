//! Additive schema over the existing canonical streaming authority.
//! No HostRequest/HostResult dispatch or legacy domain is changed.

use super::*;
use crate::HostProblem;
use crate::canonical::{Canonical, Encoder};
use sha2::{Digest, Sha256};

// Struct destructuring is exhaustive: a new field cannot silently disappear.
// Lists below are in ASCII field-name order, as required by the existing schema.
macro_rules! object {
    ($type:ident { $($field:ident),* $(,)? }) => {
        impl Canonical for $type {
            fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
                let Self { $($field),* } = self;
                out.object(stringify!($type), [$(stringify!($field)),*].len())?;
                $(out.text(stringify!($field))?; $field.encode(out)?;)*
                Ok(())
            }
        }
    };
}
macro_rules! variants {
    ($type:ident { $($variant:ident $( { $($field:ident),* } )?),* $(,)? }) => {
        impl Canonical for $type {
            fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
                match self {
                    $(Self::$variant $( { $($field),* } )? => {
                        let fields: &[&str] = &[$($(stringify!($field)),*)?];
                        out.variant(stringify!($type), stringify!($variant), fields.len())?;
                        $($(out.text(stringify!($field))?; $field.encode(out)?;)*)?
                        Ok(())
                    }),*
                }
            }
        }
    };
}

mod payload;
mod values;

fn emit<T: Canonical>(
    value: &T,
    domain: &[u8],
    limit: usize,
    sink: &mut dyn FnMut(&[u8]),
) -> Result<usize, MqMqiProblem> {
    struct Boundary<'a, T>(&'a T);
    impl<T: Canonical> Canonical for Boundary<'_, T> {
        fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
            out.text(crate::EFFECT_CANONICAL_SCHEMA)?;
            out.text(MQ_MQI_BOUNDARY_SCHEMA)?;
            self.0.encode(out)
        }
    }
    crate::canonical::encode(
        &Boundary(value),
        domain,
        limit.min(crate::MAX_CANONICAL_EFFECT_BYTES),
        sink,
    )
    .map_err(|_| MqMqiProblem::CanonicalLimit)
}

fn bytes<T: Canonical>(value: &T, domain: &[u8], limit: usize) -> Result<Vec<u8>, MqMqiProblem> {
    // Count the entire representation before reserving or copying any bytes.
    let size = emit(value, domain, limit, &mut |_| {})?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(size)
        .map_err(|_| MqMqiProblem::Allocation)?;
    emit(value, domain, limit, &mut |bytes| {
        result.extend_from_slice(bytes)
    })?;
    Ok(result)
}
fn digest<T: Canonical>(value: &T, domain: &[u8], limit: usize) -> Result<[u8; 32], MqMqiProblem> {
    let mut hasher = Sha256::new();
    emit(value, domain, limit, &mut |bytes| hasher.update(bytes))?;
    Ok(hasher.finalize().into())
}

pub fn mq_mqi_request_size(value: &MqMqiRequestEnvelope) -> Result<usize, MqMqiProblem> {
    value.limits.validate()?;
    let size = emit(
        value,
        MQ_MQI_REQUEST_DOMAIN,
        value.limits.canonical_bytes,
        &mut |_| {},
    )?;
    value.validate()?;
    Ok(size)
}
pub fn mq_mqi_request_bytes(value: &MqMqiRequestEnvelope) -> Result<Vec<u8>, MqMqiProblem> {
    mq_mqi_request_size(value)?;
    bytes(value, MQ_MQI_REQUEST_DOMAIN, value.limits.canonical_bytes)
}
pub fn mq_mqi_request_digest(value: &MqMqiRequestEnvelope) -> Result<[u8; 32], MqMqiProblem> {
    mq_mqi_request_size(value)?;
    digest(value, MQ_MQI_REQUEST_DOMAIN, value.limits.canonical_bytes)
}

/// Result bounds are hashed with the result, so a different observation budget
/// cannot reuse a result identity. All standalone helpers validate before emit.
struct ResultEnvelope<'a> {
    limits: MqMqiLimits,
    result: &'a MqMqiResult,
}
impl Canonical for ResultEnvelope<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        out.object("MqMqiResultEnvelope", 2)?;
        out.text("limits")?;
        self.limits.encode(out)?;
        out.text("result")?;
        self.result.encode(out)
    }
}
pub fn mq_mqi_result_size(value: &MqMqiResult, limits: MqMqiLimits) -> Result<usize, MqMqiProblem> {
    limits.validate()?;
    let size = emit(
        &ResultEnvelope {
            limits,
            result: value,
        },
        MQ_MQI_RESULT_DOMAIN,
        limits.canonical_bytes,
        &mut |_| {},
    )?;
    value.validate(limits)?;
    Ok(size)
}
pub fn mq_mqi_result_bytes(
    value: &MqMqiResult,
    limits: MqMqiLimits,
) -> Result<Vec<u8>, MqMqiProblem> {
    mq_mqi_result_size(value, limits)?;
    bytes(
        &ResultEnvelope {
            limits,
            result: value,
        },
        MQ_MQI_RESULT_DOMAIN,
        limits.canonical_bytes,
    )
}
pub fn mq_mqi_result_digest(
    value: &MqMqiResult,
    limits: MqMqiLimits,
) -> Result<[u8; 32], MqMqiProblem> {
    mq_mqi_result_size(value, limits)?;
    digest(
        &ResultEnvelope {
            limits,
            result: value,
        },
        MQ_MQI_RESULT_DOMAIN,
        limits.canonical_bytes,
    )
}
