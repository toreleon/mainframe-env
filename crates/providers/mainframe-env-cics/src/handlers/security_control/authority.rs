//! Narrow bridge from CICS application security commands to the installed SAF authority.

use crate::service::CicsService;
use mainframe_env_execution_api::{BoundedPayload, PrincipalId};
use mainframe_env_host_api::HostProblem;
use std::sync::Arc;

/// The information CICS needs to translate a SAF access check into QUERY SECURITY results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsSecurityAccess {
    /// SAF access rank: none, execute, read, update, control, or alter (zero through five).
    pub granted_rank: u8,
    /// The SAF decision category, including absent class/profile and inactive principal.
    pub reason: CicsSecurityAccessReason,
}

/// Bounded SAF decision categories exposed to the CICS command adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsSecurityAccessReason {
    /// The requested access was granted.
    Granted,
    /// The resource exists, but access was denied.
    Denied,
    /// The named class is unavailable to SAF.
    ClassInactive,
    /// No matching profile protects the resource.
    ProfileNotFound,
    /// The target principal does not exist.
    PrincipalNotFound,
    /// The target principal is revoked, locked, or otherwise inactive.
    PrincipalInactive,
    /// The security subsystem cannot evaluate the request.
    PolicyUnavailable,
}

/// Credential field selected by a CICS VERIFY command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsCredentialKind {
    /// Standard password verification.
    Password,
    /// Password or phrase verification according to the supplied length.
    Phrase,
}

/// Borrowed, nonpersisted verification input passed to the installed SAF bridge.
pub struct CicsCredentialRequest<'a> {
    /// The immutable issuing task principal.
    pub actor: &'a PrincipalId,
    /// The user ID whose credential is verified.
    pub user: &'a PrincipalId,
    /// Borrowed clear credential; never serialized into a provider row.
    pub credential: &'a [u8],
    /// Standard password or length-selected phrase verification.
    pub kind: CicsCredentialKind,
    /// Optional RACF group connection to check in the same transition.
    pub group: Option<&'a str>,
    /// Canonical digest of the complete CICS request, including its secret bytes.
    pub binding_digest: [u8; 32],
    /// Stable bounded key for durable SAF replay.
    pub idempotency_key: &'a str,
    /// Redacted audit correlation identity.
    pub correlation: &'a str,
    /// Observed finite logical time for this attempt.
    pub tick: u64,
}

/// Borrowed old and new credentials for one atomic SAF change transition.
pub struct CicsCredentialChangeRequest<'a> {
    /// Immutable task principal recorded in the SAF audit.
    pub actor: &'a PrincipalId,
    /// User whose credential changes after old-secret verification.
    pub user: &'a PrincipalId,
    /// Current clear credential, held only for this call.
    pub current: &'a [u8],
    /// Proposed clear credential, held only for this call.
    pub proposed: &'a [u8],
    /// Standard password or length-selected phrase mode.
    pub kind: CicsCredentialKind,
    /// Optional group connection checked before the verifier is replaced.
    pub group: Option<&'a str>,
    /// Digest of the complete canonical CICS request.
    pub binding_digest: [u8; 32],
    /// Durable replay identity.
    pub idempotency_key: &'a str,
    /// Redacted SAF audit correlation.
    pub correlation: &'a str,
    /// Observed finite logical time.
    pub tick: u64,
}

/// Borrowed CICS PassTicket issuance request bound to one durable SAF effect.
pub struct CicsPassTicketRequest<'a> {
    /// Task principal for which the ticket is issued.
    pub actor: &'a PrincipalId,
    /// Destination ESM application profile.
    pub application: &'a str,
    /// Digest of the complete canonical CICS request.
    pub binding_digest: [u8; 32],
    /// Durable replay identity.
    pub idempotency_key: &'a str,
    /// Redacted SAF audit correlation.
    pub correlation: &'a str,
    /// Observed finite logical time.
    pub tick: u64,
}

/// Source-distinct PassTicket issuance failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsPassTicketFailure {
    /// The task is running under a default or credentialless principal.
    DefaultUser,
    /// The issuing region lacks SAF generation authority.
    RegionDenied,
    /// The user/application pair lacks a PTKTDATA grant.
    TargetDenied,
    /// The external security interface is inactive.
    SecurityUnavailable,
    /// PassTicket generation is not available from the active policy.
    Unsupported,
}

/// Redacted ticket or denial returned by the installed RACF adapter.
pub struct CicsPassTicketOutcome {
    /// Secret-tagged ticket output, present only on success.
    pub ticket: Option<BoundedPayload>,
    /// Source-distinct failure, absent on success.
    pub failure: Option<CicsPassTicketFailure>,
    /// ESM response code.
    pub esm_response: i64,
    /// ESM reason code.
    pub esm_reason: i64,
}

/// Source-distinct SAF credential failures used for CICS condition translation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsCredentialFailure {
    /// Unknown user ID.
    UnknownUser,
    /// Revoked, suspended, or locked user ID.
    Revoked,
    /// The required credential is expired or missing.
    NewCredentialRequired,
    /// The supplied credential did not verify.
    InvalidCredential,
    /// The proposed new credential violates the SAF password or phrase policy.
    UnacceptableNewCredential,
    /// Old and new phrase lengths select different credential fields.
    MismatchedCredentialKind,
    /// The requested group is unknown.
    UnknownGroup,
    /// The user is not connected to the requested group.
    GroupNotConnected,
    /// The requested group connection is revoked.
    GroupRevoked,
    /// The external security manager cannot evaluate the attempt.
    PolicyUnavailable,
}

/// Nonsecret profile status returned by a successful verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsCredentialDetails {
    /// Credential change time in CICS logical clock units.
    pub changed_tick: i64,
    /// Days to expiration, or negative one if no expiration applies.
    pub days_left: i16,
    /// Expiration time, or negative one if no expiration applies.
    pub expiry_tick: i64,
    /// Prior invalid credential count.
    pub invalid_count: u16,
    /// Prior successful-use time, or zero when none exists.
    pub last_use_tick: i64,
}

/// Audited SAF result for a single password or phrase attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CicsCredentialVerification {
    /// Source-distinct denial, absent after success.
    pub failure: Option<CicsCredentialFailure>,
    /// Nonsecret status, present only after success.
    pub details: Option<CicsCredentialDetails>,
    /// External security manager return code.
    pub esm_response: i64,
    /// External security manager reason code.
    pub esm_reason: i64,
}

/// Installed RACF adapter for source-defined CICS security queries.
pub trait CicsSecurityAuthority: Send + Sync {
    /// Evaluate one resource access level and record the SAF audit before returning.
    fn query_access(
        &self,
        actor: &PrincipalId,
        target: &PrincipalId,
        class: &str,
        resource: &str,
        tick: u64,
        correlation: &str,
    ) -> Result<CicsSecurityAccess, HostProblem>;

    /// Verify one borrowed credential through the replay-safe SAF authority.
    fn verify_credential(
        &self,
        request: CicsCredentialRequest<'_>,
    ) -> Result<CicsCredentialVerification, HostProblem>;

    /// Verify the old credential and apply the proposed verifier atomically.
    fn change_credential(
        &self,
        request: CicsCredentialChangeRequest<'_>,
    ) -> Result<CicsCredentialVerification, HostProblem>;

    /// Authorize and issue one bounded, one-use PassTicket through SAF.
    fn issue_passticket(
        &self,
        request: CicsPassTicketRequest<'_>,
    ) -> Result<CicsPassTicketOutcome, HostProblem>;
}

impl CicsService {
    /// Install the security authority before dispatching security-control commands.
    pub fn bind_security_authority(
        &self,
        authority: Arc<dyn CicsSecurityAuthority>,
    ) -> Result<(), HostProblem> {
        self.security_authority
            .set(authority)
            .map_err(|_| HostProblem::IdempotencyConflict)
    }

    pub(crate) fn security_authority(
        &self,
    ) -> Result<&Arc<dyn CicsSecurityAuthority>, HostProblem> {
        self.security_authority
            .get()
            .ok_or(HostProblem::InfrastructureFailure)
    }
}
