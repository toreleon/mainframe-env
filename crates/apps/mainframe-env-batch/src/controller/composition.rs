//! Existing registry projections, with the original install/retained-selection validator.
use super::*;

impl BatchControllerRegistry {
    pub(crate) fn composition_selected(
        &self,
        application: &str,
    ) -> Result<Option<BatchControllerGeneration>, HostProblem> {
        let Some(installed) = self.applications.get(application) else {
            return Ok(None);
        };
        let selected = installed
            .selected
            .ok_or(HostProblem::InfrastructureFailure)?;
        installed
            .generations
            .get(&selected)
            .map(|retained| Some(retained.generation.clone()))
            .ok_or(HostProblem::InfrastructureFailure)
    }
    pub(crate) fn composition_retained(
        &self,
        application: &str,
        generation: u64,
    ) -> Option<BatchControllerGeneration> {
        self.applications
            .get(application)
            .and_then(|installed| installed.generations.get(&generation))
            .map(|retained| retained.generation.clone())
    }
}
