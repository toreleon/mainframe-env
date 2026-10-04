//! Effect-free initial-root preparation; observations never attest host admission.
use crate::{
    ExecutionRecord, ProviderStateIdentity, ProviderStateMutation, RootDriverClaim,
    RootProviderPublication, StoreError,
};

/// One bounded publication under an already physically admitted initial root.
/// No intent, audit, lifecycle event, work or scope enrollment is constructed.
/// Backends recheck every observation inside the mutation's physical transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootPreparationPublication {
    /// Original root admission and inserted membership observation.
    pub claim: RootDriverClaim,
    /// Exact current execution; only the original Admitted version1 is eligible.
    pub execution: ExecutionRecord,
    /// Already registered namespace or exact-row anchor of the same root.
    pub anchor: ProviderStateIdentity,
    /// Positive logical observation, not a physical Work/Job decision clock.
    pub observed_tick: u64,
    /// Owned batch; both Move endpoints require the same retained membership.
    pub mutations: Vec<ProviderStateMutation>,
}

impl RootPreparationPublication {
    /// Bound all captured input before backend cloning/decoding. `captured_bytes`
    /// is the actual retained root payload length; it supplies no permission.
    pub fn validate_bounds(
        &self,
        max_payload_bytes: usize,
        captured_bytes: usize,
    ) -> Result<(), StoreError> {
        if self.mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        if self.anchor.namespace.is_empty()
            || self.anchor.namespace.len() > crate::MAX_PROVIDER_NAMESPACE_BYTES
            || self.anchor.key.is_empty()
            || self.anchor.key.len() > crate::MAX_PROVIDER_KEY_BYTES
            || self.anchor.namespace.starts_with("durable-")
        {
            return Err(StoreError::InvalidTransition);
        }
        let admission = self.claim.admission();
        if admission.provider_namespaces.len() > 16 || admission.provider_rows.len() > 16 {
            return Err(StoreError::CapacityExceeded);
        }
        let mut bytes = self
            .claim
            .inserted_row()
            .payload
            .len()
            .checked_add(captured_bytes)
            .ok_or(StoreError::CapacityExceeded)?;
        let mut add = |size: usize| -> Result<(), StoreError> {
            bytes = bytes
                .checked_add(size)
                .ok_or(StoreError::CapacityExceeded)?;
            Ok(())
        };
        for execution in [&admission.execution, &self.execution] {
            for text in [
                execution.execution_id.as_str(),
                execution.run_unit_id.as_str(),
                execution.principal.as_str(),
                execution.selector.as_str(),
                execution.artifact.as_str(),
                execution.owner_lease.as_deref().unwrap_or(""),
            ] {
                add(text.len())?;
            }
        }
        for text in [
            admission.invocation_key.as_str(),
            admission.event.execution_id.as_str(),
            admission.event.run_unit_id.as_str(),
            admission.notification.execution_id.as_str(),
            &admission.notification.notification_id,
            &admission.notification.topic,
            &self.anchor.namespace,
            &self.anchor.key,
            &self.claim.inserted_row().namespace,
            &self.claim.inserted_row().key,
        ] {
            add(text.len())?;
        }
        add(admission.notification.payload.len())?;
        for namespace in &admission.provider_namespaces {
            add(namespace.len())?;
        }
        for identity in &admission.provider_rows {
            add(identity.namespace.len())?;
            add(identity.key.len())?;
        }
        RootProviderPublication::validate_batch_bounds(&self.mutations, bytes)?;
        admission.validate()?;
        crate::audited_publication::validate_provider_mutations(&self.mutations, max_payload_bytes)
    }
}
