use crate::runtime::{AdapterRuntime, block_on};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Row, Transaction};
use std::future::Future;
use tokio::runtime::Builder;

pub(crate) const PROVIDER_STATE_QUOTA: &str = "provider-state";
pub(crate) const ARTIFACT_OBJECT_QUOTA: &str = "artifact-object";

pub struct PostgresStateStore {
    runtime: AdapterRuntime,
    pool: PgPool,
    max_payload_bytes: usize,
    max_rows: usize,
}

impl PostgresStateStore {
    pub fn open(url: &str, max_payload_bytes: usize, max_rows: usize) -> Result<Self, StoreError> {
        if max_payload_bytes == 0 || max_rows == 0 {
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
        block_on(&runtime, async {
            initialize_quota(
                &pool,
                PROVIDER_STATE_QUOTA,
                max_rows,
                "SELECT COUNT(*) FROM provider_state",
            )
            .await
        })??;
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
        block_on(&self.runtime, future)?.map_err(infrastructure)
    }

    pub(crate) const fn max_rows(&self) -> usize {
        self.max_rows
    }
}

impl ProviderStateStore for PostgresStateStore {
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
        if max == 0 || max > self.max_rows {
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

    fn put_provider_state(
        &self,
        record: ProviderStateRecord,
        expected: Option<u64>,
    ) -> Result<(), StoreError> {
        if record.payload.len() > self.max_payload_bytes || record.version == 0 {
            return Err(StoreError::PayloadTooLarge);
        }
        let affected = if let Some(expected) = expected {
            if record.version != expected.checked_add(1).ok_or(StoreError::Conflict)? {
                return Err(StoreError::Conflict);
            }
            self.run(
                sqlx::query(
                    "UPDATE provider_state SET version=$1,payload=$2 WHERE namespace=$3 AND key=$4 AND version=$5",
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
            return block_on(&self.runtime, async {
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
            })?;
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
        if writes.is_empty()
            || writes
                .iter()
                .any(|write| write.record.payload.len() > self.max_payload_bytes)
        {
            return Err(StoreError::PayloadTooLarge);
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
        if mutations.iter().any(|mutation| {
            matches!(mutation, ProviderStateMutation::Put(write) if write.record.payload.len() > self.max_payload_bytes)
        }) {
            return Err(StoreError::PayloadTooLarge);
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

fn infrastructure(error: sqlx::Error) -> StoreError {
    StoreError::Infrastructure(error.to_string())
}
