//! Bounded observations under SQLite's existing physical writer transaction.
use super::*;
use crate::checked_read::{self as shared, Budget};
use crate::root_terminal::{
    ACTOR_NAMESPACE, Document, ROW_SCOPE_NAMESPACE, RUN_NAMESPACE, SCOPE_NAMESPACE, row_scope_key,
};
use mainframe_env_store_api::{
    CheckedProviderReadPublication, EffectRecord, ExecutionRecord, ProviderReplayAssertion,
    ROOT_DRIVER_NAMESPACE, TerminalRowDependency,
};
#[cfg(test)]
mod tests;

impl SqliteStateStore {
    pub(super) fn publish_checked_read(
        &self,
        r: CheckedProviderReadPublication,
    ) -> Result<(), StoreError> {
        let mut budget = Budget::new(r.validate_bounds(self.max_payload_bytes)?);
        crate::publication::validate(&r.publication, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let floor = self
                    .check_reads_in(
                        &mut tx,
                        &r.publication.intent,
                        &r.execution,
                        r.publication.observed_tick,
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
                self.checked_audit_keys_in(&mut tx, &r.publication, &mut budget)
                    .await?;
                self.append_audited_in(&mut tx, r.publication).await
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }

    async fn checked_audit_keys_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        publication: &mainframe_env_store_api::AuditedProviderPublication,
        budget: &mut Budget,
    ) -> Result<(), StoreError> {
        let prefix = crate::durable::audit_storage_key(&publication.audit.execution_id, "direct:");
        let bound = self
            .max_rows
            .min(mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN - 1);
        let (count, bytes, longest): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),COALESCE(SUM(bytes),0),COALESCE(MAX(bytes),0) FROM (SELECT length(CAST(key AS BLOB)) AS bytes FROM provider_state WHERE namespace=? AND substr(CAST(key AS BLOB),1,?)=? LIMIT ?)")
            .bind(crate::durable::AUDIT_NAMESPACE).bind(prefix.len() as i64).bind(prefix.as_bytes())
            .bind((bound + 1) as i64)
            .fetch_one(&mut **tx).await.map_err(infrastructure)?;
        let count = usize::try_from(count).map_err(|_| StoreError::IncompatibleVersion)?;
        let bytes = usize::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?;
        let longest = usize::try_from(longest).map_err(|_| StoreError::IncompatibleVersion)?;
        if count > bound {
            return Err(StoreError::CapacityExceeded);
        }
        if longest > mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES {
            return Err(StoreError::PayloadTooLarge);
        }
        // Existing direct ordinals remain their sole authority. Bound its keys
        // before that kernel allocates the max+1 key scan in this same writer TX.
        budget.add(bytes)
    }
    pub(super) fn assert_checked_replay(
        &self,
        r: ProviderReplayAssertion,
    ) -> Result<(), StoreError> {
        let mut budget = shared::replay_validate(&r, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = self
                .check_reads_in(
                    &mut tx,
                    &r.effect,
                    &r.execution,
                    r.observed_tick,
                    &r.dependencies,
                    &mut budget,
                )
                .await
                .map(|_| ());
            finish_read_transaction(tx, outcome).await
        })?
    }

    async fn checked_row_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        n: &str,
        k: &str,
        budget: &mut Budget,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        // Read only length first, charge the complete aggregate before payload fetch.
        let length: Option<i64> = sqlx::query_scalar(
            "SELECT length(CAST(payload AS BLOB)) FROM provider_state WHERE namespace=? AND key=?",
        )
        .bind(n)
        .bind(k)
        .fetch_optional(&mut **tx)
        .await
        .map_err(infrastructure)?;
        let Some(length) = length else {
            return Ok(None);
        };
        let length = usize::try_from(length).map_err(|_| StoreError::IncompatibleVersion)?;
        if length > self.max_payload_bytes {
            return Err(StoreError::PayloadTooLarge);
        }
        for size in [n.len(), k.len(), length] {
            budget.add(size)?;
        }
        // Existing bounded reader performs a second length check in the SAME writer TX.
        self.root_optional_row(tx, n, k).await
    }

    async fn check_reads_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        e: &EffectRecord,
        x: &ExecutionRecord,
        tick: u64,
        dependencies: &[TerminalRowDependency],
        budget: &mut Budget,
    ) -> Result<u64, StoreError> {
        let (_, mut floor) = self.root_lock(tx).await?;
        if let Some(legacy) = self
            .checked_row_in(tx, "jes-worker-meta", "logical-clock", budget)
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
        let effect = self
            .checked_row_in(tx, "durable-effect", e.key.as_str(), budget)
            .await?
            .ok_or(StoreError::NotFound)?;
        let actor = self
            .checked_row_in(tx, "durable-execution", e.execution_id.as_str(), budget)
            .await?
            .ok_or(StoreError::NotFound)?;
        let retained = crate::durable::decode_effect(&e.key, &effect.payload)?;
        let current = crate::durable::decode_execution(&actor.payload, actor.version)?;
        shared::fence(e, x, &retained, &current, tick, floor)?;
        let actor = self
            .checked_row_in(tx, ACTOR_NAMESPACE, x.execution_id.as_str(), budget)
            .await?;
        let run = self
            .checked_row_in(tx, RUN_NAMESPACE, x.run_unit_id.as_str(), budget)
            .await?;
        let doc = match actor {
            Some(actor) => {
                let root = std::str::from_utf8(&actor.payload)
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                if root.len() > mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES {
                    return Err(StoreError::CapacityExceeded);
                }
                let row = self
                    .checked_row_in(tx, ROOT_DRIVER_NAMESPACE, root, budget)
                    .await?
                    .ok_or(StoreError::NotFound)?;
                let doc = Document::read(&row)?;
                doc.require_index(&actor)?;
                doc.require_index(run.as_ref().ok_or(StoreError::NotFound)?)?;
                doc.require_writer_actor(&current)?;
                doc.require_live(tick, floor)?;
                Some(doc)
            }
            None if run.is_some() => return Err(StoreError::IncompatibleVersion),
            None => None,
        };
        for dependency in dependencies {
            let (n, k) = shared::identity(dependency);
            let ns = self.checked_row_in(tx, SCOPE_NAMESPACE, n, budget).await?;
            let row = self
                .checked_row_in(tx, ROW_SCOPE_NAMESPACE, &row_scope_key(n, k), budget)
                .await?;
            shared::scope(doc.as_ref(), n, k, ns.as_ref(), row.as_ref())?;
            let current = self.checked_row_in(tx, n, k, budget).await?;
            shared::compare(dependency, current.as_ref())?;
        }
        Ok(floor)
    }
}
