use crate::runtime::{AdapterRuntime, block_on};
use mainframe_env_store_api::{
    ArchivedRetentionRow, ProviderRetentionDependency, ProviderRetentionObservationDeletion,
    ProviderRetentionObservationSource, ProviderRetentionRow, ProviderStateArchiveDeletion,
    ProviderStateArchiveReplacement, ProviderStateMutation, ProviderStateRecord,
    ProviderStateStore, ProviderStateWrite, RetentionArchive, RetentionObservation,
    RetentionTarget, StoreError,
};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use std::future::Future;
use tokio::runtime::Builder;

mod retention_read;

fn derived_archive_bytes(max_payload_bytes: usize, max_rows: usize) -> u64 {
    u64::try_from(max_payload_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(crate::retention::RETENTION_ROW_FIXED_BYTES)
        .saturating_add(crate::retention::RETENTION_ARCHIVE_FIXED_BYTES)
        .saturating_add(
            u64::try_from(
                mainframe_env_store_api::MAX_PROVIDER_NAMESPACE_BYTES
                    + mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES
                    + mainframe_env_execution_api::InvocationLimits::default().max_identity_bytes,
            )
            .unwrap_or(u64::MAX),
        )
        .saturating_mul(u64::try_from(max_rows).unwrap_or(u64::MAX))
}

pub(crate) const PROVIDER_STATE_QUOTA: &str = "provider-state";
pub(crate) const ARTIFACT_OBJECT_QUOTA: &str = "artifact-object";
pub(crate) const RETENTION_ARCHIVE_QUOTA: &str = "retention-archive-row";
pub(crate) const RETENTION_OBSERVATION_QUOTA: &str = "retention-observation-row";

pub struct PostgresStateStore {
    runtime: AdapterRuntime,
    pool: PgPool,
    max_payload_bytes: usize,
    max_rows: usize,
    max_archive_rows: usize,
    max_archive_bytes: u64,
}

impl PostgresStateStore {
    pub fn open(url: &str, max_payload_bytes: usize, max_rows: usize) -> Result<Self, StoreError> {
        Self::open_with_retention_limits(
            url,
            max_payload_bytes,
            max_rows,
            max_rows,
            derived_archive_bytes(max_payload_bytes, max_rows),
        )
    }

    /// Open PostgreSQL with independently bounded retention archive authority.
    pub fn open_with_retention_limits(
        url: &str,
        max_payload_bytes: usize,
        max_rows: usize,
        max_archive_rows: usize,
        max_archive_bytes: u64,
    ) -> Result<Self, StoreError> {
        if max_payload_bytes == 0
            || max_rows == 0
            || max_archive_rows == 0
            || max_archive_bytes == 0
        {
            return Err(StoreError::CapacityExceeded);
        }
        let runtime = AdapterRuntime::new(
            Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
        );
        let pool = block_on(
            &runtime,
            PgPoolOptions::new().max_connections(4).connect(url),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        block_on(
            &runtime,
            sqlx::raw_sql(include_str!(
                "../migrations/postgres/0001-durable-state.sql"
            ))
            .execute(&pool),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        block_on(
            &runtime,
            sqlx::raw_sql(include_str!(
                "../migrations/postgres/0002-retention-lifecycle.sql"
            ))
            .execute(&pool),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        block_on(&runtime, async {
            initialize_quota(
                &pool,
                PROVIDER_STATE_QUOTA,
                max_rows,
                "SELECT COUNT(*) FROM provider_state",
            )
            .await
        })??;
        block_on(&runtime, async {
            initialize_quota(
                &pool,
                RETENTION_OBSERVATION_QUOTA,
                max_archive_rows,
                "SELECT COUNT(*) FROM retention_observation",
            )
            .await
        })??;
        block_on(&runtime, async {
            migrate_legacy_logical_clock(&pool, max_rows).await
        })??;
        block_on(&runtime, async {
            initialize_quota(
                &pool,
                RETENTION_ARCHIVE_QUOTA,
                max_archive_rows,
                "SELECT COUNT(*) FROM retention_archive_row",
            )
            .await
        })??;
        let store = Self {
            runtime,
            pool,
            max_payload_bytes,
            max_rows,
            max_archive_rows,
            max_archive_bytes,
        };
        store.verify_retention_authorities()?;
        Ok(store)
    }

    fn run<F, T>(&self, future: F) -> Result<T, StoreError>
    where
        F: Future<Output = Result<T, sqlx::Error>> + Send,
        T: Send,
    {
        block_on(&self.runtime, future)?.map_err(infrastructure)
    }

    pub(crate) const fn max_rows(&self) -> usize {
        self.max_rows
    }

    pub(crate) fn retention_writable_probe(&self) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = prove_provider_state_writable(&mut transaction, self.max_rows).await;
            transaction.rollback().await.map_err(infrastructure)?;
            outcome
        })?
    }

    pub(crate) fn provider_state_usage(&self) -> Result<usize, StoreError> {
        let used: i64 = self.run(
            sqlx::query_scalar("SELECT used_rows FROM store_quota WHERE quota_key=$1")
                .bind(PROVIDER_STATE_QUOTA)
                .fetch_one(&self.pool),
        )?;
        usize::try_from(used).map_err(|_| StoreError::IncompatibleVersion)
    }

    pub(crate) const fn max_payload_bytes(&self) -> usize {
        self.max_payload_bytes
    }

    pub(crate) const fn max_archive_rows(&self) -> usize {
        self.max_archive_rows
    }

    pub(crate) fn max_archive_bytes(&self) -> u64 {
        self.max_archive_bytes
    }

    pub(crate) fn retention_epoch(&self) -> Result<u64, StoreError> {
        let epoch: i64 = self.run(
            sqlx::query_scalar("SELECT epoch FROM retention_lock WHERE singleton=1")
                .fetch_one(&self.pool),
        )?;
        u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)
    }

    pub(crate) fn retention_archive_usage(&self) -> Result<(usize, u64), StoreError> {
        let (rows, bytes): (i64, i64) = self.run(
            sqlx::query_as(
                "SELECT COALESCE(SUM(row_count),0)::BIGINT,COALESCE(SUM(payload_bytes),0)::BIGINT FROM retention_archive",
            )
            .fetch_one(&self.pool),
        )?;
        Ok((
            usize::try_from(rows).map_err(|_| StoreError::IncompatibleVersion)?,
            u64::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }

    pub(crate) fn retention_observation_usage(&self) -> Result<(usize, u64), StoreError> {
        let (rows, bytes): (i64, i64) = self.run(
            sqlx::query_as(
                "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0)::BIGINT FROM retention_observation",
            )
            .fetch_one(&self.pool),
        )?;
        Ok((
            usize::try_from(rows).map_err(|_| StoreError::IncompatibleVersion)?,
            u64::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }

    fn verify_retention_authorities(&self) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                sqlx::query("SELECT epoch FROM retention_lock WHERE singleton=1 FOR UPDATE")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                sqlx::query("LOCK TABLE retention_archive,retention_archive_row IN SHARE MODE")
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                self.verify_retention_authorities_in(&mut transaction).await
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }

    async fn verify_retention_authorities_in(
        &self,
        transaction: &mut Transaction<'_, Postgres>,
    ) -> Result<(), StoreError> {
        let archives = retention_read::load_archives(
            &mut *transaction,
            None,
            self.max_archive_rows,
            None,
            self.max_archive_rows,
        )
        .await?;
        let archive_rows = archives.iter().try_fold(0usize, |total, archive| {
            total
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)
        })?;
        let archive_bytes = archives.iter().try_fold(0u64, |total, archive| {
            total
                .checked_add(crate::retention::archive_storage_bytes(&archive.rows)?)
                .ok_or(StoreError::CapacityExceeded)
        })?;
        if retention_archive_usage_in(&mut *transaction).await? != (archive_rows, archive_bytes)
            || archive_rows > self.max_archive_rows
            || archive_bytes > self.max_archive_bytes
        {
            return Err(StoreError::IncompatibleVersion);
        }
        let mut observation_rows = 0usize;
        let mut observation_bytes = 0u64;
        for target in RetentionTarget::ALL {
            for (_, observation) in self
                .load_retention_observations_in(&mut *transaction, target)
                .await?
            {
                observation_rows = observation_rows
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?;
                observation_bytes = observation_bytes
                    .checked_add(crate::retention::observation::storage_bytes(&observation)?)
                    .ok_or(StoreError::CapacityExceeded)?;
            }
        }
        if retention_observation_usage_in(&mut *transaction).await?
            != (observation_rows, observation_bytes)
            || observation_rows > self.max_archive_rows
            || observation_bytes > self.max_archive_bytes
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(())
    }

    pub(crate) fn load_retention_observations(
        &self,
        target: RetentionTarget,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        block_on(&self.runtime, async {
            let mut connection = self.pool.acquire().await.map_err(infrastructure)?;
            self.load_retention_observations_in(&mut connection, target)
                .await
        })?
    }

    async fn load_retention_observations_in(
        &self,
        connection: &mut PgConnection,
        target: RetentionTarget,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        let limit = self
            .max_archive_rows
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let rows = sqlx::query(
            "SELECT namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution,accounted_bytes \
             FROM retention_observation WHERE target=$1 ORDER BY namespace,key LIMIT $2",
        )
        .bind(crate::retention::target_name(target))
        .bind(i64::try_from(limit).map_err(|_| StoreError::CapacityExceeded)?)
        .fetch_all(&mut *connection)
        .await
        .map_err(infrastructure)?;
        if rows.len() > self.max_archive_rows {
            return Err(StoreError::IncompatibleVersion);
        }
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
                let accounted = u64::try_from(row.try_get::<i64, _>(7).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                if accounted != crate::retention::observation::storage_bytes(&observation)? {
                    return Err(StoreError::IncompatibleVersion);
                }
                Ok((
                    u64::try_from(row.try_get::<i64, _>(2).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    observation,
                ))
            })
            .collect()
    }

    pub(crate) fn commit_retention_observation(
        &self,
        observation: RetentionObservation,
        source: ProviderStateRecord,
        expected_epoch: u64,
        embedded: bool,
    ) -> Result<(u64, u64), StoreError> {
        crate::retention::observation::validate(&observation)?;
        if !embedded
            && (observation.namespace != source.namespace
                || observation.key != source.key
                || observation.source_version != source.version
                || observation.source_digest != crate::retention::source_digest(&source.payload))
        {
            return Err(StoreError::Conflict);
        }
        let accounted_bytes = crate::retention::observation::storage_bytes(&observation)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let epoch: i64 = sqlx::query_scalar(
                    "SELECT epoch FROM retention_lock WHERE singleton=1 FOR UPDATE",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)?
                    != expected_epoch
                {
                    return Err(StoreError::Conflict);
                }
                let current_source = sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(&source.namespace)
                .bind(&source.key)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::Conflict)?;
                if u64::try_from(current_source.try_get::<i64, _>(0).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    != source.version
                    || current_source.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?
                        != source.payload
                {
                    return Err(StoreError::Conflict);
                }
                let current = sqlx::query(
                    "SELECT observation_version,source_version,source_digest,observed_tick,owner_execution,accounted_bytes \
                     FROM retention_observation WHERE target=$1 AND namespace=$2 AND key=$3 FOR UPDATE",
                )
                .bind(crate::retention::target_name(observation.target))
                .bind(&observation.namespace)
                .bind(&observation.key)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if let Some(current) = current.as_ref() {
                    let same = current.try_get::<i64, _>(1).map_err(infrastructure)?
                        == i64::try_from(observation.source_version)
                            .map_err(|_| StoreError::Conflict)?
                        && current.try_get::<Vec<u8>, _>(2).map_err(infrastructure)?
                            == observation.source_digest
                        && current.try_get::<Option<String>, _>(4).map_err(infrastructure)?
                            == observation
                                .owner_execution
                                .as_ref()
                                .map(|owner| owner.as_str().to_string());
                    if same {
                        return Ok((
                            u64::try_from(
                                current.try_get::<i64, _>(0).map_err(infrastructure)?,
                            )
                            .map_err(|_| StoreError::IncompatibleVersion)?,
                            u64::try_from(
                                current.try_get::<i64, _>(3).map_err(infrastructure)?,
                            )
                            .map_err(|_| StoreError::IncompatibleVersion)?,
                        ));
                    }
                }
                if current.is_none() {
                    adjust_quota(
                        &mut transaction,
                        RETENTION_OBSERVATION_QUOTA,
                        self.max_archive_rows,
                        1,
                    )
                    .await?;
                }
                let used_bytes: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(accounted_bytes),0)::BIGINT FROM retention_observation",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                let old_bytes = current
                    .as_ref()
                    .map(|row| row.try_get::<i64, _>(5).map_err(infrastructure))
                    .transpose()?
                    .unwrap_or(0);
                let next_bytes = u64::try_from(used_bytes.saturating_sub(old_bytes))
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    .checked_add(accounted_bytes)
                    .ok_or(StoreError::CapacityExceeded)?;
                if next_bytes > self.max_archive_bytes() {
                    return Err(StoreError::CapacityExceeded);
                }
                let next_version = current
                    .as_ref()
                    .map(|row| row.try_get::<i64, _>(0).map_err(infrastructure))
                    .transpose()?
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?;
                sqlx::query(
                    "INSERT INTO retention_observation(target,namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution,accounted_bytes) \
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(target,namespace,key) DO UPDATE SET \
                     observation_version=excluded.observation_version,source_version=excluded.source_version,source_digest=excluded.source_digest,observed_tick=excluded.observed_tick,owner_execution=excluded.owner_execution,accounted_bytes=excluded.accounted_bytes",
                )
                .bind(crate::retention::target_name(observation.target))
                .bind(observation.namespace)
                .bind(observation.key)
                .bind(next_version)
                .bind(i64::try_from(observation.source_version).map_err(|_| StoreError::Conflict)?)
                .bind(observation.source_digest.to_vec())
                .bind(i64::try_from(observation.observed_tick).map_err(|_| StoreError::Conflict)?)
                .bind(observation.owner_execution.map(|owner| owner.as_str().to_string()))
                .bind(i64::try_from(accounted_bytes).map_err(|_| StoreError::CapacityExceeded)?)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                sqlx::query("UPDATE retention_lock SET epoch=epoch+1 WHERE singleton=1")
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                Ok((
                    u64::try_from(next_version).map_err(|_| StoreError::IncompatibleVersion)?,
                    observation.observed_tick,
                ))
            }
            .await;
            match outcome {
                Ok(version) => {
                    transaction.commit().await.map_err(infrastructure)?;
                    Ok(version)
                }
                Err(problem) => {
                    transaction.rollback().await.map_err(infrastructure)?;
                    Err(problem)
                }
            }
        })?
    }

    pub(crate) fn commit_retention_archive(
        &self,
        archive: &RetentionArchive,
        expected_epoch: u64,
    ) -> Result<(), StoreError> {
        let payload_bytes = crate::retention::archive_storage_bytes(&archive.rows)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(
                    &mut transaction,
                    PROVIDER_STATE_QUOTA,
                    self.max_rows,
                    -i64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?,
                )
                .await?;
                adjust_quota(
                    &mut transaction,
                    RETENTION_ARCHIVE_QUOTA,
                    self.max_archive_rows,
                    i64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?,
                )
                .await?;
                let epoch: i64 = sqlx::query_scalar(
                    "SELECT epoch FROM retention_lock WHERE singleton=1 FOR UPDATE",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)?
                    != expected_epoch
                {
                    return Err(StoreError::Conflict);
                }
                sqlx::query("LOCK TABLE retention_archive,retention_archive_row IN SHARE ROW EXCLUSIVE MODE")
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                let used_bytes: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(payload_bytes),0)::BIGINT FROM retention_archive",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                let next_bytes = u64::try_from(used_bytes)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    .checked_add(payload_bytes)
                    .ok_or(StoreError::CapacityExceeded)?;
                if next_bytes > self.max_archive_bytes() {
                    return Err(StoreError::CapacityExceeded);
                }
                let inserted = sqlx::query(
                    "INSERT INTO retention_archive(archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
                )
                .bind(&archive.archive_id)
                .bind(crate::retention::target_name(archive.target))
                .bind(i64::try_from(archive.archived_tick).map_err(|_| StoreError::Conflict)?)
                .bind(i64::try_from(archive.watermark_tick).map_err(|_| StoreError::Conflict)?)
                .bind(i64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?)
                .bind(i64::try_from(payload_bytes).map_err(|_| StoreError::CapacityExceeded)?)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if inserted != 1 {
                    return Err(StoreError::Conflict);
                }
                for (ordinal, row) in archive.rows.iter().enumerate() {
                    sqlx::query(
                        "INSERT INTO retention_archive_row(archive_id,ordinal,namespace,key,source_version,payload,retention_tick,owner_execution) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
                    )
                    .bind(&archive.archive_id)
                    .bind(i64::try_from(ordinal).map_err(|_| StoreError::CapacityExceeded)?)
                    .bind(&row.namespace)
                    .bind(&row.key)
                    .bind(i64::try_from(row.version).map_err(|_| StoreError::Conflict)?)
                    .bind(&row.payload)
                    .bind(i64::try_from(row.retention_tick).map_err(|_| StoreError::Conflict)?)
                    .bind(row.owner_execution.as_ref().map(|owner| owner.as_str()))
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                    delete_postgres_observation(&mut transaction, archive.target, row).await?;
                    let deleted = sqlx::query(
                        "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                    )
                    .bind(&row.namespace)
                    .bind(&row.key)
                    .bind(i64::try_from(row.version).map_err(|_| StoreError::Conflict)?)
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                    if deleted != 1 {
                        return Err(StoreError::Conflict);
                    }
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }

    fn commit_provider_retention_replacement(
        &self,
        request: ProviderStateArchiveReplacement,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_replacement(&request, self.max_payload_bytes)?;
        let expected_epoch = request.expected_epoch;
        let source = request.source.clone();
        let expected_version = request
            .replacement
            .expected_version
            .ok_or(StoreError::Conflict)?;
        let replacement = request.replacement.record;
        let candidates = request.rows;
        let archive = crate::retention::build_archive(
            request.target,
            request.archived_tick,
            request.watermark_tick,
            candidates
                .iter()
                .map(|candidate| ArchivedRetentionRow {
                    namespace: candidate.row.namespace.clone(),
                    key: candidate.row.key.clone(),
                    version: candidate.row.version,
                    payload: candidate.row.payload.clone(),
                    retention_tick: candidate.retention_tick,
                    owner_execution: candidate.owner_execution.clone(),
                })
                .collect(),
        )?;
        let payload_bytes = crate::retention::archive_storage_bytes(&archive.rows)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(
                    &mut transaction,
                    RETENTION_ARCHIVE_QUOTA,
                    self.max_archive_rows,
                    i64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?,
                )
                .await?;
                let epoch = sqlx::query_scalar::<_, i64>(
                    "SELECT epoch FROM retention_lock WHERE singleton=1 FOR UPDATE",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)?
                    != expected_epoch
                {
                    return Err(StoreError::Conflict);
                }
                let current = sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(&source.namespace)
                .bind(&source.key)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::Conflict)?;
                let current_version = u64::try_from(
                    current.try_get::<i64, _>(0).map_err(infrastructure)?,
                )
                .map_err(|_| StoreError::IncompatibleVersion)?;
                let current_payload: Vec<u8> = current.try_get(1).map_err(infrastructure)?;
                if current_version != source.version || current_payload != source.payload {
                    return Err(StoreError::Conflict);
                }
                for candidate in &candidates {
                    validate_postgres_provider_observation(
                        &mut transaction,
                        request.target,
                        candidate,
                    )
                    .await?;
                }
                sqlx::query("LOCK TABLE retention_archive,retention_archive_row IN SHARE ROW EXCLUSIVE MODE")
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                let used_bytes: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(payload_bytes),0)::BIGINT FROM retention_archive",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(used_bytes)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    .checked_add(payload_bytes)
                    .is_none_or(|bytes| bytes > self.max_archive_bytes())
                {
                    return Err(StoreError::CapacityExceeded);
                }
                insert_postgres_archive(&mut transaction, &archive, payload_bytes).await?;
                let affected = sqlx::query(
                    "UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4 AND version=$5",
                )
                .bind(i64::try_from(replacement.version).map_err(|_| StoreError::Conflict)?)
                .bind(replacement.payload)
                .bind(replacement.namespace)
                .bind(replacement.key)
                .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if affected != 1 {
                    return Err(StoreError::Conflict);
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })??;
        Ok(archive)
    }

    fn provider_archive_usage(&self, target: RetentionTarget) -> Result<(usize, u64), StoreError> {
        let (rows, bytes): (i64, i64) = self.run(
            sqlx::query_as(
                "SELECT COALESCE(SUM(row_count),0)::BIGINT,COALESCE(SUM(payload_bytes),0)::BIGINT \
                 FROM retention_archive WHERE target=$1",
            )
            .bind(crate::retention::target_name(target))
            .fetch_one(&self.pool),
        )?;
        Ok((
            usize::try_from(rows).map_err(|_| StoreError::IncompatibleVersion)?,
            u64::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }

    fn commit_provider_retention_deletion(
        &self,
        request: ProviderStateArchiveDeletion,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_deletion(&request, self.max_payload_bytes)?;
        let expected_epoch = request.expected_epoch;
        let candidates = request.rows;
        let archive = crate::retention::build_archive(
            request.target,
            request.archived_tick,
            request.watermark_tick,
            candidates
                .iter()
                .map(|candidate| ArchivedRetentionRow {
                    namespace: candidate.row.namespace.clone(),
                    key: candidate.row.key.clone(),
                    version: candidate.row.version,
                    payload: candidate.row.payload.clone(),
                    retention_tick: candidate.retention_tick,
                    owner_execution: candidate.owner_execution.clone(),
                })
                .collect(),
        )?;
        let storage_bytes = crate::retention::archive_storage_bytes(&archive.rows)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(
                    &mut transaction,
                    PROVIDER_STATE_QUOTA,
                    self.max_rows,
                    -i64::try_from(candidates.len())
                        .map_err(|_| StoreError::CapacityExceeded)?,
                )
                .await?;
                adjust_quota(
                    &mut transaction,
                    RETENTION_ARCHIVE_QUOTA,
                    self.max_archive_rows,
                    i64::try_from(candidates.len()).map_err(|_| StoreError::CapacityExceeded)?,
                )
                .await?;
                let epoch: i64 = sqlx::query_scalar(
                    "SELECT epoch FROM retention_lock WHERE singleton=1 FOR UPDATE",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)?
                    != expected_epoch
                {
                    return Err(StoreError::Conflict);
                }
                sqlx::query(
                    "LOCK TABLE retention_archive,retention_archive_row IN SHARE ROW EXCLUSIVE MODE",
                )
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                let used_bytes: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(payload_bytes),0)::BIGINT FROM retention_archive",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(used_bytes)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    .checked_add(storage_bytes)
                    .is_none_or(|bytes| bytes > self.max_archive_bytes())
                {
                    return Err(StoreError::CapacityExceeded);
                }
                for candidate in &candidates {
                    let current = sqlx::query(
                        "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                    )
                    .bind(&candidate.row.namespace)
                    .bind(&candidate.row.key)
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .ok_or(StoreError::Conflict)?;
                    if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?
                        != candidate.row.version
                        || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?
                            != candidate.row.payload
                    {
                        return Err(StoreError::Conflict);
                    }
                    validate_postgres_provider_observation(
                        &mut transaction,
                        request.target,
                        candidate,
                    )
                    .await?;
                    validate_postgres_provider_dependency(&mut transaction, candidate).await?;
                }
                insert_postgres_archive(&mut transaction, &archive, storage_bytes).await?;
                for candidate in &candidates {
                    let deleted = sqlx::query(
                        "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                    )
                    .bind(&candidate.row.namespace)
                    .bind(&candidate.row.key)
                    .bind(
                        i64::try_from(candidate.row.version)
                            .map_err(|_| StoreError::Conflict)?,
                    )
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                    if deleted != 1 {
                        return Err(StoreError::Conflict);
                    }
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })??;
        Ok(archive)
    }

    pub(crate) fn load_retention_archives(
        &self,
        target: Option<RetentionTarget>,
        max: usize,
        through_tick: Option<u64>,
    ) -> Result<Vec<RetentionArchive>, StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                sqlx::query("LOCK TABLE retention_archive,retention_archive_row IN SHARE MODE")
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                retention_read::load_archives(
                    &mut transaction,
                    target,
                    max,
                    through_tick,
                    self.max_archive_rows,
                )
                .await
            }
            .await;
            match outcome {
                Ok(archives) => {
                    transaction.commit().await.map_err(infrastructure)?;
                    Ok(archives)
                }
                Err(problem) => {
                    transaction.rollback().await.map_err(infrastructure)?;
                    Err(problem)
                }
            }
        })?
    }

    pub(crate) fn delete_retention_archives(
        &self,
        archives: &[RetentionArchive],
    ) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(
                    &mut transaction,
                    RETENTION_ARCHIVE_QUOTA,
                    self.max_archive_rows,
                    -i64::try_from(
                        archives.iter().map(|archive| archive.rows.len()).sum::<usize>(),
                    )
                    .map_err(|_| StoreError::CapacityExceeded)?,
                )
                .await?;
                sqlx::query("LOCK TABLE retention_archive,retention_archive_row IN SHARE ROW EXCLUSIVE MODE")
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                for archive in archives {
                    sqlx::query("DELETE FROM retention_archive_row WHERE archive_id=$1")
                        .bind(&archive.archive_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?;
                    let deleted = sqlx::query("DELETE FROM retention_archive WHERE archive_id=$1")
                        .bind(&archive.archive_id)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .rows_affected();
                    if deleted != 1 {
                        return Err(StoreError::Conflict);
                    }
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }
}

async fn prove_provider_state_writable(
    transaction: &mut Transaction<'_, Postgres>,
    expected_max_rows: usize,
) -> Result<(), StoreError> {
    let expected_max_rows =
        i64::try_from(expected_max_rows).map_err(|_| StoreError::CapacityExceeded)?;
    let quota = sqlx::query(
        "UPDATE store_quota SET used_rows=used_rows \
         WHERE quota_key=$1 AND max_rows=$2",
    )
    .bind(PROVIDER_STATE_QUOTA)
    .bind(expected_max_rows)
    .execute(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .rows_affected();
    if quota != 1 {
        return Err(StoreError::IncompatibleVersion);
    }
    let transaction_id: String = sqlx::query_scalar("SELECT txid_current()::text")
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?;
    let key = format!("transaction-{transaction_id}");
    let upserted = sqlx::query(
        "INSERT INTO provider_state(namespace,key,version,payload) \
         VALUES('server-readiness-probe',$1,1,''::bytea) \
         ON CONFLICT(namespace,key) DO UPDATE SET payload=EXCLUDED.payload",
    )
    .bind(&key)
    .execute(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .rows_affected();
    if upserted != 1 {
        return Err(StoreError::Conflict);
    }
    let deleted = sqlx::query(
        "DELETE FROM provider_state WHERE namespace='server-readiness-probe' AND key=$1",
    )
    .bind(key)
    .execute(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .rows_affected();
    if deleted == 1 {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}

impl ProviderStateStore for PostgresStateStore {
    fn advance_logical_clock(&self, observed_floor: u64) -> Result<u64, StoreError> {
        let observed_floor =
            i64::try_from(observed_floor.max(1)).map_err(|_| StoreError::CapacityExceeded)?;
        let tick: Option<i64> = self.run(
            sqlx::query_scalar(
                "UPDATE retention_lock SET clock_tick=GREATEST(clock_tick,$1) \
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
            sqlx::query("SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2")
                .bind(namespace)
                .bind(key)
                .fetch_optional(&self.pool),
        )?;
        row.map(|row| {
            Ok(ProviderStateRecord {
                namespace: namespace.into(),
                key: key.into(),
                version: u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                payload: row.try_get(1).map_err(infrastructure)?,
            })
        })
        .transpose()
    }

    fn list_provider_state(
        &self,
        namespace: &str,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        if max == 0 || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = self.run(
            sqlx::query(
                "SELECT key,version,payload FROM provider_state WHERE namespace=$1 ORDER BY key LIMIT $2",
            )
            .bind(namespace)
            .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
            .fetch_all(&self.pool),
        )?;
        rows.into_iter()
            .map(|row| {
                Ok(ProviderStateRecord {
                    namespace: namespace.into(),
                    key: row.try_get(0).map_err(infrastructure)?,
                    version: u64::try_from(row.try_get::<i64, _>(1).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    payload: row.try_get(2).map_err(infrastructure)?,
                })
            })
            .collect()
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
                 WHERE left(namespace,char_length($1))=$1 ORDER BY namespace,key LIMIT $2",
            )
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
        if let Some(expected) = expected {
            if record.version != expected.checked_add(1).ok_or(StoreError::Conflict)? {
                return Err(StoreError::Conflict);
            }
            block_on(&self.runtime, async {
                let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
                let outcome = async {
                    let affected = sqlx::query(
                        "UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4 AND version=$5",
                    )
                    .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                    .bind(record.payload)
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                    if affected != 1 {
                        return Err(StoreError::Conflict);
                    }
                    Ok(())
                }
                .await;
                finish_transaction(transaction, outcome).await
            })?
        } else {
            if record.version != 1 {
                return Err(StoreError::Conflict);
            }
            block_on(&self.runtime, async {
                let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
                let outcome = async {
                    adjust_quota(&mut transaction, PROVIDER_STATE_QUOTA, self.max_rows, 1)
                        .await?;
                    let affected = sqlx::query(
                        "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,1,$3) ON CONFLICT DO NOTHING",
                    )
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(record.payload)
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                    if affected != 1 {
                        return Err(StoreError::Conflict);
                    }
                    Ok(())
                }
                .await;
                finish_transaction(transaction, outcome).await
            })?
        }
    }

    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(&mut transaction, PROVIDER_STATE_QUOTA, self.max_rows, -1).await?;
                let affected = sqlx::query(
                    "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                )
                .bind(namespace)
                .bind(key)
                .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if affected != 1 {
                    return Err(StoreError::Conflict);
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }

    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        record.validate_move(old_key, expected_version, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let inserted = sqlx::query(
                    "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
                )
                .bind(&record.namespace)
                .bind(&record.key)
                .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                .bind(&record.payload)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                let deleted = sqlx::query(
                    "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                )
                .bind(&record.namespace)
                .bind(old_key)
                .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if inserted != 1 || deleted != 1 {
                    return Err(StoreError::Conflict);
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }

    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        if writes.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        for write in &writes {
            write.record.validate_write(self.max_payload_bytes)?;
        }
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let creates = writes
                    .iter()
                    .filter(|write| write.expected_version.is_none())
                    .count();
                if creates != 0 {
                    adjust_quota(
                        &mut transaction,
                        PROVIDER_STATE_QUOTA,
                        self.max_rows,
                        i64::try_from(creates).map_err(|_| StoreError::CapacityExceeded)?,
                    )
                    .await?;
                }
                for write in writes {
                    let record = write.record;
                    let affected = if let Some(expected) = write.expected_version {
                        if record.version != expected.checked_add(1).ok_or(StoreError::Conflict)? {
                            return Err(StoreError::Conflict);
                        }
                        sqlx::query(
                            "UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4 AND version=$5",
                        )
                        .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                        .bind(record.payload)
                        .bind(record.namespace)
                        .bind(record.key)
                        .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .rows_affected()
                    } else {
                        if record.version != 1 {
                            return Err(StoreError::Conflict);
                        }
                        sqlx::query(
                            "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,1,$3) ON CONFLICT DO NOTHING",
                        )
                        .bind(record.namespace)
                        .bind(record.key)
                        .bind(record.payload)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .rows_affected()
                    };
                    if affected != 1 {
                        return Err(StoreError::Conflict);
                    }
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }

    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        if mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        for mutation in &mutations {
            if let ProviderStateMutation::Put(write) = mutation {
                write.record.validate_write(self.max_payload_bytes)?;
            }
        }
        let quota_delta = mutations.iter().try_fold(0_i64, |delta, mutation| {
            let change = match mutation {
                ProviderStateMutation::Put(write) if write.expected_version.is_none() => 1,
                ProviderStateMutation::Delete { .. } => -1,
                ProviderStateMutation::Put(_) | ProviderStateMutation::Move { .. } => 0,
            };
            delta
                .checked_add(change)
                .ok_or(StoreError::CapacityExceeded)
        })?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                if quota_delta != 0 {
                    adjust_quota(
                        &mut transaction,
                        PROVIDER_STATE_QUOTA,
                        self.max_rows,
                        quota_delta,
                    )
                    .await?;
                }
                for mutation in mutations {
                let affected = match mutation {
                    ProviderStateMutation::Put(write) => {
                        let record = write.record;
                        if let Some(expected) = write.expected_version {
                            if record.version
                                != expected.checked_add(1).ok_or(StoreError::Conflict)?
                            {
                                return Err(StoreError::Conflict);
                            }
                            sqlx::query(
                                "UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4 AND version=$5",
                            )
                            .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                            .bind(record.payload)
                            .bind(record.namespace)
                            .bind(record.key)
                            .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                            .execute(&mut *transaction)
                            .await
                            .map_err(infrastructure)?
                            .rows_affected()
                        } else {
                            if record.version != 1 {
                                return Err(StoreError::Conflict);
                            }
                            sqlx::query(
                                "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,1,$3) ON CONFLICT DO NOTHING",
                            )
                            .bind(record.namespace)
                            .bind(record.key)
                            .bind(record.payload)
                            .execute(&mut *transaction)
                            .await
                            .map_err(infrastructure)?
                            .rows_affected()
                        }
                    }
                    ProviderStateMutation::Delete {
                        namespace,
                        key,
                        expected_version,
                    } => {
                        if namespace.is_empty() || key.is_empty() || expected_version == 0 {
                            return Err(StoreError::Conflict);
                        }
                        sqlx::query(
                            "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                        )
                        .bind(namespace)
                        .bind(key)
                        .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .rows_affected()
                    }
                    ProviderStateMutation::Move {
                        record,
                        old_key,
                        expected_version,
                    } => {
                        record.validate_move(&old_key, expected_version, self.max_payload_bytes)?;
                        let inserted = sqlx::query(
                            "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
                        )
                        .bind(&record.namespace)
                        .bind(&record.key)
                        .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                        .bind(&record.payload)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .rows_affected();
                        let deleted = sqlx::query(
                            "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                        )
                        .bind(&record.namespace)
                        .bind(old_key)
                        .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                        .execute(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .rows_affected();
                        u64::from(inserted == 1 && deleted == 1)
                    }
                };
                    if affected != 1 {
                        return Err(StoreError::Conflict);
                    }
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
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
                "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0)::BIGINT FROM retention_observation WHERE target=$1",
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
        self.commit_provider_retention_deletion(request)
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
                     FROM retention_observation WHERE target=$1 AND (namespace>$2 OR (namespace=$2 AND key>$3)) \
                     ORDER BY namespace,key LIMIT $4",
                )
                .bind(crate::retention::target_name(target))
                .bind(&after.namespace)
                .bind(&after.key)
                .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
                .fetch_all(&self.pool),
            )?,
            None => self.run(
                sqlx::query(
                    "SELECT namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution \
                     FROM retention_observation WHERE target=$1 ORDER BY namespace,key LIMIT $2",
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
            let outcome = async {
                let epoch: i64 = sqlx::query_scalar(
                    "SELECT epoch FROM retention_lock WHERE singleton=1 FOR UPDATE",
                )
                .fetch_one(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)?
                    != request.expected_epoch
                {
                    return Err(StoreError::Conflict);
                }
                let (namespace, key, expected) = match request.source {
                    ProviderRetentionObservationSource::Present(source) => {
                        (source.namespace, source.key, Some((source.version, source.payload)))
                    }
                    ProviderRetentionObservationSource::Absent(identity) => {
                        (identity.namespace, identity.key, None)
                    }
                };
                let current = sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2 FOR UPDATE",
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
                            && current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? == payload => {}
                    _ => return Err(StoreError::Conflict),
                }
                let deleted = sqlx::query(
                    "DELETE FROM retention_observation WHERE target=$1 AND namespace=$2 AND key=$3 AND observation_version=$4",
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
                adjust_quota(
                    &mut transaction,
                    RETENTION_OBSERVATION_QUOTA,
                    self.max_archive_rows,
                    -1,
                )
                .await
            }
            .await;
            finish_transaction(transaction, outcome).await
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

async fn insert_postgres_archive(
    transaction: &mut Transaction<'_, Postgres>,
    archive: &RetentionArchive,
    payload_bytes: u64,
) -> Result<(), StoreError> {
    let inserted = sqlx::query(
        "INSERT INTO retention_archive(archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
    )
    .bind(&archive.archive_id)
    .bind(crate::retention::target_name(archive.target))
    .bind(i64::try_from(archive.archived_tick).map_err(|_| StoreError::Conflict)?)
    .bind(i64::try_from(archive.watermark_tick).map_err(|_| StoreError::Conflict)?)
    .bind(i64::try_from(archive.rows.len()).map_err(|_| StoreError::CapacityExceeded)?)
    .bind(i64::try_from(payload_bytes).map_err(|_| StoreError::CapacityExceeded)?)
    .execute(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .rows_affected();
    if inserted != 1 {
        return Err(StoreError::Conflict);
    }
    for (ordinal, row) in archive.rows.iter().enumerate() {
        sqlx::query(
            "INSERT INTO retention_archive_row(archive_id,ordinal,namespace,key,source_version,payload,retention_tick,owner_execution) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(&archive.archive_id)
        .bind(i64::try_from(ordinal).map_err(|_| StoreError::CapacityExceeded)?)
        .bind(&row.namespace)
        .bind(&row.key)
        .bind(i64::try_from(row.version).map_err(|_| StoreError::Conflict)?)
        .bind(&row.payload)
        .bind(i64::try_from(row.retention_tick).map_err(|_| StoreError::Conflict)?)
        .bind(row.owner_execution.as_ref().map(|owner| owner.as_str()))
        .execute(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        delete_postgres_observation(transaction, archive.target, row).await?;
    }
    Ok(())
}

async fn delete_postgres_observation(
    transaction: &mut Transaction<'_, Postgres>,
    target: RetentionTarget,
    row: &ArchivedRetentionRow,
) -> Result<(), StoreError> {
    let current = sqlx::query(
        "SELECT source_version,source_digest,observed_tick,owner_execution FROM retention_observation \
         WHERE target=$1 AND namespace=$2 AND key=$3 FOR UPDATE",
    )
    .bind(crate::retention::target_name(target))
    .bind(&row.namespace)
    .bind(&row.key)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(infrastructure)?;
    let Some(current) = current else {
        return Ok(());
    };
    let source_version = u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let source_digest: Vec<u8> = current.try_get(1).map_err(infrastructure)?;
    let observed_tick = u64::try_from(current.try_get::<i64, _>(2).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let owner: Option<String> = current.try_get(3).map_err(infrastructure)?;
    let _matches_archived_evidence = source_version == row.version
        && source_digest == crate::retention::source_digest(&row.payload)
        && observed_tick == row.retention_tick
        && owner.as_deref() == row.owner_execution.as_ref().map(|value| value.as_str());
    sqlx::query("DELETE FROM retention_observation WHERE target=$1 AND namespace=$2 AND key=$3")
        .bind(crate::retention::target_name(target))
        .bind(&row.namespace)
        .bind(&row.key)
        .execute(&mut **transaction)
        .await
        .map_err(infrastructure)?;
    let adjusted = sqlx::query(
        "UPDATE store_quota SET used_rows=used_rows-1 \
             WHERE quota_key=$1 AND used_rows>0",
    )
    .bind(RETENTION_OBSERVATION_QUOTA)
    .execute(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .rows_affected();
    if adjusted != 1 {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(())
}

async fn validate_postgres_provider_observation(
    transaction: &mut Transaction<'_, Postgres>,
    target: RetentionTarget,
    candidate: &ProviderRetentionRow,
) -> Result<(), StoreError> {
    let Some(proof) = &candidate.observation else {
        return Ok(());
    };
    let row = sqlx::query(
        "SELECT observation_version,source_version,source_digest,observed_tick,owner_execution \
         FROM retention_observation WHERE target=$1 AND namespace=$2 AND key=$3 FOR UPDATE",
    )
    .bind(crate::retention::target_name(target))
    .bind(&proof.observation.namespace)
    .bind(&proof.observation.key)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .ok_or(StoreError::Conflict)?;
    let owner: Option<String> = row.try_get(4).map_err(infrastructure)?;
    if u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?
        != proof.version
        || u64::try_from(row.try_get::<i64, _>(1).map_err(infrastructure)?)
            .map_err(|_| StoreError::IncompatibleVersion)?
            != proof.observation.source_version
        || row.try_get::<Vec<u8>, _>(2).map_err(infrastructure)? != proof.observation.source_digest
        || u64::try_from(row.try_get::<i64, _>(3).map_err(infrastructure)?)
            .map_err(|_| StoreError::IncompatibleVersion)?
            != proof.observation.observed_tick
        || owner.as_deref()
            != proof
                .observation
                .owner_execution
                .as_ref()
                .map(|owner| owner.as_str())
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

async fn validate_postgres_provider_dependency(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &ProviderRetentionRow,
) -> Result<(), StoreError> {
    match &candidate.dependency {
        ProviderRetentionDependency::None => {
            return if candidate.owner_execution.is_none() && candidate.owner_run_unit.is_none() {
                Ok(())
            } else {
                Err(StoreError::IncompatibleVersion)
            };
        }
        ProviderRetentionDependency::DirectProduct => {
            validate_postgres_owner_dependency(transaction, candidate, false).await?;
            return Ok(());
        }
        ProviderRetentionDependency::ProviderGraph {
            required_rows,
            required_executions,
        } => {
            for required in required_rows {
                let current = sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(&required.namespace)
                .bind(&required.key)
                .fetch_optional(&mut **transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::Conflict)?;
                if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    != required.version
                    || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? != required.payload
                {
                    return Err(StoreError::Conflict);
                }
            }
            for required in required_executions {
                validate_postgres_execution_dependency(transaction, required).await?;
            }
            validate_postgres_owner_dependency(
                transaction,
                candidate,
                candidate.owner_execution.is_some(),
            )
            .await?;
            return Ok(());
        }
        _ => {}
    }
    let (owner, run) = validate_postgres_owner_dependency(transaction, candidate, true)
        .await?
        .ok_or(StoreError::Conflict)?;
    match &candidate.dependency {
        ProviderRetentionDependency::CoreEffect {
            key,
            request_digest,
            result_digest,
        } => {
            let effect = sqlx::query(
                "SELECT payload FROM provider_state WHERE namespace='durable-effect' AND key=$1",
            )
            .bind(key.as_str())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::Conflict)?;
            let payload: Vec<u8> = effect.try_get(0).map_err(infrastructure)?;
            let effect = crate::durable::decode_effect(key, &payload)?;
            if effect.execution_id != owner
                || effect.run_unit_id != run
                || effect.state != mainframe_env_store_api::EffectState::Completed
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
        ProviderRetentionDependency::CicsNested { provenance, absent } => {
            let current = sqlx::query(
                "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
            )
            .bind(&provenance.namespace)
            .bind(&provenance.key)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::Conflict)?;
            if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?
                != provenance.version
                || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? != provenance.payload
            {
                return Err(StoreError::Conflict);
            }
            for identity in absent {
                if sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(&identity.namespace)
                .bind(&identity.key)
                .fetch_one(&mut **transaction)
                .await
                .map_err(infrastructure)?
                    != 0
                {
                    return Err(StoreError::Conflict);
                }
            }
            let effects = sqlx::query(
                "SELECT key,payload FROM provider_state WHERE namespace='durable-effect'",
            )
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?;
            let mut terminal_origin = false;
            for effect in effects {
                let key: String = effect.try_get(0).map_err(infrastructure)?;
                let key = mainframe_env_execution_api::IdempotencyKey::new(
                    key,
                    mainframe_env_execution_api::InvocationLimits::default(),
                )
                .map_err(|_| StoreError::IncompatibleVersion)?;
                let payload: Vec<u8> = effect.try_get(1).map_err(infrastructure)?;
                let effect = crate::durable::decode_effect(&key, &payload)?;
                if effect.execution_id == owner && effect.run_unit_id == run {
                    if effect.key.as_str() != provenance.key {
                        continue;
                    }
                    if matches!(
                        effect.state,
                        mainframe_env_store_api::EffectState::Intent
                            | mainframe_env_store_api::EffectState::UnknownOutcome
                    ) {
                        return Err(StoreError::Conflict);
                    }
                    terminal_origin |= effect.state
                        == mainframe_env_store_api::EffectState::Completed
                        && effect.digest_format
                            == mainframe_env_store_api::EffectDigestFormat::CanonicalHostV1
                        && effect.intent.capability.as_ref().map(|item| item.as_str())
                            == Some("host.cics.execute")
                        && effect
                            .resolved_tick
                            .is_some_and(|tick| tick != 0 && candidate.retention_tick >= tick);
                }
            }
            let undo: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM provider_state WHERE namespace='cics-uow-undo' AND key=$1",
            )
            .bind(run.as_str())
            .fetch_one(&mut **transaction)
            .await
            .map_err(infrastructure)?;
            if !terminal_origin || undo != 0 {
                return Err(StoreError::Conflict);
            }
        }
        ProviderRetentionDependency::ProviderGraph { .. }
        | ProviderRetentionDependency::DirectProduct
        | ProviderRetentionDependency::None => unreachable!(),
    }
    Ok(())
}

async fn validate_postgres_execution_dependency(
    transaction: &mut Transaction<'_, Postgres>,
    owner: &mainframe_env_execution_api::ExecutionId,
) -> Result<(), StoreError> {
    let row = sqlx::query(
        "SELECT version,payload FROM provider_state WHERE namespace='durable-execution' AND key=$1",
    )
    .bind(owner.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .ok_or(StoreError::Conflict)?;
    let version = u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let execution = crate::durable::decode_execution(
        &row.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?,
        version,
    )?;
    if !execution.state.terminal()
        || sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_state WHERE namespace='durable-checkpoint' AND key=$1",
        )
        .bind(owner.as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?
            != 0
    {
        return Err(StoreError::Conflict);
    }
    for row in
        sqlx::query("SELECT key,payload FROM provider_state WHERE namespace='durable-effect'")
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?
    {
        let key = mainframe_env_execution_api::IdempotencyKey::new(
            row.try_get::<String, _>(0).map_err(infrastructure)?,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .map_err(|_| StoreError::IncompatibleVersion)?;
        let effect = crate::durable::decode_effect(
            &key,
            &row.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?,
        )?;
        if effect.execution_id == *owner
            && matches!(
                effect.state,
                mainframe_env_store_api::EffectState::Intent
                    | mainframe_env_store_api::EffectState::UnknownOutcome
            )
        {
            return Err(StoreError::Conflict);
        }
    }
    Ok(())
}

async fn validate_postgres_owner_dependency(
    transaction: &mut Transaction<'_, Postgres>,
    candidate: &ProviderRetentionRow,
    required: bool,
) -> Result<
    Option<(
        mainframe_env_execution_api::ExecutionId,
        mainframe_env_execution_api::RunUnitId,
    )>,
    StoreError,
> {
    let Some(owner) = candidate.owner_execution.as_ref() else {
        return if required || candidate.owner_run_unit.is_some() {
            Err(StoreError::IncompatibleVersion)
        } else {
            Ok(None)
        };
    };
    let run = candidate
        .owner_run_unit
        .as_ref()
        .ok_or(StoreError::IncompatibleVersion)?;
    let execution = sqlx::query(
        "SELECT version,payload FROM provider_state WHERE namespace='durable-execution' AND key=$1",
    )
    .bind(owner.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(infrastructure)?;
    let Some(execution) = execution else {
        return if required {
            Err(StoreError::Conflict)
        } else {
            Ok(None)
        };
    };
    let execution_version = u64::try_from(execution.try_get::<i64, _>(0).map_err(infrastructure)?)
        .map_err(|_| StoreError::IncompatibleVersion)?;
    let execution_payload: Vec<u8> = execution.try_get(1).map_err(infrastructure)?;
    let execution = crate::durable::decode_execution(&execution_payload, execution_version)?;
    if !execution.state.terminal()
        || execution.run_unit_id != *run
        || sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM provider_state WHERE namespace='durable-checkpoint' AND key=$1",
        )
        .bind(owner.as_str())
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?
            != 0
    {
        return Err(StoreError::Conflict);
    }
    let effects =
        sqlx::query("SELECT key,payload FROM provider_state WHERE namespace='durable-effect'")
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?;
    for effect in effects {
        let key: String = effect.try_get(0).map_err(infrastructure)?;
        let key = mainframe_env_execution_api::IdempotencyKey::new(
            key,
            mainframe_env_execution_api::InvocationLimits::default(),
        )
        .map_err(|_| StoreError::IncompatibleVersion)?;
        let payload: Vec<u8> = effect.try_get(1).map_err(infrastructure)?;
        let effect = crate::durable::decode_effect(&key, &payload)?;
        if effect.execution_id == *owner
            && effect.run_unit_id == *run
            && matches!(
                effect.state,
                mainframe_env_store_api::EffectState::Intent
                    | mainframe_env_store_api::EffectState::UnknownOutcome
            )
        {
            return Err(StoreError::Conflict);
        }
    }
    Ok(Some((owner.clone(), run.clone())))
}

async fn migrate_legacy_logical_clock(pool: &PgPool, max_rows: usize) -> Result<(), StoreError> {
    let mut transaction = pool.begin().await.map_err(infrastructure)?;
    let outcome = async {
        let Some(row) = sqlx::query(
            "SELECT version,payload FROM provider_state WHERE namespace='jes-worker-meta' AND key='logical-clock' FOR UPDATE",
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(infrastructure)?
        else {
            return Ok(());
        };
        let version: i64 = row.try_get(0).map_err(infrastructure)?;
        let payload: Vec<u8> = row.try_get(1).map_err(infrastructure)?;
        let tick = u64::from_be_bytes(
            payload
                .as_slice()
                .try_into()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        );
        let tick = i64::try_from(tick).map_err(|_| StoreError::IncompatibleVersion)?;
        if tick == 0 || version <= 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        adjust_quota(&mut transaction, PROVIDER_STATE_QUOTA, max_rows, -1).await?;
        sqlx::query(
            "UPDATE retention_lock SET clock_tick=GREATEST(clock_tick,$1) WHERE singleton=1",
        )
        .bind(tick)
        .execute(&mut *transaction)
        .await
        .map_err(infrastructure)?;
        let deleted = sqlx::query(
            "DELETE FROM provider_state WHERE namespace='jes-worker-meta' AND key='logical-clock' AND version=$1",
        )
        .bind(version)
        .execute(&mut *transaction)
        .await
        .map_err(infrastructure)?
        .rows_affected();
        if deleted != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    .await;
    finish_transaction(transaction, outcome).await
}

pub(crate) async fn initialize_quota(
    pool: &PgPool,
    quota_key: &str,
    max_rows: usize,
    count_sql: &'static str,
) -> Result<(), StoreError> {
    let max_rows = i64::try_from(max_rows).map_err(|_| StoreError::CapacityExceeded)?;
    let mut transaction = pool.begin().await.map_err(infrastructure)?;
    let outcome = async {
        let created = sqlx::query(
            "INSERT INTO store_quota(quota_key,max_rows,used_rows) VALUES($1,$2,0) ON CONFLICT DO NOTHING",
        )
        .bind(quota_key)
        .bind(max_rows)
        .execute(&mut *transaction)
        .await
        .map_err(infrastructure)?
        .rows_affected()
            == 1;
        let row =
            sqlx::query("SELECT max_rows,used_rows FROM store_quota WHERE quota_key=$1 FOR UPDATE")
                .bind(quota_key)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::IncompatibleVersion)?;
        let stored_max: i64 = row.try_get(0).map_err(infrastructure)?;
        let stored_used: i64 = row.try_get(1).map_err(infrastructure)?;
        let actual: i64 = sqlx::query_scalar(count_sql)
            .fetch_one(&mut *transaction)
            .await
            .map_err(infrastructure)?;
        if stored_max != max_rows || stored_used < 0 || actual < 0 || actual > max_rows {
            return Err(if actual > max_rows {
                StoreError::CapacityExceeded
            } else {
                StoreError::IncompatibleVersion
            });
        }
        if created && actual != 0 {
            let affected =
                sqlx::query("UPDATE store_quota SET used_rows=$1 WHERE quota_key=$2 AND used_rows=0")
                .bind(actual)
                .bind(quota_key)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
            if affected != 1 {
                return Err(StoreError::Conflict);
            }
        } else if !created && stored_used != actual {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(())
    }
    .await;
    finish_transaction(transaction, outcome).await
}

pub(crate) async fn adjust_quota(
    transaction: &mut Transaction<'_, Postgres>,
    quota_key: &str,
    expected_max_rows: usize,
    delta: i64,
) -> Result<(), StoreError> {
    let row =
        sqlx::query("SELECT max_rows,used_rows FROM store_quota WHERE quota_key=$1 FOR UPDATE")
            .bind(quota_key)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::IncompatibleVersion)?;
    let max_rows: i64 = row.try_get(0).map_err(infrastructure)?;
    let used_rows: i64 = row.try_get(1).map_err(infrastructure)?;
    if max_rows != i64::try_from(expected_max_rows).map_err(|_| StoreError::CapacityExceeded)?
        || used_rows < 0
    {
        return Err(StoreError::IncompatibleVersion);
    }
    let next = used_rows
        .checked_add(delta)
        .ok_or(StoreError::CapacityExceeded)?;
    if next < 0 {
        return Err(StoreError::IncompatibleVersion);
    }
    if next > max_rows {
        return Err(StoreError::CapacityExceeded);
    }
    let affected = sqlx::query(
        "UPDATE store_quota SET used_rows=$1 WHERE quota_key=$2 AND max_rows=$3 AND used_rows=$4",
    )
    .bind(next)
    .bind(quota_key)
    .bind(max_rows)
    .bind(used_rows)
    .execute(&mut **transaction)
    .await
    .map_err(infrastructure)?
    .rows_affected();
    if affected == 1 {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}

pub(crate) async fn finish_transaction(
    transaction: Transaction<'_, Postgres>,
    outcome: Result<(), StoreError>,
) -> Result<(), StoreError> {
    match outcome {
        Ok(()) => transaction.commit().await.map_err(infrastructure),
        Err(problem) => {
            transaction.rollback().await.map_err(infrastructure)?;
            Err(problem)
        }
    }
}

async fn retention_archive_usage_in(
    connection: &mut PgConnection,
) -> Result<(usize, u64), StoreError> {
    let (rows, bytes): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(row_count),0)::BIGINT,COALESCE(SUM(payload_bytes),0)::BIGINT FROM retention_archive",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(infrastructure)?;
    Ok((
        usize::try_from(rows).map_err(|_| StoreError::IncompatibleVersion)?,
        u64::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?,
    ))
}

async fn retention_observation_usage_in(
    connection: &mut PgConnection,
) -> Result<(usize, u64), StoreError> {
    let (rows, bytes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0)::BIGINT FROM retention_observation",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(infrastructure)?;
    Ok((
        usize::try_from(rows).map_err(|_| StoreError::IncompatibleVersion)?,
        u64::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?,
    ))
}

fn infrastructure(error: sqlx::Error) -> StoreError {
    StoreError::Infrastructure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    #[ignore = "requires isolated MAINFRAME_ENV_POSTGRES_TEST_URL pointing at PostgreSQL 18"]
    fn writable_probe_requires_provider_state_dml_and_rolls_everything_back() {
        let url = std::env::var("MAINFRAME_ENV_POSTGRES_TEST_URL")
            .expect("explicit PostgreSQL test URL required");
        let store = PostgresStateStore::open(&url, 1024 * 1024, 262_144).unwrap();
        let expected_max_rows = store.max_rows;
        drop(store);
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&url)
                .await
                .unwrap();
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let role = format!("mainframe_r09_probe_{}_{}", std::process::id(), suffix);
            // `role` is composed only from this process ID and a numeric timestamp.
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE ROLE {role} NOLOGIN")))
                .execute(&pool)
                .await
                .unwrap();
            let snapshot = || async {
                sqlx::query_as::<_, (i64, i64, i64)>(
                    "SELECT \
                       (SELECT epoch FROM retention_lock WHERE singleton=1), \
                       (SELECT used_rows FROM store_quota WHERE quota_key='provider-state'), \
                       (SELECT COUNT(*) FROM provider_state \
                        WHERE namespace='server-readiness-probe')",
                )
                .fetch_one(&pool)
                .await
                .unwrap()
            };
            let before = snapshot().await;
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "GRANT USAGE ON SCHEMA public TO {role}; \
                 GRANT SELECT ON provider_state,store_quota,retention_lock TO {role}; \
                 GRANT UPDATE ON store_quota,retention_lock TO {role}"
            )))
            .execute(&pool)
            .await
            .unwrap();

            let mut denied = pool.begin().await.unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!("SET LOCAL ROLE {role}")))
                .execute(&mut *denied)
                .await
                .unwrap();
            assert!(
                prove_provider_state_writable(&mut denied, expected_max_rows)
                    .await
                    .is_err()
            );
            denied.rollback().await.unwrap();

            sqlx::query(sqlx::AssertSqlSafe(format!(
                "GRANT INSERT,UPDATE,DELETE ON provider_state TO {role}"
            )))
            .execute(&pool)
            .await
            .unwrap();
            let mut allowed = pool.begin().await.unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!("SET LOCAL ROLE {role}")))
                .execute(&mut *allowed)
                .await
                .unwrap();
            prove_provider_state_writable(&mut allowed, expected_max_rows)
                .await
                .unwrap();
            allowed.rollback().await.unwrap();
            assert_eq!(snapshot().await, before);

            sqlx::query(sqlx::AssertSqlSafe(format!("DROP OWNED BY {role}")))
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE {role}")))
                .execute(&pool)
                .await
                .unwrap();
        });
    }
}
