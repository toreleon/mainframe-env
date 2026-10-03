//! Private attribution after physical execution/intent validation; no permit codec.
use super::*;

impl Document {
    pub(crate) fn require_writer(
        &self,
        request: &RootProviderPublication,
        intent: &EffectRecord,
        floor: u64,
    ) -> Result<(), StoreError> {
        self.require_live(request.occurrence.observed_tick, floor)?;
        self.require_occurrence(&request.occurrence, intent)?;
        self.require_writer_actor(&request.occurrence.execution)?;
        if intent.intent != request.intent {
            return Err(StoreError::Conflict);
        }
        if request
            .occurrence
            .execution
            .lease_expiry_tick
            .is_some_and(|tick| tick <= request.occurrence.observed_tick)
        {
            return Err(StoreError::LeaseConflict);
        }
        self.require_owned_identity(
            &request.occurrence.identity.namespace,
            &request.occurrence.identity.key,
        )
    }

    pub(crate) fn require_writer_actor(
        &self,
        execution: &ExecutionRecord,
    ) -> Result<(), StoreError> {
        if self.phase != Phase::Open {
            return Err(StoreError::InvalidTransition);
        }
        if execution.state != ExecutionState::Running
            || execution.terminal_tick.is_some()
            || self.run != execution.run_unit_id.as_str()
            || self.principal != execution.principal.as_str()
            || !self.actors.iter().any(|actor| {
                actor.execution == execution.execution_id.as_str()
                    && actor.artifact == execution.artifact.as_str()
                    && actor.selector == execution.selector.as_str()
                    && actor.attempt == execution.attempt
            })
        {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }

    pub(crate) fn require_owned_identity(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<(), StoreError> {
        if self.provider_namespaces.iter().any(|n| n == namespace)
            || self
                .provider_rows
                .iter()
                .any(|(n, k)| n == namespace && k == key)
        {
            Ok(())
        } else {
            Err(StoreError::InvalidTransition)
        }
    }

    pub(crate) fn require_index(&self, binding: &ProviderStateRecord) -> Result<(), StoreError> {
        if binding.version != 1 || binding.payload != self.root.as_bytes() {
            Err(StoreError::Conflict)
        } else {
            Ok(())
        }
    }

    pub(crate) fn require_scope_index(
        &self,
        binding: &ProviderStateRecord,
        namespace: &str,
        key: &str,
    ) -> Result<(), StoreError> {
        self.require_index(binding)?;
        if (binding.namespace == SCOPE_NAMESPACE
            && self.provider_namespaces.iter().any(|n| n == namespace))
            || (binding.namespace == ROW_SCOPE_NAMESPACE
                && self
                    .provider_rows
                    .iter()
                    .any(|(n, k)| n == namespace && k == key))
        {
            Ok(())
        } else {
            Err(StoreError::IncompatibleVersion)
        }
    }
}

/// Borrow every endpoint without allocating or cloning the owned batch.
pub(crate) fn mutation_endpoints(
    mutation: &ProviderStateMutation,
) -> impl Iterator<Item = (&str, &str)> {
    let (namespace, key, old) = match mutation {
        ProviderStateMutation::Put(w) => (w.record.namespace.as_str(), w.record.key.as_str(), None),
        ProviderStateMutation::Delete { namespace, key, .. } => {
            (namespace.as_str(), key.as_str(), None)
        }
        ProviderStateMutation::Move {
            record, old_key, ..
        } => (
            record.namespace.as_str(),
            record.key.as_str(),
            Some(old_key.as_str()),
        ),
    };
    std::iter::once((namespace, key)).chain(old.map(|k| (namespace, k)))
}
