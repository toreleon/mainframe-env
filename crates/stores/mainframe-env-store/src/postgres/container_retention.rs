//! Atomic provider replay archive and capacity replacement.

use super::*;

impl PostgresStateStore {
    pub(super) fn commit_provider_retention_deletion(
        &self,
        request: ProviderStateArchiveDeletion,
        capacity: Option<(
            Vec<ProviderStateRecord>,
            ProviderStateRecord,
            ProviderStateWrite,
        )>,
    ) -> Result<RetentionArchive, StoreError> {
        crate::retention::validate_provider_deletion(
            &request,
            self.max_payload_bytes,
            capacity.is_some(),
        )?;
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
                if let Some((outer_receipts, source, _)) = &capacity {
                    for outer in outer_receipts {
                        let current = sqlx::query(
                            "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                        )
                        .bind(&outer.namespace)
                        .bind(&outer.key)
                        .fetch_optional(&mut *transaction)
                        .await
                        .map_err(infrastructure)?
                        .ok_or(StoreError::Conflict)?;
                        if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                            .map_err(|_| StoreError::IncompatibleVersion)?
                            != outer.version
                            || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?
                                != outer.payload
                        {
                            return Err(StoreError::Conflict);
                        }
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
                    if u64::try_from(current.try_get::<i64, _>(0).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?
                        != source.version
                        || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)?
                            != source.payload
                    {
                        return Err(StoreError::Conflict);
                    }
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
                if let Some((_, source, replacement)) = capacity {
                    let updated = sqlx::query(
                        "UPDATE provider_state SET version=$1,payload=$2 \
                         WHERE namespace=$3 AND key=$4 AND version=$5 AND payload=$6",
                    )
                    .bind(
                        i64::try_from(replacement.record.version)
                            .map_err(|_| StoreError::Conflict)?,
                    )
                    .bind(replacement.record.payload)
                    .bind(source.namespace)
                    .bind(source.key)
                    .bind(i64::try_from(source.version).map_err(|_| StoreError::Conflict)?)
                    .bind(source.payload)
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                    if updated != 1 {
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
}
