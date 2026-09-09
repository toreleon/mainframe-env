use super::infrastructure;
use mainframe_env_store_api::{
    ArchivedRetentionRow, RetentionArchive, RetentionTarget, StoreError,
};
use sqlx::Row;
use sqlx::postgres::PgConnection;

pub(super) async fn load_archives(
    connection: &mut PgConnection,
    target: Option<RetentionTarget>,
    max: usize,
    through_tick: Option<u64>,
    max_archive_rows: usize,
) -> Result<Vec<RetentionArchive>, StoreError> {
    let manifests = sqlx::query(
        "SELECT archive_id,target,archived_tick,watermark_tick,row_count,payload_bytes \
         FROM retention_archive WHERE ($1::text IS NULL OR target=$1) \
         AND ($2::bigint IS NULL OR archived_tick<=$2) \
         ORDER BY archived_tick,archive_id LIMIT $3",
    )
    .bind(target.map(crate::retention::target_name))
    .bind(through_tick.map(|tick| i64::try_from(tick).unwrap_or(i64::MAX)))
    .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
    .fetch_all(&mut *connection)
    .await
    .map_err(infrastructure)?;
    let mut selected_rows = 0usize;
    let mut archives = Vec::new();
    for manifest in manifests {
        let stored_target = crate::retention::target_back(
            &manifest.try_get::<String, _>(1).map_err(infrastructure)?,
        )?;
        let archived_tick = u64::try_from(manifest.try_get::<i64, _>(2).map_err(infrastructure)?)
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
        let payload_bytes = u64::try_from(manifest.try_get::<i64, _>(5).map_err(infrastructure)?)
            .map_err(|_| StoreError::IncompatibleVersion)?;
        let rows = sqlx::query(
            "SELECT namespace,key,source_version,payload,retention_tick,owner_execution FROM retention_archive_row WHERE archive_id=$1 ORDER BY ordinal LIMIT $2",
        )
        .bind(&archive_id)
        .bind(
            i64::try_from(
                row_count
                    .checked_add(1)
                    .ok_or(StoreError::CapacityExceeded)?,
            )
            .map_err(|_| StoreError::CapacityExceeded)?,
        )
        .fetch_all(&mut *connection)
        .await
        .map_err(infrastructure)?;
        if rows.len() > max_archive_rows {
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
