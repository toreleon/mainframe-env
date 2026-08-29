use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, StoreError};
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tokio::runtime::{Builder, Runtime};

pub struct SqliteStateStore {
    runtime: Runtime,
    pool: SqlitePool,
    max_payload_bytes: usize,
    max_rows: usize,
}

impl SqliteStateStore {
    pub fn open(url: &str, max_payload_bytes: usize, max_rows: usize) -> Result<Self, StoreError> {
        if max_payload_bytes == 0 || max_rows == 0 {
            return Err(StoreError::CapacityExceeded);
        }
        let runtime = Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        let pool = runtime
            .block_on(SqlitePoolOptions::new().max_connections(1).connect(url))
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        runtime
            .block_on(
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS provider_state (\
                     namespace TEXT NOT NULL, key TEXT NOT NULL, version INTEGER NOT NULL, \
                     payload BLOB NOT NULL, PRIMARY KEY(namespace,key))",
                )
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

impl ProviderStateStore for SqliteStateStore {
    fn get_provider_state(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        let row = self
            .runtime
            .block_on(
                sqlx::query(
                    "SELECT version,payload FROM provider_state WHERE namespace=? AND key=?",
                )
                .bind(namespace)
                .bind(key)
                .fetch_optional(&self.pool),
            )
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
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
        if max == 0 || max > self.max_rows {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = self
            .runtime
            .block_on(
                sqlx::query(
                    "SELECT key,version,payload FROM provider_state \
                     WHERE namespace=? ORDER BY key LIMIT ?",
                )
                .bind(namespace)
                .bind(i64::try_from(max).map_err(|_| StoreError::CapacityExceeded)?)
                .fetch_all(&self.pool),
            )
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
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
                        "UPDATE provider_state SET version=?,payload=? \
                         WHERE namespace=? AND key=? AND version=?",
                    )
                    .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                    .bind(record.payload)
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&self.pool),
                )
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                .rows_affected()
        } else {
            if record.version != 1 {
                return Err(StoreError::Conflict);
            }
            self.runtime
                .block_on(
                    sqlx::query(
                        "INSERT OR IGNORE INTO provider_state(namespace,key,version,payload) \
                         VALUES(?,?,?,?)",
                    )
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(1i64)
                    .bind(record.payload)
                    .execute(&self.pool),
                )
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?
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
                sqlx::query("DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?")
                    .bind(namespace)
                    .bind(key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&self.pool),
            )
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?
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
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            let inserted = sqlx::query(
                "INSERT OR IGNORE INTO provider_state(namespace,key,version,payload) VALUES(?,?,?,?)",
            )
            .bind(&record.namespace)
            .bind(&record.key)
            .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
            .bind(&record.payload)
            .execute(&mut *transaction)
            .await
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?
            .rows_affected();
            let deleted = sqlx::query(
                "DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?",
            )
            .bind(&record.namespace)
            .bind(old_key)
            .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
            .execute(&mut *transaction)
            .await
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?
            .rows_affected();
            if inserted != 1 || deleted != 1 {
                return Err(StoreError::Conflict);
            }
            transaction
                .commit()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
