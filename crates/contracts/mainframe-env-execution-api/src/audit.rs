use crate::{CapabilityId, ExecutionId, IdempotencyKey, PrincipalId, RunUnitId};

/// Versioned contract for security-relevant host decision records.
pub const AUDIT_RECORD_CONTRACT: &str = "mainframe-env.audit-record@1";

/// A closed, machine-readable classification of a host decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditDecision {
    Success,
    Deny,
    Cancelled,
    TimedOut,
    Rejected,
    ProviderFailure,
    InfrastructureFailure,
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
    pub format: AuditResourceDigestFormat,
    pub value: [u8; 32],
}

/// The mandatory typed boundary between a host decision and durable audit storage.
///
/// The shape deliberately excludes free-form fields and request payloads. Every variable
/// identity is already bounded by the execution contract, while the resource itself is retained
/// only as a versioned digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditRecord {
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub attempt: u32,
    pub effect_sequence: u64,
    pub observed_tick: u64,
    pub principal: PrincipalId,
    pub invocation_key: IdempotencyKey,
    pub capability: CapabilityId,
    pub resource: AuditResourceDigest,
    pub decision: AuditDecision,
}
