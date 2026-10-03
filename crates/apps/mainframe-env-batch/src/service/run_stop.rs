//! Per-call containment for the actual opt-in runner, never a durable permit.
use super::*;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

/// Current synchronous control observation, not physical lease or JES authority.
/// The runner checks monotonic ticks, deadline and the original live probe. No
/// value here grants a capability or proves freshness inside a store transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchRunControl {
    /// Current finite positive tick in the original invocation's clock domain.
    pub now_tick: u64,
    /// Cancellation observed by the enclosing caller, in addition to its probe.
    pub cancellation_requested: bool,
}

/// Actual opt-in run exit observations; never a core/JES/SAF settlement permit.
/// A stopped run retains its last physical Job row and makes no terminal claim.
/// No constructor, Clone or Serde is provided; equal rows cannot mint an exit.
/// ```compile_fail
/// use mainframe_env_batch::BatchRunExit;
/// let exit = BatchRunExit {};
/// ```
/// ```compile_fail
/// use mainframe_env_batch::BatchRunExit;
/// fn clone_required<T: Clone>() {}
/// clone_required::<BatchRunExit>();
/// ```
/// ```compile_fail
/// use mainframe_env_batch::BatchRunExit;
/// fn serde_required<T: serde::Serialize>() {}
/// serde_required::<BatchRunExit>();
/// ```
pub struct BatchRunExit {
    original: Invocation,
    store: Arc<dyn ProviderStateStore>,
    admitted_row: ProviderStateRecord,
    last_row: ProviderStateRecord,
    snapshot: JobSnapshot,
    stop: Option<HostProblem>,
}

impl BatchRunExit {
    /// Exact runner original, before the shared Program binding projection.
    pub fn original(&self) -> &Invocation {
        &self.original
    }
    /// First genuine known Running ProgramService row, not a restored permit.
    pub fn admitted_row(&self) -> &ProviderStateRecord {
        &self.admitted_row
    }
    /// Last observed physical row. Stopped exits do not publish a replacement.
    pub fn last_row(&self) -> &ProviderStateRecord {
        &self.last_row
    }
    /// Snapshot decoded from that actual row, not arbitrary callback output.
    pub fn snapshot(&self) -> &JobSnapshot {
        &self.snapshot
    }
    /// Exact latched stop, if any; absence means the actual runner retired.
    pub fn stop(&self) -> Option<&HostProblem> {
        self.stop.as_ref()
    }
    /// Compare physical adapter identity without exposing its mutation port.
    pub fn uses_store(&self, store: &Arc<dyn ProviderStateStore>) -> bool {
        Arc::ptr_eq(&self.store, store)
    }
}

pub(super) struct RunLifetime {
    active: AtomicBool,
    stop: Mutex<Option<HostProblem>>,
}
impl RunLifetime {
    pub(super) fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
    fn revoke(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
    fn stop(&self) -> Option<HostProblem> {
        match self.stop.lock() {
            Ok(stop) => stop.clone(),
            Err(_) => Some(HostProblem::UnknownOutcome),
        }
    }
    pub(super) fn poison(&self, problem: HostProblem) -> HostProblem {
        self.revoke();
        let Ok(mut stop) = self.stop.lock() else {
            return HostProblem::UnknownOutcome;
        };
        if stop.is_none() || problem == HostProblem::UnknownOutcome {
            *stop = Some(problem);
        }
        stop.as_ref().expect("latched stop").clone()
    }
}

struct RunOwner {
    lifetime: Arc<RunLifetime>,
    store: Arc<dyn ProviderStateStore>,
    original: Invocation,
    first_row: ProviderStateRecord,
    current_row: RefCell<ProviderStateRecord>,
}
impl Drop for RunOwner {
    fn drop(&mut self) {
        self.lifetime.revoke();
    }
}

// Shared normalization hides the raw sequence fault. Only the explicit
// contained path conservatively fences every malformed host/step reply.
pub(super) fn fence_host_reply(contained: bool, problem: &HostProblem) -> bool {
    matches!(
        problem,
        HostProblem::UnknownOutcome
            | HostProblem::Cancelled
            | HostProblem::TimedOut
            | HostProblem::InfrastructureFailure
    ) || (contained && matches!(problem, HostProblem::Malformed))
}

pub(super) fn fence_step_error(contained: bool, problem: &HostProblem) -> bool {
    contained
        && (fence_host_reply(contained, problem)
            || matches!(
                problem,
                HostProblem::IdempotencyConflict | HostProblem::ResourceExhausted
            ))
}

// Private explicit threading through the sole existing helper graph. A plain
// Invocation keeps every legacy timing/path unchanged; it has no run owner.
pub(super) trait RunInput {
    fn original(&self) -> &Invocation;
    fn all_effects(&self) -> bool {
        false
    }
    fn loan(&self, _occurrence: &BatchEffectOccurrence<'_>) -> Result<EffectResult, HostProblem> {
        Err(HostProblem::Unsupported)
    }
    fn check(&self) -> Result<Option<BatchRunControl>, HostProblem> {
        Ok(None)
    }
    fn poison(&self, problem: HostProblem) -> HostProblem {
        problem
    }
    fn contained(&self) -> bool {
        false
    }
    fn admit_run(
        &self,
        _service: &BatchService,
        _row: &ProviderStateRecord,
    ) -> Result<Option<Arc<RunLifetime>>, HostProblem> {
        Ok(None)
    }
    fn revoke_run(&self) {}
    fn published_job(&self, _row: ProviderStateRecord) {}
}
impl RunInput for Invocation {
    fn original(&self) -> &Invocation {
        self
    }
}

pub(super) struct Projected<'a, I: ?Sized> {
    pub(super) original: &'a Invocation,
    pub(super) scope: &'a I,
}
impl<I: RunInput + ?Sized> RunInput for Projected<'_, I> {
    fn original(&self) -> &Invocation {
        self.original
    }
    fn check(&self) -> Result<Option<BatchRunControl>, HostProblem> {
        self.scope.check()
    }
    fn poison(&self, problem: HostProblem) -> HostProblem {
        self.scope.poison(problem)
    }
    fn contained(&self) -> bool {
        self.scope.contained()
    }
    fn all_effects(&self) -> bool {
        self.scope.all_effects()
    }
    fn loan(&self, occurrence: &BatchEffectOccurrence<'_>) -> Result<EffectResult, HostProblem> {
        self.scope.loan(occurrence)
    }
}

pub(super) struct RunScope<'a, O> {
    original: &'a Invocation,
    observer: RefCell<&'a mut O>,
    last_tick: Cell<Option<u64>>,
    lifetime: Arc<RunLifetime>,
    owner: RefCell<Option<RunOwner>>,
}
impl<O> RunInput for RunScope<'_, O>
where
    O: FnMut() -> Result<BatchRunControl, HostProblem>,
{
    fn original(&self) -> &Invocation {
        self.original
    }
    fn contained(&self) -> bool {
        true
    }
    fn poison(&self, problem: HostProblem) -> HostProblem {
        self.lifetime.poison(problem)
    }
    fn check(&self) -> Result<Option<BatchRunControl>, HostProblem> {
        if let Some(problem) = self.lifetime.stop() {
            return Err(problem);
        }
        let observation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut observer = self
                .observer
                .try_borrow_mut()
                .map_err(|_| HostProblem::UnknownOutcome)?;
            observer()
        }))
        .unwrap_or(Err(HostProblem::UnknownOutcome));
        let observation = observation.map_err(|problem| self.poison(problem))?;
        if let Some(problem) = self.lifetime.stop() {
            return Err(problem);
        }
        if observation.now_tick == 0
            || self
                .last_tick
                .get()
                .is_some_and(|last| observation.now_tick < last)
        {
            return Err(self.poison(HostProblem::InfrastructureFailure));
        }
        self.last_tick.set(Some(observation.now_tick));
        if observation.cancellation_requested || self.original.cancellation_requested() {
            return Err(self.poison(HostProblem::Cancelled));
        }
        if observation.now_tick >= self.original.deadline_tick {
            return Err(self.poison(HostProblem::TimedOut));
        }
        let valid = {
            let owner = self.owner.borrow();
            if let Some(owner) = owner.as_ref() {
                let row = owner.current_row.borrow();
                owner
                    .store
                    .get_provider_state(&row.namespace, &row.key)
                    .map_err(store_error)
                    .map(|current| current.as_ref() == Some(&*row))
            } else {
                Ok(true)
            }
        };
        let valid = valid.map_err(|problem| self.poison(problem))?;
        if !valid {
            return Err(self.poison(HostProblem::IdempotencyConflict));
        }
        if let Some(problem) = self.lifetime.stop() {
            return Err(problem);
        }
        Ok(Some(observation))
    }
    fn admit_run(
        &self,
        service: &BatchService,
        row: &ProviderStateRecord,
    ) -> Result<Option<Arc<RunLifetime>>, HostProblem> {
        let mut owner = self.owner.borrow_mut();
        if let Some(owner) = owner.as_ref() {
            if owner.first_row.key != row.key
                || owner.original != *self.original
                || !Arc::ptr_eq(&owner.store, &service.store)
                || !owner.lifetime.active()
            {
                return Err(HostProblem::Unauthorized);
            }
            return Ok(Some(owner.lifetime.clone()));
        }
        let lifetime = self.lifetime.clone();
        lifetime.active.store(true, Ordering::SeqCst);
        *owner = Some(RunOwner {
            lifetime: lifetime.clone(),
            original: self.original.clone(),
            store: service.store.clone(),
            first_row: row.clone(),
            current_row: RefCell::new(row.clone()),
        });
        Ok(Some(lifetime))
    }
    fn revoke_run(&self) {
        if let Some(owner) = self.owner.borrow().as_ref() {
            owner.lifetime.revoke();
        }
    }
    fn published_job(&self, row: ProviderStateRecord) {
        if let Some(owner) = self.owner.borrow().as_ref() {
            *owner.current_row.borrow_mut() = row;
        }
    }
}

impl<O> RunScope<'_, O> {
    pub(super) fn exit(
        &self,
        outcome: Result<Option<JobSnapshot>, HostProblem>,
    ) -> Result<Option<BatchRunExit>, HostProblem> {
        let owner = self.owner.borrow_mut().take();
        let Some(owner) = owner else {
            return match outcome {
                Ok(None) => Ok(None),
                Ok(Some(_)) => Err(HostProblem::Unsupported),
                Err(problem) => Err(problem),
            };
        };
        owner.lifetime.revoke();
        let row = owner
            .store
            .get_provider_state(&owner.first_row.namespace, &owner.first_row.key)
            .map_err(store_error)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if row != *owner.current_row.borrow() {
            return Err(HostProblem::UnknownOutcome);
        }
        let job: Job =
            serde_json::from_slice(&row.payload).map_err(|_| HostProblem::UnknownOutcome)?;
        if job_record(&job)? != row
            || job.owner != owner.original.principal.id().as_str()
            || job.id != owner.first_row.key
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let stop = self
            .lifetime
            .stop()
            .or_else(|| outcome.as_ref().err().cloned());
        if stop.is_none()
            && !matches!(
                job.state,
                JobState::Completed | JobState::Failed | JobState::Cancelled
            )
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(Some(BatchRunExit {
            original: owner.original.clone(),
            store: owner.store.clone(),
            admitted_row: owner.first_row.clone(),
            snapshot: snapshot(&job),
            last_row: row,
            stop,
        }))
    }
}

impl<'a, O> RunScope<'a, O> {
    pub(super) fn new(original: &'a Invocation, observe: &'a mut O) -> Self {
        Self {
            original,
            observer: RefCell::new(observe),
            last_tick: Cell::new(None),
            lifetime: Arc::new(RunLifetime {
                active: AtomicBool::new(false),
                stop: Mutex::new(None),
            }),
            owner: RefCell::new(None),
        }
    }
}

impl BatchService {
    /// Run through genuine Batch admission with bounded synchronous observations
    /// and the original Program callback. Opaque exits describe this actual run;
    /// they grant no JES/core/SAF, physical lease or terminal settlement authority.
    /// Observer/provider calls are outside Batch locks; observers must be bounded
    /// and nonblocking. Panic/reentry/control failure stops further run actions.
    pub fn run_claimed_with_run_observer<D, O>(
        &self,
        invocation: &Invocation,
        id: &str,
        initiator: &str,
        dispatch: &mut D,
        observe: &mut O,
    ) -> Result<Option<BatchRunExit>, HostProblem>
    where
        D: for<'step> FnMut(
            &RunningStepAdmission<'step>,
            &Invocation,
            EffectRequest,
        ) -> EffectResult,
        O: FnMut() -> Result<BatchRunControl, HostProblem>,
    {
        let checkpoints = self
            .checkpoint_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        if Arc::as_ptr(&self.store).cast::<()>() != Arc::as_ptr(checkpoints).cast::<()>() {
            return Err(HostProblem::Unsupported);
        }
        let scope = RunScope::new(invocation, observe);
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
                self.run_on_member(&scope, &member, initiator, false, Some(id), Some(dispatch))
            }))
            .unwrap_or_else(|_| Err(scope.poison(HostProblem::UnknownOutcome)));
            if outcome != Ok(None) {
                return std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scope.exit(outcome)
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
    fn private_latch_never_erases_unknown_or_reactivates() {
        let lifetime = RunLifetime {
            active: AtomicBool::new(true),
            stop: Mutex::new(None),
        };
        assert_eq!(
            lifetime.poison(HostProblem::InfrastructureFailure),
            HostProblem::InfrastructureFailure
        );
        assert_eq!(
            lifetime.poison(HostProblem::UnknownOutcome),
            HostProblem::UnknownOutcome
        );
        assert_eq!(
            lifetime.poison(HostProblem::Cancelled),
            HostProblem::UnknownOutcome
        );
        assert!(!lifetime.active());
    }
}
