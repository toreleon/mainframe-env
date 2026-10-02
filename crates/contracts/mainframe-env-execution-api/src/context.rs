use crate::{
    ArtifactRef, CancellationId, CapabilityId, ExecutionId, IdempotencyKey, PrincipalId, RequestId,
    RunUnitId, Selector, TraceId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Construction bounds for owned identities, grants and payloads.
/// Limits are byte/count ceilings, not authorization or runtime resource reservations.
pub struct InvocationLimits {
    /// Maximum UTF-8 bytes in identities and payload schema names.
    pub max_identity_bytes: usize,
    /// Maximum distinct grants or pinned provider-generation entries.
    pub max_capabilities: usize,
    /// Maximum raw bytes in each payload, including each invocation binding.
    pub max_payload_bytes: usize,
    /// Maximum number of binding entries in an invocation.
    pub max_bindings: usize,
    /// Maximum UTF-8 bytes in binding names and cancellation reason text.
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
/// Positive invocation-wide budgets enforced by the execution owner.
/// A per-drive [`crate::Quantum`] narrows work scheduling without replacing these budgets.
pub struct ResourceLimits {
    /// Total machine steps allowed for the invocation.
    pub max_steps: u64,
    /// Maximum owned machine storage in bytes.
    pub max_storage_bytes: u64,
    /// Maximum returned application output in bytes.
    pub max_output_bytes: u64,
    /// Maximum active call-frame depth.
    pub max_frames: u32,
    /// Maximum host-effect occurrences within the invocation.
    pub max_effects: u64,
    /// Maximum lifecycle observations within the invocation.
    pub max_events: u64,
}

impl ResourceLimits {
    /// Reject any zero budget; this does not reserve capacity or prove available headroom.
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
/// Schema-labelled bounded bytes, shared by clones and zeroized on final release.
///
/// The schema identifies interpretation; construction does not parse or validate
/// the body against that schema. Secret-schema Debug output is redacted.
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
    /// Take ownership of bytes after checking nonempty bounded schema and per-payload size.
    /// The body may be empty; schema characters are not restricted to the identity alphabet.
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

    #[must_use]
    /// Borrow the exact interpretation label without normalization.
    pub fn schema(&self) -> &str {
        &self.schema
    }
    #[must_use]
    /// Borrow the original bytes; callers still own schema-specific validation and secret handling.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Bounded principal identity and exact capability grants supplied by trusted admission.
/// Constructing this value does not authenticate a caller or grant resource-specific SAF access.
pub struct Principal {
    id: PrincipalId,
    grants: BTreeSet<CapabilityId>,
}

impl Principal {
    /// Store a validated identity and deduplicated grants within the capability count ceiling.
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

    #[must_use]
    /// Borrow the identity used for effect and audit attribution.
    pub fn id(&self) -> &PrincipalId {
        &self.id
    }
    #[must_use]
    /// Borrow the exact grants; there is no wildcard or prefix implication.
    pub fn grants(&self) -> &BTreeSet<CapabilityId> {
        &self.grants
    }
    #[must_use]
    /// Check exact set membership, independently of resource authorization and provider readiness.
    pub fn has_grant(&self, capability: &CapabilityId) -> bool {
        self.grants.contains(capability)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// Scheduling classification, independent of capability and resource permission.
pub enum ServiceClass {
    /// Latency-sensitive interactive work.
    Interactive,
    /// Queued application batch work.
    Batch,
    /// Compilation work under its own scheduling policy.
    Compiler,
    /// Work expected to block outside deterministic machine steps.
    Blocking,
    /// Product infrastructure work, without implicit elevation of grants.
    System,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Retained cancellation observation; attaching it immediately marks an invocation cancelled.
pub struct Cancellation {
    /// Stable identity for the cancellation request.
    pub id: CancellationId,
    /// Nonempty bounded explanation, not executable control text.
    pub reason: String,
    /// Request observation in the caller's logical tick domain; construction permits zero.
    pub requested_at_tick: u64,
}

#[derive(Clone, Default)]
/// One-way live cancellation flag shared by clones, with release/acquire visibility.
///
/// Equality compares shared flag identity, not its current Boolean value. The
/// probe is process-local control and supplies no durable cancellation receipt.
///
/// ```
/// use mainframe_env_execution_api::CancellationProbe;
/// let probe = CancellationProbe::new();
/// let child = probe.clone();
/// assert_eq!(probe, child);
/// assert_ne!(probe, CancellationProbe::new());
/// child.request();
/// assert!(probe.is_requested());
/// ```
pub struct CancellationProbe(Arc<AtomicBool>);

impl CancellationProbe {
    #[must_use]
    /// Allocate an independent, initially unrequested flag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the shared flag permanently; repeated requests are harmless.
    pub fn request(&self) {
        self.0.store(true, Ordering::Release);
    }

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
    /// Check nonempty bounded reason text; the supplied logical tick is retained verbatim.
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
/// Owned admitted-call context, carrying attribution and finite execution controls.
///
/// Constructors check shape/bounds, not authentication, artifact availability,
/// host topology or lifecycle ownership. Public fields can be changed by callers;
/// trusted boundaries must validate/freeze the actual context they consume.
pub struct Invocation {
    /// Request correlation identity, distinct from durable execution identity.
    pub request_id: RequestId,
    /// Durable execution actor used by journal, audit and recovery.
    pub execution_id: ExecutionId,
    /// Runtime instance associated with this occurrence.
    pub run_unit_id: RunUnitId,
    /// Original parent linkage; presence alone does not attest same-task topology.
    pub parent_execution_id: Option<ExecutionId>,
    /// Logical program/service selector admitted for execution.
    pub selector: Selector,
    /// Exact compiled artifact reference; resolution and compatibility remain external checks.
    pub artifact: ArtifactRef,
    /// Admitted principal and exact grants; resource authorization is separate.
    pub principal: Principal,
    /// Scheduling class, without permission implications.
    pub service_class: ServiceClass,
    /// Scheduler priority in its owned ordering domain.
    pub priority: u8,
    /// Positive absolute deadline in the embedding's logical tick domain, not wall-clock seconds.
    pub deadline_tick: u64,
    /// End-to-end tracing identity, also the initial audit correlation.
    pub trace_id: TraceId,
    /// Original invocation deduplication identity; effect keys have their own occurrence binding.
    pub idempotency_key: IdempotencyKey,
    /// Positive execution attempt, preserved across host attribution.
    pub attempt: u32,
    /// Invocation-wide positive runtime budgets.
    pub limits: ResourceLimits,
    /// Bounded application/configuration payloads; equal binding bytes do not mint trusted authority.
    pub bindings: BTreeMap<String, BoundedPayload>,
    /// Retained cancellation observation, if a request is already known.
    pub cancellation: Option<Cancellation>,
    /// Shared live control observed in addition to the retained cancellation.
    pub cancellation_probe: Option<CancellationProbe>,
    /// Exact generation pins for granted capabilities, selected independently of request bindings.
    pub provider_generations: BTreeMap<CapabilityId, String>,
    /// Audit correlation text initially copied from `trace_id` by [`Self::new`].
    pub audit_correlation: String,
}

impl Invocation {
    /// Check positive attempt/deadline, positive budgets and bounded bindings.
    ///
    /// Cancellation and generation pins start absent; audit correlation starts
    /// at the trace identity. This does not check whether the deadline has elapsed.
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

    /// Replace pins after checking count, nonempty bounded generation names and exact grants.
    /// Does not resolve a generation or attest that the selected provider is available.
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

    #[must_use]
    /// Attach a retained request; this marks cancellation regardless of its observation tick.
    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }

    #[must_use]
    /// Attach the exact shared live probe, without replacing any retained request.
    pub fn with_cancellation_probe(mut self, cancellation_probe: CancellationProbe) -> Self {
        self.cancellation_probe = Some(cancellation_probe);
        self
    }

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
/// Construction rejection for malformed or over-budget invocation data.
pub enum InvocationProblem {
    /// At least one runtime resource budget is zero.
    InvalidResourceLimits,
    /// The interpretation label is empty or exceeds the identity-byte ceiling.
    InvalidPayloadSchema,
    /// A raw payload exceeds its per-payload byte ceiling.
    PayloadLimitExceeded,
    /// Distinct grants exceed the admitted capability count.
    CapabilityLimitExceeded,
    /// Cancellation reason text is empty or exceeds its byte ceiling.
    InvalidCancellation,
    /// Execution attempt is zero.
    InvalidAttempt,
    /// Absolute logical deadline is zero.
    InvalidDeadline,
    /// Binding count, names or individual payloads exceed their construction bounds.
    BindingLimitExceeded,
    /// Generation pins exceed bounds, are empty, or refer to an ungranted capability.
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
