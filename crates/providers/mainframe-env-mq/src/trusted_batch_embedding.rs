//! Privileged Rust embedding for an already normalized selected MQ runtime.
//! Not application admission, registration or a topology attestation. The host
//! independently validates original installed/core/CALL/store/control authority.
use crate::mqi_lifecycle::{BatchChildBinding, FrameLease, InstalledBatchRelationship};
use crate::{MqLimits, MqReplayClock, MqService};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits};
use mainframe_env_host_api::mq_mqi::{MqMqiContext, MqMqiLimits, MqMqiUnitOfWork};
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectResult, EnterpriseAuthorizer, HostLimits, HostProblem, MqHconn,
    MqMqiEffectOccurrence, MqSyncpointOwner,
};
use mainframe_env_store_api::PlatformStore;
use std::sync::Arc;
mod rfh2_source;
pub use rfh2_source::MqBatchLeDllCodesetSource;
pub(crate) use rfh2_source::capturing as rfh2_source_capturing;
mod native_point;
pub use native_point::{
    MqTrustedBatchPointProfile, MqTrustedBatchPointTarget, MqTrustedBatchStructureProfile,
};

/// Independently selected host topology; matching Invocation IDs are not proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqTrustedBatchRelationship {
    /// The privileged host has proved an ordinary CALL in the same continuing task.
    SameTaskCall,
    /// A separate processing unit cannot inherit the parent's batch authority.
    SeparateSubtask,
}

struct Runtime {
    service: Arc<MqService>,
    provider: CapabilityDescriptor,
    host_limits: HostLimits,
    mqi_limits: MqMqiLimits,
}

/// Closed configured runtime retaining one selected service and its same store.
pub struct MqTrustedBatchRuntime {
    inner: Arc<Runtime>,
}

struct Root {
    runtime: Arc<Runtime>,
    original: Invocation,
    frame: FrameLease,
}

/// Opaque original task root. No public/Serde lease constructor or task-end hook.
pub struct MqTrustedBatchRoot {
    inner: Arc<Root>,
}

/// Closed original frame. Exclusive dispatch/lifecycle access prevents races
/// through this object; the directory remains the sole lifecycle authority.
/// Dropping any facet object makes no service or durable decision.
///
/// Opaque frames cannot be constructed or serialized by an application:
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchFrame;
/// let forged = MqTrustedBatchFrame { active: true };
/// ```
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchFrame;
/// fn serializable<T: serde::Serialize>() {}
/// serializable::<MqTrustedBatchFrame>();
/// ```
/// ```compile_fail
/// use mainframe_env_mq::MqTrustedBatchFrame;
/// fn clonable<T: Clone>() {}
/// clonable::<MqTrustedBatchFrame>();
/// ```
pub struct MqTrustedBatchFrame {
    root: Arc<Root>,
    original: Invocation,
    frame: FrameLease,
    child: Option<BatchChildBinding>,
    active: bool,
    dispatched: bool,
}

impl MqTrustedBatchRuntime {
    /// PRIVILEGED host-only Rust setup, once before activation, on this unique
    /// runtime and SAME physical store. The adapter must use the sole owned
    /// structure encoder and independently check original installed host/frame
    /// provenance. This is not application admission, JES attestation or SAF.
    ///
    /// Source callbacks must be bounded/nonblocking, may not publish or perform
    /// cleanup, and may not wait for cross-thread service reentry. Panics/errors
    /// fail closed. NoContext-only adapters may leave GMT/context Unsupported;
    /// DefaultContext then remains unavailable. Old unconfigured runtimes remain
    /// unable to execute complete producers.
    pub fn configure_producer_source(
        &mut self,
        store: &Arc<dyn PlatformStore>,
        source: Arc<dyn crate::MqTrustedBatchProducerSource>,
    ) -> Result<(), HostProblem> {
        let runtime = Arc::get_mut(&mut self.inner).ok_or(HostProblem::Unsupported)?;
        MqService::configure_producer_sources(&mut runtime.service, store, source)
    }

    /// Privileged host setup, not a ready route advertisement. Deployment/recovery
    /// supplies generation/fence, the mandatory authorizer/clock and frozen profiles.
    /// Opening requires existing strict rich rows. It never initializes, imports,
    /// advances durable fences, normalizes or falls back to legacy behavior.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        store: Arc<dyn PlatformStore>,
        limits: MqLimits,
        generation: u64,
        fence: u64,
        authorizer: Arc<dyn EnterpriseAuthorizer>,
        clock: Arc<dyn MqReplayClock>,
        provider: CapabilityDescriptor,
        host_limits: HostLimits,
        mqi_limits: MqMqiLimits,
    ) -> Result<Self, HostProblem> {
        provider
            .validate(InvocationLimits::default())
            .map_err(|_| HostProblem::ProviderFailure)?;
        if !provider.ready
            || provider.provider_id != "mainframe-env-mq"
            || provider.capability
                != CapabilityId::new("host.mq.write", InvocationLimits::default())
                    .map_err(|_| HostProblem::Malformed)?
        {
            return Err(HostProblem::ProviderFailure);
        }
        mqi_limits.validate().map_err(|_| HostProblem::Malformed)?;
        let max = HostLimits::default();
        if [
            (host_limits.max_name_bytes, max.max_name_bytes),
            (host_limits.max_record_bytes, max.max_record_bytes),
            (host_limits.max_records, max.max_records),
            (host_limits.max_fields, max.max_fields),
            (host_limits.max_audit_fields, max.max_audit_fields),
            (host_limits.max_state_bytes, max.max_state_bytes),
        ]
        .iter()
        .any(|(value, ceiling)| *value == 0 || value > ceiling)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let service =
            MqService::open_selected_mqi(store, limits, generation, fence, authorizer, clock)?;
        service.require_trusted_batch_rich()?;
        Ok(Self {
            inner: Arc::new(Runtime {
                service,
                provider,
                host_limits,
                mqi_limits,
            }),
        })
    }

    /// PRIVILEGED RUST EMBEDDING operation, not an attestation constructor for
    /// application bindings. The host independently validates genuine original
    /// admission/core/CALL/store/control and selects ordinary ZosBatch/QueueManager.
    /// Requires parentNone and preserves the bounded original exactly.
    pub fn admit_root(&self, original: Invocation) -> Result<MqTrustedBatchRoot, HostProblem> {
        let frame = self.inner.service.prepare_trusted_batch_root(&original)?;
        Ok(MqTrustedBatchRoot {
            inner: Arc::new(Root {
                runtime: self.inner.clone(),
                original,
                frame,
            }),
        })
    }

    /// Privileged SAME TASK selection checked against this live opaque parent,
    /// exact originals, physical probe and nonwidening controls. No owner/lease
    /// constructor or separately substituted parent input is accepted.
    pub fn prepare_same_task_child(
        &self,
        parent: &MqTrustedBatchFrame,
        original: Invocation,
        relationship: MqTrustedBatchRelationship,
    ) -> Result<MqTrustedBatchFrame, HostProblem> {
        parent.require_active()?;
        if !Arc::ptr_eq(&self.inner, &parent.root.runtime) {
            return Err(HostProblem::Unauthorized);
        }
        let relationship = match relationship {
            MqTrustedBatchRelationship::SameTaskCall => InstalledBatchRelationship::SameTaskCall,
            MqTrustedBatchRelationship::SeparateSubtask => {
                InstalledBatchRelationship::SeparateSubtask
            }
        };
        let child = self.inner.service.prepare_selected_batch_child(
            parent.frame,
            &parent.original,
            &original,
            relationship,
        )?;
        Ok(MqTrustedBatchFrame {
            root: parent.root.clone(),
            original,
            frame: child.frame(),
            child: Some(child),
            active: true,
            dispatched: false,
        })
    }
}

impl MqTrustedBatchRoot {
    /// Exact original root, with no mutable projection or binding insertion.
    pub fn original(&self) -> &Invocation {
        &self.inner.original
    }

    /// Retained root reference. Root task end is unsupported; child objects
    /// retain the same root even when its external wrapper is dropped.
    pub fn frame(&self) -> MqTrustedBatchFrame {
        MqTrustedBatchFrame {
            root: self.inner.clone(),
            original: self.inner.original.clone(),
            frame: self.inner.frame,
            child: None,
            active: true,
            dispatched: false,
        }
    }
}

impl MqTrustedBatchFrame {
    fn require_active(&self) -> Result<(), HostProblem> {
        if self.active {
            Ok(())
        } else {
            Err(HostProblem::Unauthorized)
        }
    }

    /// Exact original host snapshot; this facet never rewrites it.
    pub fn original(&self) -> &Invocation {
        &self.original
    }

    /// Frozen configured MQI profile, not a caller-widenable request permit.
    pub fn limits(&self) -> MqMqiLimits {
        self.root.runtime.mqi_limits
    }

    /// Fresh read-only directory owner/context. The assertion does not grant
    /// registry, UOW, state or SAF permission, or rewrite pending effect context.
    pub fn context(&self) -> Result<MqMqiContext, HostProblem> {
        self.require_active()?;
        Ok(MqMqiContext {
            owner: self
                .root
                .runtime
                .service
                .selected_batch_owner(self.frame, &self.original)?,
            syncpoint_owner: MqSyncpointOwner::QueueManager,
        })
    }

    /// Current retained UOW through the actual HCONN and logical-origin authority.
    pub fn current_unit(&self, connection: MqHconn) -> Result<MqMqiUnitOfWork, HostProblem> {
        self.require_active()?;
        self.root
            .runtime
            .service
            .selected_local_unit(self.frame, &self.original, connection)
    }

    /// Dispatch ONE original host occurrence under the existing sole authority.
    /// No substituted envelope/mutation/store. Core completion remains coordinator
    /// owned; SAF/audit/CAS/replay/uncertainty keep their existing checks and owners.
    pub fn dispatch(
        &mut self,
        original: MqMqiEffectOccurrence<'_>,
    ) -> Result<EffectResult, HostProblem> {
        self.require_active()?;
        if original.envelope().limits != self.limits() {
            return Err(HostProblem::Malformed);
        }
        // Failure cannot establish that setup rollback is still safe.
        self.dispatched = true;
        let result = self.root.runtime.service.execute_selected_mqi(
            self.frame,
            &self.original,
            original,
            &self.root.runtime.provider,
            self.root.runtime.host_limits,
        );
        if matches!(result, Err(HostProblem::UnknownOutcome)) {
            // The service already fenced under its sole mutex. Retain the
            // directory reference; this wrapper cannot infer a normal return.
            self.active = false;
        }
        result
    }

    /// Explicit child preparation abort, before any dispatch attempt. A repeated
    /// binding cannot erase an existing frame. No handle or durable work decision.
    pub fn abort_preparation(&mut self) -> Result<(), HostProblem> {
        self.require_active()?;
        if self.dispatched {
            return Err(HostProblem::Unsupported);
        }
        let child = self.child.ok_or(HostProblem::Unsupported)?;
        self.root
            .runtime
            .service
            .abort_selected_batch_child(child)?;
        self.active = false;
        Ok(())
    }

    /// Host-proven normal nonfinal child CALL return; not MQDISC, task end,
    /// implicit commit/backout or a raw abnormal-outcome mapping.
    pub fn return_normal(&mut self) -> Result<(), HostProblem> {
        self.require_active()?;
        self.child.ok_or(HostProblem::Unsupported)?;
        self.root
            .runtime
            .service
            .return_selected_batch_child(self.frame, &self.original)?;
        self.active = false;
        Ok(())
    }

    /// Unclassified/abnormal lifecycle fences this same service, revokes this
    /// wrapper and retains all authorities for real host recovery. No retry,
    /// frame retirement, backout or inferred task end. Returns UnknownOutcome.
    pub fn retain_uncertain(&mut self) -> Result<(), HostProblem> {
        self.require_active()?;
        self.root.runtime.service.fence_trusted_batch()?;
        self.active = false;
        Err(HostProblem::UnknownOutcome)
    }
}

#[cfg(test)]
mod tests;
