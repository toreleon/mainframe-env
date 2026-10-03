use crate::runtime::{AdapterRuntime, block_on};
use mainframe_env_store_api::{
    ArchivedRetentionRow, ProviderRetentionDependency, ProviderRetentionObservationDeletion,
    ProviderRetentionObservationSource, ProviderRetentionRow, ProviderStateArchiveDeletion,
    ProviderStateArchiveDeletionWithCapacity, ProviderStateArchiveReplacement,
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite,
    RetentionArchive, RetentionObservation, RetentionTarget, StoreError,
};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::{Row, Sqlite, Transaction};
use std::future::Future;
use std::path::Path;
use tokio::runtime::Builder;

mod checked_read;
mod container_retention;
mod provider_retention;
mod publication;
mod root_terminal;
use provider_retention::validate_sqlite_provider_dependency;

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

pub struct SqliteStateStore {
    runtime: AdapterRuntime,
    pool: SqlitePool,
    max_payload_bytes: usize,
    max_rows: usize,
    max_archive_rows: usize,
    max_archive_bytes: u64,
}

impl SqliteStateStore {
    pub fn open(url: &str, max_payload_bytes: usize, max_rows: usize) -> Result<Self, StoreError> {
        let max_archive_bytes = derived_archive_bytes(max_payload_bytes, max_rows);
        Self::open_with_retention_limits(
            url,
            max_payload_bytes,
            max_rows,
            max_rows,
            max_archive_bytes,
        )
    }

    /// Open SQLite with independently bounded retention archive authority.
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
                .enable_time()
                .build()
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
        );
        let pool = block_on(
            &runtime,
            SqlitePoolOptions::new().max_connections(1).connect(url),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        block_on(
            &runtime,
            sqlx::raw_sql(include_str!("../migrations/sqlite/0001-durable-state.sql"))
                .execute(&pool),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        block_on(
            &runtime,
            sqlx::raw_sql(include_str!(
                "../migrations/sqlite/0002-retention-lifecycle.sql"
            ))
            .execute(&pool),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        block_on(&runtime, async {
            let mut transaction = pool.begin().await.map_err(infrastructure)?;
            if let Some(row) = sqlx::query(
                "SELECT version,payload FROM provider_state WHERE namespace='jes-worker-meta' AND key='logical-clock'",
            )
            .fetch_optional(&mut *transaction)
            .await
            .map_err(infrastructure)?
            {
                let version: i64 = row.try_get(0).map_err(infrastructure)?;
                let payload: Vec<u8> = row.try_get(1).map_err(infrastructure)?;
                let tick = u64::from_be_bytes(
                    payload
                        .as_slice()
                        .try_into()
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                );
                let tick = i64::try_from(tick)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                if tick == 0 || version <= 0 {
                    return Err(StoreError::IncompatibleVersion);
                }
                sqlx::query(
                    "UPDATE retention_lock SET clock_tick=MAX(clock_tick,?) WHERE singleton=1",
                )
                .bind(tick)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
                let deleted = sqlx::query(
                    "DELETE FROM provider_state WHERE namespace='jes-worker-meta' AND key='logical-clock' AND version=?",
                )
                .bind(version)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if deleted != 1 {
                    return Err(StoreError::Conflict);
                }
            }
            transaction.commit().await.map_err(infrastructure)
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
        block_on(&self.runtime, future)?
            .map_err(|error| StoreError::Infrastructure(error.to_string()))
    }

    pub(crate) const fn max_rows(&self) -> usize {
        self.max_rows
    }

    pub(crate) fn retention_writable_probe(&self) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let key: String =
                    sqlx::query_scalar("SELECT 'probe-' || lower(hex(randomblob(16)))")
                        .fetch_one(&mut *transaction)
                        .await
                        .map_err(infrastructure)?;
                let upserted = sqlx::query(
                    "INSERT INTO provider_state(namespace,key,version,payload) \
                     VALUES('server-readiness-probe',?,1,X'') \
                     ON CONFLICT(namespace,key) DO UPDATE SET payload=excluded.payload",
                )
                .bind(&key)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if upserted != 1 {
                    return Err(StoreError::Conflict);
                }
                let deleted = sqlx::query(
                    "DELETE FROM provider_state \
                     WHERE namespace='server-readiness-probe' AND key=?",
                )
                .bind(key)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                (deleted == 1).then_some(()).ok_or(StoreError::Conflict)
            }
            .await;
            transaction.rollback().await.map_err(infrastructure)?;
            outcome
        })?
    }

    pub(crate) fn provider_state_usage(&self) -> Result<usize, StoreError> {
        let count: i64 = self
            .run(sqlx::query_scalar("SELECT COUNT(*) FROM provider_state").fetch_one(&self.pool))?;
        usize::try_from(count).map_err(|_| StoreError::IncompatibleVersion)
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
                "SELECT COALESCE(SUM(row_count),0),COALESCE(SUM(payload_bytes),0) FROM retention_archive",
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
                "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0) FROM retention_observation",
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
            let outcome = self.verify_retention_authorities_in(&mut transaction).await;
            finish_read_transaction(transaction, outcome).await
        })?
    }

    pub(crate) fn load_retention_observations(
        &self,
        target: RetentionTarget,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = self
                .load_retention_observations_in(&mut transaction, target)
                .await;
            finish_read_transaction(transaction, outcome).await
        })?
    }

    async fn load_retention_observations_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        target: RetentionTarget,
    ) -> Result<Vec<(u64, RetentionObservation)>, StoreError> {
        let limit = self
            .max_archive_rows
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?;
        let rows = sqlx::query(
            "SELECT namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution,accounted_bytes \
             FROM retention_observation WHERE target=? ORDER BY namespace,key LIMIT ?",
        )
        .bind(crate::retention::target_name(target))
        .bind(i64::try_from(limit).map_err(|_| StoreError::CapacityExceeded)?)
        .fetch_all(&mut **transaction)
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

    async fn verify_retention_authorities_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), StoreError> {
        let archives = self
            .load_retention_archives_in(transaction, None, self.max_archive_rows, None)
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
        let stored_archive: (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(row_count),0),COALESCE(SUM(payload_bytes),0) FROM retention_archive",
        )
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        if stored_archive
            != (
                i64::try_from(archive_rows).map_err(|_| StoreError::CapacityExceeded)?,
                i64::try_from(archive_bytes).map_err(|_| StoreError::CapacityExceeded)?,
            )
            || archive_rows > self.max_archive_rows
            || archive_bytes > self.max_archive_bytes
        {
            return Err(StoreError::IncompatibleVersion);
        }
        let mut observation_rows = 0usize;
        let mut observation_bytes = 0u64;
        for target in RetentionTarget::ALL {
            for (_, observation) in self
                .load_retention_observations_in(transaction, target)
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
        let stored_observations: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0) FROM retention_observation",
        )
        .fetch_one(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        if stored_observations
            != (
                i64::try_from(observation_rows).map_err(|_| StoreError::CapacityExceeded)?,
                i64::try_from(observation_bytes).map_err(|_| StoreError::CapacityExceeded)?,
            )
            || observation_rows > self.max_archive_rows
            || observation_bytes > self.max_archive_bytes
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(())
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
            sqlx::query("UPDATE retention_lock SET epoch=epoch WHERE singleton=1")
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
            let epoch: i64 =
                sqlx::query_scalar("SELECT epoch FROM retention_lock WHERE singleton=1")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
            if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)? != expected_epoch
            {
                return Err(StoreError::Conflict);
            }
            let current_source = sqlx::query(
                "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
            )
            .bind(&source.namespace)
            .bind(&source.key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::Conflict)?;
            if u64::try_from(
                current_source
                    .try_get::<i64, _>(0)
                    .map_err(infrastructure)?,
            )
            .map_err(|_| StoreError::IncompatibleVersion)?
                != source.version
                || current_source
                    .try_get::<Vec<u8>, _>(1)
                    .map_err(infrastructure)?
                    != source.payload
            {
                return Err(StoreError::Conflict);
            }
            let current = sqlx::query(
                "SELECT observation_version,source_version,source_digest,observed_tick,owner_execution,accounted_bytes \
                 FROM retention_observation WHERE target=? AND namespace=? AND key=?",
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
                    && current
                        .try_get::<Option<String>, _>(4)
                        .map_err(infrastructure)?
                        == observation
                            .owner_execution
                            .as_ref()
                            .map(|owner| owner.as_str().to_string());
                if same {
                    return Ok((
                        u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                            .map_err(|_| StoreError::IncompatibleVersion)?,
                        u64::try_from(current.try_get::<i64, _>(3).map_err(infrastructure)?)
                            .map_err(|_| StoreError::IncompatibleVersion)?,
                    ));
                }
            }
            let (used_rows, used_bytes): (i64, i64) = sqlx::query_as(
                "SELECT COUNT(*),COALESCE(SUM(accounted_bytes),0) FROM retention_observation",
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(infrastructure)?;
            let old_bytes = current
                .as_ref()
                .map(|row| row.try_get::<i64, _>(5).map_err(infrastructure))
                .transpose()?
                .unwrap_or(0);
            let next_rows = usize::try_from(used_rows)
                .map_err(|_| StoreError::IncompatibleVersion)?
                .checked_add(usize::from(current.is_none()))
                .ok_or(StoreError::CapacityExceeded)?;
            let next_bytes = u64::try_from(used_bytes.saturating_sub(old_bytes))
                .map_err(|_| StoreError::IncompatibleVersion)?
                .checked_add(accounted_bytes)
                .ok_or(StoreError::CapacityExceeded)?;
            if next_rows > self.max_archive_rows || next_bytes > self.max_archive_bytes() {
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
                 VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(target,namespace,key) DO UPDATE SET \
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
            transaction.commit().await.map_err(infrastructure)?;
            Ok((
                u64::try_from(next_version).map_err(|_| StoreError::IncompatibleVersion)?,
                observation.observed_tick,
            ))
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
            sqlx::query("UPDATE retention_lock SET epoch=epoch WHERE singleton=1")
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
            let epoch: i64 =
                sqlx::query_scalar("SELECT epoch FROM retention_lock WHERE singleton=1")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
            if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)? != expected_epoch
            {
                return Err(StoreError::Conflict);
            }
            let (used_rows, used_bytes): (i64, i64) = sqlx::query_as(
                "SELECT COALESCE(SUM(row_count),0),COALESCE(SUM(payload_bytes),0) FROM retention_archive",
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(infrastructure)?;
            let next_rows = usize::try_from(used_rows)
                .map_err(|_| StoreError::IncompatibleVersion)?
                .checked_add(archive.rows.len())
                .ok_or(StoreError::CapacityExceeded)?;
            let next_bytes = u64::try_from(used_bytes)
                .map_err(|_| StoreError::IncompatibleVersion)?
                .checked_add(payload_bytes)
                .ok_or(StoreError::CapacityExceeded)?;
            if next_rows > self.max_archive_rows() || next_bytes > self.max_archive_bytes() {
                return Err(StoreError::CapacityExceeded);
            }
            let inserted = sqlx::query(
                "INSERT OR IGNORE INTO retention_archive(archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes) VALUES(?,?,?,?,?,?)",
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
                    "INSERT INTO retention_archive_row(archive_id,ordinal,namespace,key,source_version,payload,retention_tick,owner_execution) VALUES(?,?,?,?,?,?,?,?)",
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
                delete_sqlite_observation(&mut transaction, archive.target, row).await?;
                let deleted = sqlx::query(
                    "DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?",
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
            transaction.commit().await.map_err(infrastructure)
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
            sqlx::query("UPDATE retention_lock SET epoch=epoch WHERE singleton=1")
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
            let epoch: i64 =
                sqlx::query_scalar("SELECT epoch FROM retention_lock WHERE singleton=1")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
            if u64::try_from(epoch).map_err(|_| StoreError::IncompatibleVersion)? != expected_epoch
            {
                return Err(StoreError::Conflict);
            }
            let current = sqlx::query(
                "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
            )
            .bind(&source.namespace)
            .bind(&source.key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(infrastructure)?
            .ok_or(StoreError::Conflict)?;
            let current_version =
                u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
            let current_payload: Vec<u8> = current.try_get(1).map_err(infrastructure)?;
            if current_version != source.version || current_payload != source.payload {
                return Err(StoreError::Conflict);
            }
            for candidate in &candidates {
                validate_sqlite_provider_observation(&mut transaction, request.target, candidate)
                    .await?;
            }
            let (used_rows, used_bytes): (i64, i64) = sqlx::query_as(
                "SELECT COALESCE(SUM(row_count),0),COALESCE(SUM(payload_bytes),0) FROM retention_archive",
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(infrastructure)?;
            if usize::try_from(used_rows)
                .map_err(|_| StoreError::IncompatibleVersion)?
                .checked_add(archive.rows.len())
                .is_none_or(|rows| rows > self.max_archive_rows())
                || u64::try_from(used_bytes)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    .checked_add(payload_bytes)
                    .is_none_or(|bytes| bytes > self.max_archive_bytes())
            {
                return Err(StoreError::CapacityExceeded);
            }
            insert_sqlite_archive(&mut transaction, &archive, payload_bytes).await?;
            let affected = sqlx::query(
                "UPDATE provider_state SET version=?,payload=? WHERE namespace=? AND key=? AND version=?",
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
            transaction.commit().await.map_err(infrastructure)
        })??;
        Ok(archive)
    }

    fn provider_archive_usage(&self, target: RetentionTarget) -> Result<(usize, u64), StoreError> {
        let (rows, bytes): (i64, i64) = self.run(
            sqlx::query_as(
                "SELECT COALESCE(SUM(row_count),0),COALESCE(SUM(payload_bytes),0) \
                 FROM retention_archive WHERE target=?",
            )
            .bind(crate::retention::target_name(target))
            .fetch_one(&self.pool),
        )?;
        Ok((
            usize::try_from(rows).map_err(|_| StoreError::IncompatibleVersion)?,
            u64::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }

    pub(crate) fn load_retention_archives(
        &self,
        target: Option<RetentionTarget>,
        max: usize,
        through_tick: Option<u64>,
    ) -> Result<Vec<RetentionArchive>, StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = self
                .load_retention_archives_in(&mut transaction, target, max, through_tick)
                .await;
            finish_read_transaction(transaction, outcome).await
        })?
    }

    async fn load_retention_archives_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        target: Option<RetentionTarget>,
        max: usize,
        through_tick: Option<u64>,
    ) -> Result<Vec<RetentionArchive>, StoreError> {
        let manifest_limit = max;
        let manifests = sqlx::query(
            "SELECT archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes \
             FROM retention_archive WHERE (? IS NULL OR target=?) \
             AND (? IS NULL OR archived_tick<=?) ORDER BY archived_tick,archive_id LIMIT ?",
        )
        .bind(target.map(crate::retention::target_name))
        .bind(target.map(crate::retention::target_name))
        .bind(through_tick.map(|tick| i64::try_from(tick).unwrap_or(i64::MAX)))
        .bind(through_tick.map(|tick| i64::try_from(tick).unwrap_or(i64::MAX)))
        .bind(i64::try_from(manifest_limit).map_err(|_| StoreError::CapacityExceeded)?)
        .fetch_all(&mut **transaction)
        .await
        .map_err(infrastructure)?;
        let mut selected_rows = 0usize;
        let mut archives = Vec::new();
        for manifest in manifests {
            let stored_target = crate::retention::target_back(
                &manifest.try_get::<String, _>(1).map_err(infrastructure)?,
            )?;
            let archived_tick =
                u64::try_from(manifest.try_get::<i64, _>(2).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
            let archive_id: String = manifest.try_get(0).map_err(infrastructure)?;
            let row_count = usize::try_from(manifest.try_get::<i64, _>(4).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?;
            let next_rows = selected_rows
                .checked_add(row_count)
                .ok_or(StoreError::CapacityExceeded)?;
            if next_rows > max && selected_rows != 0 {
                break;
            }
            selected_rows = next_rows;
            let payload_bytes =
                u64::try_from(manifest.try_get::<i64, _>(5).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
            let rows = sqlx::query(
                "SELECT namespace,key,source_version,payload,retention_tick,owner_execution FROM retention_archive_row WHERE archive_id=? ORDER BY ordinal LIMIT ?",
            )
            .bind(&archive_id)
            .bind(i64::try_from(
                row_count
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?,
            )
            .map_err(|_| StoreError::CapacityExceeded)?)
            .fetch_all(&mut **transaction)
            .await
            .map_err(infrastructure)?;
            if rows.len() > self.max_archive_rows() {
                return Err(StoreError::IncompatibleVersion);
            }
            let rows = rows
                .into_iter()
                .map(|row| {
                    Ok(ArchivedRetentionRow {
                        namespace: row.try_get(0).map_err(infrastructure)?,
                        key: row.try_get(1).map_err(infrastructure)?,
                        version: u64::try_from(row.try_get::<i64, _>(2).map_err(infrastructure)?)
                            .map_err(|_| StoreError::IncompatibleVersion)?,
                        payload: row.try_get(3).map_err(infrastructure)?,
                        retention_tick: u64::try_from(
                            row.try_get::<i64, _>(4).map_err(infrastructure)?,
                        )
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                        owner_execution: row
                            .try_get::<Option<String>, _>(5)
                            .map_err(infrastructure)?
                            .map(|owner| {
                                mainframe_env_execution_api::ExecutionId::new(
                                    owner,
                                    mainframe_env_execution_api::InvocationLimits::default(),
                                )
                                .map_err(|_| StoreError::IncompatibleVersion)
                            })
                            .transpose()?,
                    })
                })
                .collect::<Result<Vec<_>, StoreError>>()?;
            let actual_bytes = crate::retention::archive_storage_bytes(&rows)?;
            if rows.len() != row_count || actual_bytes != payload_bytes {
                return Err(StoreError::IncompatibleVersion);
            }
            let archive = crate::retention::build_archive(
                stored_target,
                archived_tick,
                u64::try_from(manifest.try_get::<i64, _>(3).map_err(infrastructure)?)
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                rows,
            )?;
            if archive.archive_id != archive_id {
                return Err(StoreError::IncompatibleVersion);
            }
            archives.push(archive);
        }
        Ok(archives)
    }

    pub(crate) fn delete_retention_archives(
        &self,
        archives: &[RetentionArchive],
    ) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            sqlx::query("UPDATE retention_lock SET epoch=epoch WHERE singleton=1")
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?;
            for archive in archives {
                sqlx::query("DELETE FROM retention_archive_row WHERE archive_id=?")
                    .bind(&archive.archive_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                let deleted = sqlx::query("DELETE FROM retention_archive WHERE archive_id=?")
                    .bind(&archive.archive_id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                if deleted != 1 {
                    return Err(StoreError::Conflict);
                }
            }
            transaction.commit().await.map_err(infrastructure)
        })?
    }

    pub fn integrity_check(&self) -> Result<(), StoreError> {
        let result: String =
            self.run(sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&self.pool))?;
        if result == "ok" {
            Ok(())
        } else {
            Err(StoreError::Infrastructure(result))
        }
    }

    pub fn backup_to(&self, destination: &Path) -> Result<(), StoreError> {
        if destination.as_os_str().is_empty() || destination.exists() {
            return Err(StoreError::AlreadyExists);
        }
        let destination = destination
            .to_str()
            .ok_or(StoreError::IncompatibleVersion)?;
        self.run(
            sqlx::query("VACUUM INTO ?")
                .bind(destination)
                .execute(&self.pool),
        )?;
        Ok(())
    }
}

async fn finish_read_transaction<T>(
    transaction: Transaction<'_, Sqlite>,
    outcome: Result<T, StoreError>,
) -> Result<T, StoreError> {
    match outcome {
        Ok(value) => {
            transaction.commit().await.map_err(infrastructure)?;
            Ok(value)
        }
        Err(problem) => {
            transaction.rollback().await.map_err(infrastructure)?;
            Err(problem)
        }
    }
}

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
        if max == 0 || max > mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = self.run(
            sqlx::query(
                "SELECT key,version,payload FROM provider_state \
                 WHERE namespace=? ORDER BY key LIMIT ?",
            )
            .bind(namespace)
            .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
            .fetch_all(&self.pool),
        )?;
        rows.into_iter()
            .map(|row| {
                Ok(ProviderStateRecord {
                    namespace: namespace.into(),
                    key: row
                        .try_get(0)
                        .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
                    version: u64::try_from(
                        row.try_get::<i64, _>(1)
                            .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
                    )
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                    payload: row
                        .try_get(2)
                        .map_err(|error| StoreError::Infrastructure(error.to_string()))?,
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

async fn insert_sqlite_archive(
    transaction: &mut Transaction<'_, Sqlite>,
    archive: &RetentionArchive,
    payload_bytes: u64,
) -> Result<(), StoreError> {
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO retention_archive(archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes) VALUES(?,?,?,?,?,?)",
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
            "INSERT INTO retention_archive_row(archive_id,ordinal,namespace,key,source_version,payload,retention_tick,owner_execution) VALUES(?,?,?,?,?,?,?,?)",
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
        delete_sqlite_observation(transaction, archive.target, row).await?;
    }
    Ok(())
}

async fn delete_sqlite_observation(
    transaction: &mut Transaction<'_, Sqlite>,
    target: RetentionTarget,
    row: &ArchivedRetentionRow,
) -> Result<(), StoreError> {
    let current = sqlx::query(
        "SELECT source_version,source_digest,observed_tick,owner_execution FROM retention_observation \
         WHERE target=? AND namespace=? AND key=?",
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
    sqlx::query("DELETE FROM retention_observation WHERE target=? AND namespace=? AND key=?")
        .bind(crate::retention::target_name(target))
        .bind(&row.namespace)
        .bind(&row.key)
        .execute(&mut **transaction)
        .await
        .map_err(infrastructure)?;
    Ok(())
}

async fn validate_sqlite_provider_observation(
    transaction: &mut Transaction<'_, Sqlite>,
    target: RetentionTarget,
    candidate: &ProviderRetentionRow,
) -> Result<(), StoreError> {
    let Some(proof) = &candidate.observation else {
        return Ok(());
    };
    let row = sqlx::query(
        "SELECT observation_version,source_version,source_digest,observed_tick,owner_execution \
         FROM retention_observation WHERE target=? AND namespace=? AND key=?",
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

fn infrastructure(error: sqlx::Error) -> StoreError {
    StoreError::Infrastructure(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn async_context_roundtrip(directory: &Path) {
        let path = directory.join("async-context.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let store = SqliteStateStore::open(&url, 1024, 16).unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "async-shell".into(),
                    key: "ready".into(),
                    version: 1,
                    payload: b"yes".to_vec(),
                },
                None,
            )
            .unwrap();
        assert_eq!(
            store
                .get_provider_state("async-shell", "ready")
                .unwrap()
                .unwrap()
                .payload,
            b"yes"
        );
    }

    #[test]
    fn sqlite_adapter_runs_inside_multithread_and_current_thread_async_shells() {
        for current_thread in [false, true] {
            let directory = std::env::temp_dir().join(format!(
                "mainframe-env-async-sqlite-{}-{:?}-{current_thread}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&directory).unwrap();
            if current_thread {
                Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async { async_context_roundtrip(&directory) });
            } else {
                Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async { async_context_roundtrip(&directory) });
            }
            std::fs::remove_file(directory.join("async-context.db")).unwrap();
            std::fs::remove_dir(directory).unwrap();
        }
    }

    #[test]
    fn sqlite_state_survives_reopen() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-sqlite-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = directory.join("state.db");
        std::fs::create_dir_all(&directory).unwrap();
        let url = format!("sqlite://{}?mode=rwc", path.display());
        {
            let store = SqliteStateStore::open(&url, 1024, 16).unwrap();
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "test".into(),
                        key: "one".into(),
                        version: 1,
                        payload: b"value".to_vec(),
                    },
                    None,
                )
                .unwrap();
        }
        {
            let store = SqliteStateStore::open(&url, 1024, 16).unwrap();
            assert_eq!(
                store
                    .get_provider_state("test", "one")
                    .unwrap()
                    .unwrap()
                    .payload,
                b"value"
            );
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn sqlite_mixed_provider_batch_rolls_back_every_operation_on_conflict() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-sqlite-mixed-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("state.db");
        let store =
            SqliteStateStore::open(&format!("sqlite://{}?mode=rwc", path.display()), 1024, 16)
                .unwrap();
        for key in ["a", "b"] {
            store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: key.into(),
                        version: 1,
                        payload: key.as_bytes().to_vec(),
                    },
                    None,
                )
                .unwrap();
        }
        assert!(matches!(
            store.mutate_provider_states_atomic(vec![
                ProviderStateMutation::Delete {
                    namespace: "dataset".into(),
                    key: "a".into(),
                    expected_version: 1,
                },
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: "b".into(),
                        version: 2,
                        payload: b"B2".to_vec(),
                    },
                    expected_version: Some(99),
                }),
            ]),
            Err(StoreError::Conflict)
        ));
        assert!(store.get_provider_state("dataset", "a").unwrap().is_some());
        assert_eq!(
            store
                .get_provider_state("dataset", "b")
                .unwrap()
                .unwrap()
                .version,
            1
        );
        store
            .mutate_provider_states_atomic(vec![
                ProviderStateMutation::Delete {
                    namespace: "dataset".into(),
                    key: "a".into(),
                    expected_version: 1,
                },
                ProviderStateMutation::Put(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: "dataset".into(),
                        key: "b".into(),
                        version: 2,
                        payload: b"B2".to_vec(),
                    },
                    expected_version: Some(1),
                }),
            ])
            .unwrap();
        assert_eq!(store.get_provider_state("dataset", "a").unwrap(), None);
        assert_eq!(
            store
                .get_provider_state("dataset", "b")
                .unwrap()
                .unwrap()
                .payload,
            b"B2"
        );
        drop(store);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn backup_restores_to_an_integrity_checked_database() {
        let directory = std::env::temp_dir().join(format!(
            "mainframe-env-sqlite-backup-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.db");
        let backup = directory.join("backup.db");
        let store =
            SqliteStateStore::open(&format!("sqlite://{}?mode=rwc", source.display()), 1024, 16)
                .unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "backup".into(),
                    key: "one".into(),
                    version: 1,
                    payload: b"durable".to_vec(),
                },
                None,
            )
            .unwrap();
        store.integrity_check().unwrap();
        store.backup_to(&backup).unwrap();
        drop(store);
        let restored =
            SqliteStateStore::open(&format!("sqlite://{}?mode=rw", backup.display()), 1024, 16)
                .unwrap();
        restored.integrity_check().unwrap();
        assert_eq!(
            restored
                .get_provider_state("backup", "one")
                .unwrap()
                .unwrap()
                .payload,
            b"durable"
        );
        drop(restored);
        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(backup);
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn archive_epoch_rejects_a_dependency_inserted_after_candidate_scan() {
        let store = SqliteStateStore::open("sqlite::memory:", 1024, 8).unwrap();
        let source = ProviderStateRecord {
            namespace: "durable-event:epoch-race".into(),
            key: "00000000000000000001".into(),
            version: 1,
            payload: b"source".to_vec(),
        };
        store.put_provider_state(source.clone(), None).unwrap();
        let scanned_epoch = store.retention_epoch().unwrap();
        store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: "durable-checkpoint".into(),
                    key: "late".into(),
                    version: 1,
                    payload: b"dependency".to_vec(),
                },
                None,
            )
            .unwrap();
        let archive = crate::retention::build_archive(
            RetentionTarget::LifecycleEvents,
            20,
            10,
            vec![ArchivedRetentionRow {
                namespace: source.namespace.clone(),
                key: source.key.clone(),
                version: source.version,
                payload: source.payload.clone(),
                retention_tick: 10,
                owner_execution: Some(
                    mainframe_env_execution_api::ExecutionId::new(
                        "epoch-race",
                        mainframe_env_execution_api::InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
            }],
        )
        .unwrap();
        assert_eq!(
            store.commit_retention_archive(&archive, scanned_epoch),
            Err(StoreError::Conflict)
        );
        assert!(
            store
                .get_provider_state(&source.namespace, &source.key)
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .load_retention_archives(None, 8, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn archive_rows_store_a_maximum_size_source_without_envelope_expansion() {
        let store = SqliteStateStore::open("sqlite::memory:", 64, 4).unwrap();
        let source = ProviderStateRecord {
            namespace: "durable-event:maximum".into(),
            key: "00000000000000000001".into(),
            version: 1,
            payload: vec![7; 64],
        };
        store.put_provider_state(source.clone(), None).unwrap();
        let epoch = store.retention_epoch().unwrap();
        let archive = crate::retention::build_archive(
            RetentionTarget::LifecycleEvents,
            20,
            10,
            vec![ArchivedRetentionRow {
                namespace: source.namespace.clone(),
                key: source.key.clone(),
                version: source.version,
                payload: source.payload,
                retention_tick: 10,
                owner_execution: Some(
                    mainframe_env_execution_api::ExecutionId::new(
                        "maximum",
                        mainframe_env_execution_api::InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
            }],
        )
        .unwrap();
        store.commit_retention_archive(&archive, epoch).unwrap();
        let loaded = store
            .load_retention_archives(Some(RetentionTarget::LifecycleEvents), 1, None)
            .unwrap();
        assert_eq!(loaded, vec![archive]);
    }

    #[test]
    fn corrupt_archive_payload_fails_closed_before_prune() {
        let store = SqliteStateStore::open("sqlite::memory:", 1024, 4).unwrap();
        let source = ProviderStateRecord {
            namespace: "durable-event:corruption".into(),
            key: "00000000000000000001".into(),
            version: 1,
            payload: b"original".to_vec(),
        };
        store.put_provider_state(source.clone(), None).unwrap();
        let epoch = store.retention_epoch().unwrap();
        let archive = crate::retention::build_archive(
            RetentionTarget::LifecycleEvents,
            20,
            10,
            vec![ArchivedRetentionRow {
                namespace: source.namespace,
                key: source.key,
                version: source.version,
                payload: source.payload,
                retention_tick: 10,
                owner_execution: Some(
                    mainframe_env_execution_api::ExecutionId::new(
                        "corruption",
                        mainframe_env_execution_api::InvocationLimits::default(),
                    )
                    .unwrap(),
                ),
            }],
        )
        .unwrap();
        store.commit_retention_archive(&archive, epoch).unwrap();
        store
            .run(
                sqlx::query(
                    "UPDATE retention_archive_row SET payload=? WHERE archive_id=? AND ordinal=0",
                )
                .bind(b"corrupt".to_vec())
                .bind(&archive.archive_id)
                .execute(&store.pool),
            )
            .unwrap();
        assert_eq!(
            store.load_retention_archives(None, 4, None),
            Err(StoreError::IncompatibleVersion)
        );
    }

    #[test]
    fn reopen_recomputes_archive_and_observation_accounting() {
        for corrupt_archive in [true, false] {
            let directory = std::env::temp_dir().join(format!(
                "mainframe-env-retention-accounting-{}-{:?}-{corrupt_archive}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join("retention.db");
            let url = format!("sqlite://{}?mode=rwc", path.display());
            let store = SqliteStateStore::open(&url, 1024, 8).unwrap();
            if corrupt_archive {
                store
                    .run(
                        sqlx::query(
                            "INSERT INTO retention_archive(archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes) VALUES('forged','lifecycle-events',20,10,1,1)",
                        )
                        .execute(&store.pool),
                    )
                    .unwrap();
            } else {
                store
                    .run(
                        sqlx::query(
                            "INSERT INTO retention_observation(target,namespace,key,observation_version,source_version,source_digest,observed_tick,owner_execution,accounted_bytes) VALUES('console-log','console-log','0000000000000001',1,1,?,10,NULL,1)",
                        )
                        .bind(vec![0_u8; 32])
                        .execute(&store.pool),
                    )
                    .unwrap();
            }
            drop(store);
            assert!(matches!(
                SqliteStateStore::open(&url, 1024, 8),
                Err(StoreError::IncompatibleVersion)
            ));
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_dir(directory);
        }
    }
}
