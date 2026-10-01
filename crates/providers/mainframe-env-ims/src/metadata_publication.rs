use crate::ImsService;
use mainframe_env_host_api::{
    HostProblem, ImsMetadataCatalog, ImsMetadataLimits, validate_ims_metadata,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateWrite, StoreError};
use serde::{Deserialize, Serialize};

const GENERATION_SCHEMA: &str = "mainframe-env.ims-metadata-generation@1";
const SELECTION_SCHEMA: &str = "mainframe-env.ims-metadata-selection@1";
const GENERATION_NAMESPACE_PREFIX: &str = "ims-v1-metadata-generation:";
const SELECTION_NAMESPACE: &str = "ims-v1-metadata-selection";
const MAX_RETAINED_GENERATIONS: usize = 64;
const MAX_METADATA_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsMetadataGeneration {
    pub application: String,
    pub generation: u64,
    pub package_identity: String,
    pub metadata_identity: String,
    pub catalog: ImsMetadataCatalog,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImsMetadataPublicationReceipt {
    pub application: String,
    pub generation: u64,
    pub package_identity: String,
    pub metadata_identity: Option<String>,
    pub replayed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredGeneration {
    schema_version: String,
    application: String,
    generation: u64,
    package_identity: String,
    metadata_identity: String,
    catalog: ImsMetadataCatalog,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SelectedGeneration {
    schema_version: String,
    application: String,
    generation: u64,
    package_identity: String,
    metadata_identity: Option<String>,
}

impl ImsService {
    /// Return the exact selected row to fence a database utility publication.
    pub(crate) fn selected_metadata_selection_fence(
        &self,
        selected: &ImsMetadataGeneration,
    ) -> Result<ProviderStateRecord, HostProblem> {
        let row = self
            .store
            .get_provider_state(SELECTION_NAMESPACE, &selected.application)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let current = decode_selection(&row)?;
        if current.application != selected.application
            || current.generation != selected.generation
            || current.package_identity != selected.package_identity
            || current.metadata_identity.as_deref() != Some(selected.metadata_identity.as_str())
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(row)
    }

    pub fn publish_metadata_generation(
        &self,
        application: &str,
        generation: u64,
        package_identity: &str,
        catalog: Option<&ImsMetadataCatalog>,
    ) -> Result<ImsMetadataPublicationReceipt, HostProblem> {
        let application = normalize_application(application)?;
        validate_generation_identity(generation, package_identity)?;
        let validated = catalog
            .map(|catalog| {
                validate_ims_metadata(catalog, ImsMetadataLimits::default())
                    .map(|identity| (catalog.clone(), identity.digest))
                    .map_err(|_| HostProblem::Malformed)
            })
            .transpose()?;
        let metadata_identity = validated.as_ref().map(|(_, identity)| identity.clone());
        let desired = SelectedGeneration {
            schema_version: SELECTION_SCHEMA.into(),
            application: application.clone(),
            generation,
            package_identity: package_identity.into(),
            metadata_identity: metadata_identity.clone(),
        };
        let selection = self
            .store
            .get_provider_state(SELECTION_NAMESPACE, &application)
            .map_err(store_error)?;
        let selection_replayed = selection
            .as_ref()
            .map(decode_selection)
            .transpose()?
            .as_ref()
            == Some(&desired);

        let generation_namespace = generation_namespace(&application);
        let generation_key = generation_key(generation);
        let existing_generation = self
            .store
            .get_provider_state(&generation_namespace, &generation_key)
            .map_err(store_error)?;
        let generation_write = if let Some((catalog, identity)) = validated {
            let stored = StoredGeneration {
                schema_version: GENERATION_SCHEMA.into(),
                application: application.clone(),
                generation,
                package_identity: package_identity.into(),
                metadata_identity: identity,
                catalog,
            };
            match existing_generation {
                Some(record) if decode_generation(&record)? == stored => None,
                Some(_) => return Err(HostProblem::IdempotencyConflict),
                None => {
                    if self
                        .store
                        .list_provider_state(&generation_namespace, MAX_RETAINED_GENERATIONS + 1)
                        .map_err(store_error)?
                        .len()
                        >= MAX_RETAINED_GENERATIONS
                    {
                        return Err(HostProblem::ResourceExhausted);
                    }
                    Some(ProviderStateWrite {
                        record: ProviderStateRecord {
                            namespace: generation_namespace,
                            key: generation_key,
                            version: 1,
                            payload: encode(&stored)?,
                        },
                        expected_version: None,
                    })
                }
            }
        } else {
            if existing_generation.is_some() {
                return Err(HostProblem::IdempotencyConflict);
            }
            None
        };

        if selection_replayed && generation_write.is_none() {
            return Ok(publication_receipt(desired, true));
        }

        let selection_version = selection.as_ref().map_or(Ok(1), |record| {
            record
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)
        })?;
        let mut writes = Vec::with_capacity(2);
        if let Some(write) = generation_write {
            writes.push(write);
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: SELECTION_NAMESPACE.into(),
                key: application,
                version: selection_version,
                payload: encode(&desired)?,
            },
            expected_version: selection.as_ref().map(|record| record.version),
        });
        self.store
            .put_provider_states_atomic(writes)
            .map_err(store_error)?;
        Ok(publication_receipt(desired, false))
    }

    pub fn selected_metadata_generation(
        &self,
        application: &str,
    ) -> Result<Option<ImsMetadataGeneration>, HostProblem> {
        let application = normalize_application(application)?;
        let Some(selection) = self
            .store
            .get_provider_state(SELECTION_NAMESPACE, &application)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        let selection = decode_selection(&selection)?;
        if selection.application != application {
            return Err(HostProblem::InfrastructureFailure);
        }
        let Some(metadata_identity) = selection.metadata_identity else {
            return Ok(None);
        };
        let record = self
            .store
            .get_provider_state(
                &generation_namespace(&application),
                &generation_key(selection.generation),
            )
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let stored = decode_generation(&record)?;
        if stored.application != application
            || stored.generation != selection.generation
            || stored.package_identity != selection.package_identity
            || stored.metadata_identity != metadata_identity
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(Some(ImsMetadataGeneration {
            application: stored.application,
            generation: stored.generation,
            package_identity: stored.package_identity,
            metadata_identity: stored.metadata_identity,
            catalog: stored.catalog,
        }))
    }
}

fn publication_receipt(
    selected: SelectedGeneration,
    replayed: bool,
) -> ImsMetadataPublicationReceipt {
    ImsMetadataPublicationReceipt {
        application: selected.application,
        generation: selected.generation,
        package_identity: selected.package_identity,
        metadata_identity: selected.metadata_identity,
        replayed,
    }
}

fn decode_generation(record: &ProviderStateRecord) -> Result<StoredGeneration, HostProblem> {
    if record.version == 0 || record.payload.len() > MAX_METADATA_BYTES {
        return Err(HostProblem::InfrastructureFailure);
    }
    let stored: StoredGeneration =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    let metadata_identity = validate_ims_metadata(&stored.catalog, ImsMetadataLimits::default())
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .digest;
    if stored.schema_version != GENERATION_SCHEMA
        || generation_key(stored.generation) != record.key
        || generation_namespace(&stored.application) != record.namespace
        || normalize_application(&stored.application).as_deref() != Ok(stored.application.as_str())
        || validate_generation_identity(stored.generation, &stored.package_identity).is_err()
        || metadata_identity != stored.metadata_identity
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(stored)
}

fn decode_selection(record: &ProviderStateRecord) -> Result<SelectedGeneration, HostProblem> {
    if record.namespace != SELECTION_NAMESPACE
        || record.version == 0
        || record.payload.len() > MAX_METADATA_BYTES
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let selected: SelectedGeneration =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if selected.schema_version != SELECTION_SCHEMA
        || selected.application != record.key
        || normalize_application(&selected.application).as_deref()
            != Ok(selected.application.as_str())
        || validate_generation_identity(selected.generation, &selected.package_identity).is_err()
        || selected
            .metadata_identity
            .as_ref()
            .is_some_and(|identity| !valid_sha256(identity))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(selected)
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, HostProblem> {
    let payload = serde_json::to_vec(value).map_err(|_| HostProblem::ProviderFailure)?;
    if payload.len() > MAX_METADATA_BYTES {
        Err(HostProblem::ResourceExhausted)
    } else {
        Ok(payload)
    }
}

fn validate_generation_identity(generation: u64, identity: &str) -> Result<(), HostProblem> {
    if generation == 0 || !valid_sha256(identity) {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn valid_sha256(identity: &str) -> bool {
    identity.len() == 71
        && identity.starts_with("sha256:")
        && identity[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn normalize_application(application: &str) -> Result<String, HostProblem> {
    let application = application.to_ascii_uppercase();
    if application.is_empty()
        || application.len() > 128
        || application
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b'_' | b'.'))
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(application)
    }
}

fn generation_namespace(application: &str) -> String {
    format!("{GENERATION_NAMESPACE_PREFIX}{application}")
}

fn generation_key(generation: u64) -> String {
    format!("{generation:020}")
}

fn store_error(error: StoreError) -> HostProblem {
    match error {
        StoreError::AlreadyExists | StoreError::Conflict => HostProblem::IdempotencyConflict,
        StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::InfrastructureFailure,
    }
}
