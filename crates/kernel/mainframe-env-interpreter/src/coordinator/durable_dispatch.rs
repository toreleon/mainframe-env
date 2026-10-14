//! Actual first-dispatch Intent custody; not a protected native-root claim.
use super::*;
use mainframe_env_host_api::AuditedEffectResult;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

/// A borrowed actual durable Intent during the original single dispatch.
///
/// No public constructor, Clone, Debug, serialization or cross-thread loan.
/// This proves current journal identity only. It does not attest compilation,
/// an exclusive RootDriverClaim, a CICS Run, a provider frame or MQ disposition.
pub struct DurableEffectDispatch<'a> {
    original: &'a Invocation,
    store: &'a Arc<dyn PlatformStore>,
    effect: &'a EffectRequest,
    intent: &'a EffectRecord,
    execution: ExecutionRecord,
    control: ExecutionControl,
    _thread_confined: PhantomData<Rc<()>>,
}

impl DurableEffectDispatch<'_> {
    pub fn original(&self) -> &Invocation {
        self.original
    }
    pub fn effect(&self) -> &EffectRequest {
        self.effect
    }
    pub fn intent(&self) -> &EffectRecord {
        self.intent
    }
    pub fn control(&self) -> ExecutionControl {
        self.control
    }
    pub fn store(&self) -> &Arc<dyn PlatformStore> {
        self.store
    }

    /// Require the exact actual actor, unchanged current execution and Intent.
    /// This is not a lock/lease across independent store writers.
    pub fn require_current(&self, original: &Invocation) -> Result<(), HostProblem> {
        if original != self.original
            || original.cancellation_requested()
            || self.control.cancellation_requested
            || self.control.now_tick >= original.deadline_tick.min(self.effect.deadline_tick)
            || self.intent.state != EffectState::Intent
            || self.intent.result_digest.is_some()
            || self.intent.resolved_tick.is_some()
            || self.intent.intent.recovery_lease.is_some()
            || self.intent.execution_id != original.execution_id
            || self.intent.run_unit_id != original.run_unit_id
            || self.intent.sequence != self.effect.sequence
            || self.effect.run_unit != original.run_unit_id
            || self.effect.idempotency_key.as_ref() != Some(&self.intent.key)
            || self.intent.digest_format != EffectDigestFormat::CanonicalHostV1
            || self.intent.request_digest != canonical_request_digest(&self.effect.request)?
            || self.intent.intent.owner != original.execution_id
            || self.intent.intent.attempt != original.attempt
            || self.intent.intent.audit_invocation_key.as_ref() != Some(&original.idempotency_key)
            || self.intent.intent.capability.as_ref()
                != Some(
                    &self
                        .effect
                        .request
                        .required_capability(InvocationLimits::default()),
                )
            || self.intent.intent.audit_resource
                != Some(canonical_audit_resource_digest(&self.effect.request))
            || self
                .store
                .get_execution(&original.execution_id)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .as_ref()
                != Some(&self.execution)
            || self
                .store
                .effect(&self.intent.key)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .as_ref()
                != Some(self.intent)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

/// Wrap exactly one original scoped dispatch while another genuine loan is held.
///
/// The callback cannot return a fabricated host result. The coordinator keeps
/// the real audited result itself. Default Unsupported performs no dispatch;
/// zero, repeated, failed or panicked wrapping retains uncertainty. A CICS
/// embedding must acquire its actual task loan inside this callback, never
/// from copied IDs or for the whole machine drive.
pub trait DurableEffectDispatchHook {
    fn with_dispatch(
        &mut self,
        _actual: &DurableEffectDispatch<'_>,
        _dispatch: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }

    /// Wrap one completed plain READ replay transport under its actual original owner.
    /// Default refusal never dispatches, returns a substitute result or restores a cursor.
    fn with_completed_plain_read(
        &mut self,
        _actual: &DurableCompletedPlainRead<'_>,
        _dispatch: &mut dyn FnMut(&(dyn std::any::Any + Send + Sync)) -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }
}

pub(super) fn invoke(
    hook: &mut dyn DurableEffectDispatchHook,
    host: &Arc<ScopedHostService>,
    original: &Invocation,
    store: &Arc<dyn PlatformStore>,
    intent: &EffectRecord,
    control: ExecutionControl,
    effect: EffectRequest,
) -> Result<(AuditedEffectResult, bool), ExecutionOutcome> {
    let refuse = || {
        failed_outcome(problem(
            FailureCategory::UnknownOutcome,
            "borrowed dispatch owner or occurrence refused; retain original Intent",
        ))
    };
    let execution = store
        .get_execution(&original.execution_id)
        .map_err(|_| refuse())?
        .ok_or_else(refuse)?;
    if execution.state != ExecutionState::Running
        || execution.execution_id != original.execution_id
        || execution.run_unit_id != original.run_unit_id
        || execution.selector != original.selector
        || execution.artifact != original.artifact
        || execution.principal != *original.principal.id()
        || execution.attempt != original.attempt
    {
        return Err(refuse());
    }
    let actual = DurableEffectDispatch {
        original,
        store,
        effect: &effect,
        intent,
        execution,
        control,
        _thread_confined: PhantomData,
    };
    actual.require_current(original).map_err(|_| refuse())?;
    let mut attempts = 0;
    let mut audited = None;
    let result = {
        let mut dispatch = || {
            if attempts != 0 {
                attempts = 2;
                return Err(HostProblem::UnknownOutcome);
            }
            attempts = 1;
            actual.require_current(original)?;
            audited = Some(host.invoke(
                original,
                control.now_tick,
                control.cancellation_requested,
                effect.clone(),
            ));
            Ok(())
        };
        catch_unwind(AssertUnwindSafe(|| {
            hook.with_dispatch(&actual, &mut dispatch)
        }))
    };
    let audited = audited.ok_or_else(refuse)?;
    let uncertain =
        attempts != 1 || !matches!(result, Ok(Ok(()))) || actual.require_current(original).is_err();
    Ok((audited, uncertain))
}
