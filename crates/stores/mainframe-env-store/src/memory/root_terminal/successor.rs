//! Fresh admitted owner + versioned scopes + immutable predecessor in one lock.
use super::*;
use mainframe_env_store_api::{ROOT_CUSTODY_NAMESPACE, RootCustodySuccessor};

pub(in crate::memory) fn guard_successor_identity(
    state: &State,
    namespace: &str,
    key: &str,
    proposed: Option<&[u8]>,
) -> Result<(), StoreError> {
    if let Some(pin) = state.provider_state.get(&(
        crate::root_terminal::successor::PIN_NAMESPACE.into(),
        row_scope_key(namespace, key),
    )) {
        crate::root_terminal::successor::require_pin(pin, namespace, key, proposed)?;
    }
    Ok(())
}
pub(in crate::memory) fn guard_successor_mutations(
    state: &State,
    mutations: &[ProviderStateMutation],
) -> Result<(), StoreError> {
    if state
        .provider_state
        .range(
            (
                crate::root_terminal::successor::PIN_NAMESPACE.into(),
                String::new(),
            )..,
        )
        .next()
        .is_none_or(|((namespace, _), _)| {
            namespace != crate::root_terminal::successor::PIN_NAMESPACE
        })
    {
        return Ok(());
    }
    for mutation in mutations {
        match mutation {
            ProviderStateMutation::Put(write) => guard_successor_identity(
                state,
                &write.record.namespace,
                &write.record.key,
                Some(&write.record.payload),
            )?,
            _ => {
                for (namespace, key) in crate::root_terminal::mutation_endpoints(mutation) {
                    guard_successor_identity(state, namespace, key, None)?;
                }
            }
        }
    }
    Ok(())
}
impl MemoryStore {
    pub(in crate::memory) fn root_admit_successor(
        &self,
        request: RootCustodySuccessor,
    ) -> Result<RootDriverClaim, StoreError> {
        request.validate_bounds()?;
        let mut state = self.lock()?;
        let old = Document::read(&request.cutoff.preserved.closing)?;
        if row(&state, ROOT_DRIVER_NAMESPACE, &old.root)? != &request.cutoff.fenced
            || row(&state, ROOT_CUSTODY_NAMESPACE, &old.root)? != &request.cutoff.marker
            || state.provider_epoch != request.cutoff.fenced_epoch
        {
            return Err(StoreError::Conflict);
        }
        // Reconstruct the original complete observation from current physical
        // records under this lock. The archived Open row is only its input; the
        // actual root is checked Uncertain/fenced above and never relabelled Open.
        let mut actual = capture(
            &state,
            &request.cutoff.preserved.claim,
            request.cutoff.preserved.closing.clone(),
            request.cutoff.preserved.observed_tick,
        )?;
        for (namespace, key) in crate::root_terminal::custody::custody_index_identities(&old) {
            let retained = row(&state, &namespace, &key)?;
            old.require_recorded_index(retained)?;
            crate::root_terminal::custody::custody_require_row(&actual, retained)?;
            actual
                .provider_dependencies
                .push(TerminalRowDependency::Exact(retained.clone()));
        }
        for (key, audit) in &state.audits {
            if old
                .actors
                .iter()
                .any(|actor| actor.execution == audit.execution_id.as_str())
            {
                let (operations, bytes) =
                    crate::root_terminal::custody::custody_remaining(&actual)?;
                if operations == 0
                    || bytes < key.len() + crate::durable::AUDIT_NAMESPACE.len() + 4096
                {
                    return Err(StoreError::CapacityExceeded);
                }
                let retained = ProviderStateRecord {
                    namespace: crate::durable::AUDIT_NAMESPACE.into(),
                    key: key.clone(),
                    version: 1,
                    payload: crate::durable::encode_audit(audit)?,
                };
                crate::root_terminal::custody::custody_require_row(&actual, &retained)?;
                actual.core_records.push(retained);
            }
        }
        actual.provider_epoch = request.cutoff.preserved.provider_epoch;
        actual.validate_bounds()?;
        if actual != request.cutoff.preserved {
            return Err(StoreError::Conflict);
        }
        let mut scopes = Vec::new();
        for namespace in &request.admission.provider_namespaces {
            if let Some(scope) = state
                .provider_state
                .get(&(SCOPE_NAMESPACE.into(), namespace.clone()))
            {
                scopes.push(scope.clone());
            }
        }
        let plan =
            crate::root_terminal::successor::plan(&request, &scopes, self.limits.max_blob_bytes)?;
        plan.document
            .require_live(request.admission.event.tick, state.logical_tick)?;
        if state
            .executions
            .values()
            .any(|execution| execution.run_unit_id == request.admission.execution.run_unit_id)
            || state
                .effects
                .values()
                .any(|effect| effect.run_unit_id == request.admission.execution.run_unit_id)
            || state
                .checkpoints
                .contains_key(&request.admission.execution.execution_id)
            || state
                .work
                .values()
                .any(|work| work.execution_id == request.admission.execution.execution_id)
        {
            return Err(StoreError::Conflict);
        }
        journal::journaled(&mut state, |state, journal| {
            write(
                state,
                journal,
                index(RUN_NAMESPACE, &plan.document.run, &plan.document.root),
                None,
                self.limits,
            )?;
            write(
                state,
                journal,
                index(ACTOR_NAMESPACE, &plan.document.root, &plan.document.root),
                None,
                self.limits,
            )?;
            write(state, journal, plan.root, None, self.limits)?;
            for change in plan.scopes.into_iter().chain(plan.frozen) {
                write(
                    state,
                    journal,
                    change.record,
                    change.expected_version,
                    self.limits,
                )?;
            }
            for (namespace, key) in &plan.document.provider_rows {
                write(
                    state,
                    journal,
                    index(
                        ROW_SCOPE_NAMESPACE,
                        &row_scope_key(namespace, key),
                        &plan.document.root,
                    ),
                    None,
                    self.limits,
                )?;
            }
            admit(
                state,
                journal,
                request.admission.execution,
                request.admission.event.clone(),
                request.admission.notification,
                self.limits,
            )?;
            write(
                state,
                journal,
                plan.marker.record,
                plan.marker.expected_version,
                self.limits,
            )?;
            journal.touch_logical_tick(state);
            state.logical_tick = request.admission.event.tick;
            Ok(plan.claim)
        })
    }
}
