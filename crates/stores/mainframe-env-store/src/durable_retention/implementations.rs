use super::*;

macro_rules! durable_retention {
    ($store:ty) => {
        impl RetentionStore for $store {
            fn retention_capacity_health(
                &self,
                policy: RetentionPolicy,
            ) -> Result<mainframe_env_store_api::RetentionCapacityHealth, StoreError> {
                let provider_rows = self.provider_state_usage()?;
                let source_usage = RetentionTarget::ALL
                    .into_iter()
                    .map(|target| (target, provider_rows, self.max_rows()))
                    .collect();
                let (archive_rows, archive_bytes) = self.retention_archive_usage()?;
                let (observation_rows, observation_bytes) = self.retention_observation_usage()?;
                crate::retention::capacity_health(
                    policy,
                    source_usage,
                    archive_rows,
                    self.max_archive_rows(),
                    archive_bytes,
                    self.max_archive_bytes(),
                    observation_rows,
                    self.max_archive_rows(),
                    observation_bytes,
                    self.max_archive_bytes(),
                )
            }

            fn retention_forecast(
                &self,
                target: RetentionTarget,
                policy: RetentionPolicy,
                now_tick: u64,
                observed_growth_per_tick: u64,
            ) -> Result<RetentionForecast, StoreError> {
                if crate::retention::dependency_sensitive_core_target(target)
                    || crate::retention::provider_owned_target(target)
                {
                    return Err(StoreError::InvalidTransition);
                }
                durable_forecast(
                    self,
                    self.max_rows(),
                    target,
                    policy,
                    now_tick,
                    observed_growth_per_tick,
                    None,
                )
            }

            fn retention_forecast_with_dependencies(
                &self,
                target: RetentionTarget,
                policy: RetentionPolicy,
                now_tick: u64,
                observed_growth_per_tick: u64,
                dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot,
            ) -> Result<RetentionForecast, StoreError> {
                durable_forecast(
                    self,
                    self.max_rows(),
                    target,
                    policy,
                    now_tick,
                    observed_growth_per_tick,
                    Some(dependencies),
                )
            }

            fn provider_validated_retention_forecast(
                &self,
                target: RetentionTarget,
                policy: RetentionPolicy,
                now_tick: u64,
                observed_growth_per_tick: u64,
                active_records: usize,
                eligible_records: usize,
                source_capacity: usize,
            ) -> Result<RetentionForecast, StoreError> {
                if source_capacity == 0 || active_records > source_capacity {
                    return Err(StoreError::IncompatibleVersion);
                }
                let provider_used = self.provider_state_usage()?;
                let shared_headroom = self.max_rows().saturating_sub(provider_used);
                let local_headroom = source_capacity.saturating_sub(active_records);
                let effective_capacity = active_records
                    .checked_add(shared_headroom.min(local_headroom))
                    .ok_or(StoreError::CapacityExceeded)?;
                let (archive_records, archive_bytes) = self.retention_archive_usage()?;
                let (observation_records, observation_bytes) =
                    self.retention_observation_usage()?;
                let mut result = forecast(
                    target,
                    policy,
                    now_tick,
                    observed_growth_per_tick,
                    ForecastCounts {
                        active: active_records,
                        eligible: eligible_records,
                        capacity: effective_capacity,
                        total_used: active_records,
                        archive_records,
                        archive_capacity: self.max_archive_rows(),
                        archive_total_used: archive_records,
                        archive_bytes,
                        archive_byte_capacity: self.max_archive_bytes(),
                        observation_records,
                        observation_capacity: self.max_archive_rows(),
                        observation_bytes,
                        observation_byte_capacity: self.max_archive_bytes(),
                        max_source_storage_bytes:
                            crate::retention::worst_case_archived_row_storage_bytes(
                                self.max_payload_bytes(),
                            ),
                    },
                )?;
                result.saturation = result.saturation.max(crate::retention::saturation(
                    self.max_rows(),
                    provider_used,
                    policy.low_watermark_percent,
                    policy.high_watermark_percent,
                ));
                Ok(result)
            }

            fn archive_and_prune(
                &self,
                policy: RetentionPolicy,
                request: RetentionRequest,
            ) -> Result<RetentionReceipt, StoreError> {
                if crate::retention::dependency_sensitive_core_target(request.target)
                    || crate::retention::provider_owned_target(request.target)
                {
                    return Err(StoreError::InvalidTransition);
                }
                durable_archive_and_prune(self, self.max_rows(), policy, request, None)
            }

            fn archive_and_prune_with_dependencies(
                &self,
                policy: RetentionPolicy,
                request: RetentionRequest,
                dependencies: &mainframe_env_store_api::CoreRetentionDependencySnapshot,
            ) -> Result<RetentionReceipt, StoreError> {
                durable_archive_and_prune(
                    self,
                    self.max_rows(),
                    policy,
                    request,
                    Some(dependencies),
                )
            }

            fn retention_archives(
                &self,
                target: RetentionTarget,
                max: usize,
            ) -> Result<Vec<RetentionArchive>, StoreError> {
                durable_archives(self, target, max)
            }

            fn prune_retention_archives_authorized(
                &self,
                policy: RetentionPolicy,
                request: mainframe_env_store_api::RetentionArchivePruneRequest,
            ) -> Result<mainframe_env_store_api::RetentionArchivePruneOutcome, StoreError> {
                durable_prune_archives(self, policy, request)
            }

            fn reconcile_retention_age(
                &self,
                request: RetentionAgeReconciliation,
                now_tick: u64,
            ) -> Result<RetentionReconciliationReceipt, StoreError> {
                durable_reconcile_retention_age(self, request, now_tick)
            }

            fn retention_legacy_rows(
                &self,
                target: RetentionTarget,
                max: usize,
            ) -> Result<Vec<RetentionLegacyRow>, StoreError> {
                durable_legacy_rows(self, self.max_rows(), target, max)
            }
        }
    };
}

durable_retention!(SqliteStateStore);
durable_retention!(PostgresStateStore);
