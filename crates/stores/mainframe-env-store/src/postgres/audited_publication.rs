//! One PostgreSQL transaction owns provider CAS, audit ordinal, quota and clock.
use super::*;
use crate::checked_read::Budget;
use crate::root_terminal::mutation_endpoints;
use mainframe_env_store_api::AuditedProviderPublication;

impl PostgresStateStore {
    pub(super) fn publish_audited(
        &self,
        request: AuditedProviderPublication,
    ) -> Result<(), StoreError> {
        crate::publication::validate(&request, self.max_payload_bytes)?;
        let mut budget = Budget::new(0);
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                self.pg_publication_quota_lock(&mut tx).await?;
                let (_, floor) = self.pg_read_lock(&mut tx).await?;
                let (intent, execution) = self
                    .pg_current_occurrence(&mut tx, &request.intent, &mut budget)
                    .await?;
                crate::publication::assert_fence(&request, &intent, &execution, floor)?;
                let doc = self
                    .pg_writer_document(
                        &mut tx,
                        &execution,
                        request.observed_tick,
                        floor,
                        &mut budget,
                        &request.mutations,
                    )
                    .await?;
                for mutation in &request.mutations {
                    for identity in mutation_endpoints(mutation) {
                        self.pg_scope_identity(&mut tx, doc.as_ref(), identity, false, &mut budget)
                            .await?;
                    }
                }
                self.pg_append_audited_in(&mut tx, request, &mut budget)
                    .await
            }
            .await;
            finish_transaction(tx, outcome).await
        })?
    }

    pub(super) async fn pg_publication_quota_lock(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(), StoreError> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT max_rows,used_rows FROM store_quota WHERE quota_key=$1 FOR UPDATE",
        )
        .bind(PROVIDER_STATE_QUOTA)
        .fetch_optional(&mut **tx)
        .await
        .map_err(infrastructure)?;
        let (max, used) = row.ok_or(StoreError::IncompatibleVersion)?;
        if max != i64::try_from(self.max_rows).map_err(|_| StoreError::CapacityExceeded)?
            || used < 0
            || used > max
        {
            return Err(StoreError::IncompatibleVersion);
        }
        Ok(())
    }

    async fn pg_next_audit(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        audit: &mainframe_env_execution_api::AuditRecord,
        budget: &mut Budget,
    ) -> Result<ProviderStateRecord, StoreError> {
        let prefix = crate::durable::audit_storage_key(&audit.execution_id, "direct:");
        let bound = self
            .max_rows
            .min(mainframe_env_store_api::MAX_PROVIDER_STATE_SCAN - 1);
        let limit = i64::try_from(bound + 1).map_err(|_| StoreError::CapacityExceeded)?;
        let prefix_length =
            i32::try_from(prefix.len()).map_err(|_| StoreError::CapacityExceeded)?;
        let (count, bytes, longest): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*)::BIGINT,COALESCE(SUM(bytes),0)::BIGINT,COALESCE(MAX(bytes),0)::BIGINT \
             FROM (SELECT octet_length(convert_to(key,'UTF8'))::BIGINT AS bytes \
             FROM provider_state WHERE namespace COLLATE \"C\"=$1 AND \
             substring(convert_to(key,'UTF8') FROM 1 FOR $2)=$3 LIMIT $4) AS bounded",
        )
        .bind(crate::durable::AUDIT_NAMESPACE)
        .bind(prefix_length)
        .bind(prefix.as_bytes())
        .bind(limit)
        .fetch_one(&mut **tx)
        .await
        .map_err(infrastructure)?;
        let count = usize::try_from(count).map_err(|_| StoreError::IncompatibleVersion)?;
        let longest = usize::try_from(longest).map_err(|_| StoreError::IncompatibleVersion)?;
        if count > bound {
            return Err(StoreError::CapacityExceeded);
        }
        if longest > mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES {
            return Err(StoreError::PayloadTooLarge);
        }
        budget.add(usize::try_from(bytes).map_err(|_| StoreError::IncompatibleVersion)?)?;
        let keys: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT convert_to(key,'UTF8') FROM provider_state \
             WHERE namespace COLLATE \"C\"=$1 AND \
             substring(convert_to(key,'UTF8') FROM 1 FOR $2)=$3 LIMIT $4",
        )
        .bind(crate::durable::AUDIT_NAMESPACE)
        .bind(prefix_length)
        .bind(prefix.as_bytes())
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(infrastructure)?;
        if keys.len() != count {
            return Err(StoreError::Conflict);
        }
        let mut ordinal = 0_u64;
        for key in keys {
            let key = std::str::from_utf8(&key).map_err(|_| StoreError::IncompatibleVersion)?;
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
        // Charge a finite JSON escaping bound before the existing codec allocates.
        let mut encoded_bound = 4096_usize;
        for text in [
            audit.execution_id.as_str(),
            audit.run_unit_id.as_str(),
            audit.principal.as_str(),
            audit.invocation_key.as_str(),
            audit.capability.as_str(),
        ] {
            if text.len() > self.max_payload_bytes {
                return Err(StoreError::PayloadTooLarge);
            }
            encoded_bound = encoded_bound
                .checked_add(
                    text.len()
                        .checked_mul(6)
                        .ok_or(StoreError::CapacityExceeded)?,
                )
                .ok_or(StoreError::CapacityExceeded)?;
        }
        budget.add(encoded_bound)?;
        let row = ProviderStateRecord {
            namespace: crate::durable::AUDIT_NAMESPACE.into(),
            key: crate::durable::audit_storage_key(
                &audit.execution_id,
                &format!("direct:{next:020}"),
            ),
            version: 1,
            payload: crate::durable::encode_audit(audit)?,
        };
        budget.row(&row, self.max_payload_bytes)?;
        Ok(row)
    }

    pub(super) async fn pg_append_audited_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        request: AuditedProviderPublication,
        budget: &mut Budget,
    ) -> Result<(), StoreError> {
        let audit = self.pg_next_audit(tx, &request.audit, budget).await?;
        let mut mutations = request.mutations;
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: audit,
            expected_version: None,
        }));
        // The existing statement trigger consumes one epoch per SQL endpoint.
        let writes = mutations.iter().try_fold(0_i64, |count, mutation| {
            count
                .checked_add(mutation_endpoints(mutation).count() as i64)
                .ok_or(StoreError::CapacityExceeded)
        })?;
        let (epoch, _) = self.pg_read_lock(tx).await?;
        epoch
            .checked_add(writes)
            .ok_or(StoreError::CapacityExceeded)?;
        let delta = mutations.iter().try_fold(0_i64, |count, mutation| {
            let delta = match mutation {
                ProviderStateMutation::Put(write) if write.expected_version.is_none() => 1,
                ProviderStateMutation::Delete { .. } => -1,
                _ => 0,
            };
            count.checked_add(delta).ok_or(StoreError::CapacityExceeded)
        })?;
        // The quota row is already held; rollback restores it together with all rows.
        adjust_quota(tx, PROVIDER_STATE_QUOTA, self.max_rows, delta).await?;
        for mutation in mutations {
            self.pg_successor_mutation(tx, &mutation, budget).await?;
            self.pg_apply_publication_mutation(tx, mutation).await?;
        }
        let affected = sqlx::query("UPDATE retention_lock SET clock_tick=$1 WHERE singleton=1")
            .bind(i64::try_from(request.observed_tick).map_err(|_| StoreError::CapacityExceeded)?)
            .execute(&mut **tx)
            .await
            .map_err(infrastructure)?
            .rows_affected();
        if affected != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }

    async fn pg_successor_mutation(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        mutation: &ProviderStateMutation,
        budget: &mut Budget,
    ) -> Result<(), StoreError> {
        for (namespace, key) in mutation_endpoints(mutation) {
            let pin_key = crate::root_terminal::row_scope_key(namespace, key);
            if let Some(pin) = self
                .pg_checked_row_limit(
                    tx,
                    crate::root_terminal::successor::PIN_NAMESPACE,
                    &pin_key,
                    4096,
                    budget,
                )
                .await?
            {
                let proposed = match mutation {
                    ProviderStateMutation::Put(write) => Some(write.record.payload.as_slice()),
                    _ => None,
                };
                crate::root_terminal::successor::require_pin(&pin, namespace, key, proposed)?;
            }
        }
        Ok(())
    }

    async fn pg_apply_publication_mutation(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        mutation: ProviderStateMutation,
    ) -> Result<(), StoreError> {
        let affected = match mutation {
            ProviderStateMutation::Put(write) => {
                write.record.validate_write(self.max_payload_bytes)?;
                let row = write.record;
                if let Some(expected) = write.expected_version {
                    if expected.checked_add(1) != Some(row.version) {
                        return Err(StoreError::Conflict);
                    }
                    sqlx::query(
                        "UPDATE provider_state SET version=$1,payload=$2 \
                        WHERE namespace COLLATE \"C\"=$3 AND key COLLATE \"C\"=$4 AND version=$5",
                    )
                    .bind(i64::try_from(row.version).map_err(|_| StoreError::Conflict)?)
                    .bind(row.payload)
                    .bind(row.namespace)
                    .bind(row.key)
                    .bind(i64::try_from(expected).map_err(|_| StoreError::Conflict)?)
                    .execute(&mut **tx)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected()
                } else {
                    if row.version != 1 {
                        return Err(StoreError::Conflict);
                    }
                    sqlx::query(
                        "INSERT INTO provider_state(namespace,key,version,payload) \
                        VALUES($1,$2,1,$3) ON CONFLICT DO NOTHING",
                    )
                    .bind(row.namespace)
                    .bind(row.key)
                    .bind(row.payload)
                    .execute(&mut **tx)
                    .await
                    .map_err(infrastructure)?
                    .rows_affected()
                }
            }
            ProviderStateMutation::Delete {
                namespace,
                key,
                expected_version,
            } => sqlx::query(
                "DELETE FROM provider_state WHERE namespace COLLATE \"C\"=$1 \
                    AND key COLLATE \"C\"=$2 AND version=$3",
            )
            .bind(namespace)
            .bind(key)
            .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
            .execute(&mut **tx)
            .await
            .map_err(infrastructure)?
            .rows_affected(),
            ProviderStateMutation::Move {
                record,
                old_key,
                expected_version,
            } => {
                record.validate_move(&old_key, expected_version, self.max_payload_bytes)?;
                let inserted = sqlx::query(
                    "INSERT INTO provider_state(namespace,key,version,payload) \
                    VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
                )
                .bind(&record.namespace)
                .bind(&record.key)
                .bind(i64::try_from(record.version).map_err(|_| StoreError::Conflict)?)
                .bind(&record.payload)
                .execute(&mut **tx)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                let deleted = sqlx::query(
                    "DELETE FROM provider_state WHERE namespace COLLATE \"C\"=$1 \
                    AND key COLLATE \"C\"=$2 AND version=$3",
                )
                .bind(&record.namespace)
                .bind(old_key)
                .bind(i64::try_from(expected_version).map_err(|_| StoreError::Conflict)?)
                .execute(&mut **tx)
                .await
                .map_err(infrastructure)?
                .rows_affected();
                u64::from(inserted == 1 && deleted == 1)
            }
        };
        if affected != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
}
