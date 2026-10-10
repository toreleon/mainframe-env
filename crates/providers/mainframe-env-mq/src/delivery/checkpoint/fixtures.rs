//! Fixture-only setup and storage observations; no production entry point.

use super::*;

impl MqDeliveryKernel {
    /// Private migration projection of already queued data without issuing PUT.
    /// Reuses the sole entry allocator and checkpoint validation, including
    /// narrowed target limits.
    pub(crate) fn import_legacy_queues(
        catalog: &MqObjectCatalog,
        limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        queues: BTreeMap<MqObjectName, Vec<MqMessage>>,
    ) -> Result<Self, MqDeliveryError> {
        let mut candidate = Self::new(catalog, limits, message_limits, MqPersistence::Persistent)?;
        for (name, messages) in queues {
            let mut entries = Vec::new();
            for message in messages {
                entries.push(Entry {
                    id: candidate.allocate_id()?,
                    message: Payload::Partial(message),
                    expires_at: None,
                });
            }
            candidate.queues.insert(name, entries.into());
        }
        let bytes = candidate.encode_live_checkpoint()?;
        Self::decode_live_checkpoint(
            &bytes,
            catalog,
            limits,
            message_limits,
            MqPersistence::Persistent,
        )
    }
}
