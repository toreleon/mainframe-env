//! One actual Memory lock; no callback, disposition or replacement old handle.
use super::*;
use mainframe_env_store_api::{ROOT_CUSTODY_NAMESPACE, RootCustodyCutoff};

impl MemoryStore {
    pub(in crate::memory) fn root_capture_cutoff(
        &self,
        claim: &RootDriverClaim,
        execution: &ExecutionRecord,
        tick: u64,
    ) -> Result<RootCustodyCutoff, StoreError> {
        claim.admission().validate()?;
        let mut state = self.lock()?;
        let current = row(
            &state,
            ROOT_DRIVER_NAMESPACE,
            claim.admission().execution.execution_id.as_str(),
        )?
        .clone();
        let mut doc = Document::read(&current)?;
        doc.require_claim(claim)?;
        doc.require_live(tick, state.logical_tick)?;
        if doc.phase != Phase::Open
            || execution.state != ExecutionState::Running
            || execution.execution_id.as_str() != doc.root
            || state.executions.get(&execution.execution_id) != Some(execution)
            || state
                .provider_state
                .contains_key(&(ROOT_CUSTODY_NAMESPACE.into(), doc.root.clone()))
        {
            return Err(StoreError::Conflict);
        }
        let mut preserved = capture(&state, claim, current.clone(), tick)?;
        // Every actual ownership index is checked under this same physical lock.
        for (namespace, key) in crate::root_terminal::custody::custody_index_identities(&doc) {
            let retained = row(&state, &namespace, &key)?;
            doc.require_recorded_index(retained)?;
            crate::root_terminal::custody::custody_require_row(&preserved, retained)?;
            preserved
                .provider_dependencies
                .push(TerminalRowDependency::Exact(retained.clone()));
            preserved.validate_bounds()?;
        }
        // Capture the full effect-audit storage rows, not counts or a global prefix.
        for (key, audit) in &state.audits {
            if doc
                .actors
                .iter()
                .any(|actor| actor.execution == audit.execution_id.as_str())
            {
                let (operations, bytes) =
                    crate::root_terminal::custody::custody_remaining(&preserved)?;
                if operations == 0
                    || bytes < key.len() + crate::durable::AUDIT_NAMESPACE.len() + 4096
                {
                    return Err(StoreError::CapacityExceeded);
                }
                let row = ProviderStateRecord {
                    namespace: crate::durable::AUDIT_NAMESPACE.into(),
                    key: key.clone(),
                    version: 1,
                    payload: crate::durable::encode_audit(audit)?,
                };
                crate::root_terminal::custody::custody_require_row(&preserved, &row)?;
                preserved.core_records.push(row);
                preserved.validate_bounds()?;
            }
        }
        doc.phase = Phase::Uncertain;
        let fenced = doc.row(
            current.version.checked_add(1).ok_or(StoreError::Conflict)?,
            self.limits.max_blob_bytes,
        )?;
        let mut result = RootCustodyCutoff {
            preserved,
            fenced,
            marker: ProviderStateRecord {
                namespace: ROOT_CUSTODY_NAMESPACE.into(),
                key: doc.root,
                version: 1,
                payload: Vec::new(),
            },
            fenced_epoch: 0,
        };
        result.marker = crate::root_terminal::custody::custody_marker(&result)?;
        result.validate_bounds()?;
        journal::journaled(&mut state, |state, journal| {
            write(
                state,
                journal,
                result.fenced.clone(),
                Some(current.version),
                self.limits,
            )?;
            write(state, journal, result.marker.clone(), None, self.limits)?;
            journal.touch_logical_tick(state);
            state.logical_tick = tick;
            result.fenced_epoch = state.provider_epoch;
            Ok(result)
        })
    }
}
