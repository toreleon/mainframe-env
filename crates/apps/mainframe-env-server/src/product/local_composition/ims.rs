//! Exact existing full IMS metadata payload bound by a genuine signed generic Resource.
use super::*;
use mainframe_env_ims::{
    ImsCatalogProjection, ImsMetadataCatalog, ImsMetadataLimits, validate_ims_metadata,
};

pub(super) fn bind_catalog(
    package: &ApplicationPackageV2,
    used: &mut BTreeSet<String>,
) -> Result<(Option<ImsMetadataCatalog>, usize), HostProblem> {
    let Some(catalog) = &package.sections.ims_metadata else {
        return Ok((None, 0));
    };
    validate_ims_metadata(catalog, ImsMetadataLimits::default())
        .map_err(|_| HostProblem::Malformed)?;
    let raw = serde_json::to_vec(catalog).map_err(|_| HostProblem::InfrastructureFailure)?;
    if raw.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
        return Err(HostProblem::ResourceExhausted);
    }
    bind_entry(
        package,
        EntryKind::Resource,
        "resource/ims-metadata",
        &raw,
        used,
    )?;
    Ok((Some(catalog.clone()), raw.len()))
}
pub(super) fn valid_projection(value: &ImsCatalogProjection, selected: &LocalSelection) -> bool {
    value.application == selected.package
        && value.generation == selected.generation
        && value.package_identity == selected.identity
        && value.content_payload().is_ok()
}
