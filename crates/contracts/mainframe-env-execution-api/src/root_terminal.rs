//! Native root lifecycle audit subjects, separate from application host effects.
use crate::{AuditDecision, CapabilityId, ExecutionId, IdempotencyKey, PrincipalId, RunUnitId};

/// Versioned terminal audit subject; old effect-audit bytes retain their meaning.
pub const ROOT_TERMINAL_AUDIT_CONTRACT: &str = "mainframe-env.root-terminal-audit@1";

/// A source-backed known native root disposition supplied by its real driver.
/// This value alone does not attest host admission, finality or a store lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootTerminalDisposition {
    /// The actual native compiled root completed normally.
    Normal {
        /// The original machine's completion code, without MQ status inference.
        return_code: i32,
    },
    /// The real native root produced a modeled ABEND after descendant closure.
    KnownAbnormal,
}

/// Credential-safe digest in the independent canonical root-terminal domain.
/// Only the shared host canonical encoder defines its preimage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootTerminalResourceDigest {
    /// SHA-256 of the bounded canonical terminal resource identity.
    pub value: [u8; 32],
}

/// Separate actual observations sharing one terminal commit, not duplicate effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootTerminalAuditRole {
    /// The provider's actual authorized local-unit settlement decision.
    ProviderSettlement,
    /// The coordinator's actual closure/terminal lifecycle observation.
    CoreClosure,
}

/// Typed security observation for terminal settlement, never an effect sequence.
/// It is not a SAF permit or a substitute for the live coordinator winner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootTerminalAudit {
    /// Distinct provider or core observation; both are required and retained.
    pub role: RootTerminalAuditRole,
    /// Exact native root actor.
    pub execution_id: ExecutionId,
    /// Original logical run identity.
    pub run_unit_id: RunUnitId,
    /// Positive original execution attempt.
    pub attempt: u32,
    /// Actual terminal lifecycle sequence in the owning core journal.
    pub lifecycle_sequence: u64,
    /// Actual finite logical observation tick.
    pub observed_tick: u64,
    /// Original authenticated principal.
    pub principal: PrincipalId,
    /// Original root invocation key, not a fabricated effect key.
    pub invocation_key: IdempotencyKey,
    /// Capability whose resolved resources were evaluated.
    pub capability: CapabilityId,
    /// Shared canonical identity of exact terminal dependencies and decision.
    pub resource: RootTerminalResourceDigest,
    /// Existing closed decision vocabulary; never an authorization grant.
    pub decision: AuditDecision,
}

/// Additive typed audit reader output under the one durable audit authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditSubjectRecord {
    /// Existing version-one original effect decision.
    Effect(crate::AuditRecord),
    /// A native root terminal observation with its own lifecycle identity.
    RootTerminal(RootTerminalAudit),
}
