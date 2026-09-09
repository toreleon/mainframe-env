use crate::postgres::{ARTIFACT_OBJECT_QUOTA, adjust_quota, finish_transaction, initialize_quota};
use crate::runtime::{AdapterRuntime, block_on};
use crate::validation;
use mainframe_env_execution_api::ArtifactRef;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, ArtifactStoreHealth, StoreError};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::runtime::Builder;

static HEALTH_PROBE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// A PostgreSQL-backed immutable object store shared by every product node.
pub struct PostgresArtifactStore {
    runtime: AdapterRuntime,
    pool: PgPool,
    max_artifact_bytes: usize,
    max_objects: usize,
}

impl PostgresArtifactStore {
    pub fn open(
        url: &str,
        max_artifact_bytes: usize,
        max_objects: usize,
    ) -> Result<Self, StoreError> {
        if max_artifact_bytes == 0 || max_objects == 0 {
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
        .map_err(infrastructure)?;
        block_on(
            &runtime,
            sqlx::raw_sql(include_str!(
                "../migrations/postgres/0001-durable-state.sql"
            ))
            .execute(&pool),
        )?
        .map_err(infrastructure)?;
        block_on(&runtime, async {
            initialize_quota(
                &pool,
                ARTIFACT_OBJECT_QUOTA,
                max_objects,
                "SELECT COUNT(*) FROM artifact_object",
            )
            .await
        })??;
        Ok(Self {
            runtime,
            pool,
            max_artifact_bytes,
            max_objects,
        })
    }

    fn run<F, T>(&self, future: F) -> Result<T, StoreError>
    where
        F: Future<Output = Result<T, sqlx::Error>> + Send,
        T: Send,
    {
        block_on(&self.runtime, future)?.map_err(infrastructure)
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.health().is_ok_and(ArtifactStoreHealth::ready)
    }
}

impl ArtifactStore for PostgresArtifactStore {
    fn health(&self) -> Result<ArtifactStoreHealth, StoreError> {
        let expected_max =
            i64::try_from(self.max_objects).map_err(|_| StoreError::CapacityExceeded)?;
        let probe_key = format!(
            "readiness-probe:{}:{}",
            std::process::id(),
            HEALTH_PROBE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let row = sqlx::query(
                    "SELECT max_rows,used_rows FROM store_quota WHERE quota_key=$1 FOR UPDATE",
                )
                .bind(ARTIFACT_OBJECT_QUOTA)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::IncompatibleVersion)?;
                let max_rows: i64 = row.try_get(0).map_err(infrastructure)?;
                let used_rows: i64 = row.try_get(1).map_err(infrastructure)?;
                let actual: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artifact_object")
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(infrastructure)?;
                if max_rows != expected_max
                    || used_rows < 0
                    || actual != used_rows
                    || used_rows > max_rows
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                let affected = sqlx::query(
                    "UPDATE store_quota SET used_rows=used_rows WHERE quota_key=$1 AND max_rows=$2",
                )
                .bind(ARTIFACT_OBJECT_QUOTA)
                .bind(expected_max)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if affected != 1 {
                    return Err(StoreError::Conflict);
                }
                let inserted = sqlx::query(
                    "INSERT INTO artifact_object(object_key,schema_version,media_type,payload_digest,payload) VALUES($1,1,'application/vnd.mainframe-env.readiness',decode(repeat('00',32),'hex'),''::bytea)",
                )
                .bind(probe_key)
                .execute(&mut *transaction)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                if inserted != 1 {
                    return Err(StoreError::Conflict);
                }
                Ok(ArtifactStoreHealth {
                    readable: true,
                    writable: true,
                    used_objects: Some(
                        usize::try_from(used_rows).map_err(|_| StoreError::IncompatibleVersion)?,
                    ),
                    max_objects: Some(self.max_objects),
                    used_bytes: None,
                    max_bytes: None,
                })
            }
            .await;
            transaction.rollback().await.map_err(infrastructure)?;
            outcome
        })?
    }

    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        if record.payload.len() > self.max_artifact_bytes
            || record.media_type.is_empty()
            || record.media_type.len() > 4_096
        {
            return Err(StoreError::PayloadTooLarge);
        }
        validation::artifact(&record)?;
        let executable = record
            .executable
            .as_ref()
            .map(validation::encode_executable_metadata)
            .transpose()?;
        if let Some(existing) = self.get_artifact(&record.artifact)? {
            return if existing == record {
                Ok(())
            } else {
                Err(StoreError::Conflict)
            };
        }
        let artifact = record.artifact.as_str().to_string();
        let attempted = block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(&mut transaction, ARTIFACT_OBJECT_QUOTA, self.max_objects, 1).await?;
                let affected = sqlx::query(
                    "INSERT INTO artifact_object(object_key,schema_version,media_type,payload_digest,payload,executable_metadata) VALUES($1,2,$2,$3,$4,$5) ON CONFLICT DO NOTHING",
                )
                .bind(artifact)
                .bind(&record.media_type)
                .bind(record.payload_digest.as_slice())
                .bind(&record.payload)
                .bind(&executable)
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
        match attempted {
            Ok(()) => Ok(()),
            Err(problem @ (StoreError::CapacityExceeded | StoreError::Conflict)) => {
                match self.get_artifact(&record.artifact)? {
                    Some(existing) if existing == record => Ok(()),
                    Some(_) => Err(StoreError::Conflict),
                    None => Err(problem),
                }
            }
            Err(problem) => Err(problem),
        }
    }

    fn get_artifact(&self, id: &ArtifactRef) -> Result<Option<ArtifactRecord>, StoreError> {
        let row = self.run(
            sqlx::query(
                "SELECT schema_version,media_type,payload_digest,payload,executable_metadata FROM artifact_object WHERE object_key=$1",
            )
            .bind(id.as_str())
            .fetch_optional(&self.pool),
        )?;
        row.map(|row| {
            let schema: i16 = row.try_get(0).map_err(infrastructure)?;
            let media_type: String = row.try_get(1).map_err(infrastructure)?;
            let digest: Vec<u8> = row.try_get(2).map_err(infrastructure)?;
            let payload: Vec<u8> = row.try_get(3).map_err(infrastructure)?;
            let executable: Option<Vec<u8>> = row.try_get(4).map_err(infrastructure)?;
            if !matches!(schema, 1 | 2)
                || media_type.is_empty()
                || media_type.len() > 4_096
                || payload.len() > self.max_artifact_bytes
                || (schema == 1 && executable.is_some())
            {
                return Err(StoreError::IncompatibleVersion);
            }
            let record = ArtifactRecord {
                artifact: id.clone(),
                media_type,
                payload_digest: digest
                    .try_into()
                    .map_err(|_| StoreError::IncompatibleVersion)?,
                payload,
                executable: executable
                    .map(|bytes| validation::decode_executable_metadata(&bytes))
                    .transpose()?,
            };
            validation::artifact(&record)?;
            Ok(record)
        })
        .transpose()
    }

    fn delete_artifact(&self, id: &ArtifactRef) -> Result<(), StoreError> {
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                adjust_quota(
                    &mut transaction,
                    ARTIFACT_OBJECT_QUOTA,
                    self.max_objects,
                    -1,
                )
                .await?;
                let affected = sqlx::query("DELETE FROM artifact_object WHERE object_key=$1")
                    .bind(id.as_str())
                    .execute(&mut *transaction)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected();
                if affected != 1 {
                    return Err(StoreError::NotFound);
                }
                Ok(())
            }
            .await;
            finish_transaction(transaction, outcome).await
        })?
    }
}

fn infrastructure(error: sqlx::Error) -> StoreError {
    StoreError::Infrastructure(error.to_string())
}
