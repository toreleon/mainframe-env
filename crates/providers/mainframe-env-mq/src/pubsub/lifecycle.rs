//! Shared volatile handle-family ownership; no store or dispatcher is introduced.

use super::*;
use std::ops::{Deref, DerefMut};

/// Scoped access to the composed message kernel. Retirement is reconciled when
/// the borrow ends, including early-return/error paths.
pub struct MqMessageHandleAccess<'a>(&'a mut MqPubsubKernel);

impl Deref for MqMessageHandleAccess<'_> {
    type Target = MqHandleKernel;
    fn deref(&self) -> &Self::Target {
        &self.0.handles
    }
}
impl DerefMut for MqMessageHandleAccess<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.handles
    }
}
impl Drop for MqMessageHandleAccess<'_> {
    fn drop(&mut self) {
        self.0.reclaim_retired_handles();
    }
}

/// Compatibility registry access with the same bounded retirement cleanup.
pub struct MqRegistryAccess<'a>(&'a mut MqPubsubKernel);

impl Deref for MqRegistryAccess<'_> {
    type Target = MqHandleRegistry;
    fn deref(&self) -> &Self::Target {
        &self.0.handles.registry
    }
}
impl DerefMut for MqRegistryAccess<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.handles.registry
    }
}
impl Drop for MqRegistryAccess<'_> {
    fn drop(&mut self) {
        self.0.reclaim_retired_handles();
    }
}

pub(super) fn handle_kernel_problem(problem: MqHandleKernelProblem) -> MqPubsubError {
    match problem {
        MqHandleKernelProblem::Handle(problem) => MqPubsubError::Handle(problem),
        MqHandleKernelProblem::Message(problem) => MqPubsubError::Message(problem),
        _ => MqPubsubError::InvalidLimits,
    }
}

impl MqPubsubKernel {
    /// Property operations use the same connections and slot budget as subscriptions.
    /// A scoped guard reconciles any direct lifetime mutation on release.
    pub fn message_handles_mut(&mut self) -> MqMessageHandleAccess<'_> {
        self.reclaim_retired_handles();
        MqMessageHandleAccess(self)
    }

    pub fn handles_mut(&mut self) -> MqRegistryAccess<'_> {
        self.reclaim_retired_handles();
        MqRegistryAccess(self)
    }

    pub(super) fn reclaim_retired_handles(&mut self) {
        self.handles.reclaim_retired_properties();
        self.reclaim_retired_bindings();
    }

    /// Retire a connection and its associated properties and subscription bindings.
    /// The frozen CICS default-connection no-op remains a no-op.
    pub fn disconnect(
        &mut self,
        owner: MqHandleOwner,
        hconn: MqHconn,
    ) -> Result<(), MqPubsubError> {
        self.handles
            .disconnect(owner, hconn)
            .map_err(handle_kernel_problem)?;
        self.reclaim_retired_bindings();
        Ok(())
    }

    /// Retire only this processing unit, preserving other owners' live state.
    pub fn end_processing_unit(&mut self, owner: MqHandleOwner) -> Result<(), MqPubsubError> {
        self.handles
            .end_processing_unit(owner)
            .map_err(handle_kernel_problem)?;
        self.reclaim_retired_bindings();
        Ok(())
    }

    /// Advance the registry epoch and invalidate every volatile family token.
    /// Durable subscriptions/publications and staged UOWs are not finalized here.
    pub fn advance_handle_epoch(&mut self, epoch: u64) -> Result<(), MqPubsubError> {
        self.handles
            .advance_epoch(epoch)
            .map_err(handle_kernel_problem)?;
        self.reclaim_retired_bindings();
        Ok(())
    }

    fn reclaim_retired_bindings(&mut self) {
        let registry = &self.handles.registry;
        let mut retired = Vec::new();
        self.bindings.retain(|(name, binding)| {
            let live = registry
                .validate(
                    binding.owner,
                    binding.hconn,
                    binding.handles.hsub.into(),
                    MqHandleKind::Subscription,
                )
                .is_ok()
                && registry
                    .validate(
                        binding.owner,
                        binding.hconn,
                        binding.handles.hobj.into(),
                        MqHandleKind::Object,
                    )
                    .is_ok();
            if !live {
                retired.push(name.clone());
            }
            live
        });
        self.callbacks.retain(|callback| {
            self.bindings.iter().any(|(_, binding)| {
                binding.hconn == callback.hconn && binding.handles.hobj == callback.hobj
            })
        });
        self.controls.retain(|control| {
            registry
                .validate_connection(control.owner, control.hconn)
                .is_ok()
        });
        for name in retired {
            if self
                .state
                .subscriptions
                .get(&name)
                .is_some_and(|entry| !entry.durable)
                && !self.bindings.iter().any(|(bound, _)| bound == &name)
            {
                self.state.subscriptions.remove(&name);
            }
        }
    }
}
