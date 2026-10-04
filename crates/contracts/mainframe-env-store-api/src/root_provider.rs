//! Original-effect attribution for registered Open-root provider mutations.
use crate::{EffectIntentMetadata, ProviderStateMutation, RootProviderRowAdmission, StoreError};

/// Bounded structural observations, not host admission or a mutable writer permit.
/// The backend reads the real intent and compares all observations under the same
/// physical transaction as publication. No caller-created EffectRecord is accepted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootProviderPublication {
    /// Original occurrence, exact running execution, registered anchor and live tick.
    pub occurrence: RootProviderRowAdmission,
    /// Exact original retained coordinator metadata, including its recovery epoch.
    pub intent: EffectIntentMetadata,
    /// Owned mutations; both Move endpoints require this same root's membership.
    pub mutations: Vec<ProviderStateMutation>,
}

impl RootProviderPublication {
    /// Refuse excessive input before backend cloning or encoding. This validates
    /// shape only; current execution, intent, membership and clocks remain physical checks.
    /// `captured_bytes` is the backend's actual retained root-row length, not
    /// caller authority. Backends check again with that length before decoding it.
    pub fn validate_bounds(
        &self,
        max_payload_bytes: usize,
        captured_bytes: usize,
    ) -> Result<(), StoreError> {
        if self.mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        if self.mutations.len() > crate::MAX_ROOT_OPERATIONS {
            return Err(StoreError::CapacityExceeded);
        }
        let admission = self.occurrence.claim.admission();
        if admission.provider_namespaces.len() > 16 || admission.provider_rows.len() > 16 {
            return Err(StoreError::CapacityExceeded);
        }
        let mut bytes = self
            .occurrence
            .claim
            .inserted_row()
            .payload
            .len()
            .checked_add(captured_bytes)
            .ok_or(StoreError::CapacityExceeded)?;
        for size in [
            admission.execution.execution_id.as_str().len(),
            admission.execution.run_unit_id.as_str().len(),
            admission.execution.principal.as_str().len(),
            admission.execution.selector.as_str().len(),
            admission.execution.artifact.as_str().len(),
            admission
                .execution
                .owner_lease
                .as_ref()
                .map_or(0, String::len),
            admission.invocation_key.as_str().len(),
            admission.notification.notification_id.len(),
            admission.notification.topic.len(),
            admission.notification.payload.len(),
            self.occurrence
                .execution
                .owner_lease
                .as_ref()
                .map_or(0, String::len),
            self.occurrence.execution.artifact.as_str().len(),
            self.occurrence.execution.selector.as_str().len(),
            self.occurrence.execution.execution_id.as_str().len(),
            self.occurrence.execution.run_unit_id.as_str().len(),
            self.occurrence.execution.principal.as_str().len(),
            self.occurrence.effect_key.as_str().len(),
            self.occurrence.identity.namespace.len(),
            self.occurrence.identity.key.len(),
            self.intent.owner.as_str().len(),
            self.intent
                .recovery_lease
                .as_ref()
                .map_or(0, |v| v.owner.len()),
            self.intent
                .capability
                .as_ref()
                .map_or(0, |v| v.as_str().len()),
            self.intent
                .audit_invocation_key
                .as_ref()
                .map_or(0, |v| v.as_str().len()),
        ] {
            bytes = bytes
                .checked_add(size)
                .ok_or(StoreError::CapacityExceeded)?;
        }
        for namespace in &admission.provider_namespaces {
            bytes = bytes
                .checked_add(namespace.len())
                .ok_or(StoreError::CapacityExceeded)?;
        }
        for identity in &admission.provider_rows {
            bytes = bytes
                .checked_add(identity.namespace.len())
                .and_then(|n| n.checked_add(identity.key.len()))
                .ok_or(StoreError::CapacityExceeded)?;
        }
        Self::validate_batch_bounds(&self.mutations, bytes)?;
        admission.validate()?;
        crate::audited_publication::validate_provider_mutations(&self.mutations, max_payload_bytes)
    }

    /// Check the combined finite row batch and captured structural bytes without
    /// allocation. Move consumes two operations. This is shape validation only;
    /// physical membership, intent and mutation validation remain mandatory.
    pub fn validate_batch_bounds(
        mutations: &[ProviderStateMutation],
        mut bytes: usize,
    ) -> Result<(), StoreError> {
        if bytes > crate::MAX_ROOT_PAYLOAD_BYTES || mutations.len() > crate::MAX_ROOT_OPERATIONS {
            return Err(StoreError::CapacityExceeded);
        }
        let mut operations = 0usize;
        for mutation in mutations {
            let (payload, namespace, key, old) = match mutation {
                ProviderStateMutation::Put(w) => (
                    w.record.payload.len(),
                    &w.record.namespace,
                    &w.record.key,
                    0,
                ),
                ProviderStateMutation::Delete { namespace, key, .. } => (0, namespace, key, 0),
                ProviderStateMutation::Move {
                    record, old_key, ..
                } => (
                    record.payload.len(),
                    &record.namespace,
                    &record.key,
                    old_key.len(),
                ),
            };
            operations = operations
                .checked_add(if matches!(mutation, ProviderStateMutation::Move { .. }) {
                    2
                } else {
                    1
                })
                .ok_or(StoreError::CapacityExceeded)?;
            for size in [payload, namespace.len(), key.len(), old] {
                bytes = bytes
                    .checked_add(size)
                    .ok_or(StoreError::CapacityExceeded)?;
            }
            if operations > crate::MAX_ROOT_OPERATIONS || bytes > crate::MAX_ROOT_PAYLOAD_BYTES {
                return Err(StoreError::CapacityExceeded);
            }
        }
        Ok(())
    }
}
