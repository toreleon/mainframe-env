use super::*;

pub(super) fn candidates(
    store: &dyn DurableRetentionBackend,
    target: RetentionTarget,
    watermark: u64,
    window_open: bool,
    capacity: usize,
    dependencies: Option<&CoreRetentionDependencySnapshot>,
) -> Result<CandidateSet, StoreError> {
    let core_dependencies = core_dependencies(store, capacity, dependencies)?;
    if target == RetentionTarget::LifecycleEvents {
        return event_candidates(store, watermark, window_open, capacity, &core_dependencies);
    }
    if target == RetentionTarget::TerminalExecutions {
        return terminal_execution_candidates(
            store,
            watermark,
            window_open,
            capacity,
            &core_dependencies,
        );
    }
    if target == RetentionTarget::TerminalWork {
        return terminal_work_candidates(
            store,
            watermark,
            window_open,
            capacity,
            &core_dependencies,
        );
    }
    if target == RetentionTarget::Audit {
        return audit_candidates(store, watermark, window_open, capacity, &core_dependencies);
    }
    let namespace = target_namespace(target).ok_or(StoreError::InvalidTransition)?;
    let rows = store.list_provider_state(namespace, capacity)?;
    let observations = observations(store, target)?;
    let active = rows.len();
    let mut eligible = Vec::new();
    for row in rows {
        let age = match target {
            RetentionTarget::TerminalExecutions | RetentionTarget::TerminalWork => None,
            RetentionTarget::DeliveredOutbox => {
                let outbox = decode_outbox(&row.payload, row.version)?;
                if !outbox.delivered || !execution_is_prunable(store, outbox.execution_id.as_str())?
                {
                    continue;
                }
                outbox
                    .delivered_tick
                    .filter(|tick| *tick != 0)
                    .map(|tick| (tick, Some(outbox.execution_id.clone())))
                    .or_else(|| {
                        observed_age(&observations, &row).map(|observation| {
                            (
                                observation.observed_tick,
                                observation.owner_execution.clone(),
                            )
                        })
                    })
                    .filter(|(tick, _)| window_open && *tick <= watermark)
            }
            RetentionTarget::ResolvedEffects => {
                let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                let effect = decode_effect(&key, &row.payload)?;
                if !matches!(effect.state, EffectState::Completed | EffectState::Failed)
                    || !execution_is_prunable(store, effect.execution_id.as_str())?
                    || core_dependencies.effect_keys.contains(&row.key)
                    || core_dependencies.unowned
                {
                    continue;
                }
                effect
                    .resolved_tick
                    .filter(|tick| *tick != 0)
                    .map(|tick| (tick, Some(effect.execution_id.clone())))
                    .or_else(|| {
                        observed_age(&observations, &row).map(|observation| {
                            (
                                observation.observed_tick,
                                observation.owner_execution.clone(),
                            )
                        })
                    })
                    .filter(|(tick, _)| window_open && *tick <= watermark)
            }
            RetentionTarget::Db2Replay | RetentionTarget::ImsReplay | RetentionTarget::MqReplay => {
                let metadata = replay_metadata(
                    &row.payload,
                    &row.key,
                    replay_schema(target).ok_or(StoreError::InvalidTransition)?,
                )?;
                let age = metadata
                    .map(|metadata| (metadata.deadline_tick, metadata.owner_execution))
                    .or_else(|| {
                        observed_age(&observations, &row).and_then(|observation| {
                            observation
                                .owner_execution
                                .clone()
                                .map(|owner| (observation.observed_tick, owner))
                        })
                    });
                match age {
                    Some((tick, owner))
                        if window_open
                            && tick <= watermark
                            && execution_is_prunable(store, owner.as_str())?
                            && effect_recovery_is_clear(store, &row.key)? =>
                    {
                        Some((tick, Some(owner)))
                    }
                    _ => None,
                }
            }
            RetentionTarget::CicsReplay => {
                let metadata = cics_replay_metadata(&row.payload)?;
                let age = metadata
                    .map(|metadata| (metadata.deadline_tick, metadata.owner_execution))
                    .or_else(|| {
                        observed_age(&observations, &row).and_then(|observation| {
                            observation
                                .owner_execution
                                .clone()
                                .map(|owner| (observation.observed_tick, owner))
                        })
                    });
                match age {
                    Some((tick, owner))
                        if window_open
                            && tick <= watermark
                            && execution_is_prunable(store, owner.as_str())?
                            && effect_recovery_is_clear(store, &row.key)? =>
                    {
                        Some((tick, Some(owner)))
                    }
                    _ => None,
                }
            }
            RetentionTarget::LifecycleEvents
            | RetentionTarget::Audit
            | RetentionTarget::RacfEvidence
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsUnitOfWork
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog => None,
        };
        if let Some((tick, owner_execution)) = age {
            eligible.push(Candidate {
                tick,
                row,
                owner_execution,
            });
        }
    }
    eligible.sort_by(|left, right| {
        left.tick
            .cmp(&right.tick)
            .then_with(|| left.row.key.cmp(&right.row.key))
    });
    Ok(CandidateSet { active, eligible })
}

fn terminal_execution_candidates(
    store: &dyn DurableRetentionBackend,
    watermark: u64,
    window_open: bool,
    capacity: usize,
    core: &CoreDependencies,
) -> Result<CandidateSet, StoreError> {
    let rows = store.list_provider_state("durable-execution", capacity)?;
    let active = rows.len();
    let observations = observations(store, RetentionTarget::TerminalExecutions)?;
    let dependencies = execution_dependencies(store, &rows, capacity, true, core)?;
    let mut eligible = Vec::new();
    for row in rows {
        let execution = decode_execution(&row.payload, row.version)?;
        let Some(terminal_tick) = execution
            .terminal_tick
            .filter(|tick| *tick != 0)
            .or_else(|| observed_age(&observations, &row).map(|item| item.observed_tick))
        else {
            continue;
        };
        if execution.state.terminal()
            && store
                .get_provider_state(
                    crate::root_terminal::ACTOR_NAMESPACE,
                    execution.execution_id.as_str(),
                )?
                .is_none()
            && window_open
            && terminal_tick <= watermark
            && !dependencies.blocked.contains(&execution.execution_id)
            && !dependencies.unowned_replay
        {
            eligible.push(Candidate {
                tick: terminal_tick,
                row,
                owner_execution: Some(execution.execution_id),
            });
        }
    }
    sort_candidates(&mut eligible);
    Ok(CandidateSet { active, eligible })
}

fn audit_candidates(
    store: &dyn DurableRetentionBackend,
    watermark: u64,
    window_open: bool,
    capacity: usize,
    core: &CoreDependencies,
) -> Result<CandidateSet, StoreError> {
    let executions = store.list_provider_state("durable-execution", capacity)?;
    let execution_by_id = executions
        .into_iter()
        .map(|row| {
            let execution = decode_execution(&row.payload, row.version)?;
            Ok((execution.execution_id.clone(), execution))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, StoreError>>()?;
    let dependencies = recovery_dependencies(store, capacity, core)?;
    let observations = observations(store, RetentionTarget::Audit)?;
    let rows = store.list_provider_state(AUDIT_NAMESPACE, capacity)?;
    let active = rows.len();
    let mut eligible = Vec::new();
    for row in rows {
        let audit = decode_audit(&row.payload)?;
        let checkpoint_free = store
            .get_provider_state("durable-checkpoint", audit.execution_id.as_str())?
            .is_none();
        let effect_clear = !dependencies
            .unresolved_effects
            .contains(&audit.execution_id)
            && effect_recovery_is_clear(store, audit.invocation_key.as_str())?;
        let owner_ready = execution_by_id
            .get(&audit.execution_id)
            .is_none_or(|execution| execution.state.terminal());
        let retention_tick = (audit.observed_tick != 0)
            .then_some(audit.observed_tick)
            .or_else(|| observed_age(&observations, &row).map(|item| item.observed_tick));
        if owner_ready
            && checkpoint_free
            && effect_clear
            && window_open
            && retention_tick.is_some_and(|tick| tick <= watermark)
        {
            eligible.push(Candidate {
                tick: retention_tick.ok_or(StoreError::Conflict)?,
                row,
                owner_execution: Some(audit.execution_id),
            });
        }
    }
    sort_candidates(&mut eligible);
    Ok(CandidateSet { active, eligible })
}

fn terminal_work_candidates(
    store: &dyn DurableRetentionBackend,
    watermark: u64,
    window_open: bool,
    capacity: usize,
    core: &CoreDependencies,
) -> Result<CandidateSet, StoreError> {
    let rows = store.list_provider_state("durable-work", capacity)?;
    let active = rows.len();
    let dependencies = recovery_dependencies(store, capacity, core)?;
    let observations = observations(store, RetentionTarget::TerminalWork)?;
    let mut eligible = Vec::new();
    for row in rows {
        let work = decode_work(&row.payload)?;
        let Some(terminal_tick) = work
            .terminal_tick
            .filter(|tick| *tick != 0)
            .or_else(|| observed_age(&observations, &row).map(|item| item.observed_tick))
        else {
            continue;
        };
        if work.state.terminal()
            && window_open
            && terminal_tick <= watermark
            && execution_is_prunable(store, work.execution_id.as_str())?
            && !dependencies.unresolved_effects.contains(&work.execution_id)
            && !dependencies.active_work.contains(&work.execution_id)
            && !dependencies.replay.contains(&work.execution_id)
            && !dependencies.unowned_replay
        {
            eligible.push(Candidate {
                tick: terminal_tick,
                row,
                owner_execution: Some(work.execution_id),
            });
        }
    }
    sort_candidates(&mut eligible);
    Ok(CandidateSet { active, eligible })
}

struct ExecutionDependencies {
    blocked: BTreeSet<mainframe_env_execution_api::ExecutionId>,
    unowned_replay: bool,
}

struct RecoveryDependencies {
    unresolved_effects: BTreeSet<mainframe_env_execution_api::ExecutionId>,
    active_work: BTreeSet<mainframe_env_execution_api::ExecutionId>,
    replay: BTreeSet<mainframe_env_execution_api::ExecutionId>,
    unowned_replay: bool,
}

fn execution_dependencies(
    store: &dyn ProviderStateStore,
    executions: &[ProviderStateRecord],
    capacity: usize,
    include_events: bool,
    core: &CoreDependencies,
) -> Result<ExecutionDependencies, StoreError> {
    let mut blocked = BTreeSet::new();
    for row in store.list_provider_state("durable-checkpoint", capacity)? {
        blocked.insert(
            mainframe_env_execution_api::ExecutionId::new(&row.key, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?,
        );
    }
    for row in store.list_provider_state("durable-outbox", capacity)? {
        blocked.insert(decode_outbox(&row.payload, row.version)?.execution_id);
    }
    for row in store.list_provider_state("durable-effect", capacity)? {
        let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
            .map_err(|_| StoreError::IncompatibleVersion)?;
        blocked.insert(decode_effect(&key, &row.payload)?.execution_id);
    }
    for row in store.list_provider_state("durable-work", capacity)? {
        blocked.insert(decode_work(&row.payload)?.execution_id);
    }
    for row in store.list_provider_state(AUDIT_NAMESPACE, capacity)? {
        blocked.insert(decode_audit(&row.payload)?.execution_id);
    }
    if include_events {
        for execution_row in executions {
            let execution = decode_execution(&execution_row.payload, execution_row.version)?;
            if !store
                .list_provider_state(
                    &format!("durable-event:{}", execution.execution_id),
                    capacity,
                )?
                .is_empty()
            {
                blocked.insert(execution.execution_id);
            }
        }
    }
    blocked.extend(core.executions.iter().cloned());
    Ok(ExecutionDependencies {
        blocked,
        unowned_replay: core.unowned,
    })
}

fn recovery_dependencies(
    store: &dyn ProviderStateStore,
    capacity: usize,
    core: &CoreDependencies,
) -> Result<RecoveryDependencies, StoreError> {
    let mut unresolved_effects = BTreeSet::new();
    for row in store.list_provider_state("durable-effect", capacity)? {
        let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
            .map_err(|_| StoreError::IncompatibleVersion)?;
        let effect = decode_effect(&key, &row.payload)?;
        if matches!(
            effect.state,
            EffectState::Intent | EffectState::UnknownOutcome
        ) {
            unresolved_effects.insert(effect.execution_id);
        }
    }
    let mut active_work = BTreeSet::new();
    for row in store.list_provider_state("durable-work", capacity)? {
        let work = decode_work(&row.payload)?;
        if !work.state.terminal() {
            active_work.insert(work.execution_id);
        }
    }
    Ok(RecoveryDependencies {
        unresolved_effects,
        active_work,
        replay: core.executions.clone(),
        unowned_replay: core.unowned,
    })
}

fn sort_candidates(candidates: &mut [Candidate]) {
    candidates.sort_by(|left, right| {
        left.tick
            .cmp(&right.tick)
            .then_with(|| left.row.namespace.cmp(&right.row.namespace))
            .then_with(|| left.row.key.cmp(&right.row.key))
    });
}

fn event_candidates(
    store: &dyn DurableRetentionBackend,
    watermark: u64,
    window_open: bool,
    capacity: usize,
    core: &CoreDependencies,
) -> Result<CandidateSet, StoreError> {
    let executions = store.list_provider_state("durable-execution", capacity)?;
    let dependencies = execution_dependencies(store, &executions, capacity, false, core)?;
    let observations = observations(store, RetentionTarget::LifecycleEvents)?;
    let mut active = 0usize;
    let mut eligible = Vec::new();
    for execution_row in executions {
        let execution = decode_execution(&execution_row.payload, execution_row.version)?;
        let rows = store.list_provider_state(
            &format!("durable-event:{}", execution.execution_id),
            capacity,
        )?;
        active = active
            .checked_add(rows.len())
            .ok_or(StoreError::CapacityExceeded)?;
        if !execution.state.terminal()
            || dependencies.blocked.contains(&execution.execution_id)
            || dependencies.unowned_replay
            || store
                .get_provider_state("durable-checkpoint", execution.execution_id.as_str())?
                .is_some()
        {
            continue;
        }
        for row in rows {
            let event = decode_event(&row.payload)?;
            let retention_tick = (event.tick != 0)
                .then_some(event.tick)
                .or_else(|| observed_age(&observations, &row).map(|item| item.observed_tick));
            if event.execution_id != execution.execution_id
                || !window_open
                || retention_tick.is_none_or(|tick| tick > watermark)
            {
                continue;
            }
            eligible.push(Candidate {
                tick: retention_tick.ok_or(StoreError::Conflict)?,
                row,
                owner_execution: Some(execution.execution_id.clone()),
            });
        }
    }
    eligible.sort_by(|left, right| {
        left.tick
            .cmp(&right.tick)
            .then_with(|| left.row.namespace.cmp(&right.row.namespace))
            .then_with(|| left.row.key.cmp(&right.row.key))
    });
    Ok(CandidateSet { active, eligible })
}

pub(super) fn execution_is_prunable(
    store: &dyn ProviderStateStore,
    execution_id: &str,
) -> Result<bool, StoreError> {
    if store
        .get_provider_state(crate::root_terminal::ACTOR_NAMESPACE, execution_id)?
        .is_some()
    {
        // A terminal row alone cannot release native root history. The future
        // shared root age/recovery authority must release this membership.
        return Ok(false);
    }
    let Some(row) = store.get_provider_state("durable-execution", execution_id)? else {
        return Err(StoreError::IncompatibleVersion);
    };
    let execution = decode_execution(&row.payload, row.version)?;
    Ok(execution.state.terminal()
        && store
            .get_provider_state("durable-checkpoint", execution_id)?
            .is_none())
}

fn effect_recovery_is_clear(
    store: &dyn ProviderStateStore,
    idempotency_key: &str,
) -> Result<bool, StoreError> {
    let Some(row) = store.get_provider_state("durable-effect", idempotency_key)? else {
        return Ok(true);
    };
    let key = IdempotencyKey::new(idempotency_key, InvocationLimits::default())
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let effect = decode_effect(&key, &row.payload)?;
    Ok(matches!(
        effect.state,
        EffectState::Completed | EffectState::Failed
    ))
}
