use mainframe_env_execution_api::{
    AuditDecision, AuditRecord, ExecutionId, IdempotencyKey, LifecycleEvent, LifecycleEventKind,
};
use mainframe_env_store_api::{
    ArtifactRecord, CheckpointRecord, EffectRecord, EffectState, ExecutionRecord, ExecutionState,
    MAX_EFFECT_RECOVERY_OWNER_BYTES, OutboxRecord, StoreError,
};
use sha2::{Digest, Sha256};

pub(crate) fn new_execution(record: &ExecutionRecord) -> Result<(), StoreError> {
    if record.version != 1 || record.attempt == 0 || record.state != ExecutionState::Admitted {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(())
    }
}

pub(crate) fn event(event: &LifecycleEvent) -> Result<(), StoreError> {
    if event.validate() {
        Ok(())
    } else {
        Err(StoreError::InvalidSequence)
    }
}

pub(crate) fn new_outbox(record: &OutboxRecord) -> Result<(), StoreError> {
    if record.notification_id.is_empty()
        || record.topic.is_empty()
        || record.sequence == 0
        || record.attempt != 0
        || record.delivered
        || record.version != 1
    {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(())
    }
}

pub(crate) fn audit(record: &AuditRecord) -> Result<(), StoreError> {
    if record.attempt == 0 || record.effect_sequence == 0 {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(())
    }
}

pub(crate) fn checkpoint(record: &CheckpointRecord) -> Result<(), StoreError> {
    if record.schema_version != 1
        || record.machine_schema_version == 0
        || record.provider_generation.is_empty()
        || record.security_classification.is_empty()
        || record.payload.is_empty()
        || record.payload_size != record.payload.len() as u64
        || Sha256::digest(&record.payload).as_slice() != record.payload_digest
    {
        Err(StoreError::IncompatibleVersion)
    } else {
        Ok(())
    }
}

pub(crate) fn artifact(record: &ArtifactRecord) -> Result<(), StoreError> {
    let digest: [u8; 32] = Sha256::digest(&record.payload).into();
    if record.payload_digest != digest
        || record.artifact.as_str() != format!("sha256:{}", hex(&digest))
    {
        Err(StoreError::IncompatibleVersion)
    } else {
        Ok(())
    }
}

pub(crate) fn effect(record: &EffectRecord) -> Result<(), StoreError> {
    if record.sequence == 0
        || record.intent.owner != record.execution_id
        || record.intent.attempt == 0
        || record.intent.recovery_after_tick < record.intent.created_tick
        || record.intent.epoch == 0
        || record.intent.recovery_lease.as_ref().is_some_and(|lease| {
            lease.owner.is_empty()
                || lease.owner.len() > MAX_EFFECT_RECOVERY_OWNER_BYTES
                || lease.attempt == 0
                || lease.epoch == 0
                || lease.expires_tick <= record.intent.created_tick
        })
    {
        return Err(StoreError::InvalidTransition);
    }
    match record.state {
        EffectState::Intent if record.result_digest.is_none() => Ok(()),
        EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome
            if record.result_digest.is_some() =>
        {
            Ok(())
        }
        _ => Err(StoreError::InvalidTransition),
    }
}

pub(crate) fn intent(record: &EffectRecord) -> Result<(), StoreError> {
    effect(record)?;
    if record.state == EffectState::Intent {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition)
    }
}

pub(crate) fn new_intent(record: &EffectRecord) -> Result<(), StoreError> {
    intent(record)?;
    if record.intent.capability.is_some() {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition)
    }
}

pub(crate) fn terminal(key: &IdempotencyKey, record: &EffectRecord) -> Result<(), StoreError> {
    effect(record)?;
    if &record.key == key && record.state != EffectState::Intent {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition)
    }
}

pub(crate) fn result(
    key: &IdempotencyKey,
    intent: &EffectRecord,
    record: &EffectRecord,
) -> Result<(), StoreError> {
    terminal(key, record)?;
    if intent.key != *key
        || intent.execution_id != record.execution_id
        || intent.run_unit_id != record.run_unit_id
        || intent.sequence != record.sequence
        || intent.digest_format != record.digest_format
        || intent.request_digest != record.request_digest
        || intent.intent != record.intent
        || intent.intent.recovery_lease.is_some()
        || intent.state != EffectState::Intent
        || intent.result_digest.is_some()
    {
        Err(StoreError::Conflict)
    } else {
        Ok(())
    }
}

pub(crate) fn stale_intent(
    record: &EffectRecord,
    now_tick: u64,
    minimum_age_ticks: u64,
) -> Result<bool, StoreError> {
    effect(record)?;
    if minimum_age_ticks == 0 {
        return Err(StoreError::InvalidTransition);
    }
    if record.state != EffectState::Intent {
        return Ok(false);
    }
    let old_enough = record
        .intent
        .created_tick
        .checked_add(minimum_age_ticks)
        .is_some_and(|boundary| {
            boundary <= now_tick && record.intent.recovery_after_tick <= now_tick
        });
    let lease_available = record
        .intent
        .recovery_lease
        .as_ref()
        .is_none_or(|lease| lease.expires_tick <= now_tick);
    Ok(old_enough && lease_available)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stale_claim(
    record: &EffectRecord,
    expected_intent_epoch: u64,
    recovery_owner: &str,
    now_tick: u64,
    minimum_age_ticks: u64,
    lease_ticks: u64,
) -> Result<(), StoreError> {
    if recovery_owner.is_empty()
        || recovery_owner.len() > MAX_EFFECT_RECOVERY_OWNER_BYTES
        || expected_intent_epoch == 0
        || lease_ticks == 0
        || now_tick.checked_add(lease_ticks).is_none()
    {
        return Err(StoreError::LeaseConflict);
    }
    if record.intent.epoch != expected_intent_epoch {
        return Err(StoreError::Conflict);
    }
    if !stale_intent(record, now_tick, minimum_age_ticks)? {
        return Err(StoreError::LeaseConflict);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stale_reconciliation(
    key: &IdempotencyKey,
    record: &EffectRecord,
    recovery_owner: &str,
    recovery_epoch: u64,
    now_tick: u64,
    final_state: EffectState,
    format: mainframe_env_store_api::EffectDigestFormat,
) -> Result<(), StoreError> {
    intent(record)?;
    if &record.key != key
        || record.digest_format != format
        || !matches!(final_state, EffectState::Completed | EffectState::Failed)
    {
        return Err(StoreError::InvalidTransition);
    }
    let lease = record
        .intent
        .recovery_lease
        .as_ref()
        .ok_or(StoreError::LeaseConflict)?;
    if lease.owner != recovery_owner
        || lease.epoch != recovery_epoch
        || lease.expires_tick <= now_tick
    {
        return Err(StoreError::LeaseConflict);
    }
    Ok(())
}

pub(crate) fn admission(
    execution: &ExecutionRecord,
    event_record: &LifecycleEvent,
    notification: &OutboxRecord,
) -> Result<(), StoreError> {
    new_execution(execution)?;
    event(event_record)?;
    new_outbox(notification)?;
    if event_record.sequence != 1
        || !matches!(&event_record.kind, LifecycleEventKind::Admitted)
        || execution.execution_id != event_record.execution_id
        || execution.run_unit_id != event_record.run_unit_id
        || execution.attempt != event_record.attempt
        || event_record.execution_id != notification.execution_id
        || event_record.sequence != notification.sequence
    {
        Err(StoreError::InvalidSequence)
    } else {
        Ok(())
    }
}

pub(crate) fn execution_step(
    execution_id: &ExecutionId,
    execution: &ExecutionRecord,
    event_record: &LifecycleEvent,
    notification: &OutboxRecord,
) -> Result<(), StoreError> {
    event(event_record)?;
    new_outbox(notification)?;
    if &execution.execution_id != execution_id
        || &event_record.execution_id != execution_id
        || execution.run_unit_id != event_record.run_unit_id
        || execution.attempt != event_record.attempt
        || event_record.execution_id != notification.execution_id
        || event_record.sequence != notification.sequence
    {
        Err(StoreError::InvalidSequence)
    } else {
        Ok(())
    }
}

pub(crate) fn effect_event(
    event_record: &LifecycleEvent,
    record: &EffectRecord,
) -> Result<(), StoreError> {
    let sequence = match &event_record.kind {
        LifecycleEventKind::EffectIntent { sequence }
            if record.state == EffectState::Intent
                && record.intent.attempt == event_record.attempt
                && record.intent.created_tick == event_record.tick
                && record.intent.epoch == event_record.sequence =>
        {
            *sequence
        }
        LifecycleEventKind::EffectResult { sequence } if record.state != EffectState::Intent => {
            *sequence
        }
        _ => return Err(StoreError::InvalidSequence),
    };
    if sequence == record.sequence {
        Ok(())
    } else {
        Err(StoreError::InvalidSequence)
    }
}

pub(crate) fn audit_event(
    execution: &ExecutionRecord,
    event_record: &LifecycleEvent,
    effect: Option<&EffectRecord>,
    record: Option<&AuditRecord>,
) -> Result<(), StoreError> {
    let expected_effect_sequence = match &event_record.kind {
        LifecycleEventKind::EffectResult { sequence } => *sequence,
        _ if record.is_none() => return Ok(()),
        _ => return Err(StoreError::InvalidSequence),
    };
    let record = record.ok_or(StoreError::InvalidSequence)?;
    audit(record)?;
    if record.execution_id != execution.execution_id
        || record.run_unit_id != execution.run_unit_id
        || record.principal != execution.principal
        || record.attempt != execution.attempt
        || record.attempt != event_record.attempt
        || record.observed_tick != event_record.tick
        || record.effect_sequence != expected_effect_sequence
        || effect
            .and_then(|effect| effect.intent.capability.as_ref())
            .is_some_and(|capability| capability != &record.capability)
        || effect
            .and_then(|effect| effect.intent.audit_resource)
            .is_some_and(|resource| resource != record.resource)
        || effect
            .and_then(|effect| effect.intent.audit_invocation_key.as_ref())
            .is_some_and(|key| key != &record.invocation_key)
    {
        Err(StoreError::InvalidSequence)
    } else {
        Ok(())
    }
}

pub(crate) fn recovered_audit(
    execution: &ExecutionRecord,
    effect: &EffectRecord,
    observed_tick: u64,
) -> Result<Option<AuditRecord>, StoreError> {
    let Some(resource) = effect.intent.audit_resource else {
        // Pre-R-02 intents remain recoverable, but did not retain enough data to synthesize an
        // audit record safely.
        return Ok(None);
    };
    let capability = effect
        .intent
        .capability
        .clone()
        .ok_or(StoreError::IncompatibleVersion)?;
    let decision = match effect.state {
        EffectState::Completed => AuditDecision::Success,
        EffectState::Failed => AuditDecision::Rejected,
        EffectState::Intent | EffectState::UnknownOutcome => {
            return Err(StoreError::InvalidTransition);
        }
    };
    let record = AuditRecord {
        execution_id: effect.execution_id.clone(),
        run_unit_id: effect.run_unit_id.clone(),
        attempt: effect.intent.attempt,
        effect_sequence: effect.sequence,
        observed_tick,
        principal: execution.principal.clone(),
        invocation_key: effect
            .intent
            .audit_invocation_key
            .clone()
            .ok_or(StoreError::IncompatibleVersion)?,
        capability,
        resource,
        decision,
    };
    audit(&record)?;
    Ok(Some(record))
}

pub(crate) fn effect_execution(
    execution: &ExecutionRecord,
    record: &EffectRecord,
) -> Result<(), StoreError> {
    if record.execution_id != execution.execution_id
        || record.run_unit_id != execution.run_unit_id
        || record.intent.owner != execution.execution_id
        || record.intent.attempt != execution.attempt
    {
        Err(StoreError::Conflict)
    } else {
        Ok(())
    }
}

pub(crate) fn checkpoint_execution(
    execution: &ExecutionRecord,
    record: &CheckpointRecord,
) -> Result<(), StoreError> {
    if record.execution_id != execution.execution_id
        || record.run_unit_id != execution.run_unit_id
        || record.artifact != execution.artifact
        || record.principal != execution.principal
    {
        Err(StoreError::IncompatibleVersion)
    } else {
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
