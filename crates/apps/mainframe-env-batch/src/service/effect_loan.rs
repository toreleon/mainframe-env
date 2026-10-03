//! Explicit contained transport; observations never grant Core/JES authority.
use super::*;
use std::cell::RefCell;

/// Actual current Batch-built host occurrence, borrowed by a privileged embedding.
/// It grants no journal, SAF, JES, physical claim/clock or settlement permission.
/// Construction is private; no Clone/Serde or conversion from rows is provided.
/// ```compile_fail
/// use mainframe_env_batch::BatchEffectOccurrence;
/// let occurrence = BatchEffectOccurrence {};
/// ```
/// ```compile_fail
/// use mainframe_env_batch::BatchEffectOccurrence;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<BatchEffectOccurrence<'static>>();
/// ```
/// ```compile_fail
/// use mainframe_env_batch::BatchEffectOccurrence;
/// fn requires_serde<T: serde::Serialize>() {}
/// requires_serde::<BatchEffectOccurrence<'static>>();
/// ```
/// ```compile_fail
/// use mainframe_env_batch::BatchEffectOccurrence;
/// fn escape<'a>(a: &'a BatchEffectOccurrence<'a>) -> &'static BatchEffectOccurrence<'static> { a }
/// ```
pub struct BatchEffectOccurrence<'call> {
    original: &'call Invocation,
    request: EffectRequest,
    program: Option<(&'call Invocation, &'call RunningStepAdmission<'call>)>,
}
impl<'call> BatchEffectOccurrence<'call> {
    /// Exact PRE-binding invocation supplied to the actual runner; not a permit.
    pub fn original(&self) -> &Invocation {
        self.original
    }
    /// Actual owned request, preserving original sequence/key/payload/deadline.
    /// These existing Batch sequences are not claimed to form a Core cursor.
    pub fn request(&self) -> &EffectRequest {
        &self.request
    }
    /// Exact POST jes.work-id Program view, present only for a genuine Program.
    pub fn program_invocation(&self) -> Option<&Invocation> {
        self.program.map(|(invocation, _)| invocation)
    }
    /// Genuine current RunningStep borrow; absent for every non-Program effect.
    pub fn running_step(&self) -> Option<&RunningStepAdmission<'call>> {
        self.program.map(|(_, admission)| admission)
    }
}

struct LoanScope<'a, D, O> {
    inner: run_stop::RunScope<'a, O>,
    dispatch: RefCell<&'a mut D>,
}
impl<D, O> RunInput for LoanScope<'_, D, O>
where
    D: for<'call> FnMut(&BatchEffectOccurrence<'call>) -> EffectResult,
    O: FnMut() -> Result<BatchRunControl, HostProblem>,
{
    fn original(&self) -> &Invocation {
        self.inner.original()
    }
    fn check(&self) -> Result<Option<BatchRunControl>, HostProblem> {
        self.inner.check()
    }
    fn poison(&self, problem: HostProblem) -> HostProblem {
        self.inner.poison(problem)
    }
    fn contained(&self) -> bool {
        true
    }
    fn all_effects(&self) -> bool {
        true
    }
    fn loan(&self, occurrence: &BatchEffectOccurrence<'_>) -> Result<EffectResult, HostProblem> {
        let mut callback = self
            .dispatch
            .try_borrow_mut()
            .map_err(|_| self.poison(HostProblem::UnknownOutcome))?;
        Ok(callback(occurrence))
    }
    fn admit_run(
        &self,
        service: &BatchService,
        row: &ProviderStateRecord,
    ) -> Result<Option<Arc<run_stop::RunLifetime>>, HostProblem> {
        self.inner.admit_run(service, row)
    }
    fn revoke_run(&self) {
        self.inner.revoke_run();
    }
    fn published_job(&self, row: ProviderStateRecord) {
        self.inner.published_job(row);
    }
}

pub(super) fn dispatch(
    scope: &(impl RunInput + ?Sized),
    request: EffectRequest,
    program: Option<(&Invocation, &RunningStepAdmission<'_>)>,
) -> EffectResult {
    let sequence = request.sequence;
    let outcome = (|| {
        scope.check()?;
        let occurrence = BatchEffectOccurrence {
            original: scope.original(),
            request,
            program,
        };
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scope.loan(&occurrence)))
                .unwrap_or(Err(HostProblem::UnknownOutcome))
                .map_err(|problem| scope.poison(problem))?;
        // Unknown cannot be masked by a simultaneous malformed sequence.
        if result.outcome == Err(HostProblem::UnknownOutcome) {
            return Err(scope.poison(HostProblem::UnknownOutcome));
        }
        if result.sequence != sequence {
            return Err(scope.poison(HostProblem::UnknownOutcome));
        }
        if let Err(problem) = &result.outcome
            && run_stop::fence_step_error(true, problem)
        {
            return Err(scope.poison(problem.clone()));
        }
        scope.check()?;
        result.outcome
    })();
    EffectResult { sequence, outcome }
}

impl BatchService {
    /// Explicit privileged transport for every actual contained-run host effect.
    /// The external owner must perform real scoped preflight/result/audit/journal
    /// settlement itself. Batch does not invoke its ordinary provider or persist
    /// a second audit on this route. Missing callback refuses before run writes.
    /// This supplies no Core cursor, physical claim/time, JES/native authority or
    /// atomic selection. Old ordinary and Program-only routes remain unchanged.
    /// Callbacks/observers must be bounded/nonblocking; panic/reentry/stop fences
    /// further effects and retirement. Drop only revokes actual volatile views.
    pub fn run_claimed_with_all_effect_dispatch<D, O>(
        &self,
        invocation: &Invocation,
        id: &str,
        initiator: &str,
        dispatch: Option<&mut D>,
        observe: &mut O,
    ) -> Result<Option<BatchRunExit>, HostProblem>
    where
        D: for<'call> FnMut(&BatchEffectOccurrence<'call>) -> EffectResult,
        O: FnMut() -> Result<BatchRunControl, HostProblem>,
    {
        let dispatch = dispatch.ok_or(HostProblem::Unsupported)?;
        let checkpoints = self
            .checkpoint_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        if Arc::as_ptr(&self.store).cast::<()>() != Arc::as_ptr(checkpoints).cast::<()>() {
            return Err(HostProblem::Unsupported);
        }
        let scope = LoanScope {
            inner: run_stop::RunScope::new(invocation, observe),
            dispatch: RefCell::new(dispatch),
        };
        scope.check()?;
        let topology = self.topology()?;
        let members = topology
            .members
            .values()
            .filter(|member| member.enabled && member.node == topology.local_node)
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        if members.is_empty() {
            return Err(HostProblem::Unsupported);
        }
        for member in members {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.run_on_member(&scope, &member, initiator, false, Some(id), None)
            }))
            .unwrap_or_else(|_| Err(scope.poison(HostProblem::UnknownOutcome)));
            if outcome != Ok(None) {
                return std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scope.inner.exit(outcome)
                }))
                .unwrap_or(Err(HostProblem::UnknownOutcome));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_borrow_reentry_poison_is_irrevocable_without_dispatch() {
        let original = crate::service::tests::invocation();
        let mut observer = || {
            Ok(BatchRunControl {
                now_tick: 1,
                cancellation_requested: false,
            })
        };
        let mut callback = |_: &BatchEffectOccurrence<'_>| panic!("reentry must not dispatch");
        let scope = LoanScope {
            inner: run_stop::RunScope::new(&original, &mut observer),
            dispatch: RefCell::new(&mut callback),
        };
        let occurrence = BatchEffectOccurrence {
            original: &original,
            request: EffectRequest {
                run_unit: original.run_unit_id.clone(),
                sequence: 1,
                deadline_tick: 100,
                idempotency_key: None,
                request: HostRequest::Security(SecurityRequest::Authorize {
                    principal: original.principal.id().clone(),
                    class: "JESJOBS".into(),
                    resource: ResourceName::new("JOB.TESTJOB", 246).unwrap(),
                    intent: AccessIntent::Execute,
                }),
            },
            program: None,
        };
        let held = scope.dispatch.borrow_mut();
        assert_eq!(scope.loan(&occurrence), Err(HostProblem::UnknownOutcome));
        drop(held);
        assert_eq!(scope.check(), Err(HostProblem::UnknownOutcome));
    }
}
