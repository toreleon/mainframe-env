//! Selected PCB binding through the existing checkpoint proposal and resolver.
use super::*;
use crate::recovery::SavedPcbPosition;
use mainframe_env_host_api::ImsDatabasePcbMetadata;

fn metadata_identity(
    state: &State,
    psb: &str,
    pcb: &ImsDatabasePcbMetadata,
) -> Result<[u8; 32], HostProblem> {
    let catalog = state.metadata.as_ref().ok_or(HostProblem::NotFound)?;
    let database = catalog
        .databases
        .iter()
        .find(|d| normalize(&d.name) == normalize(&pcb.database))
        .ok_or(HostProblem::NotFound)?;
    let bytes = serde_json::to_vec(&(psb, pcb, database))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.ims-selected-secondary-checkpoint-metadata@1\0");
    hash.update(bytes);
    Ok(hash.finalize().into())
}

fn validate_paths(
    state: &State,
    pcb: &ImsDatabasePcbMetadata,
    paths: &[&[(String, Vec<u8>)]],
) -> Result<(), HostProblem> {
    let database = state
        .metadata
        .as_ref()
        .and_then(|c| {
            c.databases
                .iter()
                .find(|d| normalize(&d.name) == normalize(&pcb.database))
        })
        .ok_or(HostProblem::NotFound)?;
    for path in paths {
        for (name, _) in *path {
            if !database
                .segments
                .iter()
                .find(|s| normalize(&s.name) == *name)
                .is_some_and(|s| s.fields.iter().any(|f| f.sequence && f.unique))
            {
                return Err(HostProblem::Unsupported);
            }
        }
    }
    Ok(())
}

pub(super) fn save(
    state: &mut State,
    versions: &RowVersions,
    run: &str,
    number: u16,
    pcb: &ImsDatabasePcbMetadata,
    digest: [u8; 32],
    limits: ImsLimits,
) -> Result<SavedPcbPosition, HostProblem> {
    let database = normalize(&pcb.database);
    generic::isolation::ensure_writer(state, run, &database)?;
    let session = state.sessions.get(run).ok_or(HostProblem::NotFound)?;
    let position = generic::pcb::position(session, number);
    let metadata_digest = metadata_identity(state, &session.psb, pcb)?;
    let mut engine = generic::restored(state, &database, limits)?;
    let version = versions
        .get(&(GENERIC_DATABASE_NAMESPACE.into(), database.clone()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.ims-secondary-checkpoint-issuance@1\0");
    hash.update(digest);
    hash.update(version.to_le_bytes());
    hash.update((session.recovery.uow_incarnation.len() as u64).to_le_bytes());
    hash.update(session.recovery.uow_incarnation.as_bytes());
    hash.update(session.recovery.uow_epoch.to_le_bytes());
    let (secondary, changed) = engine
        .save_secondary_position(
            &normalize(
                pcb.secondary_index
                    .as_deref()
                    .ok_or(HostProblem::Malformed)?,
            ),
            &position,
            metadata_digest,
            hash.finalize().into(),
        )
        .map_err(engine_error)?;
    validate_paths(
        state,
        pcb,
        &[&secondary.source_path, &secondary.current_path],
    )?;
    if changed {
        generic::isolation::publish_image(state, run, &database, engine.image(), limits)?;
    }
    Ok(SavedPcbPosition {
        gsam_format: None,
        pcb: number.to_string(),
        database: pcb.database.clone(),
        segment_key: vec![],
        gsam: None,
        secondary: Some(secondary),
    })
}

pub(super) fn restore(
    state: &State,
    psb: &str,
    number: u16,
    pcb: &ImsDatabasePcbMetadata,
    saved: &SavedPcbPosition,
    limits: ImsLimits,
) -> Result<(u16, PcbPosition, Option<String>), HostProblem> {
    let secondary = saved.secondary.as_ref().ok_or(HostProblem::Unsupported)?;
    if pcb
        .secondary_index
        .as_deref()
        .is_none_or(|name| normalize(name) != secondary.index)
        || metadata_identity(state, psb, pcb)? != secondary.metadata_digest
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    validate_paths(
        state,
        pcb,
        &[&secondary.source_path, &secondary.current_path],
    )?;
    for (name, _) in &secondary.current_path {
        if !pcb
            .sensitive_segments
            .iter()
            .any(|s| normalize(&s.name) == *name)
            || (!generic::allowed(pcb, name, ImsOperation::GetUnique)
                && !generic::pcb::key_only(pcb, name))
        {
            return Err(HostProblem::Unsupported);
        }
    }
    let engine = generic::restored(state, &normalize(&saved.database), limits)?;
    let (position, found) = engine
        .restore_secondary_position(secondary)
        .map_err(engine_error)?;
    Ok((
        number,
        position,
        Some(if found { "  " } else { "GE" }.into()),
    ))
}

fn engine_error(problem: crate::database::EngineProblem) -> HostProblem {
    match problem {
        crate::database::EngineProblem::Unsupported => HostProblem::Unsupported,
        crate::database::EngineProblem::LimitExceeded => HostProblem::ResourceExhausted,
        _ => HostProblem::ProviderFailure,
    }
}
