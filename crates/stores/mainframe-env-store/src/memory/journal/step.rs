//! Unchanged journal kernel, reusable only while the sole Memory lock is held.
use super::*;
#[allow(clippy::too_many_arguments)]
pub(in crate::memory) fn commit_step_locked(
    state: &mut State,
    limits: StoreLimits,
    execution_id: &ExecutionId,
    expected_version: u64,
    next_state: Option<ExecutionState>,
    event: LifecycleEvent,
    effect: Option<EffectRecord>,
    audit: Option<AuditRecord>,
    checkpoint: Option<CheckpointRecord>,
    notification: OutboxRecord,
) -> Result<ExecutionRecord, StoreError> {
    super::root_terminal::guard_actor(state, execution_id, next_state)?;
    super::root_terminal::guard_event(state, &event)?;
    if checkpoint.is_some() {
        super::root_terminal::guard_work(state, execution_id)?;
    }
    journaled(state, |state, journal| {
        let current = state
            .executions
            .get(execution_id)
            .cloned()
            .ok_or(StoreError::NotFound)?;
        validation::execution_step(execution_id, &current, &event, &notification)?;
        if current.version != expected_version {
            return Err(StoreError::Conflict);
        }
        if next_state.is_some_and(|next| !current.state.can_transition_to(next)) {
            return Err(StoreError::InvalidTransition);
        }
        validation::audit_event(&current, &event, effect.as_ref(), audit.as_ref())?;
        let mut updated = current.clone();
        if let Some(next) = next_state {
            updated.state = next;
            updated.terminal_tick = next
                .terminal()
                .then_some(event.tick)
                .filter(|tick| *tick != 0);
        }
        updated.version = updated.version.checked_add(1).ok_or(StoreError::Conflict)?;
        MemoryStore::validate_encoded_size(encode_execution(&updated)?, limits)?;
        journal.touch_execution(state, execution_id);
        state
            .executions
            .insert(execution_id.clone(), updated.clone());
        if let Some(mut effect) = effect {
            if matches!(effect.state, EffectState::Completed | EffectState::Failed)
                && effect.digest_format
                    == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                && effect.resolved_tick.is_none()
                && event.tick != 0
            {
                effect.resolved_tick = Some(event.tick);
            }
            validation::effect(&effect)?;
            MemoryStore::validate_encoded_size(encode_effect(&effect)?, limits)?;
            validation::effect_execution(&current, &effect)?;
            match effect.state {
                EffectState::Intent => {
                    validation::new_intent(&effect)?;
                    validation::effect_event(&event, &effect)?;
                    if state.effects.contains_key(&effect.key) {
                        return Err(StoreError::Conflict);
                    }
                    if state.effects.len() >= limits.max_effects {
                        return Err(StoreError::CapacityExceeded);
                    }
                    journal.touch_effect(state, &effect.key);
                    state.effects.insert(effect.key.clone(), effect);
                }
                EffectState::Completed | EffectState::Failed | EffectState::UnknownOutcome => {
                    let intent = state.effects.get(&effect.key).ok_or(StoreError::NotFound)?;
                    validation::result(&effect.key, intent, &effect)?;
                    validation::effect_event(&event, &effect)?;
                    journal.touch_effect(state, &effect.key);
                    state.effects.insert(effect.key.clone(), effect);
                }
            }
        }
        if let Some(audit) = audit {
            let audit_execution_id = audit.execution_id.clone();
            journal.touch_next_audit_ordinal(state);
            journal.touch_provider_epoch(state);
            if let Some(ordinal) = state.next_audit_ordinal.checked_add(1) {
                journal.record_new_audit_key(crate::durable::audit_storage_key(
                    &audit_execution_id,
                    &format!("memory:{ordinal:020}"),
                ));
            }
            MemoryStore::append_audit_locked(state, audit, limits)?;
        }
        if let Some(checkpoint) = checkpoint {
            validation::checkpoint(&checkpoint)?;
            MemoryStore::validate_encoded_size(encode_checkpoint(&checkpoint)?, limits)?;
            validation::checkpoint_execution(&current, &checkpoint)?;
            let old = state
                .checkpoints
                .get(execution_id)
                .map_or(0, |record| record.payload.len());
            if old == 0 && state.checkpoints.len() >= limits.max_checkpoints {
                return Err(StoreError::CapacityExceeded);
            }
            journal.touch_blob_bytes(state);
            MemoryStore::reserve_blob(state, old, checkpoint.payload.len(), limits)?;
            journal.touch_checkpoint(state, execution_id);
            state.checkpoints.insert(execution_id.clone(), checkpoint);
        }
        journal.touch_events(state, &event.execution_id);
        journal.touch_provider_epoch(state);
        MemoryStore::append_event_locked(state, event, limits)?;
        journal.touch_outbox(state, &notification.notification_id);
        journal.touch_blob_bytes(state);
        journal.touch_provider_epoch(state);
        MemoryStore::append_outbox_locked(state, notification, limits)?;
        journal.touch_provider_epoch(state);
        MemoryStore::bump_retention_epoch(state)?;
        Ok(updated)
    })
}
