//! Explicit quiescent schema/profile projection; never called during open.
use super::*;
impl DeliveryRows {
    pub(crate) fn upgrade_delta(
        &self,
        candidate: &MqDeliveryKernel,
        catalog: &MqObjectCatalog,
    ) -> Result<DeliveryRowDelta, DeliveryRowError> {
        let (_, old) = Self::restore(
            self.records.values().cloned().collect(),
            catalog,
            self.metadata.identity.clone(),
            self.limits,
            candidate.limits,
            candidate.message_limits,
            candidate.default_persistence,
        )?;
        let profiles = candidate
            .queues
            .iter()
            .map(|(q, v)| (q.clone(), v.profile))
            .collect();
        let expected = old.upgrade_profiles(&profiles)?;
        if expected != *candidate {
            return Err(DeliveryRowError::Identity);
        }
        prepare(
            Some(self),
            candidate,
            catalog,
            self.metadata.identity.clone(),
            self.limits,
        )
    }
}
