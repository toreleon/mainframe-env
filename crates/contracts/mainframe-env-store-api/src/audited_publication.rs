use crate::{EffectRecord, MAX_RETENTION_BATCH, ProviderStateMutation, StoreError};
use mainframe_env_execution_api::{AuditDecision, AuditRecord};

/// Product batch ceiling, shared with the existing bounded row/retention guard.
pub const MAX_AUDITED_PROVIDER_MUTATIONS: usize = MAX_RETENTION_BATCH;

/// Publish provider rows and one typed audit under an exact retained intent.
///
/// This is not a dispatch, authorization, result-finalization or retry protocol.
/// The caller supplies its observed coordinator intent verbatim. Implementations
/// must compare it under the publication lock/transaction, not before acquiring it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditedProviderPublication {
    pub intent: EffectRecord,
    pub audit: AuditRecord,
    pub observed_tick: u64,
    /// Empty is the audit-only form. Core durable rows are never provider-owned.
    pub mutations: Vec<ProviderStateMutation>,
}

impl AuditedProviderPublication {
    /// Bounded row shape checks; this alone does not admit publication.
    pub fn validate_mutations(&self, max_payload_bytes: usize) -> Result<(), StoreError> {
        if self.mutations.len() > MAX_AUDITED_PROVIDER_MUTATIONS {
            return Err(StoreError::CapacityExceeded);
        }
        if self.audit.decision == AuditDecision::Deny && !self.mutations.is_empty() {
            return Err(StoreError::InvalidTransition);
        }
        for mutation in &self.mutations {
            let (namespace, key) = match mutation {
                ProviderStateMutation::Put(write) => {
                    write.record.validate_write(max_payload_bytes)?;
                    if write
                        .expected_version
                        .map_or(write.record.version != 1, |expected| {
                            expected == 0 || expected.checked_add(1) != Some(write.record.version)
                        })
                    {
                        return Err(StoreError::Conflict);
                    }
                    (&write.record.namespace, &write.record.key)
                }
                ProviderStateMutation::Move {
                    record,
                    old_key,
                    expected_version,
                } => {
                    record.validate_move(old_key, *expected_version, max_payload_bytes)?;
                    // Both endpoints of a move must stay in provider ownership.
                    provider_identity(&record.namespace, old_key)?;
                    (&record.namespace, &record.key)
                }
                ProviderStateMutation::Delete {
                    namespace,
                    key,
                    expected_version,
                } => {
                    if *expected_version == 0 || *expected_version > i64::MAX as u64 {
                        return Err(StoreError::Conflict);
                    }
                    (namespace, key)
                }
            };
            provider_identity(namespace, key)?;
        }
        Ok(())
    }
}

fn provider_identity(namespace: &str, key: &str) -> Result<(), StoreError> {
    if namespace.is_empty()
        || namespace.len() > crate::MAX_PROVIDER_NAMESPACE_BYTES
        || key.is_empty()
        || key.len() > crate::MAX_PROVIDER_KEY_BYTES
        || namespace.starts_with("durable-")
        || (namespace == "jes-worker-meta" && key == "logical-clock")
    {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(())
    }
}
