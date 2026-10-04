//! Typed MQI host-effect records. No dispatch, handle attestation or participant authority.

use super::{EffectRequest, HostLimits, HostProblem, HostRequest, Mutation};
use crate::mq_mqi::{MqMqiCall, MqMqiLimits, MqMqiProblem, MqMqiRequestEnvelope, MqMqiResult};

mod bounds;

/// The original MQI envelope and journal occurrence identity are encoded together.
/// All occurrences, including observations, use the existing mutation identity for replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiHostRequest {
    pub envelope: MqMqiRequestEnvelope,
    pub mutation: Mutation,
}

/// Explicit source call/status and limits. The provider must compare these to the original
/// request before publication; standalone validation cannot attest that relationship.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqMqiHostResult {
    pub result: MqMqiResult,
    pub limits: MqMqiLimits,
}

fn problem(value: MqMqiProblem) -> HostProblem {
    match value {
        MqMqiProblem::Limits
        | MqMqiProblem::SelectorCount
        | MqMqiProblem::AttributeCount
        | MqMqiProblem::Buffer
        | MqMqiProblem::CanonicalLimit
        | MqMqiProblem::Allocation => HostProblem::ResourceExhausted,
        MqMqiProblem::Message(crate::MqMessageProblem::Limits) => HostProblem::ResourceExhausted,
        _ => HostProblem::Malformed,
    }
}

impl MqMqiHostRequest {
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        self.envelope.limits.validate().map_err(problem)?;
        self.mutation.validate(limits)?;
        if self.envelope.request.call() == MqMqiCall::CallbackFunction {
            // IBM MQ 9.4 catalog row 0005 describes callback parameters, not an entry point.
            return Err(HostProblem::Malformed);
        }
        // Count the actual HostRequest preimage before validators can clone bounded fields.
        crate::canonical::mq_mqi::request_size(self, self.envelope.limits.canonical_bytes)?;
        bounds::request(&self.envelope.request, limits)?;
        self.envelope.validate().map_err(problem)
    }
}

impl MqMqiHostResult {
    pub fn validate(&self, limits: HostLimits) -> Result<(), HostProblem> {
        self.limits.validate().map_err(problem)?;
        if self.result.call == MqMqiCall::CallbackFunction {
            return Err(HostProblem::Malformed);
        }
        crate::canonical::mq_mqi::result_size(self, self.limits.canonical_bytes)?;
        bounds::result(&self.result, limits)?;
        self.result.validate(self.limits).map_err(problem)
    }
}

/// An immutable borrow of one validated original effect. There is no constructor accepting
/// a second envelope or mutation, and the borrow prevents substitution while it is retained.
pub struct MqMqiEffectOccurrence<'a> {
    effect: &'a EffectRequest,
}

impl EffectRequest {
    /// Extracts the original typed MQI occurrence after ordinary effect validation, including
    /// the mandatory outer sequence/key match for observational as well as state-changing calls.
    pub fn mq_mqi_occurrence(
        &self,
        limits: HostLimits,
    ) -> Result<Option<MqMqiEffectOccurrence<'_>>, HostProblem> {
        if !matches!(self.request, HostRequest::MqMqi(_)) {
            return Ok(None);
        }
        self.validate(limits)?;
        Ok(Some(MqMqiEffectOccurrence { effect: self }))
    }
}

impl MqMqiEffectOccurrence<'_> {
    #[must_use]
    pub fn effect(&self) -> &EffectRequest {
        self.effect
    }

    #[must_use]
    pub fn request(&self) -> &MqMqiHostRequest {
        match &self.effect.request {
            HostRequest::MqMqi(request) => request,
            _ => unreachable!("occurrence can only borrow a validated typed MQI effect"),
        }
    }

    #[must_use]
    pub fn envelope(&self) -> &MqMqiRequestEnvelope {
        &self.request().envelope
    }

    #[must_use]
    pub fn mutation(&self) -> &Mutation {
        &self.request().mutation
    }
}

#[cfg(test)]
mod tests;
