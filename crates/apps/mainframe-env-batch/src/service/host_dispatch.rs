//! Sole host dispatch boundary; ordinary audit cadence and explicit external loan.
use super::*;

impl BatchService {
    pub(super) fn invoke_host(
        &self,
        invocation: &(impl RunInput + ?Sized),
        now_tick: u64,
        cancellation_requested: bool,
        request: EffectRequest,
    ) -> EffectResult {
        let control = match invocation.check() {
            Ok(control) => control,
            Err(problem) => {
                return EffectResult {
                    sequence: request.sequence,
                    outcome: Err(problem),
                };
            }
        };
        if invocation.all_effects() {
            return effect_loan::dispatch(invocation, request, None);
        }
        let result = ScopedHostService::invoke(
            &self.host,
            invocation.original(),
            control.map_or(now_tick, |control| control.now_tick),
            cancellation_requested || control.is_some_and(|control| control.cancellation_requested),
            request,
        )
        .persist_with(|audit| self.store.record_audit(audit).map_err(store_error));
        if let Err(problem) = &result.outcome
            && run_stop::fence_host_reply(invocation.contained(), problem)
        {
            return EffectResult {
                sequence: result.sequence,
                outcome: Err(invocation.poison(problem.clone())),
            };
        }
        if let Err(problem) = invocation.check() {
            return EffectResult {
                sequence: result.sequence,
                outcome: Err(problem),
            };
        }
        result
    }
}
