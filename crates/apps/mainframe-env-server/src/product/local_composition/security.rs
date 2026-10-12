//! Signed finite Profile inputs and selected-owner joins; no RACF vendor syntax is inferred.
use super::*;
use mainframe_env_racf::{RacfProfileComposition, RacfProfileProjection};

pub(super) fn bind_profiles(
    package: &ApplicationPackageV2,
    declarations: &[RacfProfileComposition],
    used: &mut BTreeSet<String>,
) -> Result<usize, HostProblem> {
    if declarations.len() > 4096 || declarations.len() != package.sections.security_resources.len()
    {
        return Err(HostProblem::Malformed);
    }
    let mut keys = BTreeSet::new();
    let mut bytes = 0usize;
    for declaration in declarations {
        let path = declaration.entry_path()?;
        if !keys.insert((declaration.class.clone(), declaration.name.clone())) {
            return Err(HostProblem::IdempotencyConflict);
        }
        if !package.sections.security_resources.iter().any(|section| {
            section.class == declaration.class
                && section.profile == declaration.name
                && section.owner == declaration.owner
        }) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let payload = declaration.canonical_payload()?;
        bytes = bytes
            .checked_add(payload.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        bind_entry(package, EntryKind::Profile, &path, &payload, used)?;
    }
    if !declarations.is_empty()
        && package
            .base
            .manifest
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Profile && !used.contains(&entry.path))
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(bytes)
}

/// Only CURRENT selected packages own names. Retained unselected history is not an owner.
pub(super) fn foreign_profile_owners(
    server: &ProductServer,
    current: &str,
    installer: &ApplicationInstallerV2,
) -> Result<BTreeSet<String>, HostProblem> {
    let maximum = PackageLimits::default().max_applications;
    let limit = maximum
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let rows = server
        .store
        .list_provider_state_bounded(LOCAL_SELECTION, limit, MAX_PROVIDER_COMPOSITION_BYTES / 8)
        .map_err(store_error)?;
    if rows.len() > maximum {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut previous_key: Option<String> = None;
    let mut result = BTreeSet::new();
    for row in rows {
        if previous_key.as_ref().is_some_and(|key| key >= &row.key) {
            return Err(HostProblem::InfrastructureFailure);
        }
        previous_key = Some(row.key.clone());
        let selected = decode_local_selection(&row)?;
        let actual = installer
            .selected(&selected.package)
            .map_err(application_install_problem)?
            .ok_or(HostProblem::NotFound)?;
        if actual.state != InstallState::Ready
            || actual.identity != selected.identity
            || actual.generation != selected.generation
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let retained = server
            .store
            .get_provider_state(
                LOCAL_GENERATION,
                &format!("{}:{}", selected.package, selected.generation),
            )
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        let retained = decode_local_selection(&retained)?;
        if retained.binding != selected.binding || retained.identity != selected.identity {
            return Err(HostProblem::IdempotencyConflict);
        }
        if selected.package != current {
            for projection in selected.racf {
                if !result.insert(projection.key()) {
                    return Err(HostProblem::IdempotencyConflict);
                }
            }
        }
    }
    Ok(result)
}

pub(super) fn projection_keys(
    projections: &[RacfProfileProjection],
) -> Result<BTreeSet<String>, HostProblem> {
    if projections.len() > 4096 {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut keys = BTreeSet::new();
    for projection in projections {
        if projection.profile.version == 0 || !keys.insert(projection.key()) {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(keys)
}
