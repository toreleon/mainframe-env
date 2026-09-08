use mainframe_env_execution_api::{
    ExecutionId, IdempotencyKey, LifecycleEvent, LifecycleEventKind,
};
use mainframe_env_store_api::{
    ArtifactRecord, CheckpointRecord, EffectRecord, EffectState, ExecutionRecord, ExecutionState,
    OutboxRecord, StoreError,
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
    if record.sequence == 0 {
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
        || intent.state != EffectState::Intent
        || intent.result_digest.is_some()
    {
        Err(StoreError::Conflict)
    } else {
        Ok(())
    }
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
        LifecycleEventKind::EffectIntent { sequence } if record.state == EffectState::Intent => {
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

pub(crate) fn effect_execution(
    execution: &ExecutionRecord,
    record: &EffectRecord,
) -> Result<(), StoreError> {
    if record.execution_id != execution.execution_id || record.run_unit_id != execution.run_unit_id
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
