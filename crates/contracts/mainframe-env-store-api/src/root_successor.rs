//! Structural storage request, not a manager permission or native attestation.
use crate::*;

/// Captured unknown predecessor and fresh actual coordinator admission.
/// The closed manager derives handoff identities from its actual MQ runtime.
/// All other represented provider rows remain fenced to the uncertain owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootCustodySuccessor {
    pub cutoff: RootCustodyCutoff,
    pub admission: RootDriverAdmission,
    pub handoff_rows: Vec<ProviderStateIdentity>,
}
impl RootCustodySuccessor {
    pub fn validate_bounds(&self) -> Result<(), StoreError> {
        self.cutoff.validate_bounds()?;
        self.admission.validate()?;
        // This transaction retains the predecessor actors and admits one new
        // root. Charge their aggregate before any physical state is changed.
        if self
            .cutoff
            .preserved
            .actors
            .len()
            .checked_add(1)
            .ok_or(StoreError::CapacityExceeded)?
            > MAX_ROOT_ACTORS
        {
            return Err(StoreError::CapacityExceeded);
        }
        if self.handoff_rows.len() != 4
            || self.admission.event.tick < self.cutoff.preserved.observed_tick
        {
            return Err(StoreError::InvalidTransition);
        }
        for (i, identity) in self.handoff_rows.iter().enumerate() {
            if identity.namespace.is_empty() || identity.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
                || identity.key.is_empty() || identity.key.len() > MAX_PROVIDER_KEY_BYTES
                || identity.namespace.starts_with("durable-") || self.handoff_rows[..i].contains(identity)
                || !self.admission.provider_namespaces.contains(&identity.namespace)
                || !self.cutoff.preserved.provider_dependencies.iter().any(|dependency| matches!(dependency, TerminalRowDependency::Exact(row) if row.namespace == identity.namespace && row.key == identity.key)) {
                return Err(StoreError::Conflict);
            }
        }
        Ok(())
    }
}
