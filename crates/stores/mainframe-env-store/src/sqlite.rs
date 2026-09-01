use crate::runtime::{AdapterRuntime, block_on};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::future::Future;
use std::path::Path;
use tokio::runtime::Builder;

pub struct SqliteStateStore {
    runtime: AdapterRuntime,
    pool: SqlitePool,
    max_payload_bytes: usize,
    max_rows: usize,
}

impl SqliteStateStore {
    pub fn open(url: &str, max_payload_bytes: usize, max_rows: usize) -> Result<Self, StoreError> {
        if max_payload_bytes == 0 || max_rows == 0 {
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
            sqlx::query(include_str!("../migrations/sqlite/0001-durable-state.sql")).execute(&pool),
        )?
        .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        Ok(Self {
            runtime,
            pool,
            max_payload_bytes,
            max_rows,
        })
    }

    fn run<F, T>(&self, future: F) -> Result<T, StoreError>
    where
        F: Future<Output = Result<T, sqlx::Error>> + Send,
        T: Send,
    {
        block_on(&self.runtime, future)?
            .map_err(|error| StoreError::Infrastructure(error.to_string()))
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

impl ProviderStateStore for SqliteStateStore {
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
        if max == 0 || max > self.max_rows {
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
            self.run(
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
            )?
            .rows_affected()
        } else {
            if record.version != 1 {
                return Err(StoreError::Conflict);
            }
            let count: i64 = self.run(
                sqlx::query_scalar("SELECT COUNT(*) FROM provider_state").fetch_one(&self.pool),
            )?;
            if usize::try_from(count).map_err(|_| StoreError::CapacityExceeded)? >= self.max_rows {
                return Err(StoreError::CapacityExceeded);
            }
            self.run(
                sqlx::query(
                    "INSERT OR IGNORE INTO provider_state(namespace,key,version,payload) \
                     VALUES(?,?,?,?)",
                )
                .bind(record.namespace)
                .bind(record.key)
                .bind(1i64)
                .bind(record.payload)
                .execute(&self.pool),
            )?
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
            .run(
                sqlx::query("DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?")
                    .bind(namespace)
                    .bind(key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&self.pool),
            )?
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
        block_on(&self.runtime, async {
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
            let deleted =
                sqlx::query("DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?")
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
        })?
    }

    fn put_provider_states_atomic(
        &self,
        writes: Vec<ProviderStateWrite>,
    ) -> Result<(), StoreError> {
        if writes.is_empty()
            || writes
                .iter()
                .any(|write| write.record.payload.len() > self.max_payload_bytes)
        {
            return Err(StoreError::PayloadTooLarge);
        }
        block_on(&self.runtime, async {
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            let creates = writes
                .iter()
                .filter(|write| write.expected_version.is_none())
                .count();
            if usize::try_from(count)
                .map_err(|_| StoreError::CapacityExceeded)?
                .checked_add(creates)
                .is_none_or(|total| total > self.max_rows)
            {
                return Err(StoreError::CapacityExceeded);
            }
            for write in writes {
                let record = write.record;
                let affected = if let Some(expected) = write.expected_version {
                    if record.version != expected.checked_add(1).ok_or(StoreError::Conflict)? {
                        return Err(StoreError::Conflict);
                    }
                    sqlx::query(
                        "UPDATE provider_state SET version=?,payload=? WHERE namespace=? AND key=? AND version=?",
                    )
                    .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                    .bind(record.payload)
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                    .rows_affected()
                } else {
                    if record.version != 1 {
                        return Err(StoreError::Conflict);
                    }
                    sqlx::query(
                        "INSERT OR IGNORE INTO provider_state(namespace,key,version,payload) VALUES(?,?,1,?)",
                    )
                    .bind(record.namespace)
                    .bind(record.key)
                    .bind(record.payload)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                    .rows_affected()
                };
                if affected != 1 {
                    return Err(StoreError::Conflict);
                }
            }
            transaction
                .commit()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))
        })?
    }

    fn mutate_provider_states_atomic(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        if mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        if mutations.iter().any(|mutation| {
            matches!(mutation, ProviderStateMutation::Put(write) if write.record.payload.len() > self.max_payload_bytes)
        }) {
            return Err(StoreError::PayloadTooLarge);
        }
        block_on(&self.runtime, async {
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
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
                                "UPDATE provider_state SET version=?,payload=? WHERE namespace=? AND key=? AND version=?",
                            )
                            .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                            .bind(record.payload)
                            .bind(record.namespace)
                            .bind(record.key)
                            .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                            .execute(&mut *transaction)
                            .await
                            .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                            .rows_affected()
                        } else {
                            if record.version != 1 {
                                return Err(StoreError::Conflict);
                            }
                            sqlx::query(
                                "INSERT OR IGNORE INTO provider_state(namespace,key,version,payload) VALUES(?,?,1,?)",
                            )
                            .bind(record.namespace)
                            .bind(record.key)
                            .bind(record.payload)
                            .execute(&mut *transaction)
                            .await
                            .map_err(|error| StoreError::Infrastructure(error.to_string()))?
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
                            "DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?",
                        )
                        .bind(namespace)
                        .bind(key)
                        .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                        .execute(&mut *transaction)
                        .await
                        .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                        .rows_affected()
                    }
                    ProviderStateMutation::Move {
                        record,
                        old_key,
                        expected_version,
                    } => {
                        if record.payload.len() > self.max_payload_bytes
                            || record.key == old_key
                            || record.version
                                != expected_version
                                    .checked_add(1)
                                    .ok_or(StoreError::Conflict)?
                        {
                            return Err(StoreError::Conflict);
                        }
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
                        u64::from(inserted == 1 && deleted == 1)
                    }
                };
                if affected != 1 {
                    return Err(StoreError::Conflict);
                }
            }
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state")
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            if usize::try_from(count).map_err(|_| StoreError::CapacityExceeded)? > self.max_rows {
                return Err(StoreError::CapacityExceeded);
            }
            transaction
                .commit()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))
        })?
    }
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
}
