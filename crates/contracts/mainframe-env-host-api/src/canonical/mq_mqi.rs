//! Actual host MQI effect framing over the sole streaming Encoder authority.

use super::*;

impl Canonical for MqMqiHostRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { envelope, mutation } = self;
        out.object("MqMqiHostRequest", 2)?;
        out.text("envelope")?;
        envelope.encode(out)?;
        out.text("mutation")?;
        mutation.encode(out)
    }
}

impl Canonical for MqMqiHostResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { limits, result } = self;
        out.object("MqMqiHostResult", 2)?;
        out.text("limits")?;
        limits.encode(out)?;
        out.text("result")?;
        result.encode(out)
    }
}

pub(super) fn encode_request(
    value: &MqMqiHostRequest,
    out: &mut Encoder<'_>,
) -> Result<(), HostProblem> {
    out.variant("HostRequest", "MqMqi", 1)?;
    out.text("0")?;
    value.encode(out)
}

pub(super) fn encode_result(
    value: &MqMqiHostResult,
    out: &mut Encoder<'_>,
) -> Result<(), HostProblem> {
    out.variant("HostResult", "MqMqi", 1)?;
    out.text("0")?;
    value.encode(out)
}

struct Request<'a>(&'a MqMqiHostRequest);
impl Canonical for Request<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        encode_request(self.0, out)
    }
}
struct Reply<'a>(&'a MqMqiHostResult);
impl Canonical for Reply<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        encode_result(self.0, out)
    }
}

pub(crate) fn request_size(value: &MqMqiHostRequest, limit: usize) -> Result<usize, HostProblem> {
    encode(
        &Request(value),
        REQUEST_DIGEST_DOMAIN,
        limit.min(MAX_CANONICAL_EFFECT_BYTES),
        &mut |_| {},
    )
}

pub(crate) fn result_size(value: &MqMqiHostResult, limit: usize) -> Result<usize, HostProblem> {
    let outcome: Result<_, HostProblem> = Ok(Reply(value));
    encode(
        &outcome,
        RESULT_DIGEST_DOMAIN,
        limit.min(MAX_CANONICAL_EFFECT_BYTES),
        &mut |_| {},
    )
}
