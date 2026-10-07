use super::{CicsProgramDefinition, CicsService, normalize_program_name};
use mainframe_env_host_api::HostProblem;

/// Private catalog observation, without command admission or response semantics.
// Prepared for a later manager-owned binding; no runtime route consumes it yet.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) enum ProgramStatusObservation {
    Definition(CicsProgramDefinition),
    NameOnly,
    /// Absent from this catalog only; other resource namespaces remain unresolved.
    NotCatalogued,
}

/// Copy the current immutable definition under the existing CICS State mutex.
///
/// This observes definition availability, not loaded-module information. Inquiry's
/// no-load boundary is pinned by INQUIRE PROGRAM (dfha8_inquireprogram.html,
/// parser lines 35–56, 566–572). Namespace visibility and command admission are
/// separate future bindings. Name normalization is the existing project policy.
#[allow(dead_code)]
pub(in crate::service) fn observe_named_status(
    service: &CicsService,
    program: &str,
) -> Result<ProgramStatusObservation, HostProblem> {
    let name = normalize_program_name(program)?;
    let state = service.lock()?;
    if let Some(generations) = state.program_definitions.get(&name) {
        let (_, latest) = generations
            .last_key_value()
            .ok_or(HostProblem::InfrastructureFailure)?;
        if generations.iter().any(|(generation, definition)| {
            *generation == 0 || definition.generation != *generation || definition.name != name
        }) {
            return Err(HostProblem::InfrastructureFailure);
        }
        Ok(ProgramStatusObservation::Definition(latest.clone()))
    } else if state.programs.contains(&name) {
        Ok(ProgramStatusObservation::NameOnly)
    } else {
        Ok(ProgramStatusObservation::NotCatalogued)
    }
}

#[cfg(test)]
mod tests;

mod security;
