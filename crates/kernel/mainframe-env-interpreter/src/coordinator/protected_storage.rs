//! Actual fresh protected storage ownership; no native/IBM terminal disposition.
use super::*;
use mainframe_env_store_api::{ProviderStateIdentity, RootDriverAdmission, RootDriverClaim};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Frozen configured storage scopes; these bytes do not attest a compiled host.
pub struct ProtectedStorageConfiguration {
    pub configuration_digest: [u8; 32],
    pub provider_namespaces: Vec<String>,
    pub provider_rows: Vec<ProviderStateIdentity>,
}

/// Borrowed actual coordinator-inserted ownership, never a copied claim producer.
///
/// There is no public constructor, Clone, Debug or serialization implementation.
/// Compilation, CICS task custody, installed ABI and MQ authority remain separate.
pub struct ProtectedStorageAdmission<'a> {
    original: &'a Invocation,
    store: &'a Arc<dyn PlatformStore>,
    claim: &'a RootDriverClaim,
}

impl ProtectedStorageAdmission<'_> {
    pub fn original(&self) -> &Invocation {
        self.original
    }
    pub fn store(&self) -> &Arc<dyn PlatformStore> {
        self.store
    }
    pub fn claim(&self) -> &RootDriverClaim {
        self.claim
    }

    /// Recheck this actual Open owner. Equality is a current storage observation,
    /// not an exclusive store transaction or a substitute for live task custody.
    pub fn require_current(&self, original: &Invocation) -> Result<(), HostProblem> {
        if original != self.original || original.cancellation_requested() {
            return Err(HostProblem::Unauthorized);
        }
        let inserted = self.claim.inserted_row();
        let current = self
            .store
            .get_provider_state(&inserted.namespace, &inserted.key)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if current.as_ref() != Some(inserted) {
            return Err(HostProblem::UnknownOutcome);
        }
        let execution = self
            .store
            .get_execution(&original.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        if execution.execution_id != original.execution_id
            || execution.run_unit_id != original.run_unit_id
            || execution.attempt != original.attempt
            || execution.selector != original.selector
            || execution.artifact != original.artifact
            || execution.principal != *original.principal.id()
            || !matches!(
                execution.state,
                ExecutionState::Admitted | ExecutionState::Queued | ExecutionState::Running
            )
            || execution.terminal_tick.is_some()
            || execution.owner_lease.is_some()
            || execution.lease_expiry_tick.is_some()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(())
    }
}

/// Explicit installed embedding. Default refusal performs no host dispatch.
///
/// The embedding must independently check the actual compiled artifact/frame,
/// live CICS task loan and physical manager before accepting either callback.
/// No completion/ABEND/RETURN, commit, backout or provider result may be supplied.
pub trait ProtectedStorageDispatchHooks {
    fn admitted(&mut self, _admission: &ProtectedStorageAdmission<'_>) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }
    fn with_dispatch(
        &mut self,
        _admission: &ProtectedStorageAdmission<'_>,
        _dispatch: &DurableEffectDispatch<'_>,
        _invoke: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }
    fn retain_uncertain(
        &mut self,
        _original: &Invocation,
        _admission: Option<&ProtectedStorageAdmission<'_>>,
    ) -> Result<(), HostProblem> {
        Err(HostProblem::Unsupported)
    }
}

pub(super) struct ProtectedStorageProgress<'a> {
    original: &'a Invocation,
    configuration: Option<ProtectedStorageConfiguration>,
    hooks: &'a mut dyn ProtectedStorageDispatchHooks,
    claim: Option<RootDriverClaim>,
    last_tick: u64,
    retained: bool,
}

struct DispatchAdapter<'a, 'b> {
    admission: ProtectedStorageAdmission<'a>,
    hooks: &'b mut dyn ProtectedStorageDispatchHooks,
}

impl DurableEffectDispatchHook for DispatchAdapter<'_, '_> {
    fn with_dispatch(
        &mut self,
        dispatch: &DurableEffectDispatch<'_>,
        invoke: &mut dyn FnMut() -> Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        self.admission.require_current(dispatch.original())?;
        if !Arc::ptr_eq(self.admission.store(), dispatch.store()) {
            return Err(HostProblem::UnknownOutcome);
        }
        self.hooks.with_dispatch(&self.admission, dispatch, invoke)
    }
}

impl ProtectedStorageProgress<'_> {
    pub(super) fn admit(
        &mut self,
        store: &Arc<dyn PlatformStore>,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        let configuration = self.configuration.take().ok_or(StoreError::Conflict)?;
        self.last_tick = event.tick;
        // The actual fresh JournalCursor records are generated by the owning
        // coordinator. Neither a supplied claim nor an ordinary Running row
        // enters this call. The backend atomically inserts the actual owner.
        self.claim = Some(store.admit_root_driver(RootDriverAdmission {
            execution,
            event,
            notification,
            invocation_key: self.original.idempotency_key.clone(),
            deadline_tick: self.original.deadline_tick,
            configuration_digest: configuration.configuration_digest,
            provider_namespaces: configuration.provider_namespaces,
            provider_rows: configuration.provider_rows,
        })?);
        let admission = ProtectedStorageAdmission {
            original: self.original,
            store,
            claim: self.claim.as_ref().ok_or(StoreError::NotFound)?,
        };
        admission
            .require_current(self.original)
            .map_err(|_| StoreError::InvalidTransition)?;
        self.hooks
            .admitted(&admission)
            .map_err(|_| StoreError::InvalidTransition)
    }

    pub(super) fn dispatch(
        &mut self,
        store: &Arc<dyn PlatformStore>,
        intent: &EffectRecord,
        host: &Arc<ScopedHostService>,
        effect: EffectRequest,
        control: ExecutionControl,
    ) -> Result<(mainframe_env_host_api::AuditedEffectResult, bool), ExecutionOutcome> {
        self.last_tick = control.now_tick;
        let claim = self.claim.as_ref().ok_or_else(|| {
            failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "protected storage admission absent",
            ))
        })?;
        let mut adapter = DispatchAdapter {
            admission: ProtectedStorageAdmission {
                original: self.original,
                store,
                claim,
            },
            hooks: &mut *self.hooks,
        };
        durable_dispatch::invoke(
            &mut adapter,
            host,
            self.original,
            store,
            intent,
            control,
            effect,
        )
    }

    pub(super) fn protect(&mut self, store: &Arc<dyn PlatformStore>, tick: u64) {
        if self.retained {
            return;
        }
        self.retained = true;
        if let Some(claim) = &self.claim
            && let Ok(Some(execution)) = store.get_execution(&self.original.execution_id)
            && tick != 0
        {
            // A failed clock/fence does not invent a newer age or settlement.
            let _ = store.fence_root_driver(claim, &execution, tick);
        }
        let admission = self.claim.as_ref().map(|claim| ProtectedStorageAdmission {
            original: self.original,
            store,
            claim,
        });
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.hooks
                .retain_uncertain(self.original, admission.as_ref())
        }));
    }
}

impl ExecutionCoordinator {
    /// Fresh storage-owned drive with actual per-effect Intent borrowing.
    ///
    /// This opt-in prerequisite has no terminal settlement authority. Every
    /// exit remains protected UnknownOutcome; it cannot earn root completion,
    /// RETURN, IBM MQ, reconnect or application acceptance. Existing local,
    /// resumable and native-terminal entries remain unchanged. Every dispatched
    /// effect needs its original durable key/Intent; unsupported profiles fail
    /// closed. Installed CICS/MQ routing is a separate higher-layer component.
    pub fn execute_protected_storage_with_control<M, F>(
        &self,
        machine: &mut M,
        original: &Invocation,
        configuration: ProtectedStorageConfiguration,
        hooks: &mut dyn ProtectedStorageDispatchHooks,
        observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
    {
        if original.parent_execution_id.is_some()
            || self.host.is_none()
            || self.store.is_none()
            || original.deadline_tick == 0
            || original.deadline_tick > i64::MAX as u64
        {
            return failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "protected storage setup refused",
            ));
        }
        let mut progress = ProtectedStorageProgress {
            original,
            configuration: Some(configuration),
            hooks,
            claim: None,
            last_tick: 0,
            retained: false,
        };
        let _outcome = catch_unwind(AssertUnwindSafe(|| {
            self.execute_inner(
                machine,
                original,
                observe,
                false,
                None,
                None,
                DriveDispatch {
                    storage: Some(&mut progress),
                    ..DriveDispatch::default()
                },
            )
        }));
        if let Some(store) = &self.store {
            progress.protect(store, progress.last_tick);
        }
        failed_outcome(problem(
            FailureCategory::UnknownOutcome,
            "protected storage root retained without terminal authority",
        ))
    }
}

impl ExecutionCoordinator {
    /// Prepare the actual machine after fresh ownership admission, before its first drive.
    /// Existing default entry and native restrictions are unchanged. The optional
    /// installed frame is obtained only by the owning admitted embedding; no
    /// invocation bytes choose a frame and no terminal settlement is introduced.
    pub fn execute_protected_storage_with_preparation<M, F, P>(
        &self,
        machine: &mut M,
        original: &Invocation,
        configuration: ProtectedStorageConfiguration,
        hooks: &mut dyn ProtectedStorageDispatchHooks,
        prepare: P,
        observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
        P: FnOnce(&mut M) -> Result<(), HostProblem>,
    {
        if original.parent_execution_id.is_some()
            || self.host.is_none()
            || self.store.is_none()
            || original.deadline_tick == 0
            || original.deadline_tick > i64::MAX as u64
        {
            return failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "protected storage setup refused",
            ));
        }
        let mut progress = ProtectedStorageProgress {
            original,
            configuration: Some(configuration),
            hooks,
            claim: None,
            last_tick: 0,
            retained: false,
        };
        let mut prepare = Some(prepare);
        let mut prepare =
            |machine: &mut M| prepare.take().ok_or(HostProblem::UnknownOutcome)?(machine);
        let _outcome = catch_unwind(AssertUnwindSafe(|| {
            self.execute_inner(
                machine,
                original,
                observe,
                false,
                None,
                Some(&mut prepare),
                DriveDispatch {
                    storage: Some(&mut progress),
                    ..DriveDispatch::default()
                },
            )
        }));
        if let Some(store) = &self.store {
            progress.protect(store, progress.last_tick);
        }
        failed_outcome(problem(
            FailureCategory::UnknownOutcome,
            "protected storage root retained without terminal authority",
        ))
    }
}
