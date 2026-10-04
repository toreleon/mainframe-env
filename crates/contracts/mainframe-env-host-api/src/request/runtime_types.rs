//! Runtime, spool, terminal, security and host-state owned records.

use super::{AccessIntent, Mutation, SecretRef};
use crate::{JobName, ResourceName, RuntimeServiceName, SessionId};
use mainframe_env_execution_api::PrincipalId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
/// Explicit runtime routing domain, separate from an application program name.
pub enum RuntimeServiceKind {
    /// Route to a language-environment service.
    LanguageEnvironment,
    /// Route to an explicit host extension service.
    HostExtension,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Named runtime service and nonzero ABI revision; selection is not provider installation or authorization.
pub struct RuntimeServiceSelector {
    /// Runtime routing domain.
    pub kind: RuntimeServiceKind,
    /// Validated service identity within that domain.
    pub name: RuntimeServiceName,
    /// Positive service ABI revision; zero is malformed.
    pub abi_version: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Job-scoped spool operations with mutation identity for writes and bounded record reads.
pub enum SpoolRequest {
    /// Append bounded records to a job-owned spool file.
    Append {
        /// Job-scoped spool owner name; not an invocation principal.
        job: JobName,
        /// Bounded nonempty ASCII spool file label without control characters.
        file: String,
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Observe the job file list.
    List {
        /// Job-scoped spool owner name; not an invocation principal.
        job: JobName,
    },
    /// Read a positive bounded page from a zero-based record index.
    Read {
        /// Job-scoped spool owner name; not an invocation principal.
        job: JobName,
        /// Bounded nonempty ASCII spool file label without control characters.
        file: String,
        /// Zero-based starting spool record index.
        start: u64,
        /// Requested record/listing ceiling bounded by HostLimits.max_records.
        max_records: u32,
    },
    /// Close a spool file to further append.
    Seal {
        /// Job-scoped spool owner name; not an invocation principal.
        job: JobName,
        /// Bounded nonempty ASCII spool file label without control characters.
        file: String,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request job artifact removal; completion may remain pending.
    Purge {
        /// Job-scoped spool owner name; not an invocation principal.
        job: JobName,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
}

/// Stable spool request schema identity; retained separately from result identity.
pub const SPOOL_REQUEST_CONTRACT: &str = "mainframe-env.spool-request@2";
/// Stable spool result schema identity, including unfinished purge observations.
pub const SPOOL_RESULT_CONTRACT: &str = "mainframe-env.spool-result@2";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
/// Retained spool file counters and seal state observed at one positive provider version.
pub struct SpoolFileSummary {
    /// Retained spool file label.
    pub file: String,
    /// Observed logical record count.
    pub record_count: u64,
    /// Observed total retained payload bytes.
    pub byte_count: u64,
    /// True when append is closed by the provider.
    pub sealed: bool,
    /// Observed provider revision, distinct from a schema version or execution sequence.
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
/// Spool records and mutation receipts, including a distinct unfinished purge outcome.
pub enum SpoolResult {
    /// Positive-version mutation receipt, preserving replay observation.
    Mutated {
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
        /// True when this mutation receipt was retained from the original replay identity.
        replayed: bool,
    },
    /// Bounded file summaries for one job.
    Files {
        /// Bounded file summaries for the job.
        files: Vec<SpoolFileSummary>,
    },
    /// Bounded record page and observed file version.
    Records {
        /// Owned record bytes retained in order without text decoding or padding changes.
        records: Vec<Vec<u8>>,
        /// True when the provider reports additional entries beyond this bounded page.
        more: bool,
        /// Observed provider revision, distinct from a schema version or execution sequence.
        version: u64,
    },
    /// Removal remains unfinished with a positive retained artifact count.
    PurgePending {
        /// Positive number still requiring purge; this is not completed deletion.
        remaining_artifacts: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Owned terminal field bytes and display coordinates; secret metadata must survive handling without exposing values.
pub struct TerminalField {
    /// Nonempty bounded field identity within the terminal session.
    pub name: String,
    /// Display row coordinate interpreted by the terminal provider.
    pub row: u16,
    /// Display column coordinate interpreted by the terminal provider.
    pub column: u16,
    /// Declared field capacity in bytes; value cannot exceed it.
    pub length: u16,
    /// Retained modified-data indication, separate from secret classification.
    pub modified: bool,
    /// Mark data requiring secret-aware presentation and audit handling.
    pub secret: bool,
    /// Exact field bytes bounded by declared capacity and max_record_bytes.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Session-scoped terminal operations; rendering and session authority remain with the provider.
pub enum TerminalRequest {
    /// Request a display session with explicit geometry.
    Open {
        /// Validated terminal session identity; ownership is checked by the provider.
        session: SessionId,
        /// Requested display row count.
        rows: u16,
        /// Requested display column count.
        columns: u16,
    },
    /// Apply bounded field updates and optional display controls.
    Write {
        /// Validated terminal session identity; ownership is checked by the provider.
        session: SessionId,
        /// Request display erasure before applying these fields.
        erase: bool,
        /// Optional display row/column position, not a provider cursor token.
        cursor: Option<(u16, u16)>,
        /// Bounded owned terminal field updates.
        fields: Vec<TerminalField>,
    },
    /// Observe the selected session output.
    Read {
        /// Validated terminal session identity; ownership is checked by the provider.
        session: SessionId,
    },
    /// Supply application input fields and attention byte.
    Input {
        /// Validated terminal session identity; ownership is checked by the provider.
        session: SessionId,
        /// Application attention identifier retained as one byte.
        aid: u8,
        /// Bounded owned terminal field updates.
        fields: Vec<TerminalField>,
    },
    /// Release the selected session resources.
    Release {
        /// Validated terminal session identity; ownership is checked by the provider.
        session: SessionId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Credential, principal, resource-access and audit requests routed to the installed security authority.
pub enum SecurityRequest {
    /// Authenticate using an external credential reference.
    Authenticate {
        /// Principal whose credential is to be authenticated.
        user: PrincipalId,
        /// External credential locator; no credential bytes enter this request.
        credential_reference: SecretRef,
    },
    /// Validate a non-login execution identity from durable security state.
    /// This request never carries or resolves a credential.
    /// It is distinct from credential authentication.
    ValidatePrincipal {
        /// Principal to validate or authorize; the typed identity alone is not trusted authentication.
        principal: PrincipalId,
    },
    /// Request an explicit resource/access decision for a principal.
    Authorize {
        /// Principal to validate or authorize; the typed identity alone is not trusted authentication.
        principal: PrincipalId,
        /// Security resource class spelling consumed by the installed authority.
        class: String,
        /// Validated resource name for the explicit access decision.
        resource: ResourceName,
        /// Requested access level, not a preapproved permission.
        intent: AccessIntent,
    },
    /// Submit an application audit observation; it grants no access.
    Audit(AuditEvent),
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Security authority observation; denial, absence and identity failures remain distinct from permission.
pub enum SecurityDecision {
    /// The authority admitted the requested decision.
    Allow,
    /// The authority refused the requested access.
    Deny,
    /// The required identity/resource was absent.
    NotFound,
    /// Credential verification failed.
    InvalidCredentials,
    /// The identity or credential is expired.
    Expired,
    /// The identity or credential is revoked.
    Revoked,
    /// The identity is locked.
    Locked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Application audit data with a resource digest; it is neither a grant nor a coordinator completion receipt.
pub struct AuditEvent {
    /// Audit operation label.
    pub action: String,
    /// Resource digest supplied by the caller; not a plaintext credential or capability grant.
    pub resource_hash: String,
    /// Decision classification retained for audit interpretation.
    pub decision: String,
    /// Bounded application audit key/value metadata.
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Version-aware host state access; mutation requests retain their original replay identity.
pub enum StateRequest {
    /// Observe the key value and provider version.
    Get {
        /// Provider state key; value shape and namespace ownership are provider-defined.
        key: String,
    },
    /// Request an owned value publication under an optional version precondition.
    Put {
        /// Provider state key; value shape and namespace ownership are provider-defined.
        key: String,
        /// Owned state bytes, bounded by max_state_bytes.
        value: Vec<u8>,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
    /// Request key removal under an optional version precondition.
    Delete {
        /// Provider state key; value shape and namespace ownership are provider-defined.
        key: String,
        /// Optional provider version precondition; interpretation and conflict handling belong to the provider.
        expected_version: Option<u64>,
        /// Original replay identity required before mutation dispatch; it does not confer permission.
        mutation: Mutation,
    },
}
