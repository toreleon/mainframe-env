//! Private original-effect/core-intent linkage, not execution or SAF authority.
//!
//! Only ordinary CoreEffect ServiceValidation is supported. Nested CICS actor/
//! root composition remains pending; an outer key cannot replace this intent.
//! The one borrowed PlatformStore supplies observation and atomic publication.
//! Caller owns validated MQ plans, real SAF/output decisions, replay/UOW CAS,
//! adoption after commit and shared UnknownOutcome/reconciliation handling.
//! No core result/outbox completion, retry, journal or durable UOW is supplied.
//!
//! Pinned MQ 9.4 baseline ibm-mq-9.4-mqi-2026-08-31 rows
//! 0001/0007/0015/0020/0021: source review only, no execution credit.

use crate::mqi_admission::{MqMqiAdmission, MqMqiAdmitted};
use crate::retention::{MqReplayOwnerKind, origin_for};
use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, AuditResourceDigest, InvocationLimits,
};
use mainframe_env_host_api::{
    HostProblem, HostRequest, MAX_CANONICAL_EFFECT_BYTES, canonical_audit_resource_digest,
    canonical_request_digest, canonical_request_size,
};
use mainframe_env_store_api::{
    AuditedProviderPublication, EffectDigestFormat, EffectRecord, EffectState, ExecutionState,
    MAX_AUDITED_PROVIDER_MUTATIONS, PlatformStore, ProviderStateMutation, StoreError,
};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum MqIntentProblem {
    NotServiceValidation,
    NestedCompositionPending,
    Host(HostProblem),
    Store(StoreError),
}
impl From<HostProblem> for MqIntentProblem {
    fn from(value: HostProblem) -> Self {
        Self::Host(value)
    }
}
impl From<StoreError> for MqIntentProblem {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// No public fields or constructor taking an EffectRecord or a second store.
pub(crate) struct MqCoreIntentBinding<'a> {
    admitted: &'a MqMqiAdmitted<'a>,
    store: &'a dyn PlatformStore,
    observed: EffectRecord,
    resource: AuditResourceDigest,
    last_tick: u64,
}

/// Borrow prevents another preparation/time observation until consumed/dropped.
/// This is a physical-publication plan, never a host operation result.
pub(crate) struct MqPreparedPublication<'b, 'a> {
    binding: &'b mut MqCoreIntentBinding<'a>,
    request: AuditedProviderPublication,
}

pub(crate) fn bind_core_intent<'a>(
    admission: &'a MqMqiAdmission<'a>,
    store: &'a dyn PlatformStore,
    now: u64,
) -> Result<MqCoreIntentBinding<'a>, MqIntentProblem> {
    let MqMqiAdmission::ServiceValidation(admitted) = admission else {
        return Err(MqIntentProblem::NotServiceValidation);
    };
    admitted.recheck_controls(now)?;
    let invocation = admitted.invocation();
    let effect = admitted.effect();
    let key = effect
        .idempotency_key
        .as_ref()
        .ok_or(HostProblem::Malformed)?;
    let (origin, outer) = origin_for(invocation, key.as_str(), effect.sequence)?;
    if origin != MqReplayOwnerKind::CoreEffect || outer.is_some() {
        return Err(MqIntentProblem::NestedCompositionPending);
    }
    let HostRequest::MqMqi(original) = &effect.request else {
        return Err(HostProblem::Malformed.into());
    };
    let request_digest = canonical_request_digest(&effect.request)?;
    let request_bytes = canonical_request_size(&effect.request, MAX_CANONICAL_EFFECT_BYTES)?;
    let capability = effect
        .request
        .required_capability(InvocationLimits::default());
    // Public-within-crate admission summaries are not a replacement source.
    // Recompute expectations from its privately borrowed immutable original.
    if admitted.host_request_digest != request_digest
        || admitted.host_request_bytes != request_bytes
        || admitted.capability != capability
        || admitted.origin != origin
        || admitted.outer_effect_key != outer
        || admitted.envelope != &original.envelope
        || admitted.mutation != &original.mutation
        || admitted.owner != original.envelope.context.owner
    {
        return Err(StoreError::Conflict.into());
    }
    let resource = canonical_audit_resource_digest(&effect.request);
    let observed = store.effect(key)?.ok_or(StoreError::NotFound)?;
    let metadata = &observed.intent;
    // The coordinator owns the epoch (journal sequence or effect sequence).
    // Do not infer an epoch from request payloads or reconstruct a retained record.
    if now > i64::MAX as u64
        || observed.key != *key
        || observed.execution_id != invocation.execution_id
        || observed.run_unit_id != invocation.run_unit_id
        || observed.sequence != effect.sequence
        || observed.sequence != admitted.mutation.sequence
        || observed.key != admitted.mutation.idempotency_key
        || observed.digest_format != EffectDigestFormat::CanonicalHostV1
        || observed.request_digest != request_digest
        || observed.state != EffectState::Intent
        || observed.result_digest.is_some()
        || observed.resolved_tick.is_some()
        || metadata.owner != invocation.execution_id
        || metadata.attempt == 0
        || metadata.attempt != invocation.attempt
        || metadata.capability.as_ref() != Some(&capability)
        || metadata.audit_resource != Some(resource)
        || metadata.audit_invocation_key.as_ref() != Some(&invocation.idempotency_key)
        || metadata.created_tick == 0
        || metadata.created_tick > now
        || metadata.recovery_after_tick != invocation.deadline_tick.min(effect.deadline_tick)
        || metadata.recovery_after_tick > i64::MAX as u64
        || now >= metadata.recovery_after_tick
        || metadata.epoch == 0
        || metadata.recovery_lease.is_some()
    {
        return Err(StoreError::Conflict.into());
    }
    // Early identity rejection only. The backend remains the final live fence
    // under its physical mutex/transaction; this read is not a lease permit.
    let execution = store
        .get_execution(&invocation.execution_id)?
        .ok_or(StoreError::NotFound)?;
    if execution.execution_id != invocation.execution_id
        || execution.run_unit_id != invocation.run_unit_id
        || execution.principal != *admitted.principal()
        || execution.attempt != invocation.attempt
        || execution.selector != invocation.selector
        || execution.artifact != invocation.artifact
        || execution.version == 0
        || execution.state != ExecutionState::Running
        || execution.terminal_tick.is_some()
        || execution.lease_expiry_tick.is_some_and(|tick| tick <= now)
    {
        return Err(StoreError::Conflict.into());
    }
    admitted.recheck_controls(now)?;
    Ok(MqCoreIntentBinding {
        admitted,
        store,
        observed,
        resource,
        last_tick: now,
    })
}

impl<'a> MqCoreIntentBinding<'a> {
    fn recheck(&mut self, now: u64) -> Result<(), MqIntentProblem> {
        if now < self.last_tick || now > i64::MAX as u64 {
            return Err(HostProblem::Malformed.into());
        }
        // A valid observation remains a time floor even if cancellation or
        // expiry rejects this boundary. An expired attempt cannot rewind time.
        self.last_tick = now;
        self.admitted.recheck_controls(now)?;
        Ok(())
    }

    /// Caller supplies an already validated MQ plan and actual decision. Shape
    /// checks here cannot attest queue/UOW semantics or an authorization decision.
    pub(crate) fn prepare(
        &mut self,
        audit: AuditRecord,
        mutations: Vec<ProviderStateMutation>,
        now: u64,
    ) -> Result<MqPreparedPublication<'_, 'a>, MqIntentProblem> {
        // Explicit post-dispatch uncertainty outranks malformed metadata or
        // expired controls. No publication/known mutation permit is produced.
        let controls = self.recheck(now);
        if audit.decision == AuditDecision::UnknownOutcome {
            return Err(HostProblem::UnknownOutcome.into());
        }
        controls?;
        let invocation = self.admitted.invocation();
        if audit.execution_id != invocation.execution_id
            || audit.run_unit_id != invocation.run_unit_id
            || audit.attempt != invocation.attempt
            || audit.effect_sequence != self.admitted.effect().sequence
            || audit.principal != *self.admitted.principal()
            || audit.invocation_key != invocation.idempotency_key
            || audit.capability != self.admitted.capability
            || audit.resource != self.resource
            || audit.observed_tick != now
        {
            return Err(StoreError::Conflict.into());
        }
        if audit.decision != AuditDecision::Success && !mutations.is_empty() {
            return Err(StoreError::InvalidTransition.into());
        }
        bounded_mq_mutations(&mutations)?;
        let request = AuditedProviderPublication {
            intent: self.observed.clone(),
            audit,
            observed_tick: now,
            mutations,
        };
        request.validate_mutations(MAX_CANONICAL_EFFECT_BYTES)?;
        Ok(MqPreparedPublication {
            binding: self,
            request,
        })
    }
}

impl MqPreparedPublication<'_, '_> {
    /// Commit only; not MQ completion, at-most-once, SAF or next-state adoption.
    /// The decision tick must still equal the supplied final observation. If
    /// time advanced, drop/reprepare with an actual audit at the new tick.
    pub(crate) fn publish(self, now: u64) -> Result<(), MqIntentProblem> {
        self.binding.recheck(now)?;
        if now != self.request.observed_tick {
            return Err(StoreError::Conflict.into());
        }
        self.binding
            .store
            .publish_provider_states_audited(self.request)?;
        Ok(())
    }
}

fn bounded_mq_mutations(mutations: &[ProviderStateMutation]) -> Result<(), StoreError> {
    if mutations.len() > MAX_AUDITED_PROVIDER_MUTATIONS {
        return Err(StoreError::CapacityExceeded);
    }
    let mut bytes = 0usize;
    for mutation in mutations {
        let (namespace, key, payload, old_key) = match mutation {
            ProviderStateMutation::Put(write) => (
                write.record.namespace.as_str(),
                write.record.key.as_str(),
                write.record.payload.len(),
                "",
            ),
            ProviderStateMutation::Delete { namespace, key, .. } => {
                (namespace.as_str(), key.as_str(), 0, "")
            }
            ProviderStateMutation::Move {
                record, old_key, ..
            } => (
                record.namespace.as_str(),
                record.key.as_str(),
                record.payload.len(),
                old_key.as_str(),
            ),
        };
        // Existing provider namespace ownership, not a new row/schema registry.
        if !namespace.starts_with("mq-") || namespace.len() == 3 {
            return Err(StoreError::InvalidTransition);
        }
        for size in [namespace.len(), key.len(), old_key.len(), payload] {
            bytes = bytes
                .checked_add(size)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        if bytes > MAX_CANONICAL_EFFECT_BYTES {
            return Err(StoreError::CapacityExceeded);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
