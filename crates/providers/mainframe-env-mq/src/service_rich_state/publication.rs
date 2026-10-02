//! Bounded private plans for composition in the existing audited transaction.
//! No store writes, owner minting, coordinator decisions or public selection.

use super::*;
use mainframe_env_store_api::MAX_AUDITED_PROVIDER_MUTATIONS;

/// Smaller than the shared physical ceiling, leaving room for service-owned
/// replay/UOW composition. The FULL composed audited request must also fit.
#[derive(Clone, Copy, Debug)]
pub(in crate::service) struct PublicationLimits {
    pub(in crate::service) mutations: usize,
    pub(in crate::service) row_bytes: usize,
    pub(in crate::service) total_bytes: usize,
}
impl Default for PublicationLimits {
    fn default() -> Self {
        Self {
            mutations: 1024,
            row_bytes: 64 << 20,
            total_bytes: 64 << 20,
        }
    }
}
impl PublicationLimits {
    fn validate(self) -> Result<(), PublicationError> {
        let ceiling = Self::default();
        for (value, max) in [
            (self.mutations, ceiling.mutations),
            (self.row_bytes, ceiling.row_bytes),
            (self.total_bytes, ceiling.total_bytes),
        ] {
            if value == 0 || value > max {
                return Err(PublicationError::Bounds);
            }
        }
        if self.mutations > MAX_AUDITED_PROVIDER_MUTATIONS {
            return Err(PublicationError::Bounds);
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::service) enum PublicationError {
    Bounds,
    Source,
    Version,
    Rows(DeliveryRowError),
    Read(ReadError),
}
impl From<DeliveryRowError> for PublicationError {
    fn from(e: DeliveryRowError) -> Self {
        Self::Rows(e)
    }
}
impl From<ReadError> for PublicationError {
    fn from(e: ReadError) -> Self {
        Self::Read(e)
    }
}

pub(in crate::service) struct RichPublicationPlan {
    mutations: Vec<ProviderStateMutation>,
    next: RichStoredState,
}
impl RichPublicationPlan {
    pub(in crate::service) fn mutations(&self) -> &[ProviderStateMutation] {
        &self.mutations
    }

    /// Adoption is authorized ONLY after the complete composed audited store
    /// transaction succeeds. Failure/uncertainty never authorizes redispatch.
    pub(in crate::service) fn into_parts(self) -> (Vec<ProviderStateMutation>, RichStoredState) {
        (self.mutations, self.next)
    }
}

impl RichStoredState {
    /// Ordinary logical publication at the EXACT captured identity. The caller
    /// supplies only a delivery candidate, never old hashes/versions/owners.
    pub(in crate::service) fn plan_delivery(
        &self,
        candidate: &MqDeliveryKernel,
        limits: PublicationLimits,
    ) -> Result<RichPublicationPlan, PublicationError> {
        self.plan(candidate, false, limits)
    }

    /// Explicit next persisted fence, same catalog/generation and live state.
    /// No implicit backout/cold recovery, coordinator permit or owner retirement.
    pub(in crate::service) fn plan_next_fence(
        &self,
        limits: PublicationLimits,
    ) -> Result<RichPublicationPlan, PublicationError> {
        self.plan(&self.delivery, true, limits)
    }

    fn plan(
        &self,
        candidate: &MqDeliveryKernel,
        advance: bool,
        limits: PublicationLimits,
    ) -> Result<RichPublicationPlan, PublicationError> {
        limits.validate()?;
        // Reuse the sole strict reader on captured records. Never rescan a store
        // or trust a caller's edits to the service's cached marker/version views.
        let (generation, fence) = self.marker.identity.generation_and_fence();
        let source_records = self
            .rows
            .captured_records()
            .cloned()
            .chain(self.retained_records.iter().cloned())
            .collect();
        let StoredAuthority::Rich(source) =
            decode_records(source_records, generation, fence, self.limits)?
        else {
            return Err(PublicationError::Source);
        };
        if source.marker != self.marker
            || source.versions != self.versions
            || source.replay != self.replay
            || source
                .catalog
                .encode()
                .map_err(|_| PublicationError::Source)?
                != self
                    .catalog
                    .encode()
                    .map_err(|_| PublicationError::Source)?
            || source
                .delivery
                .encode_live_checkpoint()
                .map_err(|_| PublicationError::Source)?
                != self
                    .delivery
                    .encode_live_checkpoint()
                    .map_err(|_| PublicationError::Source)?
        {
            return Err(PublicationError::Source);
        }
        let identity = if advance {
            source.marker.identity.next_fence()?
        } else {
            source.marker.identity.clone()
        };
        // Delta is the original semantic/projection authority. It rejects lost
        // pending/final references, regressed counters and catalog replacement.
        let delta = if advance {
            source.rows.next_fence_delta()?
        } else {
            source
                .rows
                .delta(candidate, &source.catalog, identity.clone())?
        };
        let (mut mutations, next_rows) = delta.into_parts();
        let mut retained = source.retained_records;
        for (namespace, key) in [
            (CATALOG_NAMESPACE, CATALOG_KEY),
            (STATE_NAMESPACE, STATE_KEY),
        ] {
            let old = retained
                .iter_mut()
                .find(|r| r.namespace == namespace && r.key == key)
                .ok_or(PublicationError::Source)?;
            let expected = old.version;
            old.version = expected
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or(PublicationError::Version)?;
            if namespace == STATE_NAMESPACE && advance {
                let marker = RichMarker {
                    identity: identity.clone(),
                    ..source.marker.clone()
                };
                old.payload = serde_json::to_vec(&marker).map_err(|_| PublicationError::Source)?;
            }
            // Catalog and ordinary marker retain their EXACT captured bytes.
            mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: old.clone(),
                expected_version: Some(expected),
            }));
        }
        let mut bytes = 0usize;
        if mutations.len() > limits.mutations {
            return Err(PublicationError::Bounds);
        }
        for mutation in &mutations {
            if let ProviderStateMutation::Put(w) = mutation {
                bytes = bytes
                    .checked_add(w.record.payload.len())
                    .ok_or(PublicationError::Bounds)?;
                if w.record.payload.len() > limits.row_bytes || bytes > limits.total_bytes {
                    return Err(PublicationError::Bounds);
                }
            }
        }
        // Strict restore with the captured service profile also refuses a
        // widened candidate and checks rich + retained snapshot quotas together.
        let records = next_rows
            .captured_records()
            .cloned()
            .chain(retained)
            .collect();
        let (generation, fence) = identity.generation_and_fence();
        let StoredAuthority::Rich(next) = decode_records(records, generation, fence, self.limits)?
        else {
            return Err(PublicationError::Source);
        };
        if advance
            && next
                .delivery
                .encode_live_checkpoint()
                .map_err(|_| PublicationError::Source)?
                != self
                    .delivery
                    .encode_live_checkpoint()
                    .map_err(|_| PublicationError::Source)?
        {
            return Err(PublicationError::Source);
        }
        Ok(RichPublicationPlan { mutations, next })
    }
}

#[cfg(test)]
#[path = "publication/tests.rs"]
mod tests;
