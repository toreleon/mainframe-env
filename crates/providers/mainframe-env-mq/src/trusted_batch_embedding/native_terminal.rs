//! Privileged native root integration, not application finality or row permission.
use super::*;
use crate::service::SelectedTerminalPreparation;
use mainframe_env_execution_api::RootTerminalDisposition;
use mainframe_env_store_api::{
    ProviderStateMutation, RootClosureSnapshot, RootTerminalCommit, RootTerminalPublication,
};

/// One prepared candidate retaining the original closed root and same sole MQ
/// mutex/physical store until the owning core terminal transaction is attempted.
/// No public constructor/Serde/Clone or substitute store. Drop fences the gate
/// and preserves work; it makes no durable decision or automatic retry.
pub struct MqPreparedRootTerminal<'a> {
    root: &'a MqTrustedBatchRoot,
    prepared: Option<SelectedTerminalPreparation<'a>>,
    completed: bool,
}

impl MqTrustedBatchRuntime {
    /// Exact modeled namespaces for a genuine native driver's Closing capture.
    /// Structural storage scopes do not attest compilation/topology or confer
    /// mutation, SAF, UOW or terminal permission. No wildcard is returned.
    pub fn native_terminal_namespaces(&self) -> Vec<String> {
        crate::service::selected_terminal_namespaces()
    }
}

impl MqTrustedBatchRoot {
    /// PRIVILEGED NATIVE DRIVER boundary. The embedding must retain the genuine
    /// borrowed exclusive pre-terminal compiled coordinator winner, prove the
    /// same store/provider/control setup and supply its exact Closing capture.
    /// A structural snapshot or disposition alone is not host attestation.
    /// Once entered, every escaped frame observation/dispatch is revoked.
    pub fn prepare_native_terminal<'a>(
        &'a self,
        closure: &RootClosureSnapshot,
        disposition: RootTerminalDisposition,
    ) -> Result<MqPreparedRootTerminal<'a>, HostProblem> {
        self.inner
            .terminal
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        match self.inner.runtime.service.prepare_selected_native_terminal(
            self.inner.frame,
            &self.inner.original,
            closure,
            disposition,
        ) {
            Ok(prepared) => Ok(MqPreparedRootTerminal {
                root: self,
                prepared: Some(prepared),
                completed: false,
            }),
            Err(problem) => {
                self.inner.terminal.store(3, Ordering::SeqCst);
                Err(problem)
            }
        }
    }

    /// Revoke the shared transport gate and fence the same selected authority.
    /// Unknown is not a source-known abnormal end or durable settlement choice.
    /// This callback is explicit; Drop never invokes it or retires the root.
    pub fn retain_native_uncertain(&self) -> Result<(), HostProblem> {
        if self.inner.terminal.load(Ordering::SeqCst) == 2 {
            return Err(HostProblem::UnknownOutcome);
        }
        self.inner.terminal.store(3, Ordering::SeqCst);
        self.inner.runtime.service.fence_trusted_batch()
    }
}

impl MqPreparedRootTerminal<'_> {
    /// Exact staged MQ delta; callers can compose owning core/CALL closure but
    /// cannot replace this delta or select another store/service.
    pub fn mutations(&self) -> &[ProviderStateMutation] {
        self.prepared
            .as_ref()
            .map_or(&[], |prepared| prepared.mutations())
    }
    /// Actual same-clock finite observation used by this prepared candidate.
    pub fn observed_tick(&self) -> u64 {
        self.prepared
            .as_ref()
            .map_or(0, |prepared| prepared.observed_tick())
    }
    /// Attempt one whole owning core/MQ/audit publication. No sequential
    /// fallback/retry or core completion by a fabricated application effect.
    /// Lost acknowledgement retains/fences even when the physical commit won.
    pub fn publish(
        mut self,
        plan: RootTerminalPublication,
    ) -> Result<RootTerminalCommit, HostProblem> {
        let prepared = self.prepared.take().ok_or(HostProblem::UnknownOutcome)?;
        let result = prepared.publish(plan);
        if result.is_ok() {
            self.root.inner.terminal.store(2, Ordering::SeqCst);
            self.completed = true;
        } else {
            self.root.inner.terminal.store(3, Ordering::SeqCst);
        }
        result
    }
}
impl Drop for MqPreparedRootTerminal<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.root.inner.terminal.store(3, Ordering::SeqCst);
        }
    }
}
