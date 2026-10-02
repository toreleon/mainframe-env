//! GSAM adaptation through the existing RecoverySession resolver and atomic
//! Session/database/UOW/recovery bridge, without a parallel recovery authority.
use super::*;
use crate::database::{DatabaseEngine, EngineProblem};
use crate::recovery::{SavedGsamPosition, SavedPcbPosition};
use mainframe_env_host_api::{ImsDatabaseOrganization, ImsDatabasePcbMetadata};

pub(super) fn validate_pcb(
    state: &State,
    pcb: &ImsDatabasePcbMetadata,
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    let engine = generic::restored(state, &pcb.database, limits)?;
    if engine.definition().segments.len() != 1
        || !matches!(pcb.processing_options.as_str(), "G" | "GS" | "L" | "LS")
    {
        return Err(HostProblem::Unsupported);
    }
    crate::database::gsam_format::validate_definition_route(engine.definition())?;
    Ok(())
}

pub(super) fn is_gsam(state: &State, database: &str) -> bool {
    state.metadata.as_ref().is_some_and(|c| {
        c.databases
            .iter()
            .any(|db| db.name == database && db.organization == ImsDatabaseOrganization::Gsam)
    })
}

pub(in crate::service) fn reject_basic(state: &State, psb: &str) -> Result<(), HostProblem> {
    if state
        .metadata
        .as_ref()
        .and_then(|c| c.psbs.iter().find(|p| p.name == psb))
        .is_some_and(|p| {
            p.pcbs.iter().any(
                |pcb| matches!(pcb, ImsPcbMetadata::Database(pcb) if is_gsam(state, &pcb.database)),
            )
        })
    {
        return Err(HostProblem::Unsupported);
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
    validate_pcb(state, pcb, limits)?;
    let position = generic::pcb::position(
        state.sessions.get(run).ok_or(HostProblem::NotFound)?,
        number,
    );
    let mut engine = generic::restored(state, &pcb.database, limits)?;
    let version = versions
        .get(&(GENERIC_DATABASE_NAMESPACE.into(), pcb.database.clone()))
        .ok_or(HostProblem::InfrastructureFailure)?;
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.ims-gsam-checkpoint-issuance@1\0");
    hash.update(digest);
    hash.update(version.to_le_bytes());
    let (saved, changed) = engine
        .save_gsam_position(
            &position,
            pcb.processing_options.starts_with('L'),
            hash.finalize().into(),
        )
        .map_err(engine_error)?;
    if changed {
        generic::isolation::ensure_writer(state, run, &pcb.database)?;
        generic::isolation::publish_image(state, run, &pcb.database, engine.image(), limits)?;
    }
    Ok(SavedPcbPosition {
        gsam_format: crate::database::gsam_format::identity(engine.definition())?,
        pcb: number.to_string(),
        database: pcb.database.clone(),
        segment_key: vec![],
        gsam: Some(saved),
    })
}

/// Unsettled output from an abended execution may be truncated by XRST only
/// under the existing witnessed UOW fence. Other unsettled work stays rejected.
pub(super) fn prepare_restart(
    state: &State,
    run: &str,
    positions: &[SavedPcbPosition],
    limits: ImsLimits,
) -> Result<(), HostProblem> {
    if state.pending_undo.contains_key(run) {
        return Err(HostProblem::IdempotencyConflict);
    }
    if let Some(undo) = state.generic_pending_undo.get(run) {
        for (name, prior) in undo.iter() {
            if !positions.iter().any(|p| {
                p.database == *name && matches!(p.gsam, Some(SavedGsamPosition::Output { .. }))
            }) && !(positions
                .iter()
                .any(|p| p.database == *name && p.gsam.is_some())
                && generic::restored(state, name, limits)?.gsam_identity_only_since(prior))
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
    }
    generic::isolation::ensure_backout(state, run)?;
    for saved in positions {
        if matches!(saved.gsam, Some(SavedGsamPosition::Output { .. })) {
            generic::isolation::ensure_writer(state, run, &saved.database)?;
        }
    }
    Ok(())
}

pub(super) fn restore(
    state: &mut State,
    run: &str,
    number: u16,
    pcb: &ImsDatabasePcbMetadata,
    saved: &SavedPcbPosition,
    limits: ImsLimits,
) -> Result<(u16, PcbPosition, Option<String>), HostProblem> {
    validate_pcb(state, pcb, limits)?;
    let saved_position = saved.gsam.as_ref().ok_or(HostProblem::ProviderFailure)?;
    if pcb.processing_options.starts_with('L')
        != matches!(saved_position, SavedGsamPosition::Output { .. })
    {
        return Err(HostProblem::ProviderFailure);
    }
    let mut engine: DatabaseEngine = generic::restored(state, &saved.database, limits)?;
    if saved.gsam_format != crate::database::gsam_format::identity(engine.definition())? {
        return Err(HostProblem::ProviderFailure);
    }
    if let SavedGsamPosition::Output { records, .. } = saved_position
        && engine.record_count() > *records
        && !state
            .generic_pending_undo
            .get(run)
            .is_some_and(|undo| undo.contains_key(&saved.database))
    {
        // A live suffix with no ownership witness might belong to a different
        // committed writer. A row-version change alone is never this test.
        return Err(HostProblem::UnknownOutcome);
    }
    let (position, changed) = engine
        .restore_gsam_position(saved_position)
        .map_err(engine_error)?;
    if changed {
        state
            .generic_databases
            .insert(saved.database.clone(), Arc::new(engine.image()));
        // Image replacement clears other live PCB references in the same proposal.
        generic::reset_positions(state, &saved.database, Some(run));
    }
    Ok((number, position, Some("  ".into())))
}

fn engine_error(problem: EngineProblem) -> HostProblem {
    match problem {
        EngineProblem::Unsupported => HostProblem::Unsupported,
        EngineProblem::LimitExceeded => HostProblem::ResourceExhausted,
        _ => HostProblem::ProviderFailure,
    }
}
