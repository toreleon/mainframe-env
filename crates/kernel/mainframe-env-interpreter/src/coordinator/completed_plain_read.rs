//! Original Completed plain READ custody; no first dispatch or browse restoration.
use super::*;
use mainframe_env_host_api::AuditedEffectResult;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// Borrowed only from the actual coordinator's original Completed branch.
/// It is not a compiler attestation, permission, result factory or CICS task loan.
pub struct DurableCompletedPlainRead<'a> {
    original: &'a Invocation,
    store: &'a Arc<dyn PlatformStore>,
    effect: &'a EffectRequest,
    completed: &'a EffectRecord,
    execution: ExecutionRecord,
    control: ExecutionControl,
    _thread_confined: PhantomData<Rc<()>>,
}

impl DurableCompletedPlainRead<'_> {
    pub fn original(&self) -> &Invocation {
        self.original
    }
    pub fn store(&self) -> &Arc<dyn PlatformStore> {
        self.store
    }
    pub fn effect(&self) -> &EffectRequest {
        self.effect
    }
    pub fn completed(&self) -> &EffectRecord {
        self.completed
    }
    pub fn execution(&self) -> &ExecutionRecord {
        &self.execution
    }
    pub fn control(&self) -> ExecutionControl {
        self.control
    }

    /// Recheck the original occurrence and current Running owner, without writes.
    /// The provider must additionally hold its real task lease and atomic row fence.
    pub fn require_current(&self) -> Result<(), HostProblem> {
        let original = self.original;
        let completed = self.completed;
        if !matches!(&self.effect.request,
            mainframe_env_host_api::HostRequest::Cics(request)
                if request.is_completed_plain_read_replay())
            || original.cancellation_requested()
            || self.control.cancellation_requested
            || self.control.now_tick == 0
            || self.control.now_tick >= original.deadline_tick.min(self.effect.deadline_tick)
            || completed.state != EffectState::Completed
            || completed.result_digest.is_none()
            || completed
                .resolved_tick
                .is_none_or(|tick| tick == 0 || tick > self.control.now_tick)
            || completed.intent.recovery_lease.is_some()
            || completed.execution_id != original.execution_id
            || completed.run_unit_id != original.run_unit_id
            || completed.sequence != self.effect.sequence
            || self.effect.run_unit != original.run_unit_id
            || self.effect.idempotency_key.as_ref() != Some(&completed.key)
            || completed.digest_format != EffectDigestFormat::CanonicalHostV1
            || completed.request_digest != canonical_request_digest(&self.effect.request)?
            || completed.intent.owner != original.execution_id
            || completed.intent.attempt != original.attempt
            || completed.intent.audit_invocation_key.as_ref() != Some(&original.idempotency_key)
            || completed.intent.capability.as_ref()
                != Some(
                    &self
                        .effect
                        .request
                        .required_capability(InvocationLimits::default()),
                )
            || completed.intent.audit_resource
                != Some(canonical_audit_resource_digest(&self.effect.request))
            || self.execution.state != ExecutionState::Running
            || self.execution.terminal_tick.is_some()
            || self.execution.execution_id != original.execution_id
            || self.execution.run_unit_id != original.run_unit_id
            || self.execution.selector != original.selector
            || self.execution.artifact != original.artifact
            || self.execution.principal != *original.principal.id()
            || self.execution.attempt != original.attempt
            || self
                .store
                .get_execution(&original.execution_id)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .as_ref()
                != Some(&self.execution)
            || self
                .store
                .effect(&completed.key)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .as_ref()
                != Some(completed)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

/// Retain the real scoped audited result; a hook can only wrap one replay call.
pub(super) fn invoke(
    hook: &mut dyn DurableEffectDispatchHook,
    host: &Arc<ScopedHostService>,
    original: &Invocation,
    store: &Arc<dyn PlatformStore>,
    completed: &EffectRecord,
    control: ExecutionControl,
    effect: EffectRequest,
) -> Result<AuditedEffectResult, ExecutionOutcome> {
    let refuse = || {
        failed_outcome(problem(
            FailureCategory::UnknownOutcome,
            "completed plain READ replay owner or retained result refused",
        ))
    };
    let execution = store
        .get_execution(&original.execution_id)
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    let actual = DurableCompletedPlainRead {
        original,
        store,
        effect: &effect,
        completed,
        execution,
        control,
        _thread_confined: PhantomData,
    };
    actual.require_current().map_err(|_| refuse())?;
    let digest = completed.result_digest.ok_or_else(refuse)?;
    let mut attempts = 0;
    let mut audited = None;
    let wrapped = {
        let mut dispatch = |context: &(dyn std::any::Any + Send + Sync)| {
            if attempts != 0 {
                attempts = 2;
                return Err(HostProblem::UnknownOutcome);
            }
            attempts = 1;
            actual.require_current()?;
            audited = Some(host.replay_completed_plain_read(
                original,
                control.now_tick,
                control.cancellation_requested,
                effect.clone(),
                digest,
                context,
            ));
            Ok(())
        };
        catch_unwind(AssertUnwindSafe(|| {
            hook.with_completed_plain_read(&actual, &mut dispatch)
        }))
    };
    if attempts != 1 || !matches!(wrapped, Ok(Ok(()))) || actual.require_current().is_err() {
        return Err(refuse());
    }
    audited.ok_or_else(refuse)
}
