//! Actual bounded same-store commit acknowledgement for held provider preparations.
use crate::{MAX_PROVIDER_STATE_SCAN, ProviderStateStore, ProviderStateWrite, StoreError};
use std::sync::Arc;

/// Finite 64 MiB local composition budget charges prepared and committed payload copies.
pub const MAX_PROVIDER_COMPOSITION_BYTES: usize = 64 * 1024 * 1024;

/// Non-Serde acknowledgement minted only after an actual successful atomic store write.
/// This is not permission to fabricate provider codecs, release their owners or replay a cursor.
pub struct ProviderCompositionCommit {
    store: Arc<dyn ProviderStateStore>,
    writes: Vec<ProviderStateWrite>,
}
impl ProviderCompositionCommit {
    /// Commit provider-validated preparations to their exact shared physical store.
    /// Unknown backend errors remain errors; no sequential write or readback fallback exists.
    pub fn publish(
        store: Arc<dyn ProviderStateStore>,
        mut writes: Vec<ProviderStateWrite>,
    ) -> Result<Self, StoreError> {
        if writes.len() >= MAX_PROVIDER_STATE_SCAN {
            return Err(StoreError::CapacityExceeded);
        }
        let mut bytes = 0usize;
        for write in &writes {
            write
                .record
                .validate_write(MAX_PROVIDER_COMPOSITION_BYTES)?;
            if write
                .expected_version
                .map_or(write.record.version != 1, |v| {
                    v.checked_add(1) != Some(write.record.version)
                })
            {
                return Err(StoreError::Conflict);
            }
            bytes = bytes
                .checked_add(write.record.namespace.len())
                .and_then(|n| n.checked_add(write.record.key.len()))
                .and_then(|n| n.checked_add(write.record.payload.len()))
                .ok_or(StoreError::CapacityExceeded)?;
        }
        if bytes
            .checked_mul(2)
            .is_none_or(|n| n > MAX_PROVIDER_COMPOSITION_BYTES)
        {
            return Err(StoreError::CapacityExceeded);
        }
        writes.sort_by(|a, b| {
            (&a.record.namespace, &a.record.key).cmp(&(&b.record.namespace, &b.record.key))
        });
        if writes.windows(2).any(|pair| {
            pair[0].record.namespace == pair[1].record.namespace
                && pair[0].record.key == pair[1].record.key
        }) {
            return Err(StoreError::Conflict);
        }
        store.put_provider_states_atomic(writes.clone())?;
        Ok(Self { store, writes })
    }
    /// Match exact prepared bytes and expected versions under their still-held provider owner.
    /// A different store Arc or merely equal rows cannot borrow this acknowledgement.
    pub fn covers(
        &self,
        store: &Arc<dyn ProviderStateStore>,
        writes: &[ProviderStateWrite],
    ) -> bool {
        Arc::ptr_eq(&self.store, store)
            && writes.iter().all(|write| {
                self.writes
                    .binary_search_by(|candidate| {
                        (&candidate.record.namespace, &candidate.record.key)
                            .cmp(&(&write.record.namespace, &write.record.key))
                    })
                    .is_ok_and(|index| self.writes[index] == *write)
            })
    }
}
