//! Atomic provider-retention dependency validation in the existing memory transaction owner.
use super::*;

pub(super) fn validate_memory_provider_dependency(
    state: &State,
    candidate: &ProviderRetentionRow,
) -> Result<(), StoreError> {
    match &candidate.dependency {
        ProviderRetentionDependency::CoreEffect {
            key,
            request_digest,
            result_digest,
        } => {
            let owner = candidate
                .owner_execution
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            let run = candidate
                .owner_run_unit
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            if !memory_execution_prunable(state, owner)
                || state.effects.values().any(|effect| {
                    effect.execution_id == *owner
                        && effect.run_unit_id == *run
                        && matches!(
                            effect.state,
                            EffectState::Intent | EffectState::UnknownOutcome
                        )
                })
            {
                return Err(StoreError::Conflict);
            }
            let effect = state.effects.get(key).ok_or(StoreError::Conflict)?;
            if effect.execution_id != *owner
                || effect.run_unit_id != *run
                || effect.state != EffectState::Completed
                || candidate.row.namespace == "cics-container-replay-v1"
                    && effect
                        .intent
                        .capability
                        .as_ref()
                        .map(|value| value.as_str())
                        != Some("host.cics.execute")
                || effect.digest_format
                    != mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                || effect.request_digest != *request_digest
                || effect.result_digest != Some(*result_digest)
                || effect
                    .resolved_tick
                    .is_none_or(|tick| tick == 0 || candidate.retention_tick < tick)
            {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::CicsNested {
            provenance,
            absent,
            required_executions,
        } => {
            let owner = candidate
                .owner_execution
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            let run = candidate
                .owner_run_unit
                .as_ref()
                .ok_or(StoreError::IncompatibleVersion)?;
            for required in required_executions {
                let execution = state.executions.get(required).ok_or(StoreError::Conflict)?;
                if execution.run_unit_id != *run
                    || !memory_execution_prunable(state, required)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *required
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
            if !memory_execution_prunable(state, owner)
                || state.effects.values().any(|effect| {
                    effect.execution_id == *owner
                        && effect.run_unit_id == *run
                        && matches!(
                            effect.state,
                            EffectState::Intent | EffectState::UnknownOutcome
                        )
                })
            {
                return Err(StoreError::Conflict);
            }
            if state
                .provider_state
                .get(&(provenance.namespace.clone(), provenance.key.clone()))
                != Some(provenance)
                || absent.iter().any(|identity| {
                    if identity.namespace == "durable-effect" {
                        state.effects.keys().any(|key| key.as_str() == identity.key)
                    } else {
                        state
                            .provider_state
                            .contains_key(&(identity.namespace.clone(), identity.key.clone()))
                    }
                })
            {
                return Err(StoreError::Conflict);
            }
            let mut terminal_origin = false;
            for effect in state.effects.values().filter(|effect| {
                effect.execution_id == *owner
                    && effect.run_unit_id == *run
                    && effect.key.as_str() == provenance.key
            }) {
                if matches!(
                    effect.state,
                    EffectState::Intent | EffectState::UnknownOutcome
                ) {
                    return Err(StoreError::Conflict);
                }
                terminal_origin |= effect.state == EffectState::Completed
                    && effect.digest_format
                        == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                    && effect.intent.capability.as_ref().map(|item| item.as_str())
                        == Some("host.cics.execute")
                    && effect
                        .resolved_tick
                        .is_some_and(|tick| tick != 0 && candidate.retention_tick >= tick);
            }
            if !terminal_origin
                || state
                    .provider_state
                    .contains_key(&("cics-uow-undo".into(), run.as_str().into()))
            {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::ProviderGraph {
            required_rows,
            required_executions,
        } => {
            for required in required_rows {
                if state
                    .provider_state
                    .get(&(required.namespace.clone(), required.key.clone()))
                    != Some(required)
                {
                    return Err(StoreError::Conflict);
                }
            }
            for required in required_executions {
                let execution = state.executions.get(required).ok_or(StoreError::Conflict)?;
                if !execution.state.terminal()
                    || state.checkpoints.contains_key(required)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *required
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
            if let Some(owner) = &candidate.owner_execution {
                let run = candidate
                    .owner_run_unit
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                let execution = state.executions.get(owner).ok_or(StoreError::Conflict)?;
                if !execution.state.terminal()
                    || execution.run_unit_id != *run
                    || state.checkpoints.contains_key(owner)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *owner
                            && effect.run_unit_id == *run
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
        }
        ProviderRetentionDependency::DirectProduct => {
            if let Some(owner) = &candidate.owner_execution
                && let Some(execution) = state.executions.get(owner)
            {
                let run = candidate
                    .owner_run_unit
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                if !execution.state.terminal()
                    || execution.run_unit_id != *run
                    || state.checkpoints.contains_key(owner)
                    || state.effects.values().any(|effect| {
                        effect.execution_id == *owner
                            && effect.run_unit_id == *run
                            && matches!(
                                effect.state,
                                EffectState::Intent | EffectState::UnknownOutcome
                            )
                    })
                {
                    return Err(StoreError::Conflict);
                }
            }
        }
        ProviderRetentionDependency::None => {
            if candidate.owner_execution.is_some() || candidate.owner_run_unit.is_some() {
                return Err(StoreError::IncompatibleVersion);
            }
        }
    }
    Ok(())
}
