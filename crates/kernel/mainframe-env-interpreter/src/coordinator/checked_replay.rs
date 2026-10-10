//! Actual original-dispatch capture. Public naming/downcast does not mint authority.
use super::*;
use mainframe_env_host_api::HostLimits;
use mainframe_env_host_api::mq_mqi::{MqMqiLocalTypeInquiry, MqMqiRequest};
use mainframe_env_store_api::{
    CheckedReplayRefusalStep, MAX_ROOT_PAYLOAD_BYTES, ProviderReplayAssertion,
    ProviderStateIdentity, TerminalRowDependency,
};
use std::sync::Mutex;

/// Structural current observations submitted to one synchronous replay attempt.
/// No audit, Running record, core replacement or permission is representable.
#[derive(Clone)]
pub struct CheckedReplayObservations {
    /// Exact prior receipt identity for success; absent for a missing/corrupt-receipt refusal.
    pub receipt: Option<ProviderStateIdentity>,
    /// Unique bounded Exact/Absent current rows, physically compared after callbacks.
    pub dependencies: Vec<TerminalRowDependency>,
}

struct Mailbox {
    closed: bool,
    observations: Option<CheckedReplayObservations>,
    audit: Option<AuditRecord>,
}

/// Non-Serde one-attempt capture privately minted at genuine OriginalDispatch.
/// A future receiver downcasts this exact type and independently verifies its
/// actual frame/provider/SAF. Submission requires the same PlatformStore Arc;
/// equal rows or DTOs do not substitute. Only the coordinator seals it and
/// attaches ScopedHost's actual audit. Retention after Unknown grants no retry,
/// audit-persisted flag, terminal-root permission or live handle restoration.
pub struct CheckedReplayAuditCapture {
    store: Arc<dyn PlatformStore>,
    effect: EffectRecord,
    execution: ExecutionRecord,
    invocation: Invocation,
    invocation_bytes: usize,
    tick: u64,
    mailbox: Mutex<Mailbox>,
}

impl CheckedReplayAuditCapture {
    /// Submit current structural observations once during this synchronous call.
    /// Bounds are checked before internal cloning; no callback or journal write
    /// runs here. Foreign stores, duplicate submissions and closed attempts refuse.
    pub fn submit(
        &self,
        store: &Arc<dyn PlatformStore>,
        observations: CheckedReplayObservations,
    ) -> Result<(), StoreError> {
        if !Arc::ptr_eq(store, &self.store) {
            return Err(StoreError::Conflict);
        }
        let bytes = CheckedReplayRefusalStep::validate_observation_bounds(
            &self.effect,
            &self.execution,
            &observations.dependencies,
            MAX_ROOT_PAYLOAD_BYTES,
        )?;
        // Bound the retained submission plus its transaction-owned copy together.
        if bytes
            .checked_mul(2)
            .and_then(|b| b.checked_add(self.invocation_bytes))
            .is_none_or(|b| b > MAX_ROOT_PAYLOAD_BYTES)
        {
            return Err(StoreError::CapacityExceeded);
        }
        if let Some(receipt) = &observations.receipt
            && !observations.dependencies.iter().any(|d| {
                matches!(d,TerminalRowDependency::Exact(r)
                if r.namespace==receipt.namespace && r.key==receipt.key)
            })
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut mailbox = self.mailbox.lock().map_err(|_| StoreError::Conflict)?;
        if mailbox.closed || mailbox.observations.is_some() {
            return Err(StoreError::Conflict);
        }
        mailbox.observations = Some(observations);
        Ok(())
    }

    /// Borrow original invocation identity for comparison, never admission.
    pub fn invocation(&self) -> &Invocation {
        &self.invocation
    }
    /// Borrow the exact original Completed record; it cannot be changed/adopted.
    pub fn effect(&self) -> &EffectRecord {
        &self.effect
    }
    /// Borrow the full observed current execution; callers cannot mint this capture.
    pub fn execution(&self) -> &ExecutionRecord {
        &self.execution
    }
    /// Logical observation frozen by the actual owning attempt, not UTC/output data.
    pub fn observed_tick(&self) -> u64 {
        self.tick
    }
    /// Inspect the actual scoped audit retained after failed settlement.
    /// Absence means no closed scoped reply yet, never that persistence succeeded.
    pub fn retained_audit(&self) -> Option<AuditRecord> {
        self.mailbox.lock().ok().and_then(|m| m.audit.clone())
    }

    fn new(
        store: Arc<dyn PlatformStore>,
        effect: EffectRecord,
        execution: ExecutionRecord,
        invocation: &Invocation,
        tick: u64,
    ) -> Result<Self, StoreError> {
        // Public Invocation fields may have been changed since construction.
        // Charge their complete owned representation before cloning it.
        let invocation_bytes = invocation_bound(invocation)?;
        CheckedReplayRefusalStep::validate_observation_bounds(
            &effect,
            &execution,
            &[],
            MAX_ROOT_PAYLOAD_BYTES,
        )?;
        if execution.state != ExecutionState::Running
            || execution.execution_id != invocation.execution_id
            || execution.run_unit_id != invocation.run_unit_id
            || execution.principal != *invocation.principal.id()
            || execution.attempt != invocation.attempt
            || execution.selector != invocation.selector
            || execution.artifact != invocation.artifact
            || effect.state != EffectState::Completed
            || effect.intent.capability.as_ref()
                != Some(
                    &mainframe_env_execution_api::CapabilityId::new(
                        "host.mq.write",
                        InvocationLimits::default(),
                    )
                    .map_err(|_| StoreError::InvalidTransition)?,
                )
            || effect.intent.audit_invocation_key.as_ref() != Some(&invocation.idempotency_key)
        {
            return Err(StoreError::Conflict);
        }
        Ok(Self {
            store,
            effect,
            execution,
            invocation: invocation.clone(),
            invocation_bytes,
            tick,
            mailbox: Mutex::new(Mailbox {
                closed: false,
                observations: None,
                audit: None,
            }),
        })
    }
    fn close(&self, audit: AuditRecord) -> Result<Option<CheckedReplayObservations>, StoreError> {
        let mut m = self.mailbox.lock().map_err(|_| StoreError::Conflict)?;
        if m.closed {
            return Err(StoreError::Conflict);
        }
        m.closed = true;
        m.audit = Some(audit);
        Ok(m.observations.clone())
    }
}

fn invocation_bound(i: &Invocation) -> Result<usize, StoreError> {
    let limits = InvocationLimits::default();
    if i.bindings.len() > limits.max_bindings
        || i.principal.grants().len() > limits.max_capabilities
        || i.provider_generations.len() > limits.max_capabilities
    {
        return Err(StoreError::CapacityExceeded);
    }
    // Include fixed fields and collection entry overhead, not only payloads.
    let mut bytes = 4096usize;
    let mut add = |size: usize, ceiling: usize| {
        if size > ceiling {
            return Err(StoreError::CapacityExceeded);
        }
        bytes = bytes
            .checked_add(size)
            .ok_or(StoreError::CapacityExceeded)?;
        if bytes > MAX_ROOT_PAYLOAD_BYTES / 4 {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(())
    };
    for text in [
        i.request_id.as_str(),
        i.execution_id.as_str(),
        i.run_unit_id.as_str(),
        i.selector.as_str(),
        i.artifact.as_str(),
        i.principal.id().as_str(),
        i.trace_id.as_str(),
        i.idempotency_key.as_str(),
    ] {
        add(text.len(), limits.max_identity_bytes)?;
    }
    if let Some(parent) = &i.parent_execution_id {
        add(parent.as_str().len(), limits.max_identity_bytes)?;
    }
    add(i.audit_correlation.len(), limits.max_binding_bytes)?;
    for grant in i.principal.grants() {
        add(grant.as_str().len(), limits.max_identity_bytes)?;
        add(128, 128)?;
    }
    for (capability, generation) in &i.provider_generations {
        add(capability.as_str().len(), limits.max_identity_bytes)?;
        add(generation.len(), limits.max_identity_bytes)?;
        add(128, 128)?;
    }
    for (name, payload) in &i.bindings {
        add(name.len(), limits.max_binding_bytes)?;
        add(payload.schema().len(), limits.max_identity_bytes)?;
        add(payload.bytes().len(), limits.max_payload_bytes)?;
        add(128, 128)?;
    }
    if let Some(cancellation) = &i.cancellation {
        add(cancellation.id.as_str().len(), limits.max_identity_bytes)?;
        add(cancellation.reason.len(), limits.max_binding_bytes)?;
    }
    Ok(bytes)
}

pub(super) fn eligible(effect: &EffectRequest) -> bool {
    let Ok(Some(occurrence)) = effect.mq_mqi_occurrence(HostLimits::default()) else {
        return false;
    };
    match &occurrence.envelope().request {
        MqMqiRequest::Inquire(q) => {
            MqMqiLocalTypeInquiry::from_inquiry(q.clone(), occurrence.envelope().limits).is_ok()
        }
        _ => false,
    }
}

impl ExecutionCoordinator {
    /// Select the private checked inquiry replay protocol only. This does not
    /// activate any configured provider or mint origin/SAF/root permission.
    /// Receivers default-refuse; installed activation additionally requires a
    /// real supervised late-failure consumer. All ordinary replay stays legacy.
    #[must_use]
    pub fn with_checked_inquiry_replay(mut self) -> Self {
        self.checked_inquiry_replay = true;
        self
    }

    /// Borrow the actual retained uncertain attempt from this owning driver.
    /// No public clearing/retry/adoption is provided: future supervision must
    /// reconcile physical cursor/ACK and current lifecycle before consuming it.
    pub fn pending_checked_replay(&self) -> Option<Arc<CheckedReplayAuditCapture>> {
        self.checked_replay_pending
            .lock()
            .ok()
            .and_then(|s| s.clone())
    }
}

pub(super) fn dispatch(
    coordinator: &ExecutionCoordinator,
    invocation: &Invocation,
    cursor: &mut JournalCursor<'_, '_>,
    effect: EffectRequest,
    completed: EffectRecord,
    control: ExecutionControl,
) -> Result<EffectResult, ExecutionOutcome> {
    let unknown = || {
        failed_outcome(problem(
            FailureCategory::UnknownOutcome,
            "checked replay requires owning reconciliation; no retry or known denial",
        ))
    };
    let execution = cursor
        .store
        .get_execution(&invocation.execution_id)
        .map_err(|_| unknown())?
        .ok_or_else(unknown)?;
    if completed.intent.capability.as_ref()
        != Some(
            &effect
                .request
                .required_capability(InvocationLimits::default()),
        )
        || completed.intent.audit_resource != Some(canonical_audit_resource_digest(&effect.request))
        || completed.intent.owner != invocation.execution_id
        || completed.intent.attempt != invocation.attempt
        || effect.idempotency_key.as_ref() != Some(&completed.key)
    {
        return Err(unknown());
    }
    if execution.version != cursor.version {
        return Err(unknown());
    }
    let capture = Arc::new(
        CheckedReplayAuditCapture::new(
            cursor.store.clone(),
            completed,
            execution,
            invocation,
            control.now_tick,
        )
        .map_err(|_| unknown())?,
    );
    {
        let mut slot = coordinator
            .checked_replay_pending
            .lock()
            .map_err(|_| unknown())?;
        if slot.is_some() {
            return Err(unknown());
        }
        *slot = Some(capture.clone());
    }
    let host = coordinator.host.as_ref().ok_or_else(unknown)?;
    let audited = host.replay_retained(
        invocation,
        control.now_tick,
        control.cancellation_requested,
        effect,
        capture.effect.result_digest.ok_or_else(unknown)?,
        capture.as_ref(),
    );
    let (result, audit) = audited.into_transaction_parts();
    let observations = capture
        .close(audit.clone())
        .map_err(|_| unknown())?
        .ok_or_else(unknown)?;
    if invocation.cancellation_requested() {
        return Err(unknown());
    }
    if result.outcome.is_ok() {
        let receipt = observations.receipt.ok_or_else(unknown)?;
        cursor
            .store
            .assert_provider_replay(ProviderReplayAssertion {
                effect: capture.effect.clone(),
                execution: capture.execution.clone(),
                observed_tick: capture.tick,
                receipt,
                dependencies: observations.dependencies,
            })
            .map_err(|_| unknown())?;
    } else {
        let sequence = cursor.sequence.checked_add(1).ok_or_else(unknown)?;
        let event = LifecycleEvent {
            execution_id: cursor.execution_id.clone(),
            run_unit_id: cursor.run_unit_id.clone(),
            attempt: cursor.attempt,
            sequence,
            tick: capture.tick,
            kind: LifecycleEventKind::EffectResult {
                sequence: capture.effect.sequence,
            },
        };
        let updated = cursor
            .store
            .commit_checked_replay_refusal(CheckedReplayRefusalStep {
                effect: capture.effect.clone(),
                execution: capture.execution.clone(),
                dependencies: observations.dependencies,
                audit,
                event: event.clone(),
                notification: notification(&event),
            })
            .map_err(|_| unknown())?;
        cursor.sequence = sequence;
        cursor.version = updated.version;
    }
    if matches!(result.outcome, Err(HostProblem::UnknownOutcome)) {
        return Err(unknown());
    }
    // No callback or repeated dispatch after the physical result. This slot is
    // cleared only after known assertion/settlement; failed/ambiguous ACK retains it.
    let mut slot = coordinator
        .checked_replay_pending
        .lock()
        .map_err(|_| unknown())?;
    if !slot.as_ref().is_some_and(|s| Arc::ptr_eq(s, &capture)) {
        return Err(unknown());
    }
    *slot = None;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn checked_replay_capture_bounds_public_invocation_fields_before_clone() {
        let l = InvocationLimits::default();
        let mut i = Invocation::new(
            RequestId::new("request", l).unwrap(),
            ExecutionId::new("execution", l).unwrap(),
            RunUnitId::new("run", l).unwrap(),
            None,
            Selector::new("fixture", l).unwrap(),
            ArtifactRef::new("artifact", l).unwrap(),
            Principal::new(PrincipalId::new("USER", l).unwrap(), BTreeSet::new(), l).unwrap(),
            ServiceClass::Batch,
            0,
            100,
            TraceId::new("trace", l).unwrap(),
            IdempotencyKey::new("invocation", l).unwrap(),
            1,
            ResourceLimits::default(),
            BTreeMap::new(),
            l,
        )
        .unwrap();
        assert!(invocation_bound(&i).is_ok());
        i.audit_correlation = "x".repeat(l.max_binding_bytes + 1);
        assert_eq!(invocation_bound(&i), Err(StoreError::CapacityExceeded));
        i.audit_correlation.clear();
        let payload = BoundedPayload::new("fixture@1", vec![0; l.max_payload_bytes], l).unwrap();
        for n in 0..17 {
            i.bindings.insert(format!("b{n}"), payload.clone());
        }
        assert_eq!(invocation_bound(&i), Err(StoreError::CapacityExceeded));
        i.bindings.clear();
        i.bindings
            .insert("x".repeat(l.max_binding_bytes + 1), payload);
        assert_eq!(invocation_bound(&i), Err(StoreError::CapacityExceeded));
        i.bindings.clear();
        for n in 0..=l.max_bindings {
            i.bindings.insert(
                format!("b{n}"),
                BoundedPayload::new("fixture@1", vec![], l).unwrap(),
            );
        }
        assert_eq!(invocation_bound(&i), Err(StoreError::CapacityExceeded));
    }
}
