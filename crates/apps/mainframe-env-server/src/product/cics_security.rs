//! Product binding from CICS security controls to the accepted RACF/SAF authority.

use super::*;
use mainframe_env_cics::{CicsSecurityAccess, CicsSecurityAccessReason, CicsSecurityAuthority};
use mainframe_env_execution_api::PrincipalId;
use mainframe_env_host_api::HostProblem;
use mainframe_env_racf::{
    AccessEnvironment, AccessLevel, DecisionReason, RacfService, RacrouteRequest, RacrouteResult,
    SafRequestContext,
};
use std::sync::Arc;

pub(super) const MAX_AUTH_SESSIONS_PER_USER: usize = 8;

pub(super) struct RacfCicsSecurityAuthority(Arc<RacfService>);

impl RacfCicsSecurityAuthority {
    pub(super) fn new(racf: Arc<RacfService>) -> Self {
        Self(racf)
    }
}

impl CicsSecurityAuthority for RacfCicsSecurityAuthority {
    fn query_access(
        &self,
        actor: &PrincipalId,
        target: &PrincipalId,
        class: &str,
        resource: &str,
        tick: u64,
        correlation: &str,
    ) -> Result<CicsSecurityAccess, HostProblem> {
        let context = SafRequestContext::new(
            target.clone(),
            None,
            Some(actor.clone()),
            "CICS-QUERY-SECURITY",
            correlation,
            tick,
        )?;
        let outcome = self.0.racroute(
            &context,
            RacrouteRequest::Auth {
                class: class.into(),
                resource: resource.into(),
                access: AccessLevel::Read,
                environment: AccessEnvironment {
                    tick,
                    ..AccessEnvironment::default()
                },
            },
        )?;
        let reason = match outcome.status.reason {
            DecisionReason::Granted => CicsSecurityAccessReason::Granted,
            DecisionReason::ClassInactive => CicsSecurityAccessReason::ClassInactive,
            DecisionReason::ProfileNotFound => CicsSecurityAccessReason::ProfileNotFound,
            DecisionReason::PrincipalNotFound => CicsSecurityAccessReason::PrincipalNotFound,
            DecisionReason::PrincipalInactive => CicsSecurityAccessReason::PrincipalInactive,
            DecisionReason::PolicyUnavailable | DecisionReason::StoreUnavailable => {
                CicsSecurityAccessReason::PolicyUnavailable
            }
            _ => CicsSecurityAccessReason::Denied,
        };
        let granted_rank = match outcome.result {
            Some(RacrouteResult::Decision(decision)) => decision.granted_access.rank(),
            None => 0,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        Ok(CicsSecurityAccess {
            granted_rank,
            reason,
        })
    }
}

pub(super) fn terminal_principal(value: &str) -> Result<PrincipalId, GatewayProblem> {
    PrincipalId::new(value, InvocationLimits::default())
        .map_err(|_| gateway_problem(HostProblem::Unauthorized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_bridge_uses_real_saf_access_and_audits_denials() {
        let store: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(Default::default()));
        let secrets = Arc::new(MemorySecretResolver::default());
        let racf = RacfService::open(store, secrets.clone(), Default::default()).unwrap();
        secrets.insert("secret:query-test", b"PASSWORD".to_vec());
        let credential = SecretRef::new("secret:query-test", HostLimits::default()).unwrap();
        racf.add_user("IBMUSER", &credential).unwrap();
        racf.define_profile("FACILITY", "ITEM", "IBMUSER", None)
            .unwrap();
        racf.permit("FACILITY", "ITEM", "IBMUSER", AccessIntent::Update)
            .unwrap();
        let user = PrincipalId::new("IBMUSER", InvocationLimits::default()).unwrap();
        let adapter = RacfCicsSecurityAuthority::new(racf.clone());
        let before = racf.database().summary().unwrap().audits;
        let allowed = adapter
            .query_access(&user, &user, "FACILITY", "ITEM", 1, "query-allow")
            .unwrap();
        assert_eq!(allowed.granted_rank, 3);
        assert_eq!(allowed.reason, CicsSecurityAccessReason::Granted);
        let denied = adapter
            .query_access(&user, &user, "FACILITY", "OTHER", 2, "query-deny")
            .unwrap();
        assert_eq!(denied.reason, CicsSecurityAccessReason::ProfileNotFound);
        assert_eq!(racf.database().summary().unwrap().audits, before + 2);
    }
}
