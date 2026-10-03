//! Initial membership validation, without an invented effect or lifecycle stage.
use super::*;

impl Document {
    pub(crate) fn require_preparation(
        &self,
        request: &RootPreparationPublication,
        current: &ProviderStateRecord,
        floor: u64,
    ) -> Result<(), StoreError> {
        self.require_claim(&request.claim)?;
        self.require_live(request.observed_tick, floor)?;
        if self.phase != Phase::Open
            || current != request.claim.inserted_row()
            || request.execution != request.claim.admission().execution
            || request.observed_tick < request.claim.admission().event.tick
            || self.actors.len() != 1
        {
            return Err(StoreError::Conflict);
        }
        self.require_owned_identity(&request.anchor.namespace, &request.anchor.key)?;
        for mutation in &request.mutations {
            for (namespace, key) in mutation_endpoints(mutation) {
                if namespace == durable::AUDIT_NAMESPACE {
                    return Err(StoreError::InvalidTransition);
                }
                self.require_owned_identity(namespace, key)?;
            }
        }
        Ok(())
    }
}
