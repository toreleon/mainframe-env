//! Exact current original-effect writers, inside the sole physical Memory lock.
use super::*;
use crate::root_terminal::mutation_endpoints;
use mainframe_env_store_api::RootProviderPublication;

pub(in crate::memory) fn writer_document(
    state: &State,
    execution: &ExecutionRecord,
    mutations: &[ProviderStateMutation],
) -> Result<Option<Document>, StoreError> {
    let Some(binding) = state.provider_state.get(&(
        ACTOR_NAMESPACE.into(),
        execution.execution_id.as_str().into(),
    )) else {
        if state
            .provider_state
            .contains_key(&(RUN_NAMESPACE.into(), execution.run_unit_id.as_str().into()))
        {
            return Err(StoreError::IncompatibleVersion);
        }
        return Ok(None);
    };
    let root =
        std::str::from_utf8(&binding.payload).map_err(|_| StoreError::IncompatibleVersion)?;
    let current = row(state, ROOT_DRIVER_NAMESPACE, root)?;
    RootProviderPublication::validate_batch_bounds(mutations, current.payload.len())?;
    let document = Document::read(current)?;
    document.require_index(binding)?;
    document.require_index(row(state, RUN_NAMESPACE, execution.run_unit_id.as_str())?)?;
    document.require_writer_actor(execution)?;
    Ok(Some(document))
}

pub(in crate::memory) fn guard_writer_scopes(
    state: &State,
    document: Option<&Document>,
    mutations: &[ProviderStateMutation],
    registered_only: bool,
) -> Result<(), StoreError> {
    for mutation in mutations {
        for (namespace, key) in mutation_endpoints(mutation) {
            guard_writer_identity(state, document, namespace, key, registered_only)?;
        }
    }
    Ok(())
}

fn guard_writer_identity(
    state: &State,
    document: Option<&Document>,
    namespace: &str,
    key: &str,
    registered_only: bool,
) -> Result<(), StoreError> {
    let mut found = false;
    for (n, k) in [
        (SCOPE_NAMESPACE, namespace.to_string()),
        (ROW_SCOPE_NAMESPACE, row_scope_key(namespace, key)),
    ] {
        if let Some(binding) = state.provider_state.get(&(n.into(), k)) {
            document
                .ok_or(StoreError::InvalidTransition)?
                .require_scope_index(binding, namespace, key)?;
            found = true;
        }
    }
    if !found
        && (registered_only
            || document.is_some_and(|doc| doc.require_owned_identity(namespace, key).is_ok()))
    {
        return Err(StoreError::InvalidTransition);
    }
    Ok(())
}

impl MemoryStore {
    pub(in crate::memory) fn root_mutate_provider(
        &self,
        request: RootProviderPublication,
    ) -> Result<(), StoreError> {
        request.validate_bounds(self.limits.max_blob_bytes, 0)?;
        let mut state = self.lock()?;
        if state
            .executions
            .get(&request.occurrence.execution.execution_id)
            != Some(&request.occurrence.execution)
        {
            return Err(StoreError::Conflict);
        }
        let intent = state
            .effects
            .get(&request.occurrence.effect_key)
            .ok_or(StoreError::NotFound)?;
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            request
                .occurrence
                .claim
                .admission()
                .execution
                .execution_id
                .as_str(),
        )?;
        request.validate_bounds(self.limits.max_blob_bytes, current.payload.len())?;
        let doc = Document::read(current)?;
        doc.require_index(row(
            &state,
            ACTOR_NAMESPACE,
            request.occurrence.execution.execution_id.as_str(),
        )?)?;
        doc.require_index(row(
            &state,
            RUN_NAMESPACE,
            request.occurrence.execution.run_unit_id.as_str(),
        )?)?;
        doc.require_writer(
            &request,
            intent,
            super::super::publication::logical_floor(&state)?,
        )?;
        guard_writer_identity(
            &state,
            Some(&doc),
            &request.occurrence.identity.namespace,
            &request.occurrence.identity.key,
            true,
        )?;
        guard_writer_scopes(&state, Some(&doc), &request.mutations, true)?;
        // Existing anonymous/header protections remain additional checks.
        for mutation in &request.mutations {
            for (namespace, key) in mutation_endpoints(mutation) {
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
            state.logical_tick = request.occurrence.observed_tick;
            Ok(())
        })
    }
}
