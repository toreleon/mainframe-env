use crate::{
    ArtifactRef, CancellationId, CapabilityId, ExecutionId, IdempotencyKey, PrincipalId, RequestId,
    RunUnitId, Selector, TraceId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Construction bounds for invocation identities, grants, payloads, and bindings.
///
/// String lengths are measured in UTF-8 bytes. Payload limits apply to each
/// payload, rather than to the sum of all bindings. Defaults allow 128 identity
/// bytes, capabilities, and bindings, 1 MiB per payload, and 4096 binding bytes.
pub struct InvocationLimits {
    /// Maximum byte length accepted for an identity or payload schema identifier.
    pub max_identity_bytes: usize,
    /// Maximum number of principal grants or pinned provider generations.
    pub max_capabilities: usize,
    /// Maximum bytes in one payload, including each invocation binding value.
    pub max_payload_bytes: usize,
    /// Maximum number of named payload bindings in an invocation.
    pub max_bindings: usize,
    /// Maximum bytes in a binding name or cancellation reason.
    pub max_binding_bytes: usize,
}

impl Default for InvocationLimits {
    fn default() -> Self {
        Self {
            max_identity_bytes: 128,
            max_capabilities: 128,
            max_payload_bytes: 1024 * 1024,
            max_bindings: 128,
            max_binding_bytes: 4096,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Execution budgets carried by an invocation for the runtime to enforce.
///
/// Validation requires every budget to be positive; this value does not itself
/// account for usage. Defaults are one million steps, 64 MiB of storage, 1 MiB
/// of output, 256 frames, and 65,536 effects and events each.
pub struct ResourceLimits {
    /// Execution-wide instruction-step budget.
    pub max_steps: u64,
    /// Execution storage budget in bytes.
    pub max_storage_bytes: u64,
    /// Execution output budget in bytes.
    pub max_output_bytes: u64,
    /// Maximum permitted frame depth.
    pub max_frames: u32,
    /// Execution-wide host-effect budget.
    pub max_effects: u64,
    /// Execution-wide event budget.
    pub max_events: u64,
}

impl ResourceLimits {
    /// Return these budgets unchanged if all are positive, or reject a zero budget.
    pub fn validate(self) -> Result<Self, InvocationProblem> {
        if self.max_steps == 0
            || self.max_storage_bytes == 0
            || self.max_output_bytes == 0
            || self.max_frames == 0
            || self.max_effects == 0
            || self.max_events == 0
        {
            Err(InvocationProblem::InvalidResourceLimits)
        } else {
            Ok(self)
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_storage_bytes: 64 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            max_frames: 256,
            max_effects: 65_536,
            max_events: 65_536,
        }
    }
}

#[derive(Clone)]
/// Owned opaque bytes with a nonempty, bounded schema identifier.
///
/// Clones share the byte allocation, which is zeroized when its last owner drops.
/// Construction bounds the schema and byte lengths without interpreting either.
/// Debug output redacts bytes only for the `mainframe-env.cics.secret@1` schema;
/// other payload bytes remain visible in debug output.
pub struct BoundedPayload {
    schema: String,
    bytes: std::sync::Arc<zeroize::Zeroizing<Vec<u8>>>,
}

impl std::fmt::Debug for BoundedPayload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = formatter.debug_struct("BoundedPayload");
        result.field("schema", &self.schema);
        if self.schema == "mainframe-env.cics.secret@1" {
            result.field("bytes", &"<redacted>");
        } else {
            result.field("bytes", &self.bytes);
        }
        result.finish()
    }
}

impl PartialEq for BoundedPayload {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema && self.bytes() == other.bytes()
    }
}

impl Eq for BoundedPayload {}

impl BoundedPayload {
    /// Take ownership of bytes and validate their length and the schema byte length.
    ///
    /// The schema must be nonempty. Empty byte payloads are accepted. No schema
    /// lookup or payload decoding is performed.
    pub fn new(
        schema: impl Into<String>,
        bytes: Vec<u8>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        let schema = schema.into();
        let bytes = zeroize::Zeroizing::new(bytes);
        if schema.is_empty() || schema.len() > limits.max_identity_bytes {
            return Err(InvocationProblem::InvalidPayloadSchema);
        }
        if bytes.len() > limits.max_payload_bytes {
            return Err(InvocationProblem::PayloadLimitExceeded);
        }
        Ok(Self {
            schema,
            bytes: std::sync::Arc::new(bytes),
        })
    }

    /// Borrow the schema identifier supplied at construction.
    #[must_use]
    /// Borrow the exact interpretation label without normalization.
    pub fn schema(&self) -> &str {
        &self.schema
    }
    /// Borrow the shared payload bytes without transferring ownership.
    #[must_use]
    /// Borrow the original bytes; callers still own schema-specific validation and secret handling.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Principal identity and an owned, bounded set of exact capability grants.
///
/// Grant membership does not imply a typed-resource authorization decision.
pub struct Principal {
    id: PrincipalId,
    grants: BTreeSet<CapabilityId>,
}

impl Principal {
    /// Take ownership of an identity and grant set, rejecting an excessive grant count.
    ///
    /// The identity is already typed; no external authorization service is consulted.
    pub fn new(
        id: PrincipalId,
        grants: BTreeSet<CapabilityId>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        if grants.len() > limits.max_capabilities {
            return Err(InvocationProblem::CapabilityLimitExceeded);
        }
        Ok(Self { id, grants })
    }

    /// Borrow the principal identity.
    #[must_use]
    /// Borrow the identity used for effect and audit attribution.
    pub fn id(&self) -> &PrincipalId {
        &self.id
    }
    /// Borrow the exact set of capability grants.
    #[must_use]
    /// Borrow the exact grants; there is no wildcard or prefix implication.
    pub fn grants(&self) -> &BTreeSet<CapabilityId> {
        &self.grants
    }
    /// Check exact set membership for a capability without wildcard expansion.
    #[must_use]
    /// Check exact set membership, independently of resource authorization and provider readiness.
    pub fn has_grant(&self, capability: &CapabilityId) -> bool {
        self.grants.contains(capability)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Scheduling category carried by an invocation.
///
/// This enum does not implement scheduling or assign priority ordering.
pub enum ServiceClass {
    /// Work classified for interactive service.
    Interactive,
    /// Work classified for batch service.
    Batch,
    /// Work classified for compilation service.
    Compiler,
    /// Work classified as blocking service.
    Blocking,
    /// Work classified for system service.
    System,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned cancellation record with caller-supplied identity, reason, and logical tick.
///
/// The constructor bounds the reason; public fields can subsequently be changed.
pub struct Cancellation {
    /// Identity of the cancellation request.
    pub id: CancellationId,
    /// Nonempty reason bounded by `max_binding_bytes` at construction.
    pub reason: String,
    /// Logical tick supplied by the requester; zero is not rejected here.
    pub requested_at_tick: u64,
}

#[derive(Clone, Default)]
/// Shared, one-way cancellation signal for live execution control.
///
/// Clones observe the same atomic flag, and equality compares flag identity rather
/// than the requested value. A request uses release ordering and observation uses
/// acquire ordering. Requesting cancellation does not itself stop a machine.
pub struct CancellationProbe(Arc<AtomicBool>);

impl CancellationProbe {
    /// Create an independent probe whose cancellation flag is initially false.
    #[must_use]
    /// Allocate an independent, initially unrequested flag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the shared flag permanently; repeated requests leave it set.
    pub fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Observe whether this probe or any clone has requested cancellation.
    #[must_use]
    /// Observe the current shared request with acquire ordering.
    pub fn is_requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl fmt::Debug for CancellationProbe {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancellationProbe")
            .field("requested", &self.is_requested())
            .finish_non_exhaustive()
    }
}

impl PartialEq for CancellationProbe {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CancellationProbe {}

impl Cancellation {
    /// Construct a record with a nonempty reason within the binding byte bound.
    ///
    /// The logical tick is retained unchanged and is not validated here.
    pub fn new(
        id: CancellationId,
        reason: impl Into<String>,
        requested_at_tick: u64,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        let reason = reason.into();
        if reason.is_empty() || reason.len() > limits.max_binding_bytes {
            return Err(InvocationProblem::InvalidCancellation);
        }
        Ok(Self {
            id,
            reason,
            requested_at_tick,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned admission context connecting an execution to its artifact and controls.
///
/// The constructor checks attempt, deadline, resource budgets, and binding bounds.
/// Public fields permit later mutation, so those checks are construction-time
/// guarantees. Payload bytes and live cancellation probes remain shared on clone.
pub struct Invocation {
    /// Identity of the originating request.
    pub request_id: RequestId,
    /// Stable identity of this execution.
    pub execution_id: ExecutionId,
    /// Identity of the runtime unit associated with this execution.
    pub run_unit_id: RunUnitId,
    /// Parent execution identity when this invocation is a child.
    pub parent_execution_id: Option<ExecutionId>,
    /// Program or entry selector to execute.
    pub selector: Selector,
    /// Artifact reference paired with the selector.
    pub artifact: ArtifactRef,
    /// Caller identity and capability grants used by execution admission.
    pub principal: Principal,
    /// Requested scheduling category.
    pub service_class: ServiceClass,
    /// Caller-supplied scheduling priority; the constructor accepts any `u8`.
    pub priority: u8,
    /// Nonzero logical deadline checked by the constructor.
    pub deadline_tick: u64,
    /// Trace identity used to initialize audit correlation.
    pub trace_id: TraceId,
    /// Caller-supplied identity for idempotency handling by consumers.
    pub idempotency_key: IdempotencyKey,
    /// Positive execution or delivery attempt checked at construction.
    pub attempt: u32,
    /// Validated positive execution budgets.
    pub limits: ResourceLimits,
    /// Named inputs, bounded by count, name length, and each payload length.
    pub bindings: BTreeMap<String, BoundedPayload>,
    /// Attached cancellation record; its presence counts as a cancellation request.
    pub cancellation: Option<Cancellation>,
    /// Optional shared flag for cancellation requested after construction.
    pub cancellation_probe: Option<CancellationProbe>,
    /// Provider generation pins; enrichment requires a grant for every capability.
    pub provider_generations: BTreeMap<CapabilityId, String>,
    /// Audit correlation initialized from the trace identity.
    pub audit_correlation: String,
}

impl Invocation {
    /// Construct an invocation after checking nonzero attempt and deadline,
    /// positive resource budgets, and bounded bindings.
    ///
    /// Cancellation and provider pins start empty. Audit correlation starts with
    /// the trace identity. This does not schedule work or dispatch a provider.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: RequestId,
        execution_id: ExecutionId,
        run_unit_id: RunUnitId,
        parent_execution_id: Option<ExecutionId>,
        selector: Selector,
        artifact: ArtifactRef,
        principal: Principal,
        service_class: ServiceClass,
        priority: u8,
        deadline_tick: u64,
        trace_id: TraceId,
        idempotency_key: IdempotencyKey,
        attempt: u32,
        resource_limits: ResourceLimits,
        bindings: BTreeMap<String, BoundedPayload>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        if attempt == 0 {
            return Err(InvocationProblem::InvalidAttempt);
        }
        if deadline_tick == 0 {
            return Err(InvocationProblem::InvalidDeadline);
        }
        if bindings.len() > limits.max_bindings
            || bindings.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > limits.max_binding_bytes
                    || value.bytes.len() > limits.max_payload_bytes
            })
        {
            return Err(InvocationProblem::BindingLimitExceeded);
        }
        let audit_correlation = trace_id.as_str().to_string();
        Ok(Self {
            request_id,
            execution_id,
            run_unit_id,
            parent_execution_id,
            selector,
            artifact,
            principal,
            service_class,
            priority,
            deadline_tick,
            trace_id,
            idempotency_key,
            attempt,
            limits: resource_limits.validate()?,
            bindings,
            cancellation: None,
            cancellation_probe: None,
            provider_generations: BTreeMap::new(),
            audit_correlation,
        })
    }

    /// Replace the generation pins after checking their count and byte lengths.
    ///
    /// Every pinned capability must be granted to the principal, and every
    /// generation must be nonempty. Provider availability is not checked here.
    pub fn with_provider_generations(
        mut self,
        generations: BTreeMap<CapabilityId, String>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        if generations.len() > limits.max_capabilities
            || generations.iter().any(|(capability, generation)| {
                !self.principal.has_grant(capability)
                    || generation.is_empty()
                    || generation.len() > limits.max_identity_bytes
            })
        {
            return Err(InvocationProblem::InvalidProviderGeneration);
        }
        self.provider_generations = generations;
        Ok(self)
    }

    /// Replace the attached cancellation record without revalidating its fields.
    #[must_use]
    /// Attach a retained request; this marks cancellation regardless of its observation tick.
    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    /// Replace the live cancellation probe, retaining its shared signal identity.
    #[must_use]
    /// Attach the exact shared live probe, without replacing any retained request.
    pub fn with_cancellation_probe(mut self, cancellation_probe: CancellationProbe) -> Self {
        self.cancellation_probe = Some(cancellation_probe);
        self
    }

    /// Return true for any attached cancellation record or a requested live probe.
    ///
    /// This observes control state without stopping work or resolving an outcome.
    #[must_use]
    /// Observe either retained cancellation or the live flag; deadline checks remain separate.
    pub fn cancellation_requested(&self) -> bool {
        self.cancellation.is_some()
            || self
                .cancellation_probe
                .as_ref()
                .is_some_and(CancellationProbe::is_requested)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Construction or enrichment failure for a bounded invocation value.
pub enum InvocationProblem {
    /// At least one execution budget is zero.
    InvalidResourceLimits,
    /// The payload schema identifier is empty or exceeds the identity byte bound.
    InvalidPayloadSchema,
    /// A payload exceeds the per-payload byte bound.
    PayloadLimitExceeded,
    /// The principal has more capability grants than permitted.
    CapabilityLimitExceeded,
    /// The cancellation reason is empty or exceeds the binding byte bound.
    InvalidCancellation,
    /// The invocation attempt is zero.
    InvalidAttempt,
    /// The invocation deadline tick is zero.
    InvalidDeadline,
    /// Too many bindings, an empty or oversized name, or an oversized value.
    BindingLimitExceeded,
    /// Too many generation pins, a missing grant, or an empty or oversized pin.
    InvalidProviderGeneration,
}

impl fmt::Display for InvocationProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invocation contract failed: {self:?}")
    }
}
impl std::error::Error for InvocationProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityProblem;

    #[test]
    fn zero_attempt_and_unbounded_payload_fail() {
        let limits = InvocationLimits {
            max_payload_bytes: 1,
            ..InvocationLimits::default()
        };
        assert_eq!(
            BoundedPayload::new("test@1", vec![1, 2], limits),
            Err(InvocationProblem::PayloadLimitExceeded)
        );
        assert_eq!(
            PrincipalId::new("bad space", limits),
            Err(IdentityProblem::Invalid)
        );
    }

    #[test]
    fn grants_are_exact_and_bounded() {
        let limits = InvocationLimits {
            max_capabilities: 1,
            ..InvocationLimits::default()
        };
        let id = PrincipalId::new("IBMUSER", limits).unwrap();
        let read = CapabilityId::new("host.dataset.read", limits).unwrap();
        let principal = Principal::new(id, BTreeSet::from([read.clone()]), limits).unwrap();
        assert!(principal.has_grant(&read));
        let other = CapabilityId::new("host.dataset.write", limits).unwrap();
        assert!(!principal.has_grant(&other));
    }

    #[test]
    fn secret_payload_clone_shares_zeroizing_bytes_and_debug_is_redacted() {
        let secret = BoundedPayload::new(
            "mainframe-env.cics.secret@1",
            b"ONLY-IN-MEMORY".to_vec(),
            InvocationLimits::default(),
        )
        .unwrap();
        let clone = secret.clone();
        assert_eq!(secret.bytes().as_ptr(), clone.bytes().as_ptr());
        assert_eq!(secret, clone);
        let shown = format!("{secret:?}");
        assert!(shown.contains("<redacted>"));
        assert!(!shown.contains("ONLY-IN-MEMORY"));
    }
}
