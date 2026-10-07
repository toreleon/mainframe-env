//! Initial-root preparation under the existing single Memory lock/undo journal.
use super::*;
use mainframe_env_store_api::RootPreparationPublication;

#[cfg(test)]
mod tests;

impl MemoryStore {
    pub(in crate::memory) fn root_mutate_preparation(
        &self,
        request: RootPreparationPublication,
    ) -> Result<(), StoreError> {
        request.validate_bounds(self.limits.max_blob_bytes, 0)?;
        let mut state = self.lock()?;
        let execution = &request.execution;
        if state.executions.get(&execution.execution_id) != Some(execution) {
            return Err(StoreError::Conflict);
        }
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            request.claim.admission().execution.execution_id.as_str(),
        )?;
        request.validate_bounds(self.limits.max_blob_bytes, current.payload.len())?;
        let doc = Document::read(current)?;
        doc.require_preparation(
            &request,
            current,
            super::super::publication::logical_floor(&state)?,
        )?;
        if state.events.get(&execution.execution_id).map(Vec::as_slice)
            != Some(std::slice::from_ref(&request.claim.admission().event))
            || state
                .executions
                .values()
                .filter(|e| e.run_unit_id == execution.run_unit_id)
                .count()
                != 1
            || state.effects.values().any(|e| {
                e.execution_id == execution.execution_id || e.run_unit_id == execution.run_unit_id
            })
            || state.checkpoints.contains_key(&execution.execution_id)
            || state
                .work
                .values()
                .any(|w| w.execution_id == execution.execution_id)
        {
            return Err(StoreError::InvalidTransition);
        }
        // Initial indexes must all survive; a matching anchor alone is insufficient.
        for (namespace, key) in [
            (ACTOR_NAMESPACE, doc.root.as_str()),
            (RUN_NAMESPACE, doc.run.as_str()),
        ] {
            doc.require_index(row(&state, namespace, key)?)?;
        }
        for namespace in &doc.provider_namespaces {
            doc.require_scope_index(row(&state, SCOPE_NAMESPACE, namespace)?, namespace, "")?;
        }
        for (namespace, key) in &doc.provider_rows {
            doc.require_scope_index(
                row(&state, ROW_SCOPE_NAMESPACE, &row_scope_key(namespace, key))?,
                namespace,
                key,
            )?;
        }
        // No phantom index membership can turn an initial root into another stage.
        for (namespace, expected) in [
            (ACTOR_NAMESPACE, 1),
            (RUN_NAMESPACE, 1),
            (SCOPE_NAMESPACE, doc.provider_namespaces.len()),
            (ROW_SCOPE_NAMESPACE, doc.provider_rows.len()),
        ] {
            if state
                .provider_state
                .values()
                .filter(|r| r.namespace == namespace && r.payload == doc.root.as_bytes())
                .count()
                != expected
            {
                return Err(StoreError::Conflict);
            }
        }
        provider_writer::guard_writer_identity(
            &state,
            Some(&doc),
            &request.anchor.namespace,
            &request.anchor.key,
            true,
        )?;
        provider_writer::guard_writer_scopes(&state, Some(&doc), &request.mutations, true)?;
        for mutation in &request.mutations {
            for (namespace, key) in crate::root_terminal::mutation_endpoints(mutation) {
                guard_provider(&state, namespace, key, None)?;
            }
            match mutation {
                ProviderStateMutation::Put(w) => guard_provider(
                    &state,
                    &w.record.namespace,
                    &w.record.key,
                    Some(&w.record.payload),
                )?,
                ProviderStateMutation::Move { record, .. } => guard_provider(
                    &state,
                    &record.namespace,
                    &record.key,
                    Some(&record.payload),
                )?,
                ProviderStateMutation::Delete { .. } => {}
            }
        }
        journal::journaled(&mut state, |state, journal| {
            Self::apply_mutations_locked(state, journal, request.mutations, self.limits)?;
            journal.touch_logical_tick(state);
            state.logical_tick = request.observed_tick;
            Ok(())
        })
    }
}
