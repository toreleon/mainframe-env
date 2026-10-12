//! Existing signed catalog codec and complete typed SQL sections; no implicit SQL or vendor schema.
use super::*;
use mainframe_env_db2::{Db2CatalogGeneration, Db2CatalogProjection};

pub(super) fn bind_catalog(
    server: &ProductServer,
    selected: &SelectedApplicationGeneration,
    used: &mut BTreeSet<String>,
) -> Result<(Option<Db2CatalogGeneration>, usize), HostProblem> {
    let package = selected.package();
    let Some(entry) = package
        .base
        .manifest
        .entries
        .iter()
        .find(|entry| entry.kind == EntryKind::Data && entry.path == "data/db2/catalog")
    else {
        return if package.sections.sql_tables.is_empty() && package.sections.sql_rows.is_empty() {
            Ok((None, 0))
        } else {
            Err(HostProblem::Unsupported)
        };
    };
    let raw = package
        .base
        .blobs
        .get(&entry.sha256)
        .ok_or(HostProblem::NotFound)?;
    bind_entry(package, EntryKind::Data, &entry.path, raw, used)?;
    let bytes = package
        .sections
        .sql_rows
        .iter()
        .try_fold(raw.len(), |sum, row| {
            let sum = sum
                .checked_add(row.table.len())
                .ok_or(HostProblem::ResourceExhausted)?;
            row.values.iter().try_fold(sum, |sum, (key, value)| {
                sum.checked_add(key.len())
                    .and_then(|sum| sum.checked_add(value.len()))
                    .ok_or(HostProblem::ResourceExhausted)
            })
        })?;
    if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
        return Err(HostProblem::ResourceExhausted);
    }
    let catalog = server
        .decode_application_db2_catalog(selected)?
        .ok_or(HostProblem::Malformed)?;
    catalog.validate(mainframe_env_db2::Db2Limits::default())?;
    Ok((Some(catalog), bytes))
}

pub(super) fn valid_projection(value: &Db2CatalogProjection, selected: &LocalSelection) -> bool {
    value.application == selected.package
        && value.generation == selected.generation
        && value.identity == selected.identity
        && value.content_payload().is_ok()
}
