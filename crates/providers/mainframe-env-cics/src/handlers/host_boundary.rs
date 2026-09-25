use super::super::*;

impl CicsService {
    pub(in crate::service) fn nested(
        &self,
        run: &mut Run,
        request: HostRequest,
    ) -> Result<HostResult, HostProblem> {
        run.host_sequence = run
            .host_sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        let key = request
            .is_mutating()
            .then(|| nested_key(run, run.host_sequence))
            .transpose()?;
        let nested_invocation = key
            .as_ref()
            .filter(|_| {
                matches!(
                    &request,
                    HostRequest::Dataset(_)
                        | HostRequest::Db2(_)
                        | HostRequest::Ims(_)
                        | HostRequest::Mq(_)
                )
            })
            .map(|key| {
                invocation_with_nested_origin(
                    &run.invocation,
                    key,
                    run.outer_effect_key
                        .as_deref()
                        .ok_or(HostProblem::InfrastructureFailure)?,
                )
            })
            .transpose()?;
        let result = self.invoke_host(
            nested_invocation.as_ref().unwrap_or(&run.invocation),
            run.invocation.deadline_tick.saturating_sub(1),
            false,
            EffectRequest {
                run_unit: run.invocation.run_unit_id.clone(),
                sequence: run.host_sequence,
                deadline_tick: run.invocation.deadline_tick,
                idempotency_key: key,
                request,
            },
        );
        result.outcome
    }
}
