//! Frozen original root/configuration identity through the shared encoder.
use super::*;
use crate::mq_mqi::MqMqiLimits;
use crate::{CapabilityDescriptor, HostLimits};
use mainframe_env_execution_api::{Invocation, InvocationLimits, ServiceClass};

const SETUP_DOMAIN: &[u8] = b"mainframe-env.root-terminal-setup@1\0";

#[cfg(test)]
mod tests;

/// Frozen compiled-root setup. Physical Arc/probe identity must be checked
/// independently; hashing matching descriptors never attests host admission.
pub struct RootTerminalSetup<'a> {
    /// Exact original parentNone invocation; no inserted host context.
    pub original: &'a Invocation,
    /// Exact selected provider descriptor, independently physically selected.
    pub provider: &'a CapabilityDescriptor,
    /// Frozen outer validation budgets.
    pub host_limits: HostLimits,
    /// Frozen typed MQI validation budgets.
    pub mqi_limits: MqMqiLimits,
    /// Trusted physical provider generation.
    pub generation: u64,
    /// Trusted physical provider fence.
    pub fence: u64,
    /// MQ limits ordered queues/messages-per-queue/message-bytes/handles/
    /// pending-units/replays/state-bytes, each positive and frozen at open.
    pub mq_limits: &'a [u64; 7],
    /// Exact validated compiled payload identity.
    pub content_digest: &'a [u8; 32],
    /// Existing artifact authority's full immutable manifest/payload binding.
    pub manifest_payload_digest: &'a [u8; 32],
    /// Exact validated semantic identity, not Debug or JSON.
    pub semantic_identity: &'a str,
    /// Original exact batch catalog record, also a physical closure dependency.
    pub catalog: RootTerminalResourceRow<'a>,
}

impl Canonical for RootTerminalSetup<'_> {
    fn encode(&self, out: &mut Encoder<'_>) -> Result<(), HostProblem> {
        let v = self.original;
        let bounds = InvocationLimits::default();
        if v.parent_execution_id.is_some()
            || v.attempt == 0
            || v.deadline_tick == 0
            || v.deadline_tick > i64::MAX as u64
            || v.bindings.len() > bounds.max_bindings
            || v.principal.grants().len() > bounds.max_capabilities
            || v.provider_generations.len() > bounds.max_capabilities
            || v.audit_correlation.len() > bounds.max_binding_bytes
            || v.bindings.iter().any(|(k, value)| {
                k.is_empty()
                    || k.len() > bounds.max_binding_bytes
                    || value.bytes().len() > bounds.max_payload_bytes
            })
            || v.provider_generations.iter().any(|(cap, generation)| {
                !v.principal.has_grant(cap)
                    || generation.is_empty()
                    || generation.len() > bounds.max_identity_bytes
            })
            || v.cancellation
                .as_ref()
                .is_some_and(|c| c.reason.is_empty() || c.reason.len() > bounds.max_binding_bytes)
            || self.generation == 0
            || self.generation > i64::MAX as u64
            || self.fence == 0
            || self.fence > i64::MAX as u64
            || self.mq_limits.contains(&0)
            || self.semantic_identity.is_empty()
            || self.semantic_identity.len() > bounds.max_identity_bytes
        {
            return Err(HostProblem::Malformed);
        }
        v.limits.validate().map_err(|_| HostProblem::Malformed)?;
        self.provider
            .validate(bounds)
            .map_err(|_| HostProblem::Malformed)?;
        self.mqi_limits
            .validate()
            .map_err(|_| HostProblem::Malformed)?;
        if !matches!(
            self.catalog,
            RootTerminalResourceRow::Exact {
                namespace: "batch-program",
                ..
            }
        ) {
            return Err(HostProblem::Malformed);
        }
        out.object("RootTerminalSetup", 11)?;
        out.object("OriginalRootInvocation", 21)?;
        v.request_id.as_str().encode(out)?;
        v.execution_id.as_str().encode(out)?;
        v.run_unit_id.encode(out)?;
        v.selector.as_str().encode(out)?;
        v.artifact.encode(out)?;
        v.principal.id().encode(out)?;
        out.tag(0x30)?;
        out.length(v.principal.grants().len())?;
        for grant in v.principal.grants() {
            grant.encode(out)?;
        }
        out.variant(
            "ServiceClass",
            match v.service_class {
                ServiceClass::Interactive => "Interactive",
                ServiceClass::Batch => "Batch",
                ServiceClass::Compiler => "Compiler",
                ServiceClass::Blocking => "Blocking",
                ServiceClass::System => "System",
            },
            0,
        )?;
        v.priority.encode(out)?;
        v.deadline_tick.encode(out)?;
        v.trace_id.as_str().encode(out)?;
        v.idempotency_key.encode(out)?;
        v.attempt.encode(out)?;
        out.object("ResourceLimits", 6)?;
        v.limits.max_steps.encode(out)?;
        v.limits.max_storage_bytes.encode(out)?;
        v.limits.max_output_bytes.encode(out)?;
        v.limits.max_frames.encode(out)?;
        v.limits.max_effects.encode(out)?;
        v.limits.max_events.encode(out)?;
        v.bindings.encode(out)?;
        match &v.cancellation {
            None => out.tag(0x20)?,
            Some(c) => {
                out.tag(0x21)?;
                out.object("Cancellation", 3)?;
                c.id.as_str().encode(out)?;
                c.reason.encode(out)?;
                c.requested_at_tick.encode(out)?;
            }
        }
        // Mutable probe value is deliberately excluded from the frozen digest.
        // Presence is encoded; the genuine host compares physical probe identity.
        v.cancellation_probe.is_some().encode(out)?;
        v.provider_generations.encode(out)?;
        v.audit_correlation.encode(out)?;
        out.variant("RootTopology", "ParentNoneOrdinarySameTask", 0)?;
        out.variant("RootContext", "ZosBatchQueueManager", 0)?;
        out.object("CapabilityDescriptor", 8)?;
        self.provider.capability.encode(out)?;
        self.provider.provider_id.encode(out)?;
        self.provider.generation.encode(out)?;
        self.provider.request_schema.encode(out)?;
        self.provider.result_schema.encode(out)?;
        self.provider.max_request_bytes.encode(out)?;
        self.provider.max_result_bytes.encode(out)?;
        self.provider.ready.encode(out)?;
        self.host_limits.encode(out)?;
        self.mqi_limits.encode(out)?;
        self.generation.encode(out)?;
        self.fence.encode(out)?;
        self.mq_limits.encode(out)?;
        self.content_digest.as_slice().encode(out)?;
        self.manifest_payload_digest.as_slice().encode(out)?;
        self.semantic_identity.encode(out)?;
        self.catalog.encode(out)
    }
}

/// Exact frozen setup digest without allocated preimages or a second codec.
/// This structural digest is not a substitute for physical host/store proof.
pub fn canonical_root_terminal_setup_digest(
    value: &RootTerminalSetup<'_>,
    byte_limit: usize,
) -> Result<[u8; 32], HostProblem> {
    let mut hash = Sha256::new();
    encode(
        value,
        SETUP_DOMAIN,
        byte_limit.min(MAX_CANONICAL_EFFECT_BYTES),
        &mut |b| hash.update(b),
    )?;
    Ok(hash.finalize().into())
}
