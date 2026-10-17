//! Test-owned, per-server admitted input. Never selected by a production build.
use super::*;

#[derive(Clone, Debug)]
pub(super) struct Observation {
    pub(super) execution_id: ExecutionId,
    pub(super) run_unit_id: RunUnitId,
    pub(super) principal: PrincipalId,
    pub(super) attempt: u32,
    pub(super) sequence: u64,
    pub(super) audit_key: IdempotencyKey,
    pub(super) request: HostRequest,
    pub(super) result: HostResult,
}

pub(super) struct SourceClock {
    descriptor: CapabilityDescriptor,
    observations: Mutex<Vec<Observation>>,
}

impl SourceClock {
    pub(super) fn new(input: &str) -> Result<Self, CorpusProblem> {
        // The independent fixture supplies this exact input. This is not a new
        // product clock parser or a substitute for IBM clock semantics.
        need(input == oracle::UTC_INPUT)?;
        Ok(Self {
            descriptor: CapabilityDescriptor {
                capability: CapabilityId::new("host.clock", InvocationLimits::default())
                    .map_err(|_| refused())?,
                provider_id: "mainframe-env-private-unit-source-clock".into(),
                generation: "1".into(),
                request_schema: "mainframe-env.clock.request@1".into(),
                result_schema: "mainframe-env.clock.response@1".into(),
                max_request_bytes: 256,
                max_result_bytes: 256,
                ready: true,
            },
            observations: Mutex::new(Vec::new()),
        })
    }
    pub(super) fn observations(&self) -> Result<Vec<Observation>, CorpusProblem> {
        self.observations
            .lock()
            .map(|rows| rows.clone())
            .map_err(|_| refused())
    }
}

impl HostProvider for SourceClock {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }
    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = (|| {
            let value = match &effect.request {
                HostRequest::Clock(ClockRequest::UtcTimestamp) => oracle::UTC_INPUT,
                HostRequest::Clock(ClockRequest::Date) => &oracle::UTC_INPUT[..8],
                HostRequest::Clock(ClockRequest::Time) => &oracle::UTC_INPUT[8..],
                _ => return Err(HostProblem::Malformed),
            };
            let result = HostResult::Clock(value.into());
            let mut rows = self
                .observations
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if rows.len() >= 32 {
                return Err(HostProblem::ResourceExhausted);
            }
            rows.push(Observation {
                execution_id: invocation.execution_id.clone(),
                run_unit_id: invocation.run_unit_id.clone(),
                principal: invocation.principal.id().clone(),
                attempt: invocation.attempt,
                sequence: effect.sequence,
                audit_key: invocation.idempotency_key.clone(),
                request: effect.request.clone(),
                result: result.clone(),
            });
            Ok(result)
        })();
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}
