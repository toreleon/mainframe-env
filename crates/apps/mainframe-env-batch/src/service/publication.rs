//! Same-process publication exclusion; durable publication remains the sole authority.
use super::*;
use mainframe_env_application::{
    APPLICATION_PUBLICATION_NAMESPACE, ApplicationPublicationState, PublicationSectionState,
};
use std::sync::{RwLockReadGuard, RwLockWriteGuard, TryLockError};

/// Borrowed operations under this service's actual publication write guard.
/// There is no constructor, clone, serialization, or caller-issued admission permit.
/// ```compile_fail
/// use mainframe_env_batch::BatchPublicationWrite;
/// let writer = BatchPublicationWrite {};
/// ```
pub struct BatchPublicationWrite<'a> {
    service: &'a BatchService,
    _guard: RwLockWriteGuard<'a, ()>,
}

impl BatchPublicationWrite<'_> {
    pub fn install_controllers(
        &self,
        generation: BatchControllerGeneration,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        self.service.install_controllers_locked(generation)
    }
    pub fn rollback_controllers(
        &self,
        application: &str,
        generation: u64,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        self.service
            .rollback_controllers_locked(application, generation)
    }
}

impl BatchService {
    /// One bounded acquisition. Callback reentry refuses instead of blocking on itself.
    pub fn with_publication_write<T>(
        &self,
        operation: impl FnOnce(&BatchPublicationWrite<'_>) -> Result<T, HostProblem>,
    ) -> Result<T, HostProblem> {
        let guard = self
            .publication_exclusion
            .try_write()
            .map_err(publication_lock_problem)?;
        operation(&BatchPublicationWrite {
            service: self,
            _guard: guard,
        })
    }

    /// Validate the entire prospective registry without selecting or persisting it.
    pub fn validate_controller_install(
        &self,
        generation: &BatchControllerGeneration,
    ) -> Result<(), HostProblem> {
        let durable = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        durable.registry.preflight_install(generation)?;
        let mut prospective = durable.registry.clone();
        prospective.install(generation.clone())?;
        Ok(())
    }

    /// Rollback uses retained-selection rules, including identity, rather than forward install.
    pub fn validate_controller_rollback(
        &self,
        application: &str,
        generation: u64,
        identity: &str,
    ) -> Result<(), HostProblem> {
        let durable = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut prospective = durable.registry.clone();
        let receipt = prospective.select(application, generation)?;
        if receipt.identity != identity {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    pub(super) fn admit_controller(
        &self,
        selector: &BatchControllerSelector,
    ) -> Result<(RwLockReadGuard<'_, ()>, Option<ResolvedBatchController>), HostProblem> {
        let guard = self
            .publication_exclusion
            .try_read()
            .map_err(publication_lock_problem)?;
        let controller = self.resolve_controller(selector)?;
        if let Some(controller) = &controller {
            let row = self
                .store
                .get_provider_state(APPLICATION_PUBLICATION_NAMESPACE, &controller.application)
                .map_err(store_error)?
                .ok_or(HostProblem::NotFound)?;
            if row.namespace != APPLICATION_PUBLICATION_NAMESPACE
                || row.key != controller.application
                || row.version == 0
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let state = ApplicationPublicationState::from_payload(&row.payload)?;
            if !state.matches_complete(
                &controller.application,
                controller.generation,
                &controller.identity,
            ) || state.controllers != PublicationSectionState::Applied
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        Ok((guard, controller))
    }
}
fn publication_lock_problem<T>(error: TryLockError<T>) -> HostProblem {
    match error {
        TryLockError::WouldBlock => HostProblem::IdempotencyConflict,
        TryLockError::Poisoned(_) => HostProblem::InfrastructureFailure,
    }
}

impl BatchService {
    pub fn install_controllers(
        &self,
        generation: BatchControllerGeneration,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        self.with_publication_write(|writer| writer.install_controllers(generation))
    }

    fn install_controllers_locked(
        &self,
        generation: BatchControllerGeneration,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let mut durable = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        durable.registry.preflight_install(&generation)?;
        let mut replacement = durable.registry.clone();
        let receipt = replacement.install(generation)?;
        self.persist_controllers(&mut durable, replacement)?;
        Ok(receipt)
    }

    pub fn rollback_controllers(
        &self,
        application: &str,
        generation: u64,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        self.with_publication_write(|writer| writer.rollback_controllers(application, generation))
    }

    fn rollback_controllers_locked(
        &self,
        application: &str,
        generation: u64,
    ) -> Result<BatchControllerInstallReceipt, HostProblem> {
        let mut durable = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut replacement = durable.registry.clone();
        let receipt = replacement.select(application, generation)?;
        self.persist_controllers(&mut durable, replacement)?;
        Ok(receipt)
    }

    pub(super) fn resolve_controller(
        &self,
        selector: &BatchControllerSelector,
    ) -> Result<Option<ResolvedBatchController>, HostProblem> {
        self.controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)
            .map(|durable| durable.registry.resolve(selector))
    }

    pub(super) fn verify_controller_program(
        &self,
        program: &crate::BatchControllerProgram,
    ) -> Result<String, HostProblem> {
        let name = program
            .path
            .rsplit('/')
            .next()
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_ascii_uppercase();
        let record = self
            .store
            .get_provider_state("batch-program", &name)
            .map_err(store_error)?
            .ok_or(HostProblem::NotFound)?;
        if record.payload != program.identity.as_bytes() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(name)
    }
}
