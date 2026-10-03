//! Effect-free initial preparation in the existing exclusive SQLite transaction.
use super::*;

impl SqliteStateStore {
    pub(crate) fn root_mutate_preparation(
        &self,
        request: RootPreparationPublication,
    ) -> Result<(), StoreError> {
        request.validate_bounds(self.max_payload_bytes, 0)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                // Account the actual retained root before fetching its owned
                // payload. The query and later row read share this writer TX.
                let root_key = request.claim.admission().execution.execution_id.as_str();
                let root_bytes: Option<i64> = sqlx::query_scalar(
                    "SELECT length(payload) FROM provider_state WHERE namespace=? AND key=?",
                ).bind(ROOT_DRIVER_NAMESPACE).bind(root_key)
                    .fetch_optional(&mut *tx).await.map_err(infrastructure)?;
                let root_bytes = usize::try_from(root_bytes.ok_or(StoreError::NotFound)?)
                    .map_err(|_| StoreError::CapacityExceeded)?;
                request.validate_bounds(self.max_payload_bytes, root_bytes)?;
                let actor = self.root_row(&mut tx, "durable-execution",
                    request.execution.execution_id.as_str()).await?;
                if durable::decode_execution(&actor.payload, actor.version)? != request.execution {
                    return Err(StoreError::Conflict);
                }
                let current = self.root_row(&mut tx, ROOT_DRIVER_NAMESPACE,
                    request.claim.admission().execution.execution_id.as_str()).await?;
                request.validate_bounds(self.max_payload_bytes, current.payload.len())?;
                let doc = Document::read(&current)?;
                doc.require_preparation(&request, &current, floor)?;
                let event_namespace = format!("durable-event:{}", doc.root);
                let events = self.root_rows_matching(&mut tx, &event_namespace, None, None, 1).await?;
                if events.len() != 1 || events[0].key != format!("{:020}", 1)
                    || events[0].version != 1
                    || durable::decode_event(&events[0].payload)? != request.claim.admission().event {
                    return Err(StoreError::InvalidTransition);
                }
                let actors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace='durable-execution' AND json_extract(payload,'$.run')=?")
                    .bind(&doc.run).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                let effects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace='durable-effect' AND (json_extract(payload,'$.run')=? OR json_extract(payload,'$.execution')=?)")
                    .bind(&doc.run).bind(&doc.root).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                if actors != 1 || effects != 0 { return Err(StoreError::InvalidTransition); }
                self.root_require_no_work(&mut tx, &request.execution.execution_id).await?;
                for (namespace, key) in [(ACTOR_NAMESPACE, doc.root.as_str()), (RUN_NAMESPACE, doc.run.as_str())] {
                    doc.require_index(&self.root_row(&mut tx, namespace, key).await?)?;
                }
                for namespace in &doc.provider_namespaces {
                    doc.require_scope_index(&self.root_row(&mut tx, SCOPE_NAMESPACE, namespace).await?, namespace, "")?;
                }
                for (namespace, key) in &doc.provider_rows {
                    doc.require_scope_index(&self.root_row(&mut tx, ROW_SCOPE_NAMESPACE, &row_scope_key(namespace, key)).await?, namespace, key)?;
                }
                for (namespace, expected) in [(ACTOR_NAMESPACE, 1), (RUN_NAMESPACE, 1),
                    (SCOPE_NAMESPACE, doc.provider_namespaces.len()), (ROW_SCOPE_NAMESPACE, doc.provider_rows.len())] {
                    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_state WHERE namespace=? AND payload=?")
                        .bind(namespace).bind(doc.root.as_bytes()).fetch_one(&mut *tx).await.map_err(infrastructure)?;
                    if count != expected as i64 { return Err(StoreError::Conflict); }
                }
                self.root_guard_writer_identity(&mut tx, Some(&doc), &request.anchor.namespace, &request.anchor.key, true).await?;
                self.root_guard_writer_scopes(&mut tx, Some(&doc), &request.mutations, true).await?;
                self.root_guard_mutations(&mut tx, &request.mutations).await?;
                self.apply_root_mutations_in(&mut tx, request.mutations).await?;
                self.root_clock(&mut tx, request.observed_tick).await
            }.await;
            finish_read_transaction(tx, outcome).await
        })?
    }
}
