//! Preparation through the unchanged retained metadata row codecs; no independent publication.
use super::*;
use crate::service::ImsCatalogProjection;
use mainframe_env_store_api::{MAX_PROVIDER_COMPOSITION_BYTES, MAX_PROVIDER_STATE_SCAN};

pub(crate) fn validate_projection(value: &ImsCatalogProjection) -> Result<(), HostProblem> {
    if normalize_application(&value.application)? != value.application {
        return Err(HostProblem::Malformed);
    }
    validate_generation_identity(value.generation, &value.package_identity)?;
    let actual = validate_ims_metadata(&value.catalog, ImsMetadataLimits::default())
        .map_err(|_| HostProblem::Malformed)?;
    if actual.digest != value.metadata_identity {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}
fn desired(value: &ImsCatalogProjection) -> SelectedGeneration {
    SelectedGeneration {
        schema_version: SELECTION_SCHEMA.into(),
        application: value.application.clone(),
        generation: value.generation,
        package_identity: value.package_identity.clone(),
        metadata_identity: Some(value.metadata_identity.clone()),
    }
}
fn generation_rows(
    service: &ImsService,
    application: &str,
) -> Result<Vec<ProviderStateRecord>, HostProblem> {
    let rows = service
        .store
        .list_provider_state_bounded(
            &generation_namespace(application),
            MAX_RETAINED_GENERATIONS + 1,
            MAX_PROVIDER_COMPOSITION_BYTES / 4,
        )
        .map_err(store_error)?;
    if rows.len() > MAX_RETAINED_GENERATIONS {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut last: Option<String> = None;
    for row in &rows {
        if decode_generation(row)?.application != application
            || last.as_ref().is_some_and(|key| key >= &row.key)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        last = Some(row.key.clone());
    }
    Ok(rows)
}
fn retained(
    service: &ImsService,
    selected: &SelectedGeneration,
) -> Result<Option<StoredGeneration>, HostProblem> {
    let Some(identity) = &selected.metadata_identity else {
        return Ok(None);
    };
    let row = generation_rows(service, &selected.application)?
        .into_iter()
        .find(|row| row.key == generation_key(selected.generation))
        .ok_or(HostProblem::InfrastructureFailure)?;
    let stored = decode_generation(&row)?;
    if stored.application != selected.application
        || stored.generation != selected.generation
        || stored.package_identity != selected.package_identity
        || &stored.metadata_identity != identity
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Some(stored))
}
pub(crate) fn matches_selection(
    service: &ImsService,
    expected: &ImsCatalogProjection,
) -> Result<(), HostProblem> {
    let rows = service
        .store
        .list_provider_state_bounded(
            SELECTION_NAMESPACE,
            MAX_PROVIDER_STATE_SCAN,
            MAX_PROVIDER_COMPOSITION_BYTES / 4,
        )
        .map_err(store_error)?;
    if rows.len() >= MAX_PROVIDER_STATE_SCAN {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut current = None;
    let mut last: Option<String> = None;
    for row in rows {
        if last.as_ref().is_some_and(|key| key >= &row.key) {
            return Err(HostProblem::InfrastructureFailure);
        }
        last = Some(row.key.clone());
        let selected = decode_selection(&row)?;
        let stored = retained(service, &selected)?;
        if selected.application != expected.application && stored.is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        if selected.application == expected.application {
            current = Some(row);
        }
    }
    let row = current.ok_or(HostProblem::NotFound)?;
    let selected = decode_selection(&row)?;
    if selected != desired(expected)
        || retained(service, &selected)?
            .as_ref()
            .map(|stored| &stored.catalog)
            != Some(&expected.catalog)
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}
pub(crate) fn prepare_selection(
    service: &ImsService,
    next: &ImsCatalogProjection,
    previous: Option<&ImsCatalogProjection>,
    rollback: bool,
) -> Result<Vec<ProviderStateWrite>, HostProblem> {
    validate_projection(next)?;
    let selected_rows = service
        .store
        .list_provider_state_bounded(
            SELECTION_NAMESPACE,
            MAX_PROVIDER_STATE_SCAN,
            MAX_PROVIDER_COMPOSITION_BYTES / 4,
        )
        .map_err(store_error)?;
    if selected_rows.len() >= MAX_PROVIDER_STATE_SCAN {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut current = None;
    let mut last: Option<String> = None;
    for row in selected_rows {
        if last.as_ref().is_some_and(|key| key >= &row.key) {
            return Err(HostProblem::InfrastructureFailure);
        }
        last = Some(row.key.clone());
        let selected = decode_selection(&row)?;
        let stored = retained(service, &selected)?;
        // The original runtime has one generic catalog. This finite profile owns
        // that catalog exclusively, rather than guessing independent subcatalog merging.
        if selected.application != next.application && stored.is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        if selected.application == next.application {
            if previous.map(desired).as_ref() != Some(&selected)
                || stored.as_ref().map(|value| &value.catalog)
                    != previous.map(|value| &value.catalog)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            current = Some((row, selected));
        }
    }
    if previous.is_some() && current.is_none() {
        return Err(HostProblem::IdempotencyConflict);
    }
    let namespace = generation_namespace(&next.application);
    let key = generation_key(next.generation);
    let retained_rows = generation_rows(service, &next.application)?;
    let target = retained_rows.iter().find(|row| row.key == key);
    let expected = StoredGeneration {
        schema_version: GENERATION_SCHEMA.into(),
        application: next.application.clone(),
        generation: next.generation,
        package_identity: next.package_identity.clone(),
        metadata_identity: next.metadata_identity.clone(),
        catalog: next.catalog.clone(),
    };
    let mut writes = Vec::new();
    if let Some(row) = target {
        if decode_generation(row)? != expected {
            return Err(HostProblem::IdempotencyConflict);
        }
    } else {
        if rollback {
            return Err(HostProblem::NotFound);
        }
        if retained_rows.len() >= MAX_RETAINED_GENERATIONS {
            return Err(HostProblem::ResourceExhausted);
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace,
                key,
                version: 1,
                payload: encode(&expected)?,
            },
            expected_version: None,
        });
    }
    let selected = desired(next);
    if current
        .as_ref()
        .is_some_and(|(_, value)| value == &selected)
    {
        if !writes.is_empty() {
            return Err(HostProblem::InfrastructureFailure);
        }
        return Ok(writes);
    }
    let version = current
        .as_ref()
        .map_or(0, |(row, _)| row.version)
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    writes.push(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: SELECTION_NAMESPACE.into(),
            key: next.application.clone(),
            version,
            payload: encode(&selected)?,
        },
        expected_version: current.map(|(row, _)| row.version),
    });
    Ok(writes)
}
