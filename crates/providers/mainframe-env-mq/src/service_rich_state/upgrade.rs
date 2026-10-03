//! Explicit pre-activation quiescent upgrade, never an open-time fallback.
use super::publication::{PublicationError, PublicationLimits, RichPublicationPlan};
use super::*;
use crate::MqObjectName;
use crate::delivery::full_message::QueueProfile;

impl RichStoredState {
    /// The manager must independently authorize deployment/backup/rollback.
    /// No live selected control/runtime is retired or interpreted here. A later
    /// owner-approved quiescence observation is required for activated services.
    pub(in crate::service) fn plan_profile_upgrade(
        &self,
        profiles: &BTreeMap<MqObjectName, QueueProfile>,
        limits: PublicationLimits,
    ) -> Result<RichPublicationPlan, PublicationError> {
        if self.ownership.control.is_some() || self.runtime.is_some() {
            return Err(PublicationError::Source);
        }
        let candidate = self
            .delivery
            .upgrade_profiles(profiles)
            .map_err(DeliveryRowError::from)?;
        // SAME prospective reader and marker/catalog/meta dependencies, schema
        // preserved thereafter by ordinary delta. Captured absence of a control
        // cannot race selected activation: it MUST publish under that marker CAS.
        self.plan(&candidate, false, true, Vec::new(), limits)
    }
}

#[cfg(test)]
#[path = "upgrade/tests.rs"]
mod tests;
