//! Observation only: ordered actual decisions from the sole RACF authority.
use super::*;
use setup::Saf;

const MAX_OBSERVATIONS: usize = 128;

#[derive(Clone)]
struct Occurrence {
    sequence: u64,
    key: String,
    call: String,
    phase: String,
}

#[derive(Default)]
pub(super) struct Trace {
    active: Option<Occurrence>,
    transcript: SafTranscript,
}

// Drop ends only this synchronous observation scope. It has no product authority.
pub(super) struct Scope<'a>(&'a Saf);
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if let Ok(mut trace) = self.0.0.lock() {
            trace.active = None;
        }
    }
}

impl Saf {
    pub(super) fn scope<'a>(
        &'a self,
        invocation: &Invocation,
        effect: &EffectRequest,
        phase: &str,
    ) -> Result<Scope<'a>, HostProblem> {
        let HostRequest::MqMqi(request) = &effect.request else {
            return Err(HostProblem::Malformed);
        };
        if effect.run_unit != invocation.run_unit_id || !matches!(phase, "original" | "replay") {
            return Err(HostProblem::Malformed);
        }
        let mut trace = self
            .0
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if trace.active.is_some() {
            return Err(HostProblem::Malformed);
        }
        let key = effect
            .idempotency_key
            .as_ref()
            .ok_or(HostProblem::Malformed)?;
        if trace.transcript.execution.is_empty() {
            trace.transcript.execution = invocation.execution_id.as_str().into();
            trace.transcript.run_unit = effect.run_unit.as_str().into();
            trace.transcript.invocation_key = invocation.idempotency_key.as_str().into();
        }
        // Shared attribution is exact, checked on every scope, never a copied permit.
        if trace.transcript.execution != invocation.execution_id.as_str()
            || trace.transcript.run_unit != effect.run_unit.as_str()
            || trace.transcript.invocation_key != invocation.idempotency_key.as_str()
        {
            return Err(HostProblem::Malformed);
        }
        trace.active = Some(Occurrence {
            sequence: effect.sequence,
            key: key.as_str().into(),
            call: request.envelope.request.call().label().into(),
            phase: phase.into(),
        });
        Ok(Scope(self))
    }

    pub(super) fn observations(&self) -> Result<SafTranscript, HostProblem> {
        let trace = self
            .0
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if trace.active.is_some() {
            return Err(HostProblem::Malformed);
        }
        Ok(trace.transcript.clone())
    }
}

impl EnterpriseAuthorizer for Saf {
    fn authorize(
        &self,
        principal: &PrincipalId,
        resource: &EnterpriseResource,
    ) -> Result<(), HostProblem> {
        let occurrence = {
            let trace = self
                .0
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if trace.transcript.observations.len() >= MAX_OBSERVATIONS {
                return Err(HostProblem::ResourceExhausted);
            }
            trace.active.clone().ok_or(HostProblem::Malformed)?
        };
        // No trace lock across the actual owning policy decision; no inferred allow.
        let result = EnterpriseAuthorizer::authorize(&*self.1, principal, resource);
        let observation = SafObservation {
            sequence: occurrence.sequence,
            original_key: occurrence.key,
            call: occurrence.call,
            phase: occurrence.phase,
            principal: principal.as_str().into(),
            class: resource.class.saf_class().into(),
            resource: resource.name.as_str().into(),
            intent: format!("{:?}", resource.intent),
            decision: match &result {
                Ok(()) => "allow".into(),
                Err(problem) => format!("{problem:?}"),
            },
        };
        let mut trace = self
            .0
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if trace.transcript.observations.len() >= MAX_OBSERVATIONS || trace.active.is_none() {
            return Err(HostProblem::Malformed);
        }
        trace.transcript.observations.push(observation);
        result
    }
}
