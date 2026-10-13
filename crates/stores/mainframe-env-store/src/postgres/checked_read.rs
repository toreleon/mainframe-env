//! Physical PostgreSQL fences; no provider callbacks or dispatch permissions.
use super::*;
use crate::checked_read::{self as shared, Budget};
use crate::root_terminal::{
    ACTOR_NAMESPACE, Document, ROW_SCOPE_NAMESPACE, RUN_NAMESPACE, SCOPE_NAMESPACE, row_scope_key,
};
use mainframe_env_store_api::{
    CheckedProviderReadPublication, EffectRecord, ExecutionRecord, ProviderReplayAssertion,
    ROOT_DRIVER_NAMESPACE, TerminalRowDependency,
};

impl PostgresStateStore {
    pub(crate) const fn max_rows(&self) -> usize {
        self.max_rows
    }

    pub(crate) const fn max_payload_bytes(&self) -> usize {
        self.max_payload_bytes
    }

    pub(super) fn publish_checked_read(
        &self,
        r: CheckedProviderReadPublication,
    ) -> Result<(), StoreError> {
        let mut budget = Budget::new(r.validate_bounds(self.max_payload_bytes)?);
        crate::publication::validate(&r.publication, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                // Existing writers take quota before the retention trigger lock.
                // Lock without reserving capacity until all observations are checked.
                self.pg_publication_quota_lock(&mut tx).await?;
                let (_, floor) = self.pg_read_lock(&mut tx).await?;
                self.pg_check_reads_in(
                    &mut tx,
                    &r.publication.intent,
                    &r.execution,
                    (r.publication.observed_tick, floor),
                    &r.dependencies,
                    &mut budget,
                )
                .await?;
                crate::publication::assert_fence(
                    &r.publication,
                    &r.publication.intent,
                    &r.execution,
                    floor,
                )?;
                self.pg_append_audited_in(&mut tx, r.publication, &mut budget)
                    .await
            }
            .await;
            finish_transaction(tx, outcome).await
        })?
    }

    pub(super) fn assert_checked_replay(
        &self,
        r: ProviderReplayAssertion,
    ) -> Result<(), StoreError> {
        let mut budget = shared::replay_validate(&r, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                // SELECT FOR UPDATE fences every provider mutation's BEFORE
                // STATEMENT trigger, including absent rows, without writing a tick.
                let (_, floor) = self.pg_read_lock(&mut tx).await?;
                self.pg_check_reads_in(
                    &mut tx,
                    &r.effect,
                    &r.execution,
                    (r.observed_tick, floor),
                    &r.dependencies,
                    &mut budget,
                )
                .await
                .map(|_| ())
            }
            .await;
            // Replay never changes rows, audits, quota, clock or mutation epoch.
            tx.rollback().await.map_err(infrastructure)?;
            outcome
        })?
    }

    pub(super) async fn pg_read_lock(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(i64, u64), StoreError> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT epoch,clock_tick FROM retention_lock WHERE singleton=1 FOR UPDATE",
        )
        .fetch_optional(&mut **tx)
        .await
        .map_err(infrastructure)?;
        let (epoch, floor) = row.ok_or(StoreError::IncompatibleVersion)?;
        if epoch < 0 || floor < 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok((epoch, floor as u64))
    }

    pub(super) async fn pg_checked_row(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        namespace: &str,
        key: &str,
        budget: &mut Budget,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.pg_checked_row_limit(tx, namespace, key, self.max_payload_bytes, budget)
            .await
    }

    pub(super) async fn pg_checked_row_limit(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        namespace: &str,
        key: &str,
        max_payload: usize,
        budget: &mut Budget,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        // At most two fixed-width metadata rows precede any key/payload decode.
        // UTF-8 octets are independent of character count and database collation.
        let metadata = sqlx::query(
            "SELECT octet_length(convert_to(namespace,'UTF8'))::BIGINT,\
             octet_length(convert_to(key,'UTF8'))::BIGINT,octet_length(payload)::BIGINT,\
             (CASE WHEN pg_typeof(namespace)='text'::regtype AND \
             pg_typeof(key)='text'::regtype AND pg_typeof(payload)='bytea'::regtype AND \
             pg_typeof(version)='bigint'::regtype THEN 1 ELSE 0 END)::BIGINT,\
             (CASE WHEN version>0 THEN 1 ELSE 0 END)::BIGINT \
             FROM provider_state WHERE namespace COLLATE \"C\"=$1 AND key COLLATE \"C\"=$2 LIMIT 2",
        )
        .bind(namespace)
        .bind(key)
        .fetch_all(&mut **tx)
        .await
        .map_err(infrastructure)?;
        let Some(row) = metadata.first() else {
            return Ok(None);
        };
        if metadata.len() != 1 {
            return Err(StoreError::IncompatibleVersion);
        }
        let lengths = [0, 1, 2].map(|index| {
            row.try_get::<i64, _>(index)
                .map_err(infrastructure)
                .and_then(|value| {
                    usize::try_from(value).map_err(|_| StoreError::IncompatibleVersion)
                })
        });
        let (n, k, bytes) = (
            lengths[0].clone()?,
            lengths[1].clone()?,
            lengths[2].clone()?,
        );
        if bytes > max_payload.min(self.max_payload_bytes) {
            return Err(StoreError::PayloadTooLarge);
        }
        for bytes in [n, k, bytes] {
            budget.add(bytes)?;
        }
        let shape: i64 = row.try_get(3).map_err(infrastructure)?;
        let positive: i64 = row.try_get(4).map_err(infrastructure)?;
        if n != namespace.len() || k != key.len() || shape != 1 || positive != 1 {
            return Err(StoreError::IncompatibleVersion);
        }
        drop(metadata);
        let row = sqlx::query(
            "SELECT version,payload FROM provider_state \
             WHERE namespace COLLATE \"C\"=$1 AND key COLLATE \"C\"=$2",
        )
        .bind(namespace)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(infrastructure)?
        .ok_or(StoreError::Conflict)?;
        let record = ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version: u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?,
            payload: row.try_get(1).map_err(infrastructure)?,
        };
        if record.payload.len() != bytes {
            return Err(StoreError::Conflict);
        }
        record.validate_write(self.max_payload_bytes)?;
        Ok(Some(record))
    }

    pub(super) async fn pg_current_occurrence(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        e: &EffectRecord,
        budget: &mut Budget,
    ) -> Result<(EffectRecord, ExecutionRecord), StoreError> {
        let effect = self
            .pg_checked_row(tx, "durable-effect", e.key.as_str(), budget)
            .await?
            .ok_or(StoreError::NotFound)?;
        let actor = self
            .pg_checked_row(tx, "durable-execution", e.execution_id.as_str(), budget)
            .await?
            .ok_or(StoreError::NotFound)?;
        Ok((
            crate::durable::decode_effect(&e.key, &effect.payload)?,
            crate::durable::decode_execution(&actor.payload, actor.version)?,
        ))
    }

    pub(super) async fn pg_writer_document(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        x: &ExecutionRecord,
        tick: u64,
        floor: u64,
        budget: &mut Budget,
        mutations: &[ProviderStateMutation],
    ) -> Result<Option<Document>, StoreError> {
        let actor = self
            .pg_checked_row(tx, ACTOR_NAMESPACE, x.execution_id.as_str(), budget)
            .await?;
        let run = self
            .pg_checked_row(tx, RUN_NAMESPACE, x.run_unit_id.as_str(), budget)
            .await?;
        let Some(actor) = actor else {
            if run.is_some() {
                return Err(StoreError::IncompatibleVersion);
            }
            return Ok(None);
        };
        let root =
            std::str::from_utf8(&actor.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        if root.len() > mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES {
            return Err(StoreError::CapacityExceeded);
        }
        let row = self
            .pg_checked_row(tx, ROOT_DRIVER_NAMESPACE, root, budget)
            .await?
            .ok_or(StoreError::NotFound)?;
        mainframe_env_store_api::RootProviderPublication::validate_batch_bounds(
            mutations,
            row.payload.len(),
        )?;
        let doc = Document::read(&row)?;
        doc.require_index(&actor)?;
        doc.require_index(run.as_ref().ok_or(StoreError::NotFound)?)?;
        doc.require_writer_actor(x)?;
        doc.require_live(tick, floor)?;
        Ok(Some(doc))
    }

    pub(super) async fn pg_scope_identity(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        doc: Option<&Document>,
        identity: (&str, &str),
        require_registered: bool,
        budget: &mut Budget,
    ) -> Result<(), StoreError> {
        let (n, k) = identity;
        let namespace = self.pg_checked_row(tx, SCOPE_NAMESPACE, n, budget).await?;
        let row = self
            .pg_checked_row(tx, ROW_SCOPE_NAMESPACE, &row_scope_key(n, k), budget)
            .await?;
        if !require_registered
            && namespace.is_none()
            && row.is_none()
            && doc.is_none_or(|doc| doc.require_owned_identity(n, k).is_err())
        {
            return Ok(());
        }
        shared::scope(doc, n, k, namespace.as_ref(), row.as_ref())
    }

    async fn pg_check_reads_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        e: &EffectRecord,
        x: &ExecutionRecord,
        clock: (u64, u64),
        dependencies: &[TerminalRowDependency],
        budget: &mut Budget,
    ) -> Result<(), StoreError> {
        let (tick, mut floor) = clock;
        if let Some(legacy) = self
            .pg_checked_row_limit(tx, "jes-worker-meta", "logical-clock", 8, budget)
            .await?
        {
            let old = u64::from_be_bytes(
                legacy
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| StoreError::IncompatibleVersion)?,
            );
            if old == 0 || old > i64::MAX as u64 {
                return Err(StoreError::IncompatibleVersion);
            }
            floor = floor.max(old);
        }
        let (retained, current) = self.pg_current_occurrence(tx, e, budget).await?;
        shared::fence(e, x, &retained, &current, tick, floor)?;
        let doc = self
            .pg_writer_document(tx, &current, tick, floor, budget, &[])
            .await?;
        for dependency in dependencies {
            let identity = shared::identity(dependency);
            self.pg_scope_identity(tx, doc.as_ref(), identity, true, budget)
                .await?;
            let current = self
                .pg_checked_row(tx, identity.0, identity.1, budget)
                .await?;
            shared::compare(dependency, current.as_ref())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
