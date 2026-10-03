//! Read assertions and the existing audited kernel share this sole State lock.
use super::*;
use crate::checked_read::{self as shared, Budget};
use crate::root_terminal::{
    ACTOR_NAMESPACE, Document, ROW_SCOPE_NAMESPACE, RUN_NAMESPACE, SCOPE_NAMESPACE, row_scope_key,
};
use mainframe_env_store_api::{
    CheckedProviderReadPublication, ProviderReplayAssertion, ROOT_DRIVER_NAMESPACE,
    TerminalRowDependency,
};
#[cfg(test)]
mod tests;

impl MemoryStore {
    pub(super) fn publish_checked_read(
        &self,
        r: CheckedProviderReadPublication,
    ) -> Result<(), StoreError> {
        let mut budget = Budget::new(r.validate_bounds(self.limits.max_blob_bytes)?);
        crate::publication::validate(&r.publication, self.limits.max_blob_bytes)?;
        let mut state = self.lock()?;
        check(
            &state,
            &r.publication.intent,
            &r.execution,
            r.publication.observed_tick,
            &r.dependencies,
            self.limits.max_blob_bytes,
            &mut budget,
        )?;
        // Reuse the existing exact audit principal/metadata validator as well.
        crate::publication::assert_fence(
            &r.publication,
            &r.publication.intent,
            &r.execution,
            super::publication::logical_floor(&state)?,
        )?;
        for mutation in &r.publication.mutations {
            if let ProviderStateMutation::Put(w) = mutation {
                super::root_terminal::guard_provider(
                    &state,
                    &w.record.namespace,
                    &w.record.key,
                    Some(&w.record.payload),
                )?;
            }
        }
        Self::append_audited_locked(&mut state, r.publication, self.limits)
    }
    pub(super) fn assert_checked_replay(
        &self,
        r: ProviderReplayAssertion,
    ) -> Result<(), StoreError> {
        let mut budget = shared::replay_validate(&r, self.limits.max_blob_bytes)?;
        let state = self.lock()?;
        check(
            &state,
            &r.effect,
            &r.execution,
            r.observed_tick,
            &r.dependencies,
            self.limits.max_blob_bytes,
            &mut budget,
        )
    }
}

fn get<'a>(
    state: &'a State,
    n: &str,
    k: &str,
    max: usize,
    budget: &mut Budget,
) -> Result<Option<&'a ProviderStateRecord>, StoreError> {
    let row = state.provider_state.get(&(n.into(), k.into()));
    if let Some(row) = row {
        budget.row(row, max)?;
    }
    Ok(row)
}
fn check(
    state: &State,
    e: &EffectRecord,
    x: &ExecutionRecord,
    tick: u64,
    dependencies: &[TerminalRowDependency],
    max: usize,
    budget: &mut Budget,
) -> Result<(), StoreError> {
    let retained = state.effects.get(&e.key).ok_or(StoreError::NotFound)?;
    let execution = state
        .executions
        .get(&e.execution_id)
        .ok_or(StoreError::NotFound)?;
    shared::memory_core_budget(retained, execution, max, budget)?;
    let floor = super::publication::logical_floor(state)?;
    shared::fence(e, x, retained, execution, tick, floor)?;
    let actor = get(state, ACTOR_NAMESPACE, x.execution_id.as_str(), max, budget)?;
    let run = get(state, RUN_NAMESPACE, x.run_unit_id.as_str(), max, budget)?;
    let doc = match actor {
        Some(actor) => {
            let root =
                std::str::from_utf8(&actor.payload).map_err(|_| StoreError::IncompatibleVersion)?;
            if root.len() > mainframe_env_store_api::MAX_PROVIDER_KEY_BYTES {
                return Err(StoreError::CapacityExceeded);
            }
            let row = get(state, ROOT_DRIVER_NAMESPACE, root, max, budget)?
                .ok_or(StoreError::NotFound)?;
            let doc = Document::read(row)?;
            doc.require_index(actor)?;
            doc.require_index(run.ok_or(StoreError::NotFound)?)?;
            doc.require_writer_actor(execution)?;
            doc.require_live(tick, floor)?;
            Some(doc)
        }
        None if run.is_some() => return Err(StoreError::IncompatibleVersion),
        None => None,
    };
    for dependency in dependencies {
        let (n, k) = shared::identity(dependency);
        let ns = get(state, SCOPE_NAMESPACE, n, max, budget)?;
        let row = get(
            state,
            ROW_SCOPE_NAMESPACE,
            &row_scope_key(n, k),
            max,
            budget,
        )?;
        shared::scope(doc.as_ref(), n, k, ns, row)?;
        shared::compare(dependency, get(state, n, k, max, budget)?)?;
    }
    Ok(())
}
