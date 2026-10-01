//! Exhaustive provider dependency inventory used by core retention targets.

use super::*;
use crate::cobol::retention::CobolRetentionRowDescriptor;
use mainframe_env_cics::describe_cics_undo_row;
use mainframe_env_store_api::CoreRetentionDependencySnapshot;

pub(super) const fn core_target(target: RetentionTarget) -> bool {
    matches!(
        target,
        RetentionTarget::ResolvedEffects
            | RetentionTarget::TerminalWork
            | RetentionTarget::LifecycleEvents
            | RetentionTarget::TerminalExecutions
    )
}

pub(super) fn snapshot(
    planner: &RetentionPlanner,
) -> Result<CoreRetentionDependencySnapshot, HostProblem> {
    planner.core_dependencies_impl()
}

pub(super) fn nested_cics_effects(
    planner: &RetentionPlanner,
    expected_epoch: u64,
) -> Result<(BTreeSet<String>, bool), HostProblem> {
    planner.cics_nested_outer_effect_keys(expected_epoch)
}

type ObservationMap = BTreeMap<(RetentionTarget, String, String), RetentionObservation>;

fn observed_owner(
    observations: &ObservationMap,
    target: RetentionTarget,
    row: &ProviderStateRecord,
    digest: [u8; 32],
) -> Option<ExecutionId> {
    observations
        .get(&(target, row.namespace.clone(), row.key.clone()))
        .filter(|observation| {
            observation.source_version == row.version
                && observation.source_digest == digest
                && observation.observed_tick != 0
        })
        .and_then(|observation| observation.owner_execution.clone())
}

fn collect_cobol_execution_dependencies(
    identity: &(String, String),
    descriptors: &BTreeMap<(String, String), CobolRetentionRowDescriptor>,
    visiting: &mut BTreeSet<(String, String)>,
    memo: &mut BTreeMap<(String, String), bool>,
    owners: &mut BTreeSet<ExecutionId>,
) -> bool {
    if let Some(attributable) = memo.get(identity) {
        return *attributable;
    }
    if !visiting.insert(identity.clone()) {
        memo.insert(identity.clone(), false);
        return false;
    }
    let Some(descriptor) = descriptors.get(identity) else {
        visiting.remove(identity);
        memo.insert(identity.clone(), false);
        return false;
    };
    let mut attributable = false;
    if let Some(owner) = descriptor.owner_execution.as_deref() {
        match ExecutionId::new(owner, InvocationLimits::default()) {
            Ok(owner) => {
                owners.insert(owner);
                attributable = true;
            }
            Err(_) => {
                visiting.remove(identity);
                memo.insert(identity.clone(), false);
                return false;
            }
        }
    }
    for dependency in &descriptor.dependencies {
        match dependency {
            CobolRetentionDependency::Execution(owner) => {
                let Ok(owner) = ExecutionId::new(owner, InvocationLimits::default()) else {
                    visiting.remove(identity);
                    memo.insert(identity.clone(), false);
                    return false;
                };
                owners.insert(owner);
                attributable = true;
            }
            CobolRetentionDependency::ProviderRow { namespace, key } => {
                attributable |= collect_cobol_execution_dependencies(
                    &(namespace.clone(), key.clone()),
                    descriptors,
                    visiting,
                    memo,
                    owners,
                );
            }
            CobolRetentionDependency::RunUnit(_) => {}
        }
    }
    visiting.remove(identity);
    memo.insert(identity.clone(), attributable);
    attributable
}

impl RetentionPlanner {
    fn bounded_namespace(&self, namespace: &str) -> Result<Vec<ProviderStateRecord>, HostProblem> {
        let rows = self
            .store
            .list_provider_state(namespace, MAX_SCAN)
            .map_err(store_problem)?;
        if rows.len() == MAX_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(rows)
    }

    fn bounded_prefix(&self, prefix: &str) -> Result<Vec<ProviderStateRecord>, HostProblem> {
        let rows = self
            .store
            .list_provider_state_prefix(prefix, MAX_SCAN)
            .map_err(store_problem)?;
        if rows.len() == MAX_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(rows)
    }

    fn effect_for_row(&self, row: &ProviderStateRecord) -> Option<EffectRecord> {
        let key = IdempotencyKey::new(&row.key, InvocationLimits::default()).ok()?;
        self.store.effect(&key).ok().flatten()
    }

    fn add_owner(owners: &mut BTreeSet<ExecutionId>, owner: Option<&str>, unowned: &mut bool) {
        match owner.and_then(|value| ExecutionId::new(value, InvocationLimits::default()).ok()) {
            Some(owner) => {
                owners.insert(owner);
            }
            None => *unowned = true,
        }
    }

    /// Exact outer CICS effects still referenced by nested provider replay rows.
    pub(super) fn cics_nested_outer_effect_keys(
        &self,
        expected_epoch: u64,
    ) -> Result<(BTreeSet<String>, bool), HostProblem> {
        let before = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        if before != expected_epoch {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut keys = BTreeSet::new();
        let mut unowned = false;
        for row in self.bounded_namespace("db2-v1-replay")? {
            let effect = self.effect_for_row(&row);
            match describe_db2_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => match descriptor.dependency {
                    Some(Db2ReplayDependency::CicsNested {
                        outer_effect_key, ..
                    }) => {
                        keys.insert(outer_effect_key);
                    }
                    Some(Db2ReplayDependency::CoreEffect) => {}
                    None => unowned = true,
                },
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("ims-v1-replay")? {
            let effect = self.effect_for_row(&row);
            match describe_ims_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => match descriptor.dependency {
                    Some(ImsReplayDependency::CicsNested {
                        outer_effect_key, ..
                    }) => {
                        keys.insert(outer_effect_key);
                    }
                    Some(ImsReplayDependency::CoreEffect) => {}
                    None => unowned = true,
                },
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("mq-v1-replay")? {
            let effect = self.effect_for_row(&row);
            match describe_mq_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => match descriptor.dependency {
                    Some(MqReplayDependency::CicsNested {
                        outer_effect_key, ..
                    }) => {
                        keys.insert(outer_effect_key);
                    }
                    Some(MqReplayDependency::CoreEffect) => {}
                    None => unowned = true,
                },
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("dataset-replay")? {
            match describe_dataset_replay_row(&row) {
                Ok(descriptor) => match descriptor.dependency {
                    DatasetReplayDependencyState::CicsNested {
                        outer_effect_key, ..
                    } => {
                        keys.insert(outer_effect_key);
                    }
                    DatasetReplayDependencyState::CoreEffect => {}
                    _ => unowned = true,
                },
                Err(_) => unowned = true,
            }
        }
        if self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?
            != expected_epoch
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok((keys, unowned))
    }

    /// Build the exhaustive provider-owned dependency inventory used by core retention.
    fn core_dependencies_impl(&self) -> Result<CoreRetentionDependencySnapshot, HostProblem> {
        let expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        let mut executions = BTreeSet::new();
        let mut effects = BTreeSet::new();
        let mut unowned = false;
        let mut observations = ObservationMap::new();
        for target in [
            RetentionTarget::Db2Replay,
            RetentionTarget::ImsReplay,
            RetentionTarget::MqReplay,
            RetentionTarget::DatasetReplay,
            RetentionTarget::CicsReplay,
            RetentionTarget::CicsUnitOfWork,
            RetentionTarget::CobolLifecycle,
            RetentionTarget::ConsoleLog,
        ] {
            for (_, observation) in self.observations(target)? {
                observations.insert(
                    (
                        target,
                        observation.namespace.clone(),
                        observation.key.clone(),
                    ),
                    observation,
                );
            }
        }

        for row in self.bounded_namespace("db2-v1-replay")? {
            let effect = self.effect_for_row(&row);
            match describe_db2_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => {
                    let owner = descriptor
                        .owner_execution
                        .as_deref()
                        .and_then(|owner| ExecutionId::new(owner, InvocationLimits::default()).ok())
                        .or_else(|| {
                            observed_owner(
                                &observations,
                                RetentionTarget::Db2Replay,
                                &row,
                                descriptor.payload_digest,
                            )
                        });
                    Self::add_owner(
                        &mut executions,
                        owner.as_ref().map(ExecutionId::as_str),
                        &mut unowned,
                    );
                    match descriptor.dependency {
                        Some(Db2ReplayDependency::CoreEffect) => {
                            if let Ok(key) =
                                IdempotencyKey::new(&row.key, InvocationLimits::default())
                            {
                                effects.insert(key);
                            } else {
                                unowned = true;
                            }
                        }
                        Some(Db2ReplayDependency::CicsNested {
                            outer_effect_key, ..
                        }) => {
                            match IdempotencyKey::new(outer_effect_key, InvocationLimits::default())
                            {
                                Ok(key) => {
                                    effects.insert(key);
                                }
                                Err(_) => unowned = true,
                            }
                        }
                        None if descriptor.retention
                            == Db2ReplayRetentionState::LegacyProtected =>
                        {
                            if owner.as_ref().is_some_and(|owner| {
                                self.valid_legacy_effect(
                                    &row,
                                    owner,
                                    descriptor.request_digest,
                                    descriptor.result_digest,
                                )
                                .ok()
                                .flatten()
                                .is_some()
                            }) {
                                match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                    Ok(key) => {
                                        effects.insert(key);
                                    }
                                    Err(_) => unowned = true,
                                }
                            } else {
                                unowned = true;
                            }
                        }
                        None => unowned = true,
                    }
                }
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("ims-v1-replay")? {
            let effect = self.effect_for_row(&row);
            match describe_ims_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => {
                    let owner = descriptor
                        .owner_execution
                        .as_deref()
                        .and_then(|owner| ExecutionId::new(owner, InvocationLimits::default()).ok())
                        .or_else(|| {
                            observed_owner(
                                &observations,
                                RetentionTarget::ImsReplay,
                                &row,
                                descriptor.payload_digest,
                            )
                        });
                    Self::add_owner(
                        &mut executions,
                        owner.as_ref().map(ExecutionId::as_str),
                        &mut unowned,
                    );
                    match descriptor.dependency {
                        Some(ImsReplayDependency::CoreEffect) => {
                            match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                Ok(key) => {
                                    effects.insert(key);
                                }
                                Err(_) => unowned = true,
                            }
                        }
                        Some(ImsReplayDependency::CicsNested {
                            outer_effect_key, ..
                        }) => {
                            match IdempotencyKey::new(outer_effect_key, InvocationLimits::default())
                            {
                                Ok(key) => {
                                    effects.insert(key);
                                }
                                Err(_) => unowned = true,
                            }
                        }
                        None if descriptor.retention
                            == ImsReplayRetentionState::LegacyProtected =>
                        {
                            if owner.as_ref().is_some_and(|owner| {
                                self.valid_legacy_effect(
                                    &row,
                                    owner,
                                    descriptor.request_digest,
                                    descriptor.result_digest,
                                )
                                .ok()
                                .flatten()
                                .is_some()
                            }) {
                                match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                    Ok(key) => {
                                        effects.insert(key);
                                    }
                                    Err(_) => unowned = true,
                                }
                            } else {
                                unowned = true;
                            }
                        }
                        None => unowned = true,
                    }
                }
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("mq-v1-replay")? {
            let effect = self.effect_for_row(&row);
            match describe_mq_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => {
                    let owner = descriptor
                        .owner_execution
                        .as_deref()
                        .and_then(|owner| ExecutionId::new(owner, InvocationLimits::default()).ok())
                        .or_else(|| {
                            observed_owner(
                                &observations,
                                RetentionTarget::MqReplay,
                                &row,
                                descriptor.payload_digest,
                            )
                        });
                    Self::add_owner(
                        &mut executions,
                        owner.as_ref().map(ExecutionId::as_str),
                        &mut unowned,
                    );
                    match descriptor.dependency {
                        Some(MqReplayDependency::CoreEffect) => {
                            match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                Ok(key) => {
                                    effects.insert(key);
                                }
                                Err(_) => unowned = true,
                            }
                        }
                        Some(MqReplayDependency::CicsNested {
                            outer_effect_key, ..
                        }) => {
                            match IdempotencyKey::new(outer_effect_key, InvocationLimits::default())
                            {
                                Ok(key) => {
                                    effects.insert(key);
                                }
                                Err(_) => unowned = true,
                            }
                        }
                        None if descriptor.retention == MqReplayRetentionState::LegacyProtected => {
                            if owner.as_ref().is_some_and(|owner| {
                                self.valid_legacy_effect(
                                    &row,
                                    owner,
                                    descriptor.request_digest,
                                    descriptor.result_digest,
                                )
                                .ok()
                                .flatten()
                                .is_some()
                            }) {
                                match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                    Ok(key) => {
                                        effects.insert(key);
                                    }
                                    Err(_) => unowned = true,
                                }
                            } else {
                                unowned = true;
                            }
                        }
                        None => unowned = true,
                    }
                }
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("dataset-replay")? {
            match describe_dataset_replay_row(&row) {
                Ok(descriptor) => {
                    let owner = descriptor
                        .owner_execution
                        .as_deref()
                        .and_then(|owner| ExecutionId::new(owner, InvocationLimits::default()).ok())
                        .or_else(|| {
                            observed_owner(
                                &observations,
                                RetentionTarget::DatasetReplay,
                                &row,
                                descriptor.payload_digest,
                            )
                        });
                    Self::add_owner(
                        &mut executions,
                        owner.as_ref().map(ExecutionId::as_str),
                        &mut unowned,
                    );
                    match descriptor.dependency {
                        DatasetReplayDependencyState::CoreEffect => {
                            let effect = self.effect_for_row(&row);
                            if descriptor.retention != DatasetReplayRetentionState::Terminal
                                || effect.as_ref().is_some_and(|effect| {
                                    validate_dataset_replay_effect(&row, effect).is_ok()
                                })
                            {
                                match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                    Ok(key) => {
                                        effects.insert(key);
                                    }
                                    Err(_) => unowned = true,
                                }
                            } else {
                                unowned = true;
                            }
                        }
                        DatasetReplayDependencyState::CicsNested {
                            outer_effect_key, ..
                        } => {
                            match IdempotencyKey::new(outer_effect_key, InvocationLimits::default())
                            {
                                Ok(key) => {
                                    effects.insert(key);
                                }
                                Err(_) => unowned = true,
                            }
                        }
                        DatasetReplayDependencyState::TerminalEffectRequired
                            if descriptor.retention
                                == DatasetReplayRetentionState::LegacyProtected =>
                        {
                            if owner.as_ref().is_some_and(|owner| {
                                self.valid_legacy_effect(
                                    &row,
                                    owner,
                                    descriptor.request_digest,
                                    descriptor.result_digest,
                                )
                                .ok()
                                .flatten()
                                .is_some()
                            }) {
                                match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                                    Ok(key) => {
                                        effects.insert(key);
                                    }
                                    Err(_) => unowned = true,
                                }
                            } else {
                                unowned = true;
                            }
                        }
                        DatasetReplayDependencyState::PendingResult => unowned = true,
                        DatasetReplayDependencyState::TerminalEffectRequired => unowned = true,
                    }
                }
                Err(_) => unowned = true,
            }
        }
        for row in self.bounded_namespace("cics-effect-replay-v1")? {
            let effect = self.effect_for_row(&row);
            match describe_cics_replay_row(&row, effect.as_ref(), Default::default()) {
                Ok(descriptor) => {
                    let owner = descriptor
                        .owner_execution
                        .as_deref()
                        .and_then(|owner| ExecutionId::new(owner, InvocationLimits::default()).ok())
                        .or_else(|| {
                            observed_owner(
                                &observations,
                                RetentionTarget::CicsReplay,
                                &row,
                                descriptor.payload_digest,
                            )
                        });
                    Self::add_owner(
                        &mut executions,
                        owner.as_ref().map(ExecutionId::as_str),
                        &mut unowned,
                    );
                    let attributable = descriptor.retention
                        != CicsReplayRetentionState::LegacyProtected
                        || owner.as_ref().is_some_and(|owner| {
                            self.valid_legacy_effect(
                                &row,
                                owner,
                                descriptor.request_digest,
                                descriptor.result_digest,
                            )
                            .ok()
                            .flatten()
                            .is_some()
                        });
                    if attributable {
                        match IdempotencyKey::new(&row.key, InvocationLimits::default()) {
                            Ok(key) => {
                                effects.insert(key);
                            }
                            Err(_) => unowned = true,
                        }
                    } else {
                        unowned = true;
                    }
                }
                Err(_) => unowned = true,
            }
        }

        let undo_rows = self.bounded_namespace("cics-uow-undo")?;
        let undo_by_run = undo_rows
            .iter()
            .map(|row| (row.key.as_str(), row))
            .collect::<BTreeMap<_, _>>();
        let mut known_runs = BTreeSet::new();
        for row in self.bounded_namespace("cics-uow")? {
            let preliminary = describe_cics_uow_row(&row, None);
            let undo = preliminary
                .as_ref()
                .ok()
                .and_then(|descriptor| descriptor.owner_run_unit.as_deref())
                .and_then(|run| undo_by_run.get(run).copied());
            match describe_cics_uow_row(&row, undo) {
                Ok(descriptor) => {
                    let owner = descriptor
                        .owner_execution
                        .as_deref()
                        .and_then(|owner| ExecutionId::new(owner, InvocationLimits::default()).ok())
                        .or_else(|| {
                            observed_owner(
                                &observations,
                                RetentionTarget::CicsUnitOfWork,
                                &row,
                                descriptor.payload_digest,
                            )
                        });
                    let legacy_effect = (descriptor.dependency
                        == CicsUowDependencyState::LegacyTerminal)
                        .then(|| {
                            owner
                                .as_ref()
                                .map(|owner| self.terminal_cics_effect(&row.key, Some(owner)))
                                .transpose()
                                .ok()
                                .flatten()
                                .flatten()
                        })
                        .flatten();
                    Self::add_owner(
                        &mut executions,
                        owner.as_ref().map(ExecutionId::as_str),
                        &mut unowned,
                    );
                    if let Some(task_owner) = &descriptor.task_owner_execution {
                        Self::add_owner(&mut executions, Some(task_owner), &mut unowned);
                    }
                    if let Some(run) = descriptor.owner_run_unit.clone().or_else(|| {
                        legacy_effect
                            .as_ref()
                            .map(|effect| effect.run_unit_id.to_string())
                    }) {
                        known_runs.insert(run);
                    }
                    if let Some(key) = descriptor
                        .effect_key
                        .clone()
                        .or_else(|| legacy_effect.as_ref().map(|_| row.key.clone()))
                    {
                        match IdempotencyKey::new(key, InvocationLimits::default()) {
                            Ok(key) => {
                                effects.insert(key);
                            }
                            Err(_) => unowned = true,
                        }
                    } else {
                        unowned = true;
                    }
                    if descriptor.dependency == CicsUowDependencyState::LegacyTerminal
                        && legacy_effect.is_none()
                    {
                        unowned = true;
                    }
                }
                Err(_) => unowned = true,
            }
        }
        for undo in undo_rows {
            let Ok(descriptor) = describe_cics_undo_row(&undo) else {
                unowned = true;
                continue;
            };
            if known_runs.contains(&undo.key) {
                continue;
            }
            let owner = observed_owner(
                &observations,
                RetentionTarget::CicsUnitOfWork,
                &undo,
                descriptor.payload_digest,
            );
            let attributable = owner.as_ref().is_some_and(|owner| {
                self.store
                    .get_execution(owner)
                    .ok()
                    .flatten()
                    .is_some_and(|execution| {
                        execution.state.terminal()
                            && execution.run_unit_id.as_str() == descriptor.owner_run_unit
                    })
            });
            if let Some(owner) = owner {
                executions.insert(owner);
            }
            if !attributable {
                unowned = true;
            }
        }

        let cobol_rows = self.bounded_prefix("cobol-")?;
        let cobol_replay_index = self.cobol_replay_index(&cobol_rows)?;
        let mut cobol_descriptors = BTreeMap::new();
        let mut cobol_attribution = BTreeMap::new();
        for row in &cobol_rows {
            match describe_cobol_retention_row(row) {
                Ok(descriptor) => {
                    cobol_descriptors.insert((row.namespace.clone(), row.key.clone()), descriptor);
                }
                Err(_) => unowned = true,
            }
        }
        for row in &cobol_rows {
            let identity = (row.namespace.clone(), row.key.clone());
            let Some(descriptor) = cobol_descriptors.get(&identity) else {
                continue;
            };
            let attributed_by_graph = collect_cobol_execution_dependencies(
                &identity,
                &cobol_descriptors,
                &mut BTreeSet::new(),
                &mut cobol_attribution,
                &mut executions,
            );
            let observed = observed_owner(
                &observations,
                RetentionTarget::CobolLifecycle,
                row,
                payload_digest(&row.payload),
            );
            if let Some(owner) = observed.as_ref() {
                executions.insert(owner.clone());
            }
            let observed_is_safe = observed.as_ref().is_some_and(|owner| {
                row.namespace != "cobol-call-protocol@1"
                    || self
                        .store
                        .get_execution(owner)
                        .ok()
                        .flatten()
                        .is_some_and(|execution| {
                            execution.state.terminal()
                                && self
                                    .legacy_cobol_owner_is_clear_indexed(
                                        row,
                                        &execution,
                                        &cobol_replay_index,
                                    )
                                    .unwrap_or(false)
                        })
            });
            if descriptor.state == CobolRetentionState::LegacyProtected && observed.is_none()
                || !attributed_by_graph && observed.is_none()
                || observed.is_some() && !observed_is_safe
            {
                unowned = true;
            }
        }
        for row in self.bounded_namespace("console-log")? {
            match describe_console_log_row(&row) {
                Ok(descriptor) if descriptor.retention == ConsoleLogRetentionState::Terminal => {
                    match descriptor.owner_kind {
                        Some(ConsoleLogOwnerKind::DirectProductRoute) => {}
                        Some(ConsoleLogOwnerKind::Execution) => Self::add_owner(
                            &mut executions,
                            descriptor.owner_execution.as_deref(),
                            &mut unowned,
                        ),
                        None => unowned = true,
                    }
                }
                Ok(descriptor)
                    if descriptor.retention == ConsoleLogRetentionState::LegacyProtected =>
                {
                    let observed = observations.get(&(
                        RetentionTarget::ConsoleLog,
                        row.namespace.clone(),
                        row.key.clone(),
                    ));
                    if observed.is_none_or(|observation| {
                        observation.source_version != row.version
                            || observation.source_digest != descriptor.payload_digest
                            || observation.observed_tick == 0
                            || observation.owner_execution.is_some()
                    }) {
                        unowned = true;
                    }
                }
                Ok(_) | Err(_) => unowned = true,
            }
        }

        if self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?
            != expected_epoch
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if executions.len() > mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES
            || effects.len() > mainframe_env_store_api::MAX_CORE_RETENTION_DEPENDENCIES
        {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(CoreRetentionDependencySnapshot {
            expected_epoch,
            blocked_executions: executions.into_iter().collect(),
            blocked_effect_keys: effects.into_iter().collect(),
            unowned,
        })
    }
}
