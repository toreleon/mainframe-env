//! One SQL row mutation authority, shared within a single owned transaction.
use super::*;
use mainframe_env_store_api::AuditedProviderPublication;
#[cfg(test)]
mod tests;

impl SqliteStateStore {
    pub(super) fn publish_audited(
        &self,
        request: AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        crate::publication::validate(&request, self.max_payload_bytes)?;
        block_on(&self.runtime, async {
            let mut transaction = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = self.publish_audited_in(&mut transaction, request).await;
            finish_read_transaction(transaction, outcome).await
        })?
    }

    async fn publication_row_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        key: &str,
    ) -> Result<ProviderStateRecord, StoreError> {
        let row =
            sqlx::query("SELECT version,payload FROM provider_state WHERE namespace=? AND key=?")
                .bind(namespace)
                .bind(key)
                .fetch_optional(&mut **transaction)
                .await
                .map_err(infrastructure)?
                .ok_or(StoreError::NotFound)?;
        let version: i64 = row.try_get(0).map_err(infrastructure)?;
        if version <= 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version: version as u64,
            payload: row.try_get(1).map_err(infrastructure)?,
        })
    }

    async fn publish_audited_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        request: AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        // Acquire SQLite's writer lock before observing the intent. This shares
        // the existing retention/clock lock; no lock spans provider dispatch.
        let lock = sqlx::query(
            "UPDATE retention_lock SET epoch=epoch WHERE singleton=1 RETURNING epoch,clock_tick",
        )
        .fetch_optional(&mut **transaction)
        .await
        .map_err(infrastructure)?
        .ok_or(StoreError::IncompatibleVersion)?;
        let epoch: i64 = lock.try_get(0).map_err(infrastructure)?;
        let tick: i64 = lock.try_get(1).map_err(infrastructure)?;
        if epoch < 0 || tick < 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        // Even an audit-only or unscoped row batch belongs to its enrolled
        // actor. Assert the root gate under this same physical writer lock.
        self.root_guard_actor(transaction, &request.intent.execution_id, false)
            .await?;
        let writes = request
            .mutations
            .iter()
            .try_fold(1_i64, |total, mutation| {
                total
                    .checked_add(if matches!(mutation, ProviderStateMutation::Move { .. }) {
                        2
                    } else {
                        1
                    })
                    .ok_or(StoreError::CapacityExceeded)
            })?;
        epoch
            .checked_add(writes)
            .ok_or(StoreError::CapacityExceeded)?;
        let effect_row = self
            .publication_row_in(transaction, "durable-effect", request.intent.key.as_str())
            .await?;
        let retained = crate::durable::decode_effect(&request.intent.key, &effect_row.payload)?;
        let execution_row = self
            .publication_row_in(
                transaction,
                "durable-execution",
                request.intent.execution_id.as_str(),
            )
            .await?;
        let execution =
            crate::durable::decode_execution(&execution_row.payload, execution_row.version)?;
        crate::publication::assert_fence(&request, &retained, &execution, tick as u64)?;
        // Same writer TX, after exact original intent validation; never a DTO permit.
        let document = self
            .root_writer_document(transaction, &execution, &request.mutations)
            .await?;
        if let Some(document) = &document {
            document.require_live(request.observed_tick, tick as u64)?;
        }
        self.root_guard_writer_scopes(transaction, document.as_ref(), &request.mutations, false)
            .await?;

        self.append_audited_in(transaction, request).await
    }
    pub(super) async fn append_audited_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        request: AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        // Use the existing direct audit ordinal/key namespace and exact codec.
        // Prefix matching is by bytes, including non-ASCII execution identities.
        let prefix = crate::durable::audit_storage_key(&request.audit.execution_id, "direct:");
        let bound = self
            .max_rows
            .min(mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN - 1);
        let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM provider_state WHERE namespace=? AND substr(CAST(key AS BLOB),1,?)=? LIMIT ?")
            .bind(crate::durable::AUDIT_NAMESPACE).bind(prefix.len() as i64).bind(prefix.as_bytes())
            .bind((bound+1) as i64).fetch_all(&mut **transaction).await.map_err(infrastructure)?;
        if keys.len() > bound {
            return Err(StoreError::CapacityExceeded);
        }
        let mut ordinal = 0_u64;
        for key in keys {
            let suffix = key
                .strip_prefix(&prefix)
                .ok_or(StoreError::IncompatibleVersion)?;
            let value = suffix
                .parse::<u64>()
                .map_err(|_| StoreError::IncompatibleVersion)?;
            if value == 0 || suffix != format!("{value:020}") {
                return Err(StoreError::IncompatibleVersion);
            }
            ordinal = ordinal.max(value);
        }
        let next = ordinal.checked_add(1).ok_or(StoreError::CapacityExceeded)?;
        let audit_row = ProviderStateRecord {
            namespace: crate::durable::AUDIT_NAMESPACE.into(),
            key: crate::durable::audit_storage_key(
                &request.audit.execution_id,
                &format!("direct:{next:020}"),
            ),
            version: 1,
            payload: crate::durable::encode_audit(&request.audit)?,
        };
        audit_row.validate_write(self.max_payload_bytes)?;
        let mut mutations = request.mutations;
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: audit_row,
            expected_version: None,
        }));
        self.apply_mutations_in(transaction, mutations).await?;
        sqlx::query("UPDATE retention_lock SET clock_tick=? WHERE singleton=1")
            .bind(request.observed_tick as i64)
            .execute(&mut **transaction)
            .await
            .map_err(infrastructure)?;
        Ok(())
    }
    pub(super) fn mutate_provider_rows(
        &self,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        if mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        for mutation in &mutations {
            if let ProviderStateMutation::Put(write) = mutation {
                write.record.validate_write(self.max_payload_bytes)?;
            }
        }
        block_on(&self.runtime, async {
            let mut transaction = self
                .pool
                .begin()
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
            let outcome = self.apply_mutations_in(&mut transaction, mutations).await;
            finish_read_transaction(transaction, outcome).await
        })?
    }

    pub(super) async fn apply_mutations_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        self.root_lock(transaction).await?;
        self.root_guard_mutations(transaction, &mutations).await?;
        self.apply_root_mutations_in(transaction, mutations).await
    }

    // Reuses the sole row mutation primitive after the root transaction has
    // established complete Closing ownership. Never called by an external writer.
    pub(in crate::sqlite) async fn apply_root_mutations_in(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        mutations: Vec<ProviderStateMutation>,
    ) -> Result<(), StoreError> {
        // The existing per-row triggers bump this signed integer. SQLite would
        // otherwise promote an overflowing epoch to REAL and lose fencing.
        // Check all physical endpoints before any touched row under this TX.
        let writes = mutations.iter().try_fold(0_i64, |total, mutation| {
            total
                .checked_add(crate::root_terminal::mutation_endpoints(mutation).count() as i64)
                .ok_or(StoreError::CapacityExceeded)
        })?;
        let epoch: i64 = sqlx::query_scalar("SELECT epoch FROM retention_lock WHERE singleton=1")
            .fetch_one(&mut **transaction)
            .await
            .map_err(infrastructure)?;
        if epoch < 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        epoch
            .checked_add(writes)
            .ok_or(StoreError::CapacityExceeded)?;
        for mutation in mutations {
            match &mutation {
                ProviderStateMutation::Put(w) => w.record.validate_write(self.max_payload_bytes)?,
                ProviderStateMutation::Move {
                    record,
                    old_key,
                    expected_version,
                } => record.validate_move(old_key, *expected_version, self.max_payload_bytes)?,
                ProviderStateMutation::Delete { .. } => {}
            }
            let affected = match mutation {
                ProviderStateMutation::Put(write) => {
                    let record = write.record;
                    if let Some(expected) = write.expected_version {
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
                    .execute(&mut **transaction)
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
                    .execute(&mut **transaction)
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
                    .execute(&mut **transaction)
                    .await
                    .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                    .rows_affected()
                }
                ProviderStateMutation::Move {
                    record,
                    old_key,
                    expected_version,
                } => {
                    record.validate_move(&old_key, expected_version, self.max_payload_bytes)?;
                    let inserted = sqlx::query(
                    "INSERT OR IGNORE INTO provider_state(namespace,key,version,payload) VALUES(?,?,?,?)",
                )
                .bind(&record.namespace)
                .bind(&record.key)
                .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                .bind(&record.payload)
                .execute(&mut **transaction)
                .await
                .map_err(|error| StoreError::Infrastructure(error.to_string()))?
                .rows_affected();
                    let deleted = sqlx::query(
                        "DELETE FROM provider_state WHERE namespace=? AND key=? AND version=?",
                    )
                    .bind(&record.namespace)
                    .bind(old_key)
                    .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                    .execute(&mut **transaction)
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
            .fetch_one(&mut **transaction)
            .await
            .map_err(|error| StoreError::Infrastructure(error.to_string()))?;
        if usize::try_from(count).map_err(|_| StoreError::CapacityExceeded)? > self.max_rows {
            return Err(StoreError::CapacityExceeded);
        }
        Ok(())
    }
}
