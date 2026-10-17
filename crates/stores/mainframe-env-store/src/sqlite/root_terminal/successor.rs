//! Same actual SQLite writer transaction; copied paths/rows are not custody.
use super::*;

impl SqliteStateStore {
    pub(in crate::sqlite) async fn root_guard_successor_identity(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        namespace: &str,
        key: &str,
        proposed: Option<&[u8]>,
    ) -> Result<(), StoreError> {
        let pin_key = row_scope_key(namespace, key);
        if let Some(pin) = self
            .root_optional_row_bounded(
                tx,
                crate::root_terminal::successor::PIN_NAMESPACE,
                &pin_key,
                4096 + crate::root_terminal::successor::PIN_NAMESPACE.len() + pin_key.len(),
            )
            .await?
        {
            crate::root_terminal::successor::require_pin(&pin, namespace, key, proposed)?;
        }
        Ok(())
    }
    pub(in crate::sqlite) async fn root_guard_successor_mutations(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        mutations: &[ProviderStateMutation],
    ) -> Result<(), StoreError> {
        let exists: i64 =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM provider_state WHERE namespace=?)")
                .bind(crate::root_terminal::successor::PIN_NAMESPACE)
                .fetch_one(&mut **tx)
                .await
                .map_err(infrastructure)?;
        if exists == 0 {
            return Ok(());
        }
        for mutation in mutations {
            for (namespace, key) in crate::root_terminal::mutation_endpoints(mutation) {
                let proposed = match mutation {
                    ProviderStateMutation::Put(write) => Some(write.record.payload.as_slice()),
                    _ => None,
                };
                self.root_guard_successor_identity(tx, namespace, key, proposed)
                    .await?;
            }
        }
        Ok(())
    }
    pub(crate) fn root_admit_successor(
        &self,
        request: RootCustodySuccessor,
    ) -> Result<RootDriverClaim, StoreError> {
        request.validate_bounds()?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let floor = self.root_require_cutoff_in(&mut tx, &request.cutoff).await?;
                let mut scopes = Vec::new();
                for namespace in &request.admission.provider_namespaces {
                    if let Some(row) = self.root_optional_row(&mut tx, SCOPE_NAMESPACE, namespace).await? { scopes.push(row); }
                }
                let plan = crate::root_terminal::successor::plan(&request, &scopes, self.max_payload_bytes)?;
                plan.document.require_live(request.admission.event.tick, floor)?;
                let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace='durable-execution' AND json_extract(payload,'$.run')=?")
                    .bind(&plan.document.run).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                let effects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace='durable-effect' AND json_extract(payload,'$.run')=?")
                    .bind(&plan.document.run).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                if count != 0 || effects != 0 { return Err(StoreError::Conflict); }
                self.root_require_no_work(&mut tx, &request.admission.execution.execution_id).await?;
                let mut mutations = admission_rows(&request.admission.execution, &request.admission.event, &request.admission.notification)?;
                mutations.extend([index(RUN_NAMESPACE, &plan.document.run, &plan.document.root), index(ACTOR_NAMESPACE, &plan.document.root, &plan.document.root), put(plan.root, None)]);
                for write in plan.scopes.into_iter().chain(plan.frozen) { mutations.push(put(write.record, write.expected_version)); }
                for (namespace, key) in &plan.document.provider_rows { mutations.push(index(ROW_SCOPE_NAMESPACE, &row_scope_key(namespace, key), &plan.document.root)); }
                mutations.push(put(plan.marker.record, plan.marker.expected_version));
                self.apply_root_mutations_in(&mut tx, mutations).await?;
                self.root_clock(&mut tx, request.admission.event.tick).await?;
                Ok(plan.claim)
            }.await;
            finish_read_transaction(tx, outcome).await
        })?
    }
}
