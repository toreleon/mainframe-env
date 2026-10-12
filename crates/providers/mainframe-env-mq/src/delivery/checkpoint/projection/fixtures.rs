//! Fixture-only setup and storage observations; no production entry point.

use super::*;

impl Projection {
    fn into_live(self) -> Result<Checkpoint, MqDeliveryError> {
        Ok(Checkpoint {
            schema_version: LIVE_SCHEMA.into(),
            manager: self.manager,
            default_persistent: self.default_persistent,
            tick: self.tick,
            next_id: self.next_id,
            next_cursor: self.next_cursor,
            queues: self
                .queues
                .into_iter()
                .map(QueueTwo::into_live)
                .collect::<Result<_, _>>()?,
            pending: self
                .pending
                .into_iter()
                .map(UnitTwo::into_live)
                .collect::<Result<_, _>>()?,
            finalized: self.finalized,
            cursors: self.cursors,
        })
    }
}

impl MqDeliveryKernel {
    /// Strict schema-selected private storage read. Old public readers keep @1.
    pub(crate) fn decode_stored(
        bytes: &[u8],
        catalog: &MqObjectCatalog,
        limits: MqDeliveryLimits,
        message_limits: MqMessageLimits,
        persistence: MqPersistence,
        cold: bool,
    ) -> Result<Self, MqDeliveryError> {
        limits.validate()?;
        message_limits.validate()?;
        preflight::check(bytes, limits, message_limits)?;
        #[derive(Deserialize)]
        struct Header {
            schema_version: String,
        }
        let header: Header =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        if header.schema_version
            == if cold {
                MQ_DELIVERY_SCHEMA
            } else {
                Self::LIVE_CHECKPOINT_SCHEMA
            }
        {
            return if cold {
                Self::decode(bytes, catalog, limits, message_limits, persistence)
            } else {
                Self::decode_live_checkpoint(bytes, catalog, limits, message_limits, persistence)
            };
        }
        if header.schema_version != if cold { COLD_SCHEMA } else { LIVE_SCHEMA } {
            return Err(MqDeliveryError::UnsupportedSchema);
        }
        let projection: Projection =
            serde_json::from_slice(bytes).map_err(|_| MqDeliveryError::CorruptSnapshot)?;
        if cold
            && (!projection.pending.is_empty()
                || !projection.cursors.is_empty()
                || projection
                    .queues
                    .iter()
                    .flat_map(|q| &q.messages)
                    .any(|m| !m.persistent))
        {
            return Err(MqDeliveryError::CorruptSnapshot);
        }
        Self::restore_projection(
            projection.into_live()?,
            Self::new(catalog, limits, message_limits, persistence)?,
        )
    }
}
