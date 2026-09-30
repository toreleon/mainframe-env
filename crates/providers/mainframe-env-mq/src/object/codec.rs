//! Strict deterministic snapshot codec for the MQ object catalog.

use super::*;
use serde::{Deserialize, Serialize};

impl MqObjectCatalog {
    pub fn encode(&self) -> Result<Vec<u8>, MqObjectError> {
        let envelope = CatalogEnvelope {
            schema_version: MQ_OBJECT_CATALOG_SCHEMA.into(),
            queue_manager: self.queue_manager.clone(),
            objects: self.objects.values().cloned().collect(),
            model_instances: self.instances.values().cloned().collect(),
            next_dynamic_id: self.next_dynamic_id,
        };
        let bytes = serde_json::to_vec(&envelope).map_err(|_| MqObjectError::CorruptSnapshot)?;
        if bytes.len() > self.limits.max_persisted_bytes {
            return Err(MqObjectError::ResourceExhausted);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8], limits: MqObjectLimits) -> Result<Self, MqObjectError> {
        validate_limits(limits)?;
        if bytes.len() > limits.max_persisted_bytes {
            return Err(MqObjectError::ResourceExhausted);
        }
        let identity: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| MqObjectError::CorruptSnapshot)?;
        match identity.get("schema_version") {
            Some(serde_json::Value::String(schema)) if schema == MQ_OBJECT_CATALOG_SCHEMA => {}
            Some(serde_json::Value::String(_)) => return Err(MqObjectError::UnsupportedSchema),
            _ => return Err(MqObjectError::CorruptSnapshot),
        }
        let envelope: CatalogEnvelope =
            serde_json::from_slice(bytes).map_err(|_| MqObjectError::CorruptSnapshot)?;
        if envelope.next_dynamic_id == 0
            || !strictly_sorted_by(&envelope.objects, definition_key)
            || !strictly_sorted_by(&envelope.model_instances, |instance| instance.name.clone())
        {
            return Err(MqObjectError::CorruptSnapshot);
        }
        let mut catalog = Self::new(envelope.queue_manager, envelope.objects, limits)?;
        if envelope.model_instances.len() > limits.max_dynamic_instances {
            return Err(MqObjectError::ResourceExhausted);
        }
        let mut instance_ids = BTreeSet::new();
        for instance in envelope.model_instances {
            if instance.instance_id == 0
                || instance.instance_id >= envelope.next_dynamic_id
                || !instance_ids.insert(instance.instance_id)
                || catalog.queue_definition(&instance.name).is_some()
                || catalog.instances.contains_key(&instance.name)
            {
                return Err(MqObjectError::CorruptSnapshot);
            }
            match catalog.queue_definition(&instance.model) {
                Some(MqObjectDefinition::ModelQueue {
                    definition_type,
                    trigger_process,
                    ..
                }) if *definition_type == instance.definition_type
                    && *trigger_process == instance.trigger_process => {}
                Some(_) => return Err(MqObjectError::InvalidReferenceKind),
                None => return Err(MqObjectError::MissingReference),
            }
            catalog.instances.insert(instance.name.clone(), instance);
        }
        catalog.next_dynamic_id = envelope.next_dynamic_id;
        Ok(catalog)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CatalogEnvelope {
    schema_version: String,
    queue_manager: MqQueueManagerDefinition,
    objects: Vec<MqObjectDefinition>,
    model_instances: Vec<MqModelInstance>,
    next_dynamic_id: u64,
}

fn strictly_sorted_by<T, K: Ord>(values: &[T], key: impl Fn(&T) -> K) -> bool {
    values
        .windows(2)
        .all(|window| key(&window[0]) < key(&window[1]))
}
