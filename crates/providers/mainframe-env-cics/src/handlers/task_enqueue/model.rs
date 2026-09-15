use super::{
    CATALOG_KEY, CATALOG_NAMESPACE, CodecReader, LOCK_NAMESPACE, decode_catalog, field, hash_framed,
};
use crate::service::{CicsLimits, CicsService, store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{
    ProviderStateRecord, ProviderStateStore, ProviderStateWrite, StoreError,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MODEL_NAMESPACE: &str = "cics-enqueue-model-v1";
const MODEL_CATALOG_NAMESPACE: &str = "cics-enqueue-model-catalog-v1";
const MODEL_CATALOG_KEY: &str = "models";
const MODEL_MAGIC: &[u8; 8] = b"MECENQM1";
const MODEL_CATALOG_MAGIC: &[u8; 8] = b"MECENQMC";

/// One installed CICS ENQMODEL definition used by ENQ and DEQ routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsEnqueueModelDefinition {
    /// CSD definition name; it is not used to match application resources.
    pub name: String,
    /// One exact resource name or a prefix ending in `*`.
    pub enqueue_name: String,
    /// Nonblank four-character sysplex enqueue scope; `None` is region-local.
    pub enqueue_scope: Option<String>,
    /// Disabled matching models abend ENQ requests.
    pub enabled: bool,
}

impl CicsService {
    /// Install one complete, durable set of ENQMODEL definitions.
    ///
    /// The first installation requires an empty enqueue authority because it
    /// changes local lock identities from the single-region compatibility
    /// profile to explicit APPLID/SYSID scoping.
    pub fn register_enqueue_models(
        &self,
        definitions: &[CicsEnqueueModelDefinition],
    ) -> Result<(), HostProblem> {
        let normalized = definitions
            .iter()
            .map(normalize_enqueue_model)
            .collect::<Result<Vec<_>, _>>()?;
        let mut names = BTreeSet::new();
        if normalized
            .iter()
            .any(|definition| !names.insert(definition.name.clone()))
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if normalized.is_empty() {
            return Ok(());
        }
        let mut state = self.lock()?;
        if normalized.len() > self.limits.max_enqueue_models {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut merged = BTreeMap::new();
        for definition in &normalized {
            merged.insert(definition.name.clone(), definition.clone());
        }
        validate_models(merged.values())?;
        if !state.enqueue_models.is_empty() {
            return if state.enqueue_models == merged {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        require_empty_authority(self.store.as_ref())?;
        let mut writes = normalized
            .iter()
            .map(enqueue_model_write)
            .collect::<Result<Vec<_>, _>>()?;
        writes.push(enqueue_model_catalog_write(&merged)?);
        self.store
            .put_provider_states_atomic(writes)
            .map_err(|error| match error {
                StoreError::AlreadyExists | StoreError::Conflict => {
                    HostProblem::IdempotencyConflict
                }
                other => store_error(other),
            })?;
        state.enqueue_models = merged;
        Ok(())
    }
}

pub(crate) fn load_enqueue_models(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<BTreeMap<String, CicsEnqueueModelDefinition>, HostProblem> {
    let maximum = limits
        .max_enqueue_models
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = store
        .list_provider_state(MODEL_NAMESPACE, maximum)
        .map_err(store_error)?;
    if rows.len() > limits.max_enqueue_models {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut models = BTreeMap::new();
    for row in rows {
        if row.version != 1 {
            return Err(HostProblem::InfrastructureFailure);
        }
        let model = decode_model(&row.payload)?;
        if row.key != model.name || models.insert(row.key, model).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    validate_models(models.values())?;
    let catalog = store
        .get_provider_state(MODEL_CATALOG_NAMESPACE, MODEL_CATALOG_KEY)
        .map_err(store_error)?;
    match (models.is_empty(), catalog) {
        (true, None) => {}
        (false, Some(record))
            if record.version == 1 && record.payload == encode_model_catalog(&models)? => {}
        _ => return Err(HostProblem::InfrastructureFailure),
    }
    Ok(models)
}

pub(super) fn validate_active_catalog(
    store: &dyn ProviderStateStore,
    models: &BTreeMap<String, CicsEnqueueModelDefinition>,
) -> Result<(), HostProblem> {
    let catalog = store
        .get_provider_state(MODEL_CATALOG_NAMESPACE, MODEL_CATALOG_KEY)
        .map_err(store_error)?;
    match (models.is_empty(), catalog) {
        (true, None) => Ok(()),
        (false, Some(record))
            if record.version == 1 && record.payload == encode_model_catalog(models)? =>
        {
            Ok(())
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

pub(crate) fn normalize_enqueue_model(
    definition: &CicsEnqueueModelDefinition,
) -> Result<CicsEnqueueModelDefinition, HostProblem> {
    let name = definition.name.trim().to_ascii_uppercase();
    let enqueue_name = definition.enqueue_name.clone();
    let enqueue_scope = match definition.enqueue_scope.as_deref() {
        None => None,
        Some(value) if value.chars().all(char::is_whitespace) => None,
        Some(value) => Some(value.to_ascii_uppercase()),
    };
    if name.is_empty()
        || name.chars().count() > 8
        || !name
            .chars()
            .all(|character| model_character(character, false))
        || enqueue_name.is_empty()
        || enqueue_name.chars().count() > 255
        || !enqueue_name
            .chars()
            .all(|character| model_character(character, true))
        || enqueue_scope.as_ref().is_some_and(|scope| {
            scope.chars().count() != 4
                || !scope
                    .chars()
                    .all(|character| model_character(character, false))
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(CicsEnqueueModelDefinition {
        name,
        enqueue_name,
        enqueue_scope,
        enabled: definition.enabled,
    })
}

pub(crate) fn validate_models<'a>(
    models: impl IntoIterator<Item = &'a CicsEnqueueModelDefinition>,
) -> Result<(), HostProblem> {
    let models = models.into_iter().collect::<Vec<_>>();
    for (index, model) in models.iter().enumerate() {
        if models[index + 1..]
            .iter()
            .any(|other| patterns_overlap(&model.enqueue_name, &other.enqueue_name))
        {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

pub(crate) fn enqueue_model_write(
    definition: &CicsEnqueueModelDefinition,
) -> Result<ProviderStateWrite, HostProblem> {
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: MODEL_NAMESPACE.into(),
            key: definition.name.clone(),
            version: 1,
            payload: encode_model(definition)?,
        },
        expected_version: None,
    })
}

pub(crate) fn enqueue_model_catalog_write(
    models: &BTreeMap<String, CicsEnqueueModelDefinition>,
) -> Result<ProviderStateWrite, HostProblem> {
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: MODEL_CATALOG_NAMESPACE.into(),
            key: MODEL_CATALOG_KEY.into(),
            version: 1,
            payload: encode_model_catalog(models)?,
        },
        expected_version: None,
    })
}

pub(crate) fn require_empty_authority(store: &dyn ProviderStateStore) -> Result<(), HostProblem> {
    if !store
        .list_provider_state(LOCK_NAMESPACE, 1)
        .map_err(store_error)?
        .is_empty()
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    match store
        .get_provider_state(CATALOG_NAMESPACE, CATALOG_KEY)
        .map_err(store_error)?
    {
        None => Ok(()),
        Some(record) if decode_catalog(&record.payload)? == 0 => Ok(()),
        Some(_) => Err(HostProblem::InfrastructureFailure),
    }
}

pub(super) fn model_character(character: char, allow_asterisk: bool) -> bool {
    character.is_ascii_alphanumeric()
        || "$@#./-_%&?!:|\"=¬,;<>&".contains(character)
        || (allow_asterisk && character == '*')
}

fn patterns_overlap(first: &str, second: &str) -> bool {
    match (first.strip_suffix('*'), second.strip_suffix('*')) {
        (Some(first), Some(second)) => first.starts_with(second) || second.starts_with(first),
        (Some(prefix), None) => second.starts_with(prefix),
        (None, Some(prefix)) => first.starts_with(prefix),
        (None, None) => first == second,
    }
}

pub(super) fn enqueue_model_matches(pattern: &str, resource: &[u8]) -> bool {
    let pattern = pattern.as_bytes();
    pattern
        .strip_suffix(b"*")
        .map_or(resource == pattern, |prefix| resource.starts_with(prefix))
}

fn encode_model(definition: &CicsEnqueueModelDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut out = MODEL_MAGIC.to_vec();
    field(&mut out, definition.name.as_bytes())?;
    field(&mut out, definition.enqueue_name.as_bytes())?;
    match &definition.enqueue_scope {
        Some(scope) => {
            out.push(1);
            field(&mut out, scope.as_bytes())?;
        }
        None => out.push(0),
    }
    out.push(u8::from(definition.enabled));
    Ok(out)
}

fn decode_model(payload: &[u8]) -> Result<CicsEnqueueModelDefinition, HostProblem> {
    let mut reader = CodecReader::new(payload, MODEL_MAGIC)?;
    let definition = CicsEnqueueModelDefinition {
        name: reader.text(32)?,
        enqueue_name: reader.text(1024)?,
        enqueue_scope: match reader.byte()? {
            0 => None,
            1 => Some(reader.text(16)?),
            _ => return Err(HostProblem::InfrastructureFailure),
        },
        enabled: match reader.byte()? {
            0 => false,
            1 => true,
            _ => return Err(HostProblem::InfrastructureFailure),
        },
    };
    reader.finish()?;
    if normalize_enqueue_model(&definition).ok().as_ref() != Some(&definition) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(definition)
}

fn encode_model_catalog(
    models: &BTreeMap<String, CicsEnqueueModelDefinition>,
) -> Result<Vec<u8>, HostProblem> {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.cics.enqueue-model-catalog@1\0");
    for model in models.values() {
        let encoded = encode_model(model)?;
        hash_framed(&mut hash, &encoded);
    }
    let mut payload = MODEL_CATALOG_MAGIC.to_vec();
    payload.extend_from_slice(
        &u32::try_from(models.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    payload.extend_from_slice(&hash.finalize());
    Ok(payload)
}
