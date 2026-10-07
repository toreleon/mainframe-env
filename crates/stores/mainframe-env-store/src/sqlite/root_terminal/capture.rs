//! Complete indexed/scoped capture, with count/byte refusal before loading rows.
use super::*;
impl SqliteStateStore {
    pub(in crate::sqlite) async fn root_lock(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<(u64, u64), StoreError> {
        let row = sqlx::query(
            "UPDATE retention_lock SET epoch=epoch WHERE singleton=1 RETURNING epoch,clock_tick",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(infrastructure)?;
        Ok((
            u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?,
            u64::try_from(row.try_get::<i64, _>(1).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }
    pub(super) async fn root_clock(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        tick: u64,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE retention_lock SET clock_tick=? WHERE singleton=1")
            .bind(i64::try_from(tick).map_err(|_| StoreError::LeaseConflict)?)
            .execute(&mut **tx)
            .await
            .map_err(infrastructure)?;
        Ok(())
    }
    pub(in crate::sqlite) async fn root_optional_row(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        key: &str,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        self.root_optional_row_bounded(tx, namespace, key, MAX_ROOT_PAYLOAD_BYTES)
            .await
    }
    async fn root_optional_row_bounded(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        key: &str,
        bytes: usize,
    ) -> Result<Option<ProviderStateRecord>, StoreError> {
        if bytes < namespace.len() + key.len() {
            return Err(StoreError::CapacityExceeded);
        }
        let length: Option<i64> = sqlx::query_scalar(
            "SELECT length(payload) FROM provider_state WHERE namespace=? AND key=?",
        )
        .bind(namespace)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await
        .map_err(infrastructure)?;
        let Some(length) = length else {
            return Ok(None);
        };
        if length < 0
            || length as u64
                > self
                    .max_payload_bytes
                    .min(bytes.saturating_sub(namespace.len() + key.len())) as u64
        {
            return Err(StoreError::CapacityExceeded);
        }
        let row =
            sqlx::query("SELECT version,payload FROM provider_state WHERE namespace=? AND key=?")
                .bind(namespace)
                .bind(key)
                .fetch_one(&mut **tx)
                .await
                .map_err(infrastructure)?;
        let result = ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version: u64::try_from(row.try_get::<i64, _>(0).map_err(infrastructure)?)
                .map_err(|_| StoreError::IncompatibleVersion)?,
            payload: row.try_get(1).map_err(infrastructure)?,
        };
        result.validate_write(self.max_payload_bytes)?;
        Ok(Some(result))
    }
    pub(in crate::sqlite) async fn root_row(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        key: &str,
    ) -> Result<ProviderStateRecord, StoreError> {
        self.root_optional_row(tx, namespace, key)
            .await?
            .ok_or(StoreError::NotFound)
    }
    pub(in crate::sqlite) async fn root_rows_matching(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        prefix: Option<&str>,
        actor: Option<&str>,
        max: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        self.root_rows_bounded(tx, namespace, prefix, actor, max, MAX_ROOT_PAYLOAD_BYTES)
            .await
    }
    async fn root_rows_bounded(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        prefix: Option<&str>,
        actor: Option<&str>,
        max: usize,
        byte_ceiling: usize,
    ) -> Result<Vec<ProviderStateRecord>, StoreError> {
        // Byte prefix is exact (including non-ASCII identities), not SQL LIKE.
        let sql = "SELECT COUNT(*),COALESCE(SUM(length(payload)+length(CAST(key AS BLOB))),0) FROM provider_state WHERE namespace=? AND (? IS NULL OR substr(CAST(key AS BLOB),1,?)=?) AND (? IS NULL OR json_extract(payload,'$.execution')=?)";
        let stats = sqlx::query(sql)
            .bind(namespace)
            .bind(prefix)
            .bind(prefix.map(str::len).unwrap_or(0) as i64)
            .bind(prefix.map(str::as_bytes))
            .bind(actor)
            .bind(actor)
            .fetch_one(&mut **tx)
            .await
            .map_err(infrastructure)?;
        let count = usize::try_from(stats.try_get::<i64, _>(0).map_err(infrastructure)?)
            .map_err(|_| StoreError::CapacityExceeded)?;
        let bytes = u64::try_from(stats.try_get::<i64, _>(1).map_err(infrastructure)?)
            .map_err(|_| StoreError::CapacityExceeded)?;
        let bytes = bytes
            .checked_add(
                (count as u64)
                    .checked_mul(namespace.len() as u64)
                    .ok_or(StoreError::CapacityExceeded)?,
            )
            .ok_or(StoreError::CapacityExceeded)?;
        if count > max || bytes > byte_ceiling.min(MAX_ROOT_PAYLOAD_BYTES) as u64 {
            return Err(StoreError::CapacityExceeded);
        }
        let rows = sqlx::query("SELECT key,version,payload FROM provider_state WHERE namespace=? AND (? IS NULL OR substr(CAST(key AS BLOB),1,?)=?) AND (? IS NULL OR json_extract(payload,'$.execution')=?) ORDER BY key LIMIT ?")
            .bind(namespace).bind(prefix).bind(prefix.map(str::len).unwrap_or(0) as i64).bind(prefix.map(str::as_bytes)).bind(actor).bind(actor)
            .bind((max + 1) as i64).fetch_all(&mut **tx).await.map_err(infrastructure)?;
        if rows.len() != count {
            return Err(StoreError::Conflict);
        }
        rows.into_iter()
            .map(|row| {
                let record = ProviderStateRecord {
                    namespace: namespace.into(),
                    key: row.try_get(0).map_err(infrastructure)?,
                    version: u64::try_from(row.try_get::<i64, _>(1).map_err(infrastructure)?)
                        .map_err(|_| StoreError::IncompatibleVersion)?,
                    payload: row.try_get(2).map_err(infrastructure)?,
                };
                record.validate_write(self.max_payload_bytes)?;
                Ok(record)
            })
            .collect()
    }
    pub(super) async fn root_require_no_work(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &ExecutionId,
    ) -> Result<(), StoreError> {
        if self
            .root_optional_row(tx, "durable-checkpoint", actor.as_str())
            .await?
            .is_some()
            || !self
                .root_rows_matching(tx, "durable-work", None, Some(actor.as_str()), 0)
                .await?
                .is_empty()
        {
            return Err(StoreError::InvalidTransition);
        }
        Ok(())
    }
    pub(super) async fn root_capture(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        claim: &RootDriverClaim,
        closing: ProviderStateRecord,
        tick: u64,
    ) -> Result<RootClosureSnapshot, StoreError> {
        let doc = Document::read(&closing)?;
        let mut budget = CaptureBudget {
            operations: doc.actors.len() + 2,
            bytes: closing
                .payload
                .len()
                .checked_add(claim.inserted_row().payload.len())
                .and_then(|n| n.checked_add(doc.actors.len().checked_mul(4096)?))
                .ok_or(StoreError::CapacityExceeded)?,
        };
        budget.check()?;
        let mut snapshot = RootClosureSnapshot {
            claim: claim.clone(),
            closing,
            actors: Vec::with_capacity(doc.actors.len()),
            provider_dependencies: Vec::new(),
            core_records: Vec::new(),
            provider_epoch: self.root_lock(tx).await?.0,
            observed_tick: tick,
        };
        for namespace in &doc.provider_namespaces {
            let rows = self
                .root_rows_bounded(
                    tx,
                    namespace,
                    None,
                    None,
                    budget.remaining_operations(),
                    budget.remaining_bytes(),
                )
                .await?;
            for row in rows {
                budget.row(&row)?;
                snapshot
                    .provider_dependencies
                    .push(TerminalRowDependency::Exact(row));
            }
        }
        for (namespace, key) in &doc.provider_rows {
            if budget.remaining_operations() == 0 {
                return Err(StoreError::CapacityExceeded);
            }
            snapshot.provider_dependencies.push(
                match self
                    .root_optional_row_bounded(tx, namespace, key, budget.remaining_bytes())
                    .await?
                {
                    Some(row) => {
                        budget.row(&row)?;
                        TerminalRowDependency::Exact(row)
                    }
                    None => {
                        budget.add(1, namespace.len() + key.len())?;
                        TerminalRowDependency::Absent {
                            namespace: namespace.clone(),
                            key: key.clone(),
                        }
                    }
                },
            );
        }
        for identity in &doc.actors {
            let id = ExecutionId::new(&identity.execution, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?;
            self.root_require_no_work(tx, &id).await?;
            if budget.remaining_operations() == 0 {
                return Err(StoreError::CapacityExceeded);
            }
            let execution_row = self
                .root_optional_row_bounded(
                    tx,
                    "durable-execution",
                    &identity.execution,
                    budget.remaining_bytes(),
                )
                .await?
                .ok_or(StoreError::NotFound)?;
            budget.row(&execution_row)?;
            let execution =
                durable::decode_execution(&execution_row.payload, execution_row.version)?;
            let effects_rows = self
                .root_rows_bounded(
                    tx,
                    "durable-effect",
                    None,
                    Some(&identity.execution),
                    budget.remaining_operations() / 2,
                    budget.remaining_bytes() / 2,
                )
                .await?;
            for row in &effects_rows {
                budget.row(row)?;
                budget.add(1, row.payload.len().max(4096))?;
            }
            let effects = effects_rows
                .iter()
                .map(|row| {
                    let key = mainframe_env_execution_api::IdempotencyKey::new(
                        &row.key,
                        InvocationLimits::default(),
                    )
                    .map_err(|_| StoreError::IncompatibleVersion)?;
                    durable::decode_effect(&key, &row.payload)
                })
                .collect::<Result<Vec<_>, _>>()?;
            let events = self
                .root_rows_bounded(
                    tx,
                    &format!("durable-event:{}", identity.execution),
                    None,
                    None,
                    budget.remaining_operations(),
                    budget.remaining_bytes(),
                )
                .await?;
            for row in &events {
                budget.row(row)?;
            }
            let last_event =
                durable::decode_event(&events.last().ok_or(StoreError::NotFound)?.payload)?;
            let outboxes = self
                .root_rows_bounded(
                    tx,
                    "durable-outbox",
                    None,
                    Some(&identity.execution),
                    budget.remaining_operations(),
                    budget.remaining_bytes(),
                )
                .await?;
            for row in &outboxes {
                budget.row(row)?;
            }
            snapshot.core_records.push(execution_row);
            snapshot.core_records.extend(effects_rows);
            snapshot.core_records.extend(events);
            snapshot.core_records.extend(outboxes);
            let parent = identity
                .parent
                .as_ref()
                .map(|p| {
                    ExecutionId::new(p, InvocationLimits::default())
                        .map_err(|_| StoreError::IncompatibleVersion)
                })
                .transpose()?;
            if let Some(binding) = &identity.call {
                budget.add(
                    2,
                    binding.catalog.payload.len()
                        + binding.catalog.namespace.len()
                        + binding.catalog.key.len()
                        + binding.namespace.len()
                        + binding.key.len(),
                )?;
                self.root_dependency(tx, &TerminalRowDependency::Exact(binding.catalog.clone()))
                    .await?;
            }
            snapshot.actors.push(RootActorSnapshot {
                execution,
                parent,
                call: identity.call.clone(),
                effects,
                checkpoint: None,
                work: Vec::new(),
                last_event,
            });
            snapshot.validate_bounds()?;
        }
        snapshot.validate_bounds()?;
        Ok(snapshot)
    }
    pub(super) async fn root_dependency(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        dependency: &TerminalRowDependency,
    ) -> Result<(), StoreError> {
        match dependency {
            TerminalRowDependency::Exact(expected)
                if self
                    .root_row(tx, &expected.namespace, &expected.key)
                    .await?
                    == *expected =>
            {
                Ok(())
            }
            TerminalRowDependency::Absent { namespace, key }
                if self.root_optional_row(tx, namespace, key).await?.is_none() =>
            {
                Ok(())
            }
            _ => Err(StoreError::Conflict),
        }
    }
    pub(super) async fn root_audit_ordinal(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        execution: &ExecutionId,
    ) -> Result<u64, StoreError> {
        let prefix = durable::audit_storage_key(execution, "direct:");
        let rows = self
            .root_rows_matching(
                tx,
                durable::AUDIT_NAMESPACE,
                Some(&prefix),
                None,
                MAX_ROOT_OPERATIONS,
            )
            .await?;
        rows.into_iter().try_fold(0_u64, |highest, row| {
            let suffix = row
                .key
                .strip_prefix(&prefix)
                .ok_or(StoreError::IncompatibleVersion)?;
            let value = suffix
                .parse::<u64>()
                .map_err(|_| StoreError::IncompatibleVersion)?;
            if value == 0 || suffix != format!("{value:020}") {
                return Err(StoreError::IncompatibleVersion);
            }
            Ok(highest.max(value))
        })
    }
}

struct CaptureBudget {
    operations: usize,
    bytes: usize,
}
impl CaptureBudget {
    fn remaining_operations(&self) -> usize {
        MAX_ROOT_OPERATIONS.saturating_sub(self.operations)
    }
    fn remaining_bytes(&self) -> usize {
        MAX_ROOT_PAYLOAD_BYTES.saturating_sub(self.bytes)
    }
    fn check(&self) -> Result<(), StoreError> {
        if self.operations > MAX_ROOT_OPERATIONS || self.bytes > MAX_ROOT_PAYLOAD_BYTES {
            Err(StoreError::CapacityExceeded)
        } else {
            Ok(())
        }
    }
    fn add(&mut self, operations: usize, bytes: usize) -> Result<(), StoreError> {
        self.operations = self
            .operations
            .checked_add(operations)
            .ok_or(StoreError::CapacityExceeded)?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(StoreError::CapacityExceeded)?;
        self.check()
    }
    fn row(&mut self, row: &ProviderStateRecord) -> Result<(), StoreError> {
        self.add(
            1,
            row.payload
                .len()
                .checked_add(row.namespace.len() + row.key.len())
                .ok_or(StoreError::CapacityExceeded)?,
        )
    }
}
