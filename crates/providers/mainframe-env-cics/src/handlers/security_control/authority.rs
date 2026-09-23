//! Narrow bridge from CICS application security commands to the installed SAF authority.

use crate::service::CicsService;
use mainframe_env_execution_api::PrincipalId;
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
