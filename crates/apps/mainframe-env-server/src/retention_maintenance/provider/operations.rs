//! RACF, legacy inventory, and explicit age-reconciliation operations.

use super::*;

impl RetentionPlanner {
    fn racf_policy(&self, max_batch_records: usize) -> RacfRetentionPolicy {
        RacfRetentionPolicy {
            retain_ticks: self.policy.audit_ticks.max(self.policy.idempotency_ticks),
            low_watermark_percent: self.policy.low_watermark_percent,
            high_watermark_percent: self.policy.high_watermark_percent,
            max_batch_records,
        }
    }

    pub(super) fn racf_forecast(
        &self,
        now_tick: u64,
        growth: u64,
    ) -> Result<RetentionForecast, HostProblem> {
        let expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        if self.racf_migration_pending {
            let active = SecurityDatabase::legacy_rows_for_retention(
                self.store.as_ref(),
                SecurityDatabaseLimits::default(),
                MAX_SCAN,
            )?
            .len();
            self.require_epoch(expected_epoch)?;
            let capacity =
                target_capacity(&self.store, self.policy, RetentionTarget::RacfEvidence)?;
            let forecast = self
                .store
                .provider_validated_retention_forecast(
                    RetentionTarget::RacfEvidence,
                    self.policy,
                    now_tick,
                    growth,
                    active,
                    0,
                    capacity,
                )
                .map_err(store_problem)?;
            self.require_epoch(expected_epoch)?;
            return Ok(forecast);
        }
        let Some(racf) = &self.racf else {
            let capacity =
                target_capacity(&self.store, self.policy, RetentionTarget::RacfEvidence)?;
            let forecast = self
                .store
                .provider_validated_retention_forecast(
                    RetentionTarget::RacfEvidence,
                    self.policy,
                    now_tick,
                    growth,
                    0,
                    0,
                    capacity,
                )
                .map_err(store_problem)?;
            #[cfg(test)]
            self.run_forecast_epoch_hook();
            self.require_epoch(expected_epoch)?;
            return Ok(forecast);
        };
        let observations = self
            .observations(RetentionTarget::RacfEvidence)?
            .into_iter()
            .map(|(_, row)| row)
            .collect::<Vec<_>>();
        let forecast = racf.retention_forecast_with_observations(
            self.racf_policy(self.policy.max_batch),
            now_tick,
            &observations,
        )?;
        let active = forecast
            .audits
            .saturating_add(forecast.transactions)
            .saturating_add(forecast.recovery_records);
        let eligible = forecast
            .eligible_audits
            .saturating_add(forecast.eligible_transactions)
            .saturating_add(forecast.eligible_recovery_records);
        let capacity = active
            .checked_add(
                forecast
                    .audit_headroom
                    .min(forecast.transaction_headroom)
                    .min(forecast.recovery_headroom),
            )
            .ok_or(HostProblem::ResourceExhausted)?;
        let forecast = self
            .store
            .provider_validated_retention_forecast(
                RetentionTarget::RacfEvidence,
                self.policy,
                now_tick,
                growth,
                active,
                eligible,
                capacity,
            )
            .map_err(store_problem)?;
        #[cfg(test)]
        self.run_forecast_epoch_hook();
        self.require_epoch(expected_epoch)?;
        Ok(forecast)
    }

    fn require_epoch(&self, expected_epoch: u64) -> Result<(), HostProblem> {
        if self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?
            == expected_epoch
        {
            Ok(())
        } else {
            Err(HostProblem::IdempotencyConflict)
        }
    }

    pub(super) fn racf_archive(
        &self,
        now_tick: u64,
        max_records: usize,
    ) -> Result<RetentionReceipt, HostProblem> {
        if self.racf_migration_pending {
            let expected_epoch = self
                .store
                .provider_state_retention_epoch()
                .map_err(store_problem)?;
            let active = SecurityDatabase::legacy_rows_for_retention(
                self.store.as_ref(),
                SecurityDatabaseLimits::default(),
                MAX_SCAN,
            )?
            .len();
            self.require_epoch(expected_epoch)?;
            return Ok(RetentionReceipt {
                target: RetentionTarget::RacfEvidence,
                watermark_tick: now_tick
                    .saturating_sub(self.policy.audit_ticks.max(self.policy.idempotency_ticks)),
                examined: active,
                archived: 0,
                pruned: 0,
                protected: active,
                archive_id: None,
                observations_created: 0,
                observations_reused: 0,
                stale_observations_removed: 0,
            });
        }
        let Some(racf) = &self.racf else {
            let watermark_tick =
                now_tick.saturating_sub(self.policy.audit_ticks.max(self.policy.idempotency_ticks));
            return Ok(RetentionReceipt {
                target: RetentionTarget::RacfEvidence,
                watermark_tick,
                examined: 0,
                archived: 0,
                pruned: 0,
                protected: 0,
                archive_id: None,
                observations_created: 0,
                observations_reused: 0,
                stale_observations_removed: 0,
            });
        };
        let observation_maintenance = self.observe_racf_legacy(now_tick, max_records)?;
        if observation_maintenance.created != 0 || observation_maintenance.stale_removed != 0 {
            let counts = observation_maintenance.counts.unwrap_or_default();
            return Ok(RetentionReceipt {
                target: RetentionTarget::RacfEvidence,
                watermark_tick: counts.watermark_tick,
                examined: counts.examined,
                archived: 0,
                pruned: 0,
                protected: counts.examined.saturating_sub(counts.eligible),
                archive_id: None,
                observations_created: observation_maintenance.created,
                observations_reused: observation_maintenance.reused,
                stale_observations_removed: observation_maintenance.stale_removed,
            });
        }
        let remaining = max_records.saturating_sub(
            observation_maintenance
                .created
                .saturating_add(observation_maintenance.stale_removed),
        );
        let expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        let observations = self.observations(RetentionTarget::RacfEvidence)?;
        if self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?
            != expected_epoch
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let plain = observations
            .iter()
            .map(|(_, observation)| observation.clone())
            .collect::<Vec<_>>();
        let racf_policy = self.racf_policy(remaining.max(1));
        let forecast = racf.retention_forecast_with_observations(racf_policy, now_tick, &plain)?;
        let examined = forecast
            .audits
            .saturating_add(forecast.transactions)
            .saturating_add(forecast.recovery_records);
        let eligible = forecast
            .eligible_audits
            .saturating_add(forecast.eligible_transactions)
            .saturating_add(forecast.eligible_recovery_records);
        let (archived, archive_id, watermark_tick) = if remaining == 0 {
            (0, None, forecast.watermark_tick)
        } else {
            let receipt = racf.archive_and_prune_with_observations(
                racf_policy,
                now_tick,
                &observations,
                expected_epoch,
            )?;
            (
                receipt
                    .archived_audits
                    .saturating_add(receipt.archived_transactions)
                    .saturating_add(receipt.archived_recovery_records),
                receipt.archive_id,
                receipt.watermark_tick,
            )
        };
        Ok(RetentionReceipt {
            target: RetentionTarget::RacfEvidence,
            watermark_tick,
            examined,
            archived,
            pruned: archived,
            protected: examined.saturating_sub(eligible),
            archive_id,
            observations_created: observation_maintenance.created,
            observations_reused: observation_maintenance.reused,
            stale_observations_removed: observation_maintenance.stale_removed,
        })
    }

    fn observe_racf_legacy(
        &self,
        now_tick: u64,
        max_records: usize,
    ) -> Result<ObservationMaintenance, HostProblem> {
        let Some(racf) = &self.racf else {
            return Ok(ObservationMaintenance::default());
        };
        let expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        let source = self
            .store
            .get_provider_state("racf-database-v2", "authority")
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let descriptors = racf.retention_descriptors()?;
        let descriptor_by_identity = descriptors
            .iter()
            .map(|descriptor| {
                (
                    (descriptor.namespace.as_str(), descriptor.key.as_str()),
                    descriptor,
                )
            })
            .collect::<BTreeMap<_, _>>();
        let current = self.observations(RetentionTarget::RacfEvidence)?;
        let plain = current
            .iter()
            .filter_map(|(_, observation)| {
                descriptor_by_identity
                    .get(&(observation.namespace.as_str(), observation.key.as_str()))
                    .filter(|descriptor| {
                        observation.owner_execution.is_none()
                            && observation.source_version == descriptor.source_version
                            && observation.source_digest == descriptor.source_digest
                    })
                    .map(|_| observation.clone())
            })
            .collect::<Vec<_>>();
        let forecast = racf.retention_forecast_with_observations(
            self.racf_policy(max_records.max(1)),
            now_tick,
            &plain,
        )?;
        let mut maintenance = ObservationMaintenance {
            counts: Some(ObservationCounts {
                examined: forecast
                    .audits
                    .saturating_add(forecast.transactions)
                    .saturating_add(forecast.recovery_records),
                eligible: forecast
                    .eligible_audits
                    .saturating_add(forecast.eligible_transactions)
                    .saturating_add(forecast.eligible_recovery_records),
                watermark_tick: forecast.watermark_tick,
            }),
            ..ObservationMaintenance::default()
        };
        for (version, observation) in &current {
            if maintenance.stale_removed == max_records {
                break;
            }
            let stale = descriptor_by_identity
                .get(&(observation.namespace.as_str(), observation.key.as_str()))
                .is_none_or(|descriptor| {
                    observation.source_version != descriptor.source_version
                        || observation.source_digest != descriptor.source_digest
                        || observation.owner_execution.is_some()
                });
            if !stale {
                continue;
            }
            self.store
                .delete_provider_retention_observation(ProviderRetentionObservationDeletion {
                    expected_epoch,
                    source: ProviderRetentionObservationSource::Present(source.clone()),
                    observation: observation.clone(),
                    expected_observation_version: *version,
                })
                .map_err(store_problem)?;
            maintenance.stale_removed = 1;
            return Ok(maintenance);
        }
        let current_by_identity = current
            .iter()
            .map(|(_, row)| ((row.namespace.as_str(), row.key.as_str()), row))
            .collect::<BTreeMap<_, _>>();
        for descriptor in descriptors
            .iter()
            .filter(|descriptor| descriptor.intrinsic_tick.is_none())
        {
            if maintenance
                .stale_removed
                .saturating_add(maintenance.created)
                >= max_records
            {
                break;
            }
            if current_by_identity
                .get(&(descriptor.namespace.as_str(), descriptor.key.as_str()))
                .is_some_and(|row| {
                    row.source_version == descriptor.source_version
                        && row.source_digest == descriptor.source_digest
                })
            {
                maintenance.reused += 1;
                continue;
            }
            match self.store.record_provider_retention_observation(
                source.clone(),
                expected_epoch,
                RetentionObservation {
                    target: RetentionTarget::RacfEvidence,
                    namespace: descriptor.namespace.clone(),
                    key: descriptor.key.clone(),
                    source_version: descriptor.source_version,
                    source_digest: descriptor.source_digest,
                    observed_tick: now_tick,
                    owner_execution: None,
                },
            ) {
                Ok(_) => {}
                Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge) => break,
                Err(problem) => return Err(store_problem(problem)),
            }
            maintenance.created = 1;
            return Ok(maintenance);
        }
        Ok(maintenance)
    }

    pub(super) fn provider_legacy_rows(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        let observations = self.observations(target)?;
        let observed = observations
            .iter()
            .map(|(_, row)| {
                (
                    (row.namespace.as_str(), row.key.as_str()),
                    (row.source_version, row.source_digest),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let rows = self.bounded_rows(target)?;
        let cics_uow_index = if target == RetentionTarget::CicsUnitOfWork {
            Some(self.cics_uow_index(&rows)?)
        } else {
            None
        };
        let mut result = Vec::new();
        for row in rows {
            let (legacy, digest) = match target {
                RetentionTarget::Db2Replay
                | RetentionTarget::ImsReplay
                | RetentionTarget::MqReplay
                | RetentionTarget::DatasetReplay
                | RetentionTarget::CicsReplay => {
                    let descriptor = self.replay_view(target, &row)?;
                    (
                        descriptor.state == ReplayState::Legacy,
                        descriptor.payload_digest,
                    )
                }
                RetentionTarget::CicsUnitOfWork => {
                    if row.namespace == "cics-uow-undo" {
                        let descriptor = mainframe_env_cics::describe_cics_undo_row(&row)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        (
                            !cics_uow_index
                                .as_ref()
                                .ok_or(HostProblem::InfrastructureFailure)?
                                .blocks_undo(&descriptor.owner_run_unit),
                            descriptor.payload_digest,
                        )
                    } else {
                        let descriptor = describe_cics_uow_row(&row, None)
                            .map_err(|_| HostProblem::InfrastructureFailure)?;
                        (
                            descriptor.dependency == CicsUowDependencyState::LegacyTerminal,
                            descriptor.payload_digest,
                        )
                    }
                }
                RetentionTarget::CobolLifecycle => {
                    let descriptor = describe_cobol_retention_row(&row)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    (
                        descriptor.state == CobolRetentionState::LegacyProtected,
                        payload_digest(&row.payload),
                    )
                }
                RetentionTarget::SpoolJobs => {
                    let descriptor = describe_spool_retention_row(&row, SpoolLimits::default())
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    (
                        descriptor.retention == SpoolRetentionState::LegacyProtected,
                        payload_digest(&row.payload),
                    )
                }
                RetentionTarget::ConsoleLog => {
                    let descriptor = describe_console_log_row(&row)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    (
                        descriptor.retention == ConsoleLogRetentionState::LegacyProtected,
                        descriptor.payload_digest,
                    )
                }
                _ => return Err(HostProblem::Unsupported),
            };
            if legacy
                && observed
                    .get(&(row.namespace.as_str(), row.key.as_str()))
                    .is_none_or(|(version, observed_digest)| {
                        *version != row.version || *observed_digest != digest
                    })
            {
                result.push(RetentionLegacyRow {
                    target,
                    namespace: row.namespace,
                    key: row.key,
                    source_version: row.version,
                });
                if result.len() == max {
                    break;
                }
            }
        }
        Ok(result)
    }

    pub(super) fn racf_legacy_rows(
        &self,
        max: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        let Some(racf) = &self.racf else {
            return Ok(Vec::new());
        };
        let observations = self.observations(RetentionTarget::RacfEvidence)?;
        let observed = observations
            .iter()
            .map(|(_, row)| {
                (
                    (row.namespace.as_str(), row.key.as_str()),
                    (row.source_version, row.source_digest),
                )
            })
            .collect::<BTreeMap<_, _>>();
        Ok(racf
            .retention_descriptors()?
            .into_iter()
            .filter(|descriptor| {
                descriptor.intrinsic_tick.is_none()
                    && observed
                        .get(&(descriptor.namespace.as_str(), descriptor.key.as_str()))
                        .is_none_or(|(version, digest)| {
                            *version != descriptor.source_version
                                || *digest != descriptor.source_digest
                        })
            })
            .take(max)
            .map(|descriptor| RetentionLegacyRow {
                target: RetentionTarget::RacfEvidence,
                namespace: descriptor.namespace,
                key: descriptor.key,
                source_version: descriptor.source_version,
            })
            .collect())
    }

    pub(super) fn reconcile_provider(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        if matches!(
            request.target,
            RetentionTarget::Db2Replay
                | RetentionTarget::ImsReplay
                | RetentionTarget::MqReplay
                | RetentionTarget::DatasetReplay
                | RetentionTarget::CicsReplay
        ) {
            let namespace = replay_namespace(request.target).ok_or(HostProblem::Malformed)?;
            if request.namespace != namespace {
                return Err(HostProblem::Malformed);
            }
            let owner = request
                .owner_execution
                .clone()
                .ok_or(HostProblem::Malformed)?;
            let (expected_epoch, row) = self.exact_reconciliation_row(&request)?;
            let descriptor = self.replay_view(request.target, &row)?;
            if descriptor.state != ReplayState::Legacy
                || self
                    .valid_legacy_effect(
                        &row,
                        &owner,
                        descriptor.request_digest,
                        descriptor.result_digest,
                    )?
                    .is_none()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            return self.record_observation(
                row,
                expected_epoch,
                request,
                descriptor.payload_digest,
                now_tick,
                Some(owner),
            );
        }
        if matches!(
            request.target,
            RetentionTarget::CicsUnitOfWork | RetentionTarget::CobolLifecycle
        ) {
            let owner = request
                .owner_execution
                .clone()
                .ok_or(HostProblem::Malformed)?;
            if request.target == RetentionTarget::CicsUnitOfWork
                && !matches!(request.namespace.as_str(), "cics-uow" | "cics-uow-undo")
                || request.target == RetentionTarget::CobolLifecycle
                    && !cobol_namespace(&request.namespace)
            {
                return Err(HostProblem::Malformed);
            }
            let (expected_epoch, row) = self.exact_reconciliation_row(&request)?;
            let digest = if request.target == RetentionTarget::CicsUnitOfWork {
                if row.namespace == "cics-uow-undo" {
                    let descriptor = mainframe_env_cics::describe_cics_undo_row(&row)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    let terminal_owner = self
                        .store
                        .get_execution(&owner)
                        .map_err(store_problem)?
                        .is_some_and(|execution| {
                            execution.state.terminal()
                                && execution.run_unit_id.as_str() == descriptor.owner_run_unit
                        });
                    if !terminal_owner
                        || self.cics_uow_exists_for_run(&descriptor.owner_run_unit)?
                    {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    descriptor.payload_digest
                } else {
                    let descriptor = describe_cics_uow_row(&row, None)
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                    if descriptor.dependency != CicsUowDependencyState::LegacyTerminal
                        || self.terminal_cics_effect(&row.key, Some(&owner))?.is_none()
                    {
                        return Err(HostProblem::IdempotencyConflict);
                    }
                    descriptor.payload_digest
                }
            } else {
                let descriptor = describe_cobol_retention_row(&row)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                let terminal_owner = self.store.get_execution(&owner).map_err(store_problem)?;
                if descriptor.state != CobolRetentionState::LegacyProtected
                    || terminal_owner
                        .as_ref()
                        .is_none_or(|execution| !execution.state.terminal())
                    || !self.legacy_cobol_owner_is_clear(
                        &row,
                        terminal_owner
                            .as_ref()
                            .ok_or(HostProblem::IdempotencyConflict)?,
                    )?
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                payload_digest(&row.payload)
            };
            return self.record_observation(
                row,
                expected_epoch,
                request,
                digest,
                now_tick,
                Some(owner),
            );
        }
        if matches!(
            request.target,
            RetentionTarget::SpoolJobs | RetentionTarget::ConsoleLog
        ) {
            let namespace = if request.target == RetentionTarget::SpoolJobs {
                "jes-spool"
            } else {
                "console-log"
            };
            if request.namespace != namespace || request.owner_execution.is_some() {
                return Err(HostProblem::Malformed);
            }
            let (expected_epoch, row) = self.exact_reconciliation_row(&request)?;
            let digest = if request.target == RetentionTarget::SpoolJobs {
                let descriptor = describe_spool_retention_row(&row, SpoolLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if descriptor.retention != SpoolRetentionState::LegacyProtected {
                    return Err(HostProblem::IdempotencyConflict);
                }
                payload_digest(&row.payload)
            } else {
                let descriptor = describe_console_log_row(&row)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
                if descriptor.retention != ConsoleLogRetentionState::LegacyProtected {
                    return Err(HostProblem::IdempotencyConflict);
                }
                descriptor.payload_digest
            };
            return self.record_observation(row, expected_epoch, request, digest, now_tick, None);
        }
        Err(HostProblem::Unsupported)
    }

    pub(super) fn reconcile_racf(
        &self,
        request: RetentionAgeReconciliation,
        now_tick: u64,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        if request.owner_execution.is_some() {
            return Err(HostProblem::Malformed);
        }
        let Some(racf) = &self.racf else {
            return Err(HostProblem::NotFound);
        };
        let expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        let descriptor = racf
            .retention_descriptors()?
            .into_iter()
            .find(|descriptor| {
                descriptor.namespace == request.namespace && descriptor.key == request.key
            })
            .ok_or(HostProblem::NotFound)?;
        if descriptor.source_version != request.expected_version
            || descriptor.intrinsic_tick.is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let source = self
            .store
            .get_provider_state("racf-database-v2", "authority")
            .map_err(store_problem)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?
            != expected_epoch
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.record_observation(
            source,
            expected_epoch,
            request,
            descriptor.source_digest,
            now_tick,
            None,
        )
    }

    fn exact_reconciliation_row(
        &self,
        request: &RetentionAgeReconciliation,
    ) -> Result<(u64, ProviderStateRecord), HostProblem> {
        let expected_epoch = self
            .store
            .provider_state_retention_epoch()
            .map_err(store_problem)?;
        let row = self
            .store
            .get_provider_state(&request.namespace, &request.key)
            .map_err(store_problem)?
            .ok_or(HostProblem::NotFound)?;
        if row.version != request.expected_version {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok((expected_epoch, row))
    }

    #[allow(clippy::too_many_arguments)]
    fn record_observation(
        &self,
        source: ProviderStateRecord,
        expected_epoch: u64,
        request: RetentionAgeReconciliation,
        source_digest: [u8; 32],
        now_tick: u64,
        owner_execution: Option<ExecutionId>,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        self.store
            .record_provider_retention_observation(
                source,
                expected_epoch,
                RetentionObservation {
                    target: request.target,
                    namespace: request.namespace,
                    key: request.key,
                    source_version: request.expected_version,
                    source_digest,
                    observed_tick: now_tick,
                    owner_execution,
                },
            )
            .map_err(store_problem)
    }
}
