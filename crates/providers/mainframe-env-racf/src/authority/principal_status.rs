use super::*;

impl RacfService {
    /// Return the durable RACF state needed to validate a non-login execution identity.
    pub fn principal_status(
        &self,
        principal: &PrincipalId,
    ) -> Result<SecurityDecision, HostProblem> {
        let snapshot = self.database.read()?;
        if !snapshot.subsystem.running || !snapshot.database_status.active {
            return Ok(SecurityDecision::Deny);
        }
        Ok(match snapshot.principals.get(principal.as_str()) {
            None => SecurityDecision::NotFound,
            Some(profile) => match profile.state {
                PrincipalState::Active => SecurityDecision::Allow,
                PrincipalState::PasswordExpired => SecurityDecision::Expired,
                PrincipalState::Revoked | PrincipalState::Suspended => SecurityDecision::Revoked,
                PrincipalState::Locked => SecurityDecision::Locked,
            },
        })
    }
}
