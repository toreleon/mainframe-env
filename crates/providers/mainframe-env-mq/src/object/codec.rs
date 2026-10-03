//! Strict deterministic snapshot codec for the MQ object catalog.

use super::*;
use serde::{Deserialize, Serialize};
mod preflight;

impl MqObjectCatalog {
    pub fn encode(&self) -> Result<Vec<u8>, MqObjectError> {
        let envelope = CatalogEnvelope {
            schema_version: MQ_OBJECT_CATALOG_SCHEMA.into(),
            queue_manager: self.queue_manager.clone(),
            objects: self.objects.values().cloned().collect(),
            model_instances: self.instances.values().cloned().collect(),
            next_dynamic_id: self.next_dynamic_id,
        };
        let bytes = if let Some(native_attributes) = &self.native_attributes {
            self.validate_native_attributes(native_attributes)?;
            serde_json::to_vec(&NativeCatalogEnvelope {
                schema_version: MQ_OBJECT_NATIVE_CATALOG_SCHEMA.into(),
                queue_manager: envelope.queue_manager,
                objects: envelope.objects,
                model_instances: envelope.model_instances,
                next_dynamic_id: envelope.next_dynamic_id,
                native_attributes: native_attributes.clone(),
            })
        } else {
            serde_json::to_vec(&envelope)
        }
        .map_err(|_| MqObjectError::CorruptSnapshot)?;
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
        let identity: Schema =
            serde_json::from_slice(bytes).map_err(|_| MqObjectError::CorruptSnapshot)?;
        let (envelope, native) = match identity.schema_version.as_str() {
            MQ_OBJECT_CATALOG_SCHEMA => (
                serde_json::from_slice::<CatalogEnvelope>(bytes)
                    .map_err(|_| MqObjectError::CorruptSnapshot)?,
                None,
            ),
            MQ_OBJECT_NATIVE_CATALOG_SCHEMA => {
                preflight::check(bytes, limits)?;
                let e: NativeCatalogEnvelope =
                    serde_json::from_slice(bytes).map_err(|_| MqObjectError::CorruptSnapshot)?;
                (
                    CatalogEnvelope {
                        schema_version: MQ_OBJECT_CATALOG_SCHEMA.into(),
                        queue_manager: e.queue_manager,
                        objects: e.objects,
                        model_instances: e.model_instances,
                        next_dynamic_id: e.next_dynamic_id,
                    },
                    Some(e.native_attributes),
                )
            }
            _ => return Err(MqObjectError::UnsupportedSchema),
        };
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
        if let Some(attrs) = native {
            catalog.validate_native_attributes(&attrs)?;
            catalog.native_attributes = Some(attrs);
        }
        Ok(catalog)
    }
}

// Discriminator only: ignored fields are streamed, never a semantic Value tree.
#[derive(Deserialize)]
struct Schema {
    schema_version: String,
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
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeCatalogEnvelope {
    schema_version: String,
    queue_manager: MqQueueManagerDefinition,
    objects: Vec<MqObjectDefinition>,
    model_instances: Vec<MqModelInstance>,
    next_dynamic_id: u64,
    native_attributes: MqNativeAttributes,
}

fn strictly_sorted_by<T, K: Ord>(values: &[T], key: impl Fn(&T) -> K) -> bool {
    values
        .windows(2)
        .all(|window| key(&window[0]) < key(&window[1]))
}
