use crate::{CapabilityId, ExecutionId, IdempotencyKey, PrincipalId, RunUnitId};

/// Versioned contract for security-relevant host decision records.
pub const AUDIT_RECORD_CONTRACT: &str = "mainframe-env.audit-record@1";

/// A closed, machine-readable classification of a host decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditDecision {
    /// The authorized host operation completed successfully.
    Success,
    /// Authorization denied access before provider dispatch.
    Deny,
    /// Cancellation won before the operation completed.
    Cancelled,
    /// The operation exceeded its declared deadline.
    TimedOut,
    /// Input validation or another pre-dispatch policy rejected the operation.
    Rejected,
    /// The selected provider could not complete the operation.
    ProviderFailure,
    /// Host infrastructure failed while invoking the provider.
    InfrastructureFailure,
    /// A mutation may have committed, but its durable outcome is unavailable.
    UnknownOutcome,
}

/// The canonical encoding and digest algorithm used for an audited resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditResourceDigestFormat {
    /// SHA-256 over explicit host-request canonical bytes in the audit-resource v1 domain.
    CanonicalHostResourceV1,
    /// SHA-256 over the canonical capability in the bounded oversized-resource v1 domain.
    CanonicalHostOversizedResourceV1,
}

/// An opaque, credential-safe resource identity suitable for durable audit storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditResourceDigest {
    /// Versioned canonical domain and algorithm used to produce `value`.
    pub format: AuditResourceDigestFormat,
    /// Credential-safe SHA-256 resource identity.
    pub value: [u8; 32],
}

/// The mandatory typed boundary between a host decision and durable audit storage.
///
/// The shape deliberately excludes free-form fields and request payloads. Every variable
/// identity is already bounded by the execution contract, while the resource itself is retained
/// only as a versioned digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRecord {
    /// Durable execution associated with the decision.
    pub execution_id: ExecutionId,
    /// Runtime instance that requested the host effect.
    pub run_unit_id: RunUnitId,
    /// Positive execution attempt that observed the decision.
    pub attempt: u32,
    /// Monotonic host-effect sequence within the execution.
    pub effect_sequence: u64,
    /// Logical clock tick at which the decision was observed.
    pub observed_tick: u64,
    /// Authenticated principal evaluated by the host boundary.
    pub principal: PrincipalId,
    /// Invocation identity used to correlate and deduplicate the decision.
    pub invocation_key: IdempotencyKey,
    /// Capability selected for authorization and provider routing.
    pub capability: CapabilityId,
    /// Opaque canonical identity of the protected resource.
    pub resource: AuditResourceDigest,
    /// Closed outcome classification for the security decision.
    pub decision: AuditDecision,
}
