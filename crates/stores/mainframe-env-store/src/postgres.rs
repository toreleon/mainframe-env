use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio::runtime::{Builder, Runtime};

pub struct PostgresStateStore {
    runtime: Runtime,
    pool: PgPool,
    max_payload_bytes: usize,
    max_rows: usize,
}

impl PostgresStateStore {
    pub fn open(url: &str, max_payload_bytes: usize, max_rows: usize) -> Result<Self, StoreError> {
        if max_payload_bytes == 0 || max_rows == 0 {
            return Err(StoreError::CapacityExceeded);
        }
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        let pool = runtime
            .block_on(PgPoolOptions::new().max_connections(4).connect(url))
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        runtime
            .block_on(
                sqlx::query(include_str!(
                    "../migrations/postgres/0001-durable-state.sql"
                ))
                .execute(&pool),
            )
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        Ok(Self {
            runtime,
            pool,
            max_payload_bytes,
            max_rows,
        })
    }
}

impl ProviderStateStore for PostgresStateStore {
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        let row = self
            .runtime
            .block_on(
                sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=$1 AND key=$2",
                )
                .bind(namespace)
                .bind(key)
                .fetch_optional(&self.pool),
            )
            .map_err(infrastructure)?;
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
        if max == 0 || max > self.max_rows {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = self
            .runtime
            .block_on(
                sqlx::query(
                    "SELECT key,version,payload FROM provider_state WHERE namespace=$1 ORDER BY key LIMIT $2",
                )
                .bind(namespace)
                .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
                .fetch_all(&self.pool),
            )
            .map_err(infrastructure)?;
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

    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        if record.payload.len() > self.max_payload_bytes || record.version == 0 {
            return Err(StoreError::PayloadTooLarge);
        }
        let affected = if let Some(expected) = expected {
            if record.version != expected + 1 {
                return Err(StoreError::Conflict);
            }
            self.runtime
                .block_on(
                    sqlx::query(
                        "UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4 AND version=$5",
                    )
                    .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                    .bind(record.payload)
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&self.pool),
                )
                .map_err(infrastructure)?
                .rows_affected()
        } else {
            if record.version != 1 {
                return Err(StoreError::Conflict);
            }
            let count: i64 = self
                .runtime
                .block_on(
                    sqlx::query_scalar("SELECT COUNT(*) FROM provider_state").fetch_one(&self.pool),
                )
                .map_err(infrastructure)?;
            if usize::try_from(count).map_err(|_| StoreError::CapacityExceeded)? >= self.max_rows {
                return Err(StoreError::CapacityExceeded);
            }
            self.runtime
                .block_on(
                    sqlx::query(
                        "INSERT INTO provider_state(namespace,key,version,payload) VALUES($1,$2,1,$3) ON CONFLICT DO NOTHING",
                    )
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(record.payload)
                    .execute(&self.pool),
                )
                .map_err(infrastructure)?
                .rows_affected()
        };
        if affected == 1 {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        }
    }

    fn delete_provider_state(
        &self,
        namespace: &str,
        key: &str,
        expected: u64,
    ) -> Result<(), StoreError> {
        let affected = self
            .runtime
            .block_on(
                sqlx::query(
                    "DELETE FROM provider_state WHERE namespace=$1 AND key=$2 AND version=$3",
                )
                .bind(namespace)
                .bind(key)
                .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                .execute(&self.pool),
            )
            .map_err(infrastructure)?
            .rows_affected();
        if affected == 1 {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        }
    }

    fn move_provider_state(
        &self,
        record: ProviderStateRecord,
        old_key: &str,
        expected_version: u64,
    ) -> Result<(), StoreError> {
        if record.payload.len() > self.max_payload_bytes
            || record.key == old_key
            || record.version
                != expected_version
                    .checked_add(1)
                    .ok_or(StoreError::Conflict)?
        {
            return Err(StoreError::Conflict);
        }
        self.runtime.block_on(async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
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
            transaction.commit().await.map_err(infrastructure)
        })
    }
}

fn infrastructure(error: sqlx::Error) -> StoreError {
    StoreError::Infrastructure(error.to_string())
}
