use crate::postgres::{ARTIFACT_OBJECT_QUOTA, adjust_quota, finish_transaction, initialize_quota};
use crate::runtime::{AdapterRuntime, block_on};
use crate::validation;
use mainframe_env_execution_api::ArtifactRef;
use mainframe_env_store_api::{ArtifactRecord, ArtifactStore, StoreError};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::future::Future;
use tokio::runtime::Builder;

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
        self.run(
            sqlx::query_scalar::<_, i64>(
                "SELECT used_rows FROM store_quota WHERE quota_key=$1 AND max_rows=$2",
            )
            .bind(ARTIFACT_OBJECT_QUOTA)
            .bind(i64::try_from(self.max_objects).unwrap_or(i64::MAX))
            .fetch_optional(&self.pool),
        )
        .is_ok_and(|row| row.is_some())
    }
}

impl ArtifactStore for PostgresArtifactStore {
    fn put_artifact(&self, record: ArtifactRecord) -> Result<(), StoreError> {
        if record.payload.len() > self.max_artifact_bytes
            || record.media_type.is_empty()
            || record.media_type.len() > 4_096
        {
            return Err(StoreError::PayloadTooLarge);
        }
        validation::artifact(&record)?;
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
                    "INSERT INTO artifact_object(object_key,schema_version,media_type,payload_digest,payload) VALUES($1,1,$2,$3,$4) ON CONFLICT DO NOTHING",
                )
                .bind(artifact)
                .bind(&record.media_type)
                .bind(record.payload_digest.as_slice())
                .bind(&record.payload)
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
                "SELECT schema_version,media_type,payload_digest,payload FROM artifact_object WHERE object_key=$1",
            )
            .bind(id.as_str())
            .fetch_optional(&self.pool),
        )?;
        row.map(|row| {
            let schema: i16 = row.try_get(0).map_err(infrastructure)?;
            let media_type: String = row.try_get(1).map_err(infrastructure)?;
            let digest: Vec<u8> = row.try_get(2).map_err(infrastructure)?;
            let payload: Vec<u8> = row.try_get(3).map_err(infrastructure)?;
            if schema != 1
                || media_type.is_empty()
                || media_type.len() > 4_096
                || payload.len() > self.max_artifact_bytes
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
