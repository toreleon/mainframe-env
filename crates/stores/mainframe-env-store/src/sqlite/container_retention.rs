//! Atomic provider replay archive and capacity replacement.

use super::*;

impl SqliteStateStore {
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
            if usize::try_from(used_rows)
                .map_err(|_| StoreError::IncompatibleVersion)?
                .checked_add(archive.rows.len())
                .is_none_or(|rows| rows > self.max_archive_rows())
                || u64::try_from(used_bytes)
                    .map_err(|_| StoreError::IncompatibleVersion)?
                    .checked_add(storage_bytes)
                    .is_none_or(|bytes| bytes > self.max_archive_bytes())
            {
                return Err(StoreError::CapacityExceeded);
            }
            for candidate in &candidates {
                let current = sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
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
                validate_sqlite_provider_observation(&mut transaction, request.target, candidate)
                    .await?;
                validate_sqlite_provider_dependency(&mut transaction, candidate).await?;
            }
            if let Some((outer_receipts, source, _)) = &capacity {
                for outer in outer_receipts {
                    let current = sqlx::query(
                        "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
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
                    "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
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
                    || current.try_get::<Vec<u8>, _>(1).map_err(infrastructure)? != source.payload
                {
                    return Err(StoreError::Conflict);
                }
            }
            insert_sqlite_archive(&mut transaction, &archive, storage_bytes).await?;
            for candidate in &candidates {
                let deleted = sqlx::query(
                    "DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?",
                )
                .bind(&candidate.row.namespace)
                .bind(&candidate.row.key)
                .bind(i64::try_from(candidate.row.version).map_err(|_| StoreError::Conflict)?)
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
                    "UPDATE provider_state SET version=?,payload=? \
                     WHERE namespace=? AND key=? AND version=? AND payload=?",
                )
                .bind(i64::try_from(replacement.record.version).map_err(|_| StoreError::Conflict)?)
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
            transaction.commit().await.map_err(infrastructure)
        })??;
        Ok(archive)
    }
}
