//! Provider-state trait routing retains the existing sqlite authority.
use super::*;

impl ProviderStateStore for SqliteStateStore {
    crate::checked_read::checked_read_methods!();
    fn advance_logical_clock(&self, observed_floor: u64) -> Result<u64, StoreError> {
        let observed_floor =
            i64::try_from(observed_floor.max(1)).map_err(|_| StoreError::CapacityExceeded)?;
        let tick: Option<i64> = self.run(
            sqlx::query_scalar(
                "UPDATE retention_lock SET clock_tick=MAX(clock_tick,?) \
                 WHERE singleton=1 RETURNING clock_tick",
            )
            .bind(observed_floor)
            .fetch_optional(&self.pool),
        )?;
        let tick = tick.ok_or(StoreError::CapacityExceeded)?;
        u64::try_from(tick).map_err(|_| StoreError::IncompatibleVersion)
    }

    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        let row = self.run(
            sqlx::query("SELECT version,payload FROM provider_state WHERE namespace=? AND key=?")
                .bind(namespace)
                .bind(key)
                .fetch_optional(&self.pool),
        )?;
        row.map(|row| {
            let version: i64 = row
                .try_get(0)
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            let payload: Vec<u8> = row
                .try_get(1)
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            Ok(ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version: u64::try_from(version).map_err(|_| StoreError::IncompatibleVersion)?,
                payload,
            })
        })
        .transpose()
    }

    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.legacy_provider_page(namespace, max)
    }

    fn list_provider_state_bounded(
        &self,
        namespace: &str,
        max: usize,
        max_bytes: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.bounded_provider_page(namespace, max, max_bytes)
    }

    fn list_provider_state_prefix(
        &self,
        namespace_prefix: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if namespace_prefix.is_empty()
            || namespace_prefix.len() > mainframe_env_store_api::MAX_PROVIDER_NAMESPACE_BYTES
            || max == 0
            || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN
        {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = self.run(
            sqlx::query(
                "SELECT namespace,key,version,payload FROM provider_state \
                 WHERE substr(namespace,1,length(?))=? ORDER BY namespace,key LIMIT ?",
            )
            .bind(namespace_prefix)
            .bind(namespace_prefix)
            .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
            .fetch_all(&self.pool),
        )?;
        rows.into_iter()
            .map(|row| {
                Ok(ProviderStateRecord {
                    namespace: row.try_get(0).map_err(infrastructure)?,
                    key: row.try_get(1).map_err(infrastructure)?,
                    version: u64::try_from(row.try_get::<i64, _>(2).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    payload: row.try_get(3).map_err(infrastructure)?,
                })
            })
            .collect()
    }

    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        record.validate_write(self.max_payload_bytes)?;
        self.mutate_provider_rows(vec![ProviderStateMutation::Put(ProviderStateWrite {
            record,
            expected_version: expected,
        })])
    }

    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        self.mutate_provider_rows(vec![ProviderStateMutation::Delete {
            namespace: namespace.into(),
            key: key.into(),
            expected_version: expected,
        }])
    }

    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        record.validate_move(old_key, expected_version, self.max_payload_bytes)?;
        self.mutate_provider_rows(vec![ProviderStateMutation::Move {
            record,
            old_key: old_key.into(),
            expected_version,
        }])
    }

    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        self.mutate_provider_rows(writes.into_iter().map(ProviderStateMutation::Put).collect())
    }

    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.mutate_provider_rows(mutations)
    }

    fn archive_provider_state_replacement(
        &self,
        request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        self.commit_provider_retention_replacement(request)
    }

    fn provider_retention_archive_usage(
        &self,
        target: RetentionTarget,
    ) -> Result<(usize, u64), StoreError> {
        self.provider_archive_usage(target)
    }

    fn provider_retention_authority_usage(
        &self,
        target: RetentionTarget,
    ) -> Result<mainframe_env_store_api::RetentionAuthorityUsage, StoreError> {
        let (archive_rows, archive_bytes) = self.provider_archive_usage(target)?;
        let (shared_archive_rows, shared_archive_bytes) = self.retention_archive_usage()?;
        let (shared_observation_rows, shared_observation_bytes) =
            self.retention_observation_usage()?;
        let (observation_rows, observation_bytes): (i64, i64) = self.run(
            sqlx::query_as(
                "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0) FROM retention_observation WHERE target=?",
            )
            .bind(crate::retention::target_name(target))
            .fetch_one(&self.pool),
        )?;
        Ok(mainframe_env_store_api::RetentionAuthorityUsage {
            archive_rows,
            shared_archive_rows,
            archive_row_capacity: self.max_archive_rows,
            archive_bytes,
            shared_archive_bytes,
            archive_byte_capacity: self.max_archive_bytes,
            observation_rows: usize::try_from(observation_rows)
                .map_err(|_| StoreError::IncompatibleVersion)?,
            shared_observation_rows,
            observation_row_capacity: self.max_archive_rows,
            observation_bytes: u64::try_from(observation_bytes)
                .map_err(|_| StoreError::IncompatibleVersion)?,
            shared_observation_bytes,
            observation_byte_capacity: self.max_archive_bytes,
        })
    }

    fn provider_state_retention_epoch(&self) -> Result<u64, StoreError> {
        self.retention_epoch()
    }

    fn archive_provider_state_deletion(
        &self,
        request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        self.commit_provider_retention_deletion(request, None)
    }

    fn archive_provider_state_deletion_with_capacity(
        &self,
        request: ProviderStateArchiveDeletionWithCapacity,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_container_replay_archive(&request, self.max_payload_bytes)?;
        self.commit_provider_retention_deletion(
            request.deletion,
            Some((
                request.outer_receipts,
                request.capacity_source,
                request.capacity_replacement,
            )),
        )
    }

    fn provider_retention_observations(
        &self,
        target: RetentionTarget,
        max: usize,
    ) -> Result<Vec<RetentionObservation>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(self
            .load_retention_observations(target)?
            .into_iter()
            .map(|(_, observation)| observation)
            .take(max)
            .collect())
    }

    fn provider_retention_observation_page(
        &self,
        target: RetentionTarget,
        after: Option<&mainframe_env_store_api::ProviderStateIdentity>,
        max: usize,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_RETENTION_BATCH {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = match after {
            Some(after) => self.run(
                sqlx::query(
                    "SELECT namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution \
                     FROM retention_observation WHERE target=? AND (namespace>? OR (namespace=? AND key>?)) \
                     ORDER BY namespace,key LIMIT ?",
                )
                .bind(crate::retention::target_name(target))
                .bind(&after.namespace)
                .bind(&after.namespace)
                .bind(&after.key)
                .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
                .fetch_all(&self.pool),
            )?,
            None => self.run(
                sqlx::query(
                    "SELECT namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution \
                     FROM retention_observation WHERE target=? ORDER BY namespace,key LIMIT ?",
                )
                .bind(crate::retention::target_name(target))
                .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
                .fetch_all(&self.pool),
            )?,
        };
        rows.into_iter()
            .map(|row| {
                let digest: Vec<u8> = row.try_get(4).map_err(infrastructure)?;
                let observation = RetentionObservation {
                    target,
                    namespace: row.try_get(0).map_err(infrastructure)?,
                    key: row.try_get(1).map_err(infrastructure)?,
                    source_version: u64::try_from(
                        row.try_get::<i64, _>(3).map_err(infrastructure)?,
                    )
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                    source_digest: digest
                        .try_into()
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    observed_tick: u64::try_from(row.try_get::<i64, _>(5).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    owner_execution: row
                        .try_get::<Option<String>, _>(6)
                        .map_err(infrastructure)?
                        .map(|owner| {
                            mainframe_env_execution_api::ExecutionId::new(
                                owner,
                                mainframe_env_execution_api::InvocationLimits::default(),
                            )
                            .map_err(|_| StoreError::IncompatibleVersion)
                        })
                        .transpose()?,
                };
                crate::retention::observation::validate(&observation)?;
                Ok((
                    u64::try_from(row.try_get::<i64, _>(2).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    observation,
                ))
            })
            .collect()
    }

    fn delete_provider_retention_observation(
        &self,
        request: ProviderRetentionObservationDeletion,
    ) -> Result<(), StoreError> {
        let observation = request.observation;
        crate::retention::observation::validate(&observation)?;
        if request.expected_observation_version == 0 {
            return Err(StoreError::Conflict);
        }
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            sqlx::query("UPDATE retention_lock SET epoch=epoch WHERE singleton=1")
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
            let epoch: i64 =
                sqlx::query_scalar("SELECT epoch FROM retention_lock WHERE singleton=1")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
            if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)?
                != request.expected_epoch
            {
                return Err(StoreError::Conflict);
            }
            let (namespace, key, expected) = match request.source {
                ProviderRetentionObservationSource::Present(source) => (
                    source.namespace,
                    source.key,
                    Some((source.version, source.payload)),
                ),
                ProviderRetentionObservationSource::Absent(identity) => {
                    (identity.namespace, identity.key, None)
                }
            };
            let current = sqlx::query(
                "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
            )
            .bind(namespace)
            .bind(key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(infrastructure)?;
            match (current, expected) {
                (None, None) => {}
                (Some(current), Some((version, payload)))
                    if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?
                        == version
                        && current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? == payload => {
                }
                _ => return Err(StoreError::Conflict),
            }
            let deleted = sqlx::query(
                    "DELETE FROM retention_observation WHERE target=? AND namespace=? AND key=? AND observation_version=?",
                )
                .bind(crate::retention::target_name(observation.target))
                .bind(observation.namespace)
                .bind(observation.key)
                .bind(
                    i64::try_from(request.expected_observation_version)
                        .map_err(|_| StoreError::Conflict)?,
                )
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
            if deleted != 1 {
                return Err(StoreError::Conflict);
            }
            sqlx::query("UPDATE retention_lock SET epoch=epoch+1 WHERE singleton=1")
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
            transaction.commit().await.map_err(infrastructure)
        })?
    }

    fn record_provider_retention_observation(
        &self,
        source: ProviderStateRecord,
        expected_epoch: u64,
        observation: RetentionObservation,
    ) -> Result<mainframe_env_store_api::RetentionReconciliationReceipt, StoreError> {
        let target = observation.target;
        let namespace = observation.namespace.clone();
        let key = observation.key.clone();
        let source_version = observation.source_version;
        let (observation_version, reconciled_tick) =
            self.commit_retention_observation(observation, source, expected_epoch, true)?;
        Ok(mainframe_env_store_api::RetentionReconciliationReceipt {
            target,
            namespace,
            key,
            source_version,
            observation_version,
            reconciled_tick,
        })
    }
}
