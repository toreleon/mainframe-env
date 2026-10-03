//! Original-effect scoped writes and private audit attribution in the existing TX.
use super::*;
use crate::root_terminal::mutation_endpoints;

impl SqliteStateStore {
    pub(in crate::sqlite) async fn root_writer_document(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        execution: &ExecutionRecord,
        mutations: &[ProviderStateMutation],
    ) -> Result<Option<Document>, StoreError> {
        let Some(binding) = self
            .root_optional_row(tx, ACTOR_NAMESPACE, execution.execution_id.as_str())
            .await?
        else {
            if self
                .root_optional_row(tx, RUN_NAMESPACE, execution.run_unit_id.as_str())
                .await?
                .is_some()
            {
                return Err(StoreError::IncompatibleVersion);
            }
            return Ok(None);
        };
        let root =
            std::str::from_utf8(&binding.payload).map_err(|_| StoreError::IncompatibleVersion)?;
        let current = self.root_row(tx, ROOT_DRIVER_NAMESPACE, root).await?;
        RootProviderPublication::validate_batch_bounds(mutations, current.payload.len())?;
        let doc = Document::read(&current)?;
        doc.require_index(&binding)?;
        doc.require_index(
            &self
                .root_row(tx, RUN_NAMESPACE, execution.run_unit_id.as_str())
                .await?,
        )?;
        doc.require_writer_actor(execution)?;
        Ok(Some(doc))
    }
    pub(in crate::sqlite) async fn root_guard_writer_scopes(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        doc: Option<&Document>,
        mutations: &[ProviderStateMutation],
        registered_only: bool,
    ) -> Result<(), StoreError> {
        for mutation in mutations {
            for (namespace, key) in mutation_endpoints(mutation) {
                self.root_guard_writer_identity(tx, doc, namespace, key, registered_only)
                    .await?;
            }
        }
        Ok(())
    }
    async fn root_guard_writer_identity(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        doc: Option<&Document>,
        namespace: &str,
        key: &str,
        registered_only: bool,
    ) -> Result<(), StoreError> {
        let mut found = false;
        for (n, k) in [
            (SCOPE_NAMESPACE, namespace.to_string()),
            (ROW_SCOPE_NAMESPACE, row_scope_key(namespace, key)),
        ] {
            if let Some(binding) = self.root_optional_row(tx, n, &k).await? {
                doc.ok_or(StoreError::InvalidTransition)?
                    .require_scope_index(&binding, namespace, key)?;
                found = true;
            }
        }
        if !found
            && (registered_only
                || doc.is_some_and(|doc| doc.require_owned_identity(namespace, key).is_ok()))
        {
            return Err(StoreError::InvalidTransition);
        }
        Ok(())
    }
    pub(crate) fn root_mutate_provider(
        &self,
        request: RootProviderPublication,
    ) -> Result<(), StoreError> {
        request.validate_bounds(self.max_payload_bytes, 0)?;
        block_on(&self.runtime, async {
            let mut tx = self.pool.begin().await.map_err(infrastructure)?;
            let outcome = async {
                let (_, floor) = self.root_lock(&mut tx).await?;
                let actor = self
                    .root_row(
                        &mut tx,
                        "durable-execution",
                        request.occurrence.execution.execution_id.as_str(),
                    )
                    .await?;
                let execution = durable::decode_execution(&actor.payload, actor.version)?;
                if execution != request.occurrence.execution {
                    return Err(StoreError::Conflict);
                }
                let row = self
                    .root_row(
                        &mut tx,
                        "durable-effect",
                        request.occurrence.effect_key.as_str(),
                    )
                    .await?;
                let intent = durable::decode_effect(&request.occurrence.effect_key, &row.payload)?;
                let current = self
                    .root_row(
                        &mut tx,
                        ROOT_DRIVER_NAMESPACE,
                        request
                            .occurrence
                            .claim
                            .admission()
                            .execution
                            .execution_id
                            .as_str(),
                    )
                    .await?;
                request.validate_bounds(self.max_payload_bytes, current.payload.len())?;
                let doc = Document::read(&current)?;
                doc.require_index(
                    &self
                        .root_row(&mut tx, ACTOR_NAMESPACE, execution.execution_id.as_str())
                        .await?,
                )?;
                doc.require_index(
                    &self
                        .root_row(&mut tx, RUN_NAMESPACE, execution.run_unit_id.as_str())
                        .await?,
                )?;
                doc.require_writer(&request, &intent, floor)?;
                self.root_guard_writer_identity(
                    &mut tx,
                    Some(&doc),
                    &request.occurrence.identity.namespace,
                    &request.occurrence.identity.key,
                    true,
                )
                .await?;
                self.root_guard_writer_scopes(&mut tx, Some(&doc), &request.mutations, true)
                    .await?;
                self.root_guard_mutations(&mut tx, &request.mutations)
                    .await?;
                self.apply_root_mutations_in(&mut tx, request.mutations)
                    .await?;
                self.root_clock(&mut tx, request.occurrence.observed_tick)
                    .await
            }
            .await;
            finish_read_transaction(tx, outcome).await
        })?
    }
}
