//! Frozen exhaustive host envelope dispatch; field and variant bytes unchanged.
use super::*;

impl Canonical for HostRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(v0) => {
                out.variant("HostRequest", "Dataset", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Program(v0) => {
                out.variant("HostRequest", "Program", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Spool(v0) => {
                out.variant("HostRequest", "Spool", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Terminal(v0) => {
                out.variant("HostRequest", "Terminal", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Security(v0) => {
                out.variant("HostRequest", "Security", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Clock(v0) => {
                out.variant("HostRequest", "Clock", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::State(v0) => {
                out.variant("HostRequest", "State", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Cics(v0) => {
                out.variant("HostRequest", "Cics", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Db2(v0) => {
                out.variant("HostRequest", "Db2", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Ims(v0) => {
                out.variant("HostRequest", "Ims", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::ImsRecovery(v0) => {
                out.variant("HostRequest", "ImsRecovery", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::ImsNavigation(v0) => {
                out.variant("HostRequest", "ImsNavigation", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::ImsGsam(v0) => {
                out.variant("HostRequest", "ImsGsam", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::ImsPcbFeedbackV1(v0) => {
                out.variant("HostRequest", "ImsPcbFeedbackV1", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::Mq(v0) => {
                out.variant("HostRequest", "Mq", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::MqMqi(v0) => super::mq_mqi::encode_request(v0, out),
        }
    }
}

impl Canonical for HostResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Dataset(v0) => {
                out.variant("HostResult", "Dataset", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Program(v0) => {
                out.variant("HostResult", "Program", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Spool(v0) => {
                out.variant("HostResult", "Spool", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Terminal(v0) => {
                out.variant("HostResult", "Terminal", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Security(v0) => {
                out.variant("HostResult", "Security", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Clock(v0) => {
                out.variant("HostResult", "Clock", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::State { value, version } => {
                out.variant("HostResult", "State", 2)?;
                out.text("value")?;
                value.encode(out)?;
                out.text("version")?;
                version.encode(out)?;
                Ok(())
            }
            Self::Cics(v0) => {
                out.variant("HostResult", "Cics", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Db2(v0) => {
                out.variant("HostResult", "Db2", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::Ims(v0) => {
                out.variant("HostResult", "Ims", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::ImsRecovery(v0) => {
                out.variant("HostResult", "ImsRecovery", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::ImsGsam(v0) => {
                out.variant("HostResult", "ImsGsam", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::ImsPcbFeedbackV1(v0) => {
                out.variant("HostResult", "ImsPcbFeedbackV1", 1)?;
                out.text("0")?;
                v0.encode(out)
            }
            Self::Mq(v0) => {
                out.variant("HostResult", "Mq", 1)?;
                out.text("0")?;
                v0.encode(out)?;
                Ok(())
            }
            Self::MqMqi(v0) => super::mq_mqi::encode_result(v0, out),
        }
    }
}

impl Canonical for EffectRequest {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self {
            deadline_tick,
            idempotency_key,
            request,
            run_unit,
            sequence,
        } = self;
        out.object("EffectRequest", 5)?;
        out.text("deadline_tick")?;
        deadline_tick.encode(out)?;
        out.text("idempotency_key")?;
        idempotency_key.encode(out)?;
        out.text("request")?;
        request.encode(out)?;
        out.text("run_unit")?;
        run_unit.encode(out)?;
        out.text("sequence")?;
        sequence.encode(out)?;
        Ok(())
    }
}

impl Canonical for EffectResult {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let Self { outcome, sequence } = self;
        out.object("EffectResult", 2)?;
        out.text("outcome")?;
        outcome.encode(out)?;
        out.text("sequence")?;
        sequence.encode(out)?;
        Ok(())
    }
}

impl Canonical for HostProblem {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        match self {
            Self::Malformed => out.variant("HostProblem", "Malformed", 0),
            Self::Unsupported => out.variant("HostProblem", "Unsupported", 0),
            Self::UnsupportedCapability { capability, detail } => {
                out.variant("HostProblem", "UnsupportedCapability", 2)?;
                out.text("capability")?;
                capability.encode(out)?;
                out.text("detail")?;
                detail.encode(out)?;
                Ok(())
            }
            Self::NotFound => out.variant("HostProblem", "NotFound", 0),
            Self::Condition {
                name,
                response,
                response2,
            } => {
                out.variant("HostProblem", "Condition", 3)?;
                out.text("name")?;
                name.encode(out)?;
                out.text("response")?;
                response.encode(out)?;
                out.text("response2")?;
                response2.encode(out)?;
                Ok(())
            }
            Self::Unauthorized => out.variant("HostProblem", "Unauthorized", 0),
            Self::Cancelled => out.variant("HostProblem", "Cancelled", 0),
            Self::TimedOut => out.variant("HostProblem", "TimedOut", 0),
            Self::ResourceExhausted => out.variant("HostProblem", "ResourceExhausted", 0),
            Self::ProviderFailure => out.variant("HostProblem", "ProviderFailure", 0),
            Self::InfrastructureFailure => out.variant("HostProblem", "InfrastructureFailure", 0),
            Self::MissingIdempotency => out.variant("HostProblem", "MissingIdempotency", 0),
            Self::IdempotencyConflict => out.variant("HostProblem", "IdempotencyConflict", 0),
            Self::UnknownOutcome => out.variant("HostProblem", "UnknownOutcome", 0),
        }
    }
}
