//! Full-codec provider row normalization and candidate construction.

use super::*;

impl RetentionPlanner {
    pub(super) fn candidate(
        &self,
        target: RetentionTarget,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
        nested_cics_effects: &BTreeSet<String>,
        unattributed_nested_cics: bool,
        cics_uow_index: &CicsUowIndex,
        cobol_replay_index: &CobolReplayIndex,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        match target {
            RetentionTarget::Db2Replay
            | RetentionTarget::ImsReplay
            | RetentionTarget::MqReplay
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsReplay => self.replay_candidate(target, row, observations),
            RetentionTarget::CicsUnitOfWork if row.namespace == "cics-uow-undo" => {
                self.cics_undo_candidate(row, observations, cics_uow_index)
            }
            RetentionTarget::CicsUnitOfWork => self.cics_uow_candidate(
                row,
                observations,
                nested_cics_effects,
                unattributed_nested_cics,
            ),
            RetentionTarget::CobolLifecycle => {
                self.cobol_candidate(row, observations, cobol_replay_index)
            }
            RetentionTarget::SpoolJobs => self.spool_candidate(row, observations),
            RetentionTarget::ConsoleLog => self.console_candidate(row, observations),
            _ => Err(HostProblem::Unsupported),
        }
    }

    pub(super) fn effect(&self, key: &str) -> Result<Option<EffectRecord>, HostProblem> {
        let key = IdempotencyKey::new(key, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.store.effect(&key).map_err(store_problem)
    }

    pub(super) fn replay_view(
        &self,
        target: RetentionTarget,
        row: &ProviderStateRecord,
    ) -> Result<ReplayView, HostProblem> {
        let effect = self.effect(&row.key)?;
        match target {
            RetentionTarget::Db2Replay => {
                let value = describe_db2_replay_row(row, effect.as_ref(), Db2Limits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                Ok(ReplayView {
                    state: match value.retention {
                        Db2ReplayRetentionState::LegacyProtected => ReplayState::Legacy,
                        Db2ReplayRetentionState::PendingProtected => ReplayState::Pending,
                        Db2ReplayRetentionState::Terminal => ReplayState::Terminal,
                    },
                    owner_execution: value.owner_execution,
                    owner_run_unit: value.owner_run_unit,
                    terminal_tick: value.terminal_tick,
                    request_digest: value.request_digest,
                    result_digest: value.result_digest,
                    payload_digest: value.payload_digest,
                    dependency: match value.dependency {
                        Some(Db2ReplayDependency::CoreEffect) => ReplayDependency::Core,
                        Some(Db2ReplayDependency::CicsNested {
                            outer_effect_key, ..
                        }) => ReplayDependency::CicsNested { outer_effect_key },
                        None => ReplayDependency::None,
                    },
                })
            }
            RetentionTarget::ImsReplay => {
                let value = describe_ims_replay_row(row, effect.as_ref(), ImsLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                Ok(ReplayView {
                    state: match value.retention {
                        ImsReplayRetentionState::LegacyProtected => ReplayState::Legacy,
                        ImsReplayRetentionState::PendingProtected => ReplayState::Pending,
                        ImsReplayRetentionState::Terminal => ReplayState::Terminal,
                    },
                    owner_execution: value.owner_execution,
                    owner_run_unit: value.owner_run_unit,
                    terminal_tick: value.terminal_tick,
                    request_digest: value.request_digest,
                    result_digest: value.result_digest,
                    payload_digest: value.payload_digest,
                    dependency: match value.dependency {
                        Some(ImsReplayDependency::CoreEffect) => ReplayDependency::Core,
                        Some(ImsReplayDependency::CicsNested {
                            outer_effect_key, ..
                        }) => ReplayDependency::CicsNested { outer_effect_key },
                        None => ReplayDependency::None,
                    },
                })
            }
            RetentionTarget::MqReplay => {
                let value = describe_mq_replay_row(row, effect.as_ref(), MqLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                Ok(ReplayView {
                    state: match value.retention {
                        MqReplayRetentionState::LegacyProtected => ReplayState::Legacy,
                        MqReplayRetentionState::PendingProtected => ReplayState::Pending,
                        MqReplayRetentionState::Terminal => ReplayState::Terminal,
                    },
                    owner_execution: value.owner_execution,
                    owner_run_unit: value.owner_run_unit,
                    terminal_tick: value.terminal_tick,
                    request_digest: value.request_digest,
                    result_digest: value.result_digest,
                    payload_digest: value.payload_digest,
                    dependency: match value.dependency {
                        Some(MqReplayDependency::CoreEffect) => ReplayDependency::Core,
                        Some(MqReplayDependency::CicsNested {
                            outer_effect_key, ..
                        }) => ReplayDependency::CicsNested { outer_effect_key },
                        None => ReplayDependency::None,
                    },
                })
            }
            RetentionTarget::DatasetReplay => {
                let value = describe_dataset_replay_row(row)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if value.retention == DatasetReplayRetentionState::Terminal
                    && value.dependency == DatasetReplayDependencyState::CoreEffect
                {
                    validate_dataset_replay_effect(
                        row,
                        effect.as_ref().ok_or(HostProblem::InfrastructureFailure)?,
                    )
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                }
                Ok(ReplayView {
                    state: match value.retention {
                        DatasetReplayRetentionState::LegacyProtected => ReplayState::Legacy,
                        DatasetReplayRetentionState::PendingProtected => ReplayState::Pending,
                        DatasetReplayRetentionState::Terminal => ReplayState::Terminal,
                    },
                    owner_execution: value.owner_execution,
                    owner_run_unit: value.owner_run_unit,
                    terminal_tick: value.terminal_tick,
                    request_digest: value.request_digest,
                    result_digest: value.result_digest,
                    payload_digest: value.payload_digest,
                    dependency: match value.dependency {
                        DatasetReplayDependencyState::CoreEffect => ReplayDependency::Core,
                        DatasetReplayDependencyState::CicsNested {
                            outer_effect_key, ..
                        } => ReplayDependency::CicsNested { outer_effect_key },
                        _ => ReplayDependency::None,
                    },
                })
            }
            RetentionTarget::CicsReplay => {
                let value = describe_cics_replay_row(row, effect.as_ref(), Default::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                Ok(ReplayView {
                    state: match value.retention {
                        CicsReplayRetentionState::LegacyProtected => ReplayState::Legacy,
                        CicsReplayRetentionState::PendingProtected => ReplayState::Pending,
                        CicsReplayRetentionState::Terminal => ReplayState::Terminal,
                    },
                    owner_execution: value.owner_execution,
                    owner_run_unit: value.owner_run_unit,
                    terminal_tick: value.terminal_tick,
                    request_digest: value.request_digest,
                    result_digest: value.result_digest,
                    payload_digest: value.payload_digest,
                    dependency: ReplayDependency::Core,
                })
            }
            _ => Err(HostProblem::Unsupported),
        }
    }

    fn replay_candidate(
        &self,
        target: RetentionTarget,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        let view = self.replay_view(target, row)?;
        if view.state == ReplayState::Pending
            || view.state == ReplayState::Terminal
                && matches!(view.dependency, ReplayDependency::None)
        {
            return Ok(None);
        }
        let key = IdempotencyKey::new(&row.key, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let (owner, run, tick, dependency) = if view.state == ReplayState::Legacy {
            let Some((_, observation)) = exact_observation(observations, row, view.payload_digest)
            else {
                return Ok(None);
            };
            let Some(owner) = observation.owner_execution.clone() else {
                return Ok(None);
            };
            let Some(effect) =
                self.valid_legacy_effect(row, &owner, view.request_digest, view.result_digest)?
            else {
                return Ok(None);
            };
            (
                owner,
                effect.run_unit_id.clone(),
                observation
                    .observed_tick
                    .max(effect.resolved_tick.unwrap_or_default()),
                ProviderRetentionDependency::CoreEffect {
                    key: key.clone(),
                    request_digest: view.request_digest,
                    result_digest: view.result_digest,
                },
            )
        } else {
            let owner = parse_execution(view.owner_execution.as_deref())?;
            let run = parse_run(view.owner_run_unit.as_deref())?;
            let Some(base_tick) = view.terminal_tick.filter(|tick| *tick != 0) else {
                return Ok(None);
            };
            match view.dependency {
                ReplayDependency::Core => {
                    let Some(effect_tick) = self
                        .effect(&row.key)?
                        .and_then(|effect| effect.resolved_tick)
                        .filter(|tick| *tick != 0)
                    else {
                        return Ok(None);
                    };
                    (
                        owner,
                        run,
                        base_tick.max(effect_tick),
                        ProviderRetentionDependency::CoreEffect {
                            key: key.clone(),
                            request_digest: view.request_digest,
                            result_digest: view.result_digest,
                        },
                    )
                }
                ReplayDependency::CicsNested { outer_effect_key } => {
                    let Some((dependency, tick)) = self.nested_dependency(
                        &owner,
                        &run,
                        &row.key,
                        &outer_effect_key,
                        base_tick,
                    )?
                    else {
                        return Ok(None);
                    };
                    (owner, run, tick, dependency)
                }
                ReplayDependency::None => return Ok(None),
            }
        };
        let observation = exact_observation(observations, row, view.payload_digest)
            .filter(|(_, value)| {
                value.owner_execution.as_ref() == Some(&owner) && value.observed_tick <= tick
            })
            .map(|(version, value)| RetentionObservationProof {
                version,
                observation: value.clone(),
            });
        Ok(Some(ProviderRetentionRow {
            row: row.clone(),
            owner_execution: Some(owner),
            owner_run_unit: Some(run),
            retention_tick: tick,
            observation,
            dependency,
        }))
    }

    pub(super) fn valid_legacy_effect(
        &self,
        row: &ProviderStateRecord,
        owner: &ExecutionId,
        request_digest: [u8; 32],
        result_digest: [u8; 32],
    ) -> Result<Option<EffectRecord>, HostProblem> {
        Ok(self.effect(&row.key)?.filter(|effect| {
            effect.execution_id == *owner
                && effect.intent.owner == *owner
                && effect.state == EffectState::Completed
                && effect.digest_format == EffectDigestFormat::CanonicalHostV1
                && effect.request_digest == request_digest
                && effect.result_digest == Some(result_digest)
                && effect.resolved_tick.is_some_and(|tick| tick != 0)
        }))
    }

    fn nested_dependency(
        &self,
        owner: &ExecutionId,
        run: &RunUnitId,
        nested_key: &str,
        outer_effect_key: &str,
        provider_tick: u64,
    ) -> Result<Option<(ProviderRetentionDependency, u64)>, HostProblem> {
        let Some(provenance) = self
            .store
            .get_provider_state("cics-uow", outer_effect_key)
            .map_err(store_problem)?
        else {
            return Ok(None);
        };
        let undo = self
            .store
            .get_provider_state("cics-uow-undo", run.as_str())
            .map_err(store_problem)?;
        let descriptor = describe_cics_uow_row(&provenance, undo.as_ref())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if descriptor.dependency != CicsUowDependencyState::Clear
            || descriptor.effect_key.as_deref() != Some(outer_effect_key)
            || descriptor.owner_execution.as_deref() != Some(owner.as_str())
            || descriptor.owner_run_unit.as_deref() != Some(run.as_str())
        {
            return Ok(None);
        }
        let Some(terminal_tick) = descriptor.terminal_tick else {
            return Ok(None);
        };
        let Some(outer) = self.terminal_cics_effect(outer_effect_key, Some(owner))? else {
            return Ok(None);
        };
        if outer.run_unit_id != *run {
            return Ok(None);
        }
        Ok(Some((
            ProviderRetentionDependency::CicsNested {
                provenance,
                absent: vec![
                    ProviderStateIdentity {
                        namespace: "cics-uow-undo".into(),
                        key: run.as_str().into(),
                    },
                    ProviderStateIdentity {
                        namespace: "durable-effect".into(),
                        key: nested_key.into(),
                    },
                ],
            },
            provider_tick
                .max(terminal_tick)
                .max(outer.resolved_tick.unwrap_or_default()),
        )))
    }

    pub(super) fn terminal_cics_effect(
        &self,
        key: &str,
        owner: Option<&ExecutionId>,
    ) -> Result<Option<EffectRecord>, HostProblem> {
        Ok(self.effect(key)?.filter(|effect| {
            owner.is_none_or(|owner| effect.execution_id == *owner && effect.intent.owner == *owner)
                && effect.state == EffectState::Completed
                && effect.digest_format == EffectDigestFormat::CanonicalHostV1
                && effect.result_digest.is_some()
                && effect.resolved_tick.is_some_and(|tick| tick != 0)
                && effect
                    .intent
                    .capability
                    .as_ref()
                    .is_some_and(|capability| capability.as_str() == "host.cics.execute")
        }))
    }

    fn cics_uow_candidate(
        &self,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
        nested_cics_effects: &BTreeSet<String>,
        unattributed_nested_cics: bool,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        let preliminary =
            describe_cics_uow_row(row, None).map_err(|_| HostProblem::InfrastructureFailure)?;
        let undo = preliminary
            .owner_run_unit
            .as_deref()
            .map(|run| {
                self.store
                    .get_provider_state("cics-uow-undo", run)
                    .map_err(store_problem)
            })
            .transpose()?
            .flatten();
        let descriptor = describe_cics_uow_row(row, undo.as_ref())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let legacy_observation = (descriptor.dependency == CicsUowDependencyState::LegacyTerminal)
            .then(|| exact_observation(observations, row, descriptor.payload_digest))
            .flatten()
            .filter(|(_, observation)| observation.owner_execution.is_some());
        let owner = descriptor
            .owner_execution
            .as_deref()
            .map(|value| ExecutionId::new(value, InvocationLimits::default()))
            .transpose()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .or_else(|| {
                legacy_observation.and_then(|(_, observation)| observation.owner_execution.clone())
            });
        let Some(owner) = owner else {
            return Ok(None);
        };
        let Some(effect) = self.terminal_cics_effect(&row.key, Some(&owner))? else {
            return Ok(None);
        };
        let run = descriptor
            .owner_run_unit
            .as_deref()
            .map(|value| RunUnitId::new(value, InvocationLimits::default()))
            .transpose()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .unwrap_or_else(|| effect.run_unit_id.clone());
        let terminal_tick = descriptor
            .terminal_tick
            .or_else(|| legacy_observation.map(|(_, observation)| observation.observed_tick));
        let legacy_undo_present = if descriptor.dependency == CicsUowDependencyState::LegacyTerminal
        {
            self.store
                .get_provider_state("cics-uow-undo", run.as_str())
                .map_err(store_problem)?
                .map(|undo| {
                    mainframe_env_cics::describe_cics_undo_row(&undo)
                        .map(|_| true)
                        .map_err(|_| HostProblem::InfrastructureFailure)
                })
                .transpose()?
                .unwrap_or(false)
        } else {
            false
        };
        let effect_key = descriptor.effect_key.as_deref().unwrap_or(&row.key);
        if !matches!(
            descriptor.dependency,
            CicsUowDependencyState::Clear | CicsUowDependencyState::LegacyTerminal
        ) || unattributed_nested_cics
            || nested_cics_effects.contains(effect_key)
            || legacy_undo_present
            || effect.run_unit_id != run
        {
            return Ok(None);
        }
        let Some(tick) = terminal_tick
            .map(|tick| tick.max(effect.resolved_tick.unwrap_or_default()))
            .filter(|tick| *tick != 0)
        else {
            return Ok(None);
        };
        let observation =
            legacy_observation.map(|(version, observation)| RetentionObservationProof {
                version,
                observation: observation.clone(),
            });
        Ok(Some(ProviderRetentionRow {
            row: row.clone(),
            owner_execution: Some(owner),
            owner_run_unit: Some(run),
            retention_tick: tick,
            observation,
            dependency: ProviderRetentionDependency::ProviderGraph {
                required_rows: Vec::new(),
                required_executions: Vec::new(),
            },
        }))
    }

    fn cics_undo_candidate(
        &self,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
        index: &CicsUowIndex,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        let descriptor = mainframe_env_cics::describe_cics_undo_row(row)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if index.blocks_undo(&descriptor.owner_run_unit) {
            return Ok(None);
        }
        let Some((version, observation)) =
            exact_observation(observations, row, descriptor.payload_digest)
        else {
            return Ok(None);
        };
        let Some(owner) = observation.owner_execution.clone() else {
            return Ok(None);
        };
        let Some(execution) = self
            .store
            .get_execution(&owner)
            .map_err(store_problem)?
            .filter(|execution| {
                execution.state.terminal()
                    && execution.run_unit_id.as_str() == descriptor.owner_run_unit
            })
        else {
            return Ok(None);
        };
        Ok(Some(ProviderRetentionRow {
            row: row.clone(),
            owner_execution: Some(owner),
            owner_run_unit: Some(execution.run_unit_id),
            retention_tick: observation.observed_tick,
            observation: Some(RetentionObservationProof {
                version,
                observation: observation.clone(),
            }),
            dependency: ProviderRetentionDependency::ProviderGraph {
                required_rows: Vec::new(),
                required_executions: Vec::new(),
            },
        }))
    }

    fn cobol_candidate(
        &self,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
        replay_index: &CobolReplayIndex,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        let descriptor =
            describe_cobol_retention_row(row).map_err(|_| HostProblem::InfrastructureFailure)?;
        if descriptor.state == CobolRetentionState::Active {
            return Ok(None);
        }
        let digest = payload_digest(&row.payload);
        let legacy_observation = (descriptor.state == CobolRetentionState::LegacyProtected)
            .then(|| exact_observation(observations, row, digest))
            .flatten()
            .filter(|(_, observation)| observation.owner_execution.is_some());
        let Some(tick) = descriptor
            .terminal_tick()
            .or_else(|| legacy_observation.map(|(_, observation)| observation.observed_tick))
        else {
            return Ok(None);
        };
        let owner = descriptor
            .owner_execution
            .as_deref()
            .map(|value| ExecutionId::new(value, InvocationLimits::default()))
            .transpose()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .or_else(|| {
                legacy_observation.and_then(|(_, observation)| observation.owner_execution.clone())
            });
        let Some(owner) = owner else {
            return Ok(None);
        };
        let Some(execution) = self
            .store
            .get_execution(&owner)
            .map_err(store_problem)?
            .filter(|execution| execution.state.terminal())
        else {
            return Ok(None);
        };
        if !self.legacy_cobol_owner_is_clear_indexed(row, &execution, replay_index)? {
            return Ok(None);
        }
        let descriptor_run = descriptor
            .owner_run_unit
            .as_deref()
            .map(|value| RunUnitId::new(value, InvocationLimits::default()))
            .transpose()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if descriptor_run
            .as_ref()
            .is_some_and(|run| *run != execution.run_unit_id)
        {
            return Ok(None);
        }
        let run = execution.run_unit_id;
        let mut required_rows = Vec::new();
        let mut required_executions = Vec::new();
        for dependency in descriptor.dependencies {
            match dependency {
                CobolRetentionDependency::Execution(value) => {
                    let dependency = ExecutionId::new(value, InvocationLimits::default())
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    if dependency != owner {
                        required_executions.push(dependency);
                    }
                }
                CobolRetentionDependency::RunUnit(expected) if expected != run.as_str() => {
                    return Ok(None);
                }
                CobolRetentionDependency::RunUnit(_) => {}
                CobolRetentionDependency::ProviderRow { namespace, key } => {
                    let Some(required) = self
                        .store
                        .get_provider_state(&namespace, &key)
                        .map_err(store_problem)?
                    else {
                        return Ok(None);
                    };
                    required_rows.push(required);
                }
            }
        }
        let observation =
            legacy_observation.map(|(version, observation)| RetentionObservationProof {
                version,
                observation: observation.clone(),
            });
        Ok(Some(ProviderRetentionRow {
            row: row.clone(),
            owner_execution: Some(owner),
            owner_run_unit: Some(run),
            retention_tick: tick,
            observation,
            dependency: ProviderRetentionDependency::ProviderGraph {
                required_rows,
                required_executions,
            },
        }))
    }

    pub(super) fn legacy_cobol_owner_is_clear(
        &self,
        row: &ProviderStateRecord,
        execution: &mainframe_env_store_api::ExecutionRecord,
    ) -> Result<bool, HostProblem> {
        let rows = self
            .store
            .list_provider_state("cobol-call-replay@1", MAX_SCAN)
            .map_err(store_problem)?;
        if rows.len() == MAX_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        let index = self.cobol_replay_index(&rows)?;
        self.legacy_cobol_owner_is_clear_indexed(row, execution, &index)
    }

    pub(super) fn legacy_cobol_owner_is_clear_indexed(
        &self,
        row: &ProviderStateRecord,
        execution: &mainframe_env_store_api::ExecutionRecord,
        replay_index: &CobolReplayIndex,
    ) -> Result<bool, HostProblem> {
        if row.namespace != "cobol-call-protocol@1" {
            return Ok(true);
        }
        if row.key != crate::cobol::retention::protocol_key(execution.run_unit_id.as_str())
            || self
                .store
                .get_provider_state("cobol-call-protocol@2", &row.key)
                .map_err(store_problem)?
                .is_some()
        {
            return Ok(false);
        }
        let run_state_key = crate::cobol::retention::run_state_key(
            execution.run_unit_id.as_str(),
            execution.principal.as_str(),
        );
        let instance_namespace = format!("cobol-instance@1:{run_state_key}");
        if !self
            .store
            .list_provider_state(&instance_namespace, 1)
            .map_err(store_problem)?
            .is_empty()
        {
            return Ok(false);
        }
        Ok(!replay_index.has_legacy_replay
            && !replay_index.runs.contains(execution.run_unit_id.as_str()))
    }

    pub(super) fn cobol_replay_index(
        &self,
        rows: &[ProviderStateRecord],
    ) -> Result<CobolReplayIndex, HostProblem> {
        let mut index = CobolReplayIndex::default();
        for replay in rows
            .iter()
            .filter(|row| row.namespace == "cobol-call-replay@1")
        {
            #[cfg(test)]
            {
                index.scanned_rows += 1;
            }
            let descriptor = describe_cobol_retention_row(replay)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            index.has_legacy_replay |= descriptor.state == CobolRetentionState::LegacyProtected;
            if let Some(run) = descriptor.owner_run_unit {
                index.runs.insert(run);
            }
        }
        Ok(index)
    }

    pub(super) fn cics_uow_exists_for_run(&self, run: &str) -> Result<bool, HostProblem> {
        let rows = self
            .store
            .list_provider_state("cics-uow", MAX_SCAN)
            .map_err(store_problem)?;
        if rows.len() == MAX_SCAN {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.cics_uow_index(&rows)?.blocks_undo(run))
    }

    pub(super) fn cics_uow_index(
        &self,
        rows: &[ProviderStateRecord],
    ) -> Result<CicsUowIndex, HostProblem> {
        let mut index = CicsUowIndex::default();
        for row in rows.iter().filter(|row| row.namespace == "cics-uow") {
            #[cfg(test)]
            {
                index.scanned_rows += 1;
            }
            let descriptor =
                describe_cics_uow_row(row, None).map_err(|_| HostProblem::InfrastructureFailure)?;
            if let Some(run) = descriptor.owner_run_unit {
                index.known_runs.insert(run);
                continue;
            }
            let Some(effect) = self.terminal_cics_effect(&row.key, None)? else {
                // An unattributed legacy UOW can own any run's undo authority. It must
                // suppress orphan cleanup until a canonical outer effect resolves the run.
                index.has_unattributed_uow = true;
                continue;
            };
            index.known_runs.insert(effect.run_unit_id.to_string());
        }
        Ok(index)
    }

    fn spool_candidate(
        &self,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        let descriptor = describe_spool_retention_row(row, SpoolLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let (tick, observation) = match descriptor.retention {
            SpoolRetentionState::Terminal => (descriptor.terminal_tick, None),
            SpoolRetentionState::LegacyProtected => {
                let observation =
                    exact_observation(observations, row, payload_digest(&row.payload))
                        .filter(|(_, observation)| observation.owner_execution.is_none());
                (
                    observation.map(|(_, observation)| observation.observed_tick),
                    observation.map(|(version, observation)| RetentionObservationProof {
                        version,
                        observation: observation.clone(),
                    }),
                )
            }
            SpoolRetentionState::LiveJob | SpoolRetentionState::PurgeRecovery => (None, None),
        };
        Ok(tick
            .filter(|tick| *tick != 0)
            .map(|retention_tick| ProviderRetentionRow {
                row: row.clone(),
                owner_execution: None,
                owner_run_unit: None,
                retention_tick,
                observation,
                dependency: ProviderRetentionDependency::None,
            }))
    }

    fn console_candidate(
        &self,
        row: &ProviderStateRecord,
        observations: &ObservationMap,
    ) -> Result<Option<ProviderRetentionRow>, HostProblem> {
        let descriptor =
            describe_console_log_row(row).map_err(|_| HostProblem::InfrastructureFailure)?;
        let legacy_observation = (descriptor.retention
            == ConsoleLogRetentionState::LegacyProtected)
            .then(|| exact_observation(observations, row, descriptor.payload_digest))
            .flatten()
            .filter(|(_, observation)| observation.owner_execution.is_none());
        let Some(tick) = descriptor
            .terminal_tick
            .or_else(|| legacy_observation.map(|(_, observation)| observation.observed_tick))
        else {
            return Ok(None);
        };
        let (owner_execution, owner_run_unit, dependency) = match descriptor.owner_kind {
            Some(ConsoleLogOwnerKind::Execution) => (
                Some(parse_execution(descriptor.owner_execution.as_deref())?),
                Some(parse_run(descriptor.owner_run_unit.as_deref())?),
                ProviderRetentionDependency::ProviderGraph {
                    required_rows: Vec::new(),
                    required_executions: Vec::new(),
                },
            ),
            Some(ConsoleLogOwnerKind::DirectProductRoute) | None => {
                (None, None, ProviderRetentionDependency::DirectProduct)
            }
        };
        let observation =
            legacy_observation.map(|(version, observation)| RetentionObservationProof {
                version,
                observation: observation.clone(),
            });
        Ok(Some(ProviderRetentionRow {
            row: row.clone(),
            owner_execution,
            owner_run_unit,
            retention_tick: tick,
            observation,
            dependency,
        }))
    }

    pub(super) fn clean_observations(
        &self,
        target: RetentionTarget,
        expected_epoch: u64,
        source_rows: &[ProviderStateRecord],
        max_removals: usize,
    ) -> Result<(usize, u64), HostProblem> {
        let source_by_identity = source_rows
            .iter()
            .map(|row| ((row.namespace.as_str(), row.key.as_str()), row))
            .collect::<BTreeMap<_, _>>();
        let mut removed = 0;
        for (version, observation) in self.observations(target)? {
            if removed == max_removals {
                break;
            }
            let source =
                source_by_identity.get(&(observation.namespace.as_str(), observation.key.as_str()));
            let stale = source.is_none_or(|row| {
                observation.source_version != row.version
                    || observation.source_digest != payload_digest(&row.payload)
            });
            if !stale {
                continue;
            }
            let source_assertion = source.map_or_else(
                || {
                    ProviderRetentionObservationSource::Absent(ProviderStateIdentity {
                        namespace: observation.namespace.clone(),
                        key: observation.key.clone(),
                    })
                },
                |row| ProviderRetentionObservationSource::Present((*row).clone()),
            );
            self.store
                .delete_provider_retention_observation(ProviderRetentionObservationDeletion {
                    expected_epoch,
                    source: source_assertion,
                    observation,
                    expected_observation_version: version,
                })
                .map_err(store_problem)?;
            removed = 1;
            let next_epoch = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            return Ok((removed, next_epoch));
        }
        Ok((removed, expected_epoch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{ArtifactRef, PrincipalId, Selector};
    use mainframe_env_store::{MemoryStore, StoreLimits};
    use mainframe_env_store_api::{ExecutionRecord, ExecutionState};

    #[test]
    fn unattributed_legacy_uow_blocks_reconciled_orphan_undo() {
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let run = RunUnitId::new("undo-run", InvocationLimits::default()).unwrap();
        let owner = ExecutionId::new("undo-owner", InvocationLimits::default()).unwrap();
        let execution = ExecutionRecord {
            execution_id: owner.clone(),
            run_unit_id: run.clone(),
            selector: Selector::new("program:UNDO", InvocationLimits::default()).unwrap(),
            artifact: ArtifactRef::new("sha256:undo", InvocationLimits::default()).unwrap(),
            principal: PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap(),
            state: ExecutionState::Admitted,
            attempt: 1,
            version: 1,
            owner_lease: None,
            lease_expiry_tick: None,
            terminal_tick: None,
        };
        store.create_execution(execution).unwrap();
        let mut version = 1;
        for (state, tick) in [
            (ExecutionState::Queued, 1),
            (ExecutionState::Running, 2),
            (ExecutionState::Completing, 3),
            (ExecutionState::Completed, 4),
        ] {
            version = store
                .transition_execution(&owner, version, state, tick)
                .unwrap()
                .version;
        }
        let mut pending_uow = b"MECU1".to_vec();
        pending_uow.push(b'C');
        pending_uow.extend_from_slice(&3_u32.to_be_bytes());
        pending_uow.extend_from_slice(b"TX1");
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: "outer-without-effect".into(),
                    version: 1,
                    payload: pending_uow,
                },
                None,
            )
            .unwrap();
        let mut legacy_uow = b"MECU1".to_vec();
        legacy_uow.push(b'c');
        legacy_uow.extend_from_slice(&3_u32.to_be_bytes());
        legacy_uow.extend_from_slice(b"TX1");
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "cics-uow".into(),
                    key: "outer-without-effect".into(),
                    version: 2,
                    payload: legacy_uow,
                },
                Some(1),
            )
            .unwrap();
        let mut undo_payload = b"MECUNDO1".to_vec();
        undo_payload.extend_from_slice(&1_u32.to_be_bytes());
        undo_payload.push(2);
        undo_payload.extend_from_slice(&4_u32.to_be_bytes());
        undo_payload.extend_from_slice(b"USER");
        undo_payload.extend_from_slice(&0_u32.to_be_bytes());
        let undo = ProviderStateRecord {
            namespace: "cics-uow-undo".into(),
            key: run.as_str().into(),
            version: 1,
            payload: undo_payload,
        };
        store.put_provider_state(undo.clone(), None).unwrap();
        let expected_epoch = store.provider_state_retention_epoch().unwrap();
        store
            .record_provider_retention_observation(
                undo.clone(),
                expected_epoch,
                RetentionObservation {
                    target: RetentionTarget::CicsUnitOfWork,
                    namespace: undo.namespace.clone(),
                    key: undo.key.clone(),
                    source_version: undo.version,
                    source_digest: payload_digest(&undo.payload),
                    observed_tick: 10,
                    owner_execution: Some(owner),
                },
            )
            .unwrap();
        let planner = RetentionPlanner::from_existing(
            store,
            RetentionPolicy {
                lifecycle_ticks: 1,
                idempotency_ticks: 1,
                audit_ticks: 1,
                archive_ticks: 1,
                low_watermark_percent: 70,
                high_watermark_percent: 85,
                max_batch: 8,
            },
            None,
        )
        .unwrap();
        let plan = planner
            .provider_plan(RetentionTarget::CicsUnitOfWork, 100, 0)
            .unwrap();
        assert!(plan.rows.is_empty());
    }

    #[test]
    fn family_indexes_decode_each_row_once_at_scale() {
        const ROWS: usize = 512;
        let store: Arc<dyn PlatformStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
        let planner = RetentionPlanner::from_existing(
            store,
            RetentionPolicy {
                lifecycle_ticks: 1,
                idempotency_ticks: 1,
                audit_ticks: 1,
                archive_ticks: 1,
                low_watermark_percent: 70,
                high_watermark_percent: 85,
                max_batch: 8,
            },
            None,
        )
        .unwrap();
        let mut uows = Vec::with_capacity(ROWS);
        let mut replays = Vec::with_capacity(ROWS);
        for sequence in 1..=ROWS {
            let mut uow = b"MECU1".to_vec();
            uow.push(b'C');
            uow.extend_from_slice(&3_u32.to_be_bytes());
            uow.extend_from_slice(b"TX1");
            uows.push(ProviderStateRecord {
                namespace: "cics-uow".into(),
                key: format!("outer-{sequence}"),
                version: 1,
                payload: uow,
            });

            let key = format!("{sequence:064x}");
            replays.push(ProviderStateRecord {
                namespace: "cobol-call-replay@1".into(),
                key: key.clone(),
                version: 2,
                payload: serde_json::to_vec(&serde_json::json!({
                    "schema_version": 1,
                    "fingerprint": "a".repeat(64),
                    "child_execution": format!("online-call-execution-{key}"),
                    "reply": {
                        "schema": "mainframe-env.cobol.call@1",
                        "bytes": [],
                    },
                }))
                .unwrap(),
            });
        }
        let uow_index = planner.cics_uow_index(&uows).unwrap();
        let replay_index = planner.cobol_replay_index(&replays).unwrap();
        assert_eq!(uow_index.scanned_rows, ROWS);
        assert_eq!(replay_index.scanned_rows, ROWS);
        assert!(uow_index.has_unattributed_uow);
        assert!(replay_index.has_legacy_replay);
    }
}
