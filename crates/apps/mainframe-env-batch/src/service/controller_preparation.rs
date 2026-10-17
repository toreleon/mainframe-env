//! Finite genuine controller-registry participant; controller execution still requires Complete publication.
use super::*;
use mainframe_env_store_api::{
    MAX_PROVIDER_COMPOSITION_BYTES, ProviderCompositionCommit, ProviderStateWrite,
};
use std::sync::MutexGuard;

/// Full selected generation from the original registry codec, without JES execution authority.
#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct BatchControllerProjection {
    /// Actual selected application, generation, identity and every original controller field.
    pub generation: BatchControllerGeneration,
}
impl BatchControllerProjection {
    /// Validate and encode full canonical participating content using the unchanged registry validator.
    pub fn content_payload(&self) -> Result<Vec<u8>, HostProblem> {
        if self.generation.controllers.is_empty()
            || self.generation.controllers.iter().any(|item| {
                !matches!(
                    (&item.selector, &item.plan),
                    (
                        BatchControllerSelector::TsoRun { .. },
                        BatchControllerPlan::ProgramCall
                    )
                )
            })
        {
            return Err(HostProblem::Unsupported);
        }
        if self.generation.controllers.len() > crate::controller::MAX_CONTROLLERS_PER_GENERATION {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut registry = BatchControllerRegistry::default();
        registry.install(self.generation.clone())?;
        if registry
            .composition_selected(&self.generation.application)?
            .as_ref()
            != Some(&self.generation)
        {
            return Err(HostProblem::Malformed);
        }
        let raw = serde_json::to_vec(self).map_err(|_| HostProblem::InfrastructureFailure)?;
        if raw.len() > MAX_PROVIDER_COMPOSITION_BYTES / 8 {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(raw)
    }
}

/// Actual registry owner and full original-codec CAS row retained until same-store acknowledgement.
pub struct PreparedBatchControllers<'a> {
    service: &'a BatchService,
    durable: MutexGuard<'a, DurableControllers>,
    next: BatchControllerRegistry,
    next_version: u64,
    writes: Vec<ProviderStateWrite>,
    projection: BatchControllerProjection,
}
impl BatchService {
    /// Prepare a finite signed TSO ProgramCall generation without selecting live cache or writing storage.
    /// The owning server also holds its original publication exclusion; ordinary controller writes take that exclusion.
    pub fn prepare_controller_composition(
        &self,
        generation: BatchControllerGeneration,
        previous: Option<&BatchControllerProjection>,
        rollback: bool,
    ) -> Result<PreparedBatchControllers<'_>, HostProblem> {
        let offered = BatchControllerProjection { generation };
        offered.content_payload()?;
        let durable = self.controller_composition_lock()?;
        verify_actual_cache(self, &durable)?;
        let application = &offered.generation.application;
        let current = durable.registry.composition_selected(application)?;
        if let Some(prior) = previous {
            prior.content_payload()?;
            if prior.generation.application != *application
                || current.as_ref() != Some(&prior.generation)
                || prior
                    .generation
                    .controllers
                    .iter()
                    .map(|item| (&item.name, &item.selector, &item.program.path))
                    .collect::<Vec<_>>()
                    != offered
                        .generation
                        .controllers
                        .iter()
                        .map(|item| (&item.name, &item.selector, &item.program.path))
                        .collect::<Vec<_>>()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
        } else if current.is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = durable.registry.clone();
        if rollback {
            if previous.is_none() {
                return Err(HostProblem::NotFound);
            }
            if next
                .composition_retained(application, offered.generation.generation)
                .as_ref()
                != Some(&offered.generation)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            next.select(application, offered.generation.generation)?;
        } else {
            next.install(offered.generation.clone())?;
        }
        if next.composition_selected(application)?.as_ref() != Some(&offered.generation) {
            return Err(HostProblem::IdempotencyConflict);
        }
        let before = durable.registry.state_payload()?;
        let after = next.state_payload()?;
        if before
            .len()
            .checked_add(after.len())
            .is_none_or(|size| size > MAX_PROVIDER_COMPOSITION_BYTES / 2)
            || after.len() > MAX_PROVIDER_COMPOSITION_BYTES / 4
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        let mut next_version = durable.store_version;
        if before != after {
            next_version = next_version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: CONTROLLER_STATE_NAMESPACE.into(),
                    key: CONTROLLER_STATE_KEY.into(),
                    version: next_version,
                    payload: after,
                },
                expected_version: (durable.store_version != 0).then_some(durable.store_version),
            });
        }
        Ok(PreparedBatchControllers {
            service: self,
            durable,
            next,
            next_version,
            writes,
            projection: offered,
        })
    }

    /// Verify selected full static controller content under the actual owner; all JES runtime rows remain independent.
    pub fn selected_controller_composition_matches(
        &self,
        expected: &BatchControllerProjection,
    ) -> Result<(), HostProblem> {
        expected.content_payload()?;
        let durable = self.controller_composition_lock()?;
        verify_actual_cache(self, &durable)?;
        if durable
            .registry
            .composition_selected(&expected.generation.application)?
            .as_ref()
            != Some(&expected.generation)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }
    pub(super) fn controller_composition_lock(
        &self,
    ) -> Result<MutexGuard<'_, DurableControllers>, HostProblem> {
        if self
            .controller_composition_uncertain
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let owner = self
            .controllers
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if self
            .controller_composition_uncertain
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(owner)
    }
}
fn verify_actual_cache(
    service: &BatchService,
    durable: &DurableControllers,
) -> Result<(), HostProblem> {
    let canonical = durable.registry.state_payload()?;
    if canonical.len() > MAX_PROVIDER_COMPOSITION_BYTES / 4 {
        return Err(HostProblem::ResourceExhausted);
    }
    // The legacy registry is one global key. Refuse any unexpected key in the
    // new finite observation; this does not grant later external-writer exclusion.
    let mut actual = service
        .store
        .list_provider_state_bounded(
            CONTROLLER_STATE_NAMESPACE,
            2,
            MAX_PROVIDER_COMPOSITION_BYTES / 4,
        )
        .map_err(store_error)?;
    if actual.len() > 1
        || actual.iter().any(|row| {
            row.namespace != CONTROLLER_STATE_NAMESPACE || row.key != CONTROLLER_STATE_KEY
        })
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    match actual.pop() {
        None if durable.store_version == 0
            && durable.registry.state_payload()?
                == BatchControllerRegistry::default().state_payload()? =>
        {
            Ok(())
        }
        Some(row)
            if row.namespace == CONTROLLER_STATE_NAMESPACE
                && row.key == CONTROLLER_STATE_KEY
                && row.version != 0
                && row.version == durable.store_version
                && row.payload == canonical =>
        {
            Ok(())
        }
        _ => Err(HostProblem::IdempotencyConflict),
    }
}
impl PreparedBatchControllers<'_> {
    /// Exact full original registry row and CAS version; no job/scheduler/topology mutation is included.
    pub fn writes(&self) -> &[ProviderStateWrite] {
        &self.writes
    }
    /// Full selected generation, including genuine retained rollback content.
    pub fn projection(&self) -> &BatchControllerProjection {
        &self.projection
    }
    /// Require the actual same provider store and every prepared row in the real commit.
    pub fn accepts(&self, commit: &ProviderCompositionCommit) -> bool {
        commit.covers(&self.service.store, &self.writes)
    }
    /// Fence this live controller owner after unknown acknowledgement; authoritative reopen is required.
    pub fn fence_unknown(&self) {
        self.service
            .controller_composition_uncertain
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Publish actual cache only after a genuine same-store acknowledgement of the complete prepared row.
    pub fn finish(mut self, commit: &ProviderCompositionCommit) -> Result<(), HostProblem> {
        if !self.accepts(commit) {
            self.fence_unknown();
            return Err(HostProblem::UnknownOutcome);
        }
        self.durable.registry = self.next;
        self.durable.store_version = self.next_version;
        Ok(())
    }
}

// Original persistence method, relocated byte-exact; ordinary controller publication remains unchanged.
impl BatchService {
    pub(super) fn persist_controllers(
        &self,
        durable: &mut DurableControllers,
        registry: BatchControllerRegistry,
    ) -> Result<(), HostProblem> {
        let payload = registry.state_payload()?;
        let version = durable
            .store_version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONTROLLER_STATE_NAMESPACE.into(),
                    key: CONTROLLER_STATE_KEY.into(),
                    version,
                    payload,
                },
                (durable.store_version != 0).then_some(durable.store_version),
            )
            .map_err(store_error)?;
        durable.store_version = version;
        durable.registry = registry;
        Ok(())
    }
}
