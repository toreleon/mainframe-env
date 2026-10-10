//! Fixture-only setup and storage observations; no production entry point.

use super::*;
use mainframe_env_store_api::ProviderStateStore;

impl DeliveryRows {
    /// Explicit creation only. The service must authorize creating/migrating this
    /// authority and reconcile legacy state before calling this. Never falls back
    /// from failed restore. Orphan rich rows prohibit initialization.
    pub(crate) fn initialize(
        store: &dyn ProviderStateStore,
        kernel: &MqDeliveryKernel,
        catalog: &MqObjectCatalog,
        identity: DeliveryRowIdentity,
        limits: DeliveryRowLimits,
    ) -> Result<DeliveryRowDelta, DeliveryRowError> {
        limits.validate()?;
        if !store.list_provider_state_prefix(PREFIX, 1)?.is_empty() {
            return Err(DeliveryRowError::Corrupt);
        }
        prepare(None, kernel, catalog, identity, limits)
    }
}
