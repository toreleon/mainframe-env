//! Explicit native-root drive. Legacy coordinator entries retain their behavior.
use super::*;
use mainframe_env_execution_api::{Abend, Completion, RootTerminalDisposition};
use mainframe_env_store_api::{
    ProviderStateIdentity, RootClosureSnapshot, RootDriverAdmission, RootDriverClaim,
    RootTerminalCommit, RootTerminalPublication, RootTerminalStep,
};
use std::panic::{AssertUnwindSafe, catch_unwind};
mod child;
pub use child::NativeChildEnrollment;
#[cfg(test)]
mod tests;

pub(super) fn local_effect(effect: &EffectRequest) -> bool {
    match &effect.request {
        mainframe_env_host_api::HostRequest::MqMqi(original) => {
            original.mutation.transaction.is_none()
        }
        mainframe_env_host_api::HostRequest::Program(
            mainframe_env_host_api::ProgramRequest::Call {
                program,
                service,
                payload,
                ..
            },
        ) => {
            payload.schema() == "mainframe-env.cobol.call@1"
                && (service.is_none()
                    || (program.as_str() == "CEE3ABD"
                        && service.as_ref().is_some_and(|selector| {
                            selector.kind
                                == mainframe_env_host_api::RuntimeServiceKind::LanguageEnvironment
                                && selector.name.as_str() == "CEE3ABD"
                                && selector.abi_version == 1
                        })))
        }
        _ => false,
    }
}

/// Frozen storage/configuration observations from the genuine compiled host.
/// Construction does not attest compilation, host topology or physical identity.
pub struct NativeRootConfiguration {
    /// Shared canonical digest of the exact original/frozen admitted setup.
    pub configuration_digest: [u8; 32],
    /// Exact run-specific namespaces whose writers must maintain Closing.
    pub provider_namespaces: Vec<String>,
    /// Exact initial shared-namespace lifecycle keys, never wildcard grants.
    pub provider_rows: Vec<ProviderStateIdentity>,
}

/// Original machine observation held by its exclusive pre-terminal drive.
/// Unknown/control/transport failures do not become a known native disposition.
pub enum NativeRootTermination {
    /// The actual root's unmodified machine completion.
    Completed(Completion),
    /// The actual root's explicitly modeled native ABEND.
    Abended(Abend),
}
impl NativeRootTermination {
    /// Source-policy selector only; original output/code remains separately retained.
    pub fn disposition(&self) -> RootTerminalDisposition {
        match self {
            Self::Completed(value) => RootTerminalDisposition::Normal {
                return_code: value.return_code,
            },
            Self::Abended(_) => RootTerminalDisposition::KnownAbnormal,
        }
    }
}

/// Borrowed genuine coordinator admission, not an application assertion.
/// No public constructor, Clone or Serde; the configured host still independently
/// validates compilation/catalog/setup and ordinary topology before accepting it.
pub struct NativeRootAdmission<'a> {
    original: &'a Invocation,
    store: &'a Arc<dyn PlatformStore>,
    claim: &'a RootDriverClaim,
}
impl NativeRootAdmission<'_> {
    /// Exact original parentNone snapshot; it cannot be rewritten through this port.
    pub fn original(&self) -> &Invocation {
        self.original
    }
    /// Actual physical adapter borrowed from the exclusive coordinator.
    pub fn store(&self) -> &Arc<dyn PlatformStore> {
        self.store
    }
    /// Actual inserted core ownership observation, never a reconstructed lease.
    pub fn claim(&self) -> &RootDriverClaim {
        self.claim
    }
}

/// Borrowed exclusive known pre-terminal winner. Construction is private to
/// this real coordinator drive; completed rows or final flags cannot mint it.
pub struct WinningRootTerminal<'a> {
    admission: NativeRootAdmission<'a>,
    termination: &'a NativeRootTermination,
    closure: RootClosureSnapshot,
}
impl WinningRootTerminal<'_> {
    /// Same original coordinator admission and physical setup.
    pub fn admission(&self) -> &NativeRootAdmission<'_> {
        &self.admission
    }
    /// Exact actual machine completion/ABEND; no numeric status mapping is inferred.
    pub fn termination(&self) -> &NativeRootTermination {
        self.termination
    }
    /// Complete same-store Closing capture before any terminal publication.
    pub fn closure(&self) -> &RootClosureSnapshot {
        &self.closure
    }
    /// Build only the owning original core steps/outbox. Provider deltas/audits
    /// remain empty until the same service prepares the complete physical plan.
    pub fn publication_plan(
        &self,
        observed_tick: u64,
    ) -> Result<RootTerminalPublication, StoreError> {
        self.closure.validate_bounds()?;
        if observed_tick < self.closure.observed_tick
            || observed_tick >= self.admission.original.deadline_tick
            || observed_tick == 0
            || observed_tick > i64::MAX as u64
        {
            return Err(StoreError::LeaseConflict);
        }
        let root = &self.closure.actors[0].execution;
        let kinds = match self.termination {
            NativeRootTermination::Completed(completion) => vec![
                (ExecutionState::Completing, LifecycleEventKind::Completing),
                (
                    ExecutionState::Completed,
                    LifecycleEventKind::Completed {
                        return_code: completion.return_code,
                    },
                ),
            ],
            NativeRootTermination::Abended(_) => {
                vec![(ExecutionState::Failed, LifecycleEventKind::Abend)]
            }
        };
        if root
            .version
            .checked_add(kinds.len() as u64)
            .is_none_or(|v| v > self.admission.original.limits.max_events)
        {
            return Err(StoreError::CapacityExceeded);
        }
        let steps = kinds
            .into_iter()
            .enumerate()
            .map(|(index, (next_state, kind))| {
                let event = LifecycleEvent {
                    execution_id: root.execution_id.clone(),
                    run_unit_id: root.run_unit_id.clone(),
                    sequence: root.version + index as u64 + 1,
                    attempt: root.attempt,
                    tick: observed_tick,
                    kind,
                };
                RootTerminalStep {
                    notification: notification(&event),
                    event,
                    next_state,
                }
            })
            .collect();
        Ok(RootTerminalPublication {
            closure: self.closure.clone(),
            disposition: self.termination.disposition(),
            observed_tick,
            steps,
            dependencies: Vec::new(),
            mutations: Vec::new(),
            audits: Vec::new(),
        })
    }
}

/// Trusted configured-host integration, called outside physical transactions.
/// Implementations must retain genuine compiled/catalog/host/store/control/MQ
/// authority; this trait is not an application finality or authorization port.
pub trait NativeRootHooks {
    /// Eagerly associate the actual root/closed frame before machine dispatch.
    /// A failure retains the inserted ownership; it cannot make the root fresh.
    fn admitted(&mut self, original: &NativeRootAdmission<'_>) -> Result<(), HostProblem>;
    /// Prepare and publish one complete settlement through the same selected
    /// service/store. No callback is executed under the backend TX/lock.
    fn settle(
        &mut self,
        winner: &WinningRootTerminal<'_>,
    ) -> Result<RootTerminalCommit, HostProblem>;
    /// Revoke/fence and retain an unclassified lifetime. No implicit DISC,
    /// backout, commit, retry, recovery claim or normal child return is permitted.
    fn retain_uncertain(
        &mut self,
        original: &Invocation,
        claim: Option<&RootDriverClaim>,
    ) -> Result<(), HostProblem>;
}

pub(super) struct NativeProgress<'a> {
    original: &'a Invocation,
    configuration: Option<NativeRootConfiguration>,
    hooks: &'a mut dyn NativeRootHooks,
    claim: Option<RootDriverClaim>,
    termination: Option<NativeRootTermination>,
    committed: Option<RootTerminalCommit>,
    retained: bool,
    last_tick: u64,
}
impl ExecutionCoordinator {
    /// Deliberate trusted native-root entry. The actual host must first admit
    /// the genuine compiled artifact/catalog and exact physical frozen setup.
    /// Requires parentNone and a durable journal. Legacy/local/resume entries
    /// do not acquire this hook or terminal permission. All unclassified exits
    /// become protected shared UnknownOutcome; Drop/panic never select backout.
    pub fn execute_root_with_control<M, F, P>(
        &self,
        machine: &mut M,
        original: &Invocation,
        configuration: NativeRootConfiguration,
        hooks: &mut dyn NativeRootHooks,
        prepare: P,
        observe: F,
    ) -> ExecutionOutcome
    where
        M: Machine<Effect = EffectRequest, EffectResult = EffectResult>,
        F: FnMut() -> Result<ExecutionControl, ExecutionControlError>,
        P: FnOnce(&mut M) -> Result<(), HostProblem>,
    {
        if original.parent_execution_id.is_some()
            || self.store.is_none()
            || self.host.is_none()
            || original.deadline_tick == 0
            || original.deadline_tick > i64::MAX as u64
        {
            return failed_outcome(problem(
                FailureCategory::UnknownOutcome,
                "native root setup refused",
            ));
        }
        let mut progress = NativeProgress {
            original,
            configuration: Some(configuration),
            hooks,
            claim: None,
            termination: None,
            committed: None,
            retained: false,
            last_tick: 0,
        };
        // Actual inserted ownership + eager host root precede machine binding.
        // Preparation is invoked once before any machine drive/effect intent.
        let mut prepare = Some(prepare);
        let mut prepare =
            |machine: &mut M| prepare.take().ok_or(HostProblem::UnknownOutcome)?(machine);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.execute_inner(
                machine,
                original,
                observe,
                false,
                Some(&mut progress),
                Some(&mut prepare),
                None,
            )
        }));
        if progress.committed.is_some() {
            return outcome.unwrap_or_else(|_| {
                failed_outcome(problem(
                    FailureCategory::UnknownOutcome,
                    "native terminal reply uncertain",
                ))
            });
        }
        if !progress.retained
            && let Some(store) = &self.store
        {
            progress.protect(store, progress.last_tick);
        }
        failed_outcome(problem(
            FailureCategory::UnknownOutcome,
            "native root retained for owned recovery",
        ))
    }
}
impl NativeProgress<'_> {
    pub(super) fn require_effect(
        &mut self,
        store: &Arc<dyn PlatformStore>,
        tick: u64,
        effect: &EffectRequest,
    ) -> Result<(), HostProblem> {
        self.last_tick = tick;
        let allowed = local_effect(effect);
        if !allowed || effect.idempotency_key.is_none() {
            self.protect(store, tick);
            return Err(HostProblem::Unsupported);
        }
        Ok(())
    }
    pub(super) fn admit(
        &mut self,
        store: &Arc<dyn PlatformStore>,
        execution: ExecutionRecord,
        event: LifecycleEvent,
        notification: OutboxRecord,
    ) -> Result<(), StoreError> {
        let configuration = self.configuration.take().ok_or(StoreError::Conflict)?;
        self.last_tick = event.tick;
        let claim = store.admit_root_driver(RootDriverAdmission {
            execution,
            event,
            notification,
            invocation_key: self.original.idempotency_key.clone(),
            deadline_tick: self.original.deadline_tick,
            configuration_digest: configuration.configuration_digest,
            provider_namespaces: configuration.provider_namespaces,
            provider_rows: configuration.provider_rows,
        })?;
        self.claim = Some(claim);
        let proof = NativeRootAdmission {
            original: self.original,
            store,
            claim: self.claim.as_ref().ok_or(StoreError::NotFound)?,
        };
        self.hooks
            .admitted(&proof)
            .map_err(|_| StoreError::InvalidTransition)
    }
    pub(super) fn completed(&mut self, value: &Completion) {
        if value.output.bytes().len() as u64 <= self.original.limits.max_output_bytes {
            self.termination = Some(NativeRootTermination::Completed(value.clone()));
        }
    }
    pub(super) fn abended(&mut self, value: &Abend) {
        if !value.code.is_empty()
            && value.code.len() <= 128
            && value.reason.as_ref().is_none_or(|v| v.len() <= 16384)
        {
            self.termination = Some(NativeRootTermination::Abended(value.clone()));
        }
    }
    fn protect(&mut self, store: &Arc<dyn PlatformStore>, tick: u64) {
        if self.retained {
            return;
        }
        self.retained = true;
        if let Some(claim) = &self.claim
            && let Ok(Some(execution)) = store.get_execution(&self.original.execution_id)
        {
            // A failed clock observation does not guess a newer tick. Keep
            // the actual durable owner even when fencing cannot be written.
            if tick != 0 {
                let _ = store.fence_root_driver(claim, &execution, tick);
            }
        }
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.hooks
                .retain_uncertain(self.original, self.claim.as_ref())
        }));
    }
    pub(super) fn intercept(
        &mut self,
        cursor: &mut JournalCursor<'_, '_>,
        kind: &LifecycleEventKind,
    ) -> Result<(), StoreError> {
        self.last_tick = cursor.tick;
        if let Some(commit) = &self.committed {
            return match (&self.termination, kind) {
                (
                    Some(NativeRootTermination::Completed(value)),
                    LifecycleEventKind::Completed { return_code },
                ) if value.return_code == *return_code
                    && commit.execution.state == ExecutionState::Completed =>
                {
                    Ok(())
                }
                _ => Err(StoreError::InvalidTransition),
            };
        }
        let known = matches!(
            (&self.termination, kind),
            (
                Some(NativeRootTermination::Completed(_)),
                LifecycleEventKind::Completing
            ) | (
                Some(NativeRootTermination::Abended(_)),
                LifecycleEventKind::Abend
            )
        );
        if !known || self.retained {
            self.protect(&cursor.store, cursor.tick);
            return Err(StoreError::InvalidTransition);
        }
        let result = (|| {
            let claim = self.claim.as_ref().ok_or(StoreError::NotFound)?;
            let execution = cursor
                .store
                .get_execution(&cursor.execution_id)?
                .ok_or(StoreError::NotFound)?;
            if execution.version != cursor.version || execution.state != ExecutionState::Running {
                return Err(StoreError::Conflict);
            }
            let closure = cursor
                .store
                .close_root_driver(claim, &execution, cursor.tick)?;
            let winner = WinningRootTerminal {
                admission: NativeRootAdmission {
                    original: self.original,
                    store: &cursor.store,
                    claim,
                },
                termination: self
                    .termination
                    .as_ref()
                    .ok_or(StoreError::InvalidTransition)?,
                closure,
            };
            let commit = self
                .hooks
                .settle(&winner)
                .map_err(|_| StoreError::InvalidTransition)?;
            let expected = match winner.termination {
                NativeRootTermination::Completed(_) => ExecutionState::Completed,
                NativeRootTermination::Abended(_) => ExecutionState::Failed,
            };
            if commit.claim != *claim
                || commit.execution.state != expected
                || cursor.store.get_execution(&cursor.execution_id)?.as_ref()
                    != Some(&commit.execution)
                || cursor
                    .store
                    .get_provider_state(&commit.winner.namespace, &commit.winner.key)?
                    .as_ref()
                    != Some(&commit.winner)
            {
                return Err(StoreError::Conflict);
            }
            cursor.version = commit.execution.version;
            cursor.sequence = commit.execution.version;
            self.committed = Some(commit);
            Ok(())
        })();
        if result.is_err() {
            self.protect(&cursor.store, cursor.tick);
        }
        result
    }
}
