//! Product binding from CICS security controls to the accepted RACF/SAF authority.

mod cics_token;
use super::*;
use mainframe_env_cics::{
    CicsCredentialChangeRequest, CicsCredentialDetails, CicsCredentialFailure, CicsCredentialKind,
    CicsCredentialRequest, CicsCredentialVerification, CicsPassTicketFailure,
    CicsPassTicketOutcome, CicsPassTicketRequest, CicsSecurityAccess, CicsSecurityAccessReason,
    CicsSecurityAuthority, CicsSecurityTokenKind, CicsTokenFailure, CicsTokenVerification,
    CicsTokenVerificationRequest,
};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits, PrincipalId};
use mainframe_env_host_api::HostProblem;
use mainframe_env_racf::{
    AccessEnvironment, AccessLevel, CredentialFailure, CredentialKind, DecisionOutcome,
    DecisionReason, RacfService, RacrouteRequest, RacrouteResult, SafRequestContext, TokenKind,
};
use std::sync::Arc;

pub(super) const MAX_AUTH_SESSIONS_PER_USER: usize = 8;

pub(super) struct RacfCicsSecurityAuthority {
    racf: Arc<RacfService>,
    secrets: Arc<MemorySecretResolver>,
}

impl RacfCicsSecurityAuthority {
    pub(super) fn new(racf: Arc<RacfService>, secrets: Arc<MemorySecretResolver>) -> Self {
        Self { racf, secrets }
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
        let outcome = self.racf.racroute(
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

    fn verify_credential(
        &self,
        request: CicsCredentialRequest<'_>,
    ) -> Result<CicsCredentialVerification, HostProblem> {
        let mut identity = Sha256::new();
        identity.update(b"mainframe-env.cics-credential-ref@1\0");
        identity.update(request.idempotency_key.as_bytes());
        identity.update(request.actor.as_str().as_bytes());
        let reference = SecretRef::new(
            format!("cics:verify:{:x}", identity.finalize()),
            HostLimits::default(),
        )?;
        let _scope = self
            .secrets
            .scoped(&reference, request.credential.to_vec())?;
        let context = SafRequestContext::new(
            request.actor.clone(),
            None,
            None,
            request.idempotency_key,
            request.correlation,
            request.tick,
        )?;
        let outcome = self.racf.racroute(
            &context,
            RacrouteRequest::VerifyCredential {
                user: request.user.clone(),
                credential_reference: reference,
                kind: match request.kind {
                    CicsCredentialKind::Password => CredentialKind::Password,
                    CicsCredentialKind::Phrase => CredentialKind::Phrase,
                },
                group: request.group.map(str::to_string),
                binding_digest: request.binding_digest,
            },
        )?;
        map_credential_outcome(outcome)
    }

    fn change_credential(
        &self,
        request: CicsCredentialChangeRequest<'_>,
    ) -> Result<CicsCredentialVerification, HostProblem> {
        let mut identity = Sha256::new();
        identity.update(b"mainframe-env.cics-credential-change-ref@1\0");
        identity.update(request.idempotency_key.as_bytes());
        identity.update(request.actor.as_str().as_bytes());
        let reference = SecretRef::new(
            format!("cics:change:{:x}", identity.finalize()),
            HostLimits::default(),
        )?;
        let old_len = u16::try_from(request.current.len()).map_err(|_| HostProblem::Malformed)?;
        let new_len = u16::try_from(request.proposed.len()).map_err(|_| HostProblem::Malformed)?;
        let mut packet = Vec::with_capacity(4 + usize::from(old_len) + usize::from(new_len));
        packet.extend_from_slice(&old_len.to_be_bytes());
        packet.extend_from_slice(&new_len.to_be_bytes());
        packet.extend_from_slice(request.current);
        packet.extend_from_slice(request.proposed);
        let _scope = self.secrets.scoped(&reference, packet)?;
        let context = SafRequestContext::new(
            request.actor.clone(),
            None,
            None,
            request.idempotency_key,
            request.correlation,
            request.tick,
        )?;
        let outcome = self.racf.racroute(
            &context,
            RacrouteRequest::ChangeCredential {
                user: request.user.clone(),
                credential_reference: reference,
                kind: match request.kind {
                    CicsCredentialKind::Password => CredentialKind::Password,
                    CicsCredentialKind::Phrase => CredentialKind::Phrase,
                },
                group: request.group.map(str::to_string),
                binding_digest: request.binding_digest,
            },
        )?;
        map_credential_outcome(outcome)
    }

    fn issue_passticket(
        &self,
        request: CicsPassTicketRequest<'_>,
    ) -> Result<CicsPassTicketOutcome, HostProblem> {
        let context = SafRequestContext::new(
            request.actor.clone(),
            None,
            None,
            request.idempotency_key,
            request.correlation,
            request.tick,
        )?;
        let outcome = self.racf.racroute(
            &context,
            RacrouteRequest::IssuePassTicket {
                application: request.application.into(),
                binding_digest: request.binding_digest,
            },
        )?;
        let (ticket, failure) = match outcome.result {
            Some(RacrouteResult::PassTicketIssued { ticket, .. })
                if outcome.status.reason == DecisionReason::Granted =>
            {
                (
                    Some(
                        BoundedPayload::new(
                            "mainframe-env.cics.secret@1",
                            ticket.ok_or(HostProblem::UnknownOutcome)?.into_bytes(),
                            InvocationLimits::default(),
                        )
                        .map_err(|_| HostProblem::ResourceExhausted)?,
                    ),
                    None,
                )
            }
            Some(RacrouteResult::PassTicketIssued { origin_denied, .. }) => {
                let failure = if matches!(
                    outcome.status.reason,
                    DecisionReason::PolicyUnavailable | DecisionReason::StoreUnavailable
                ) {
                    CicsPassTicketFailure::SecurityUnavailable
                } else if origin_denied {
                    CicsPassTicketFailure::RegionDenied
                } else if outcome.status.reason == DecisionReason::ClassInactive {
                    CicsPassTicketFailure::Unsupported
                } else {
                    CicsPassTicketFailure::TargetDenied
                };
                (None, Some(failure))
            }
            None if outcome.status.reason == DecisionReason::CredentialInvalid => {
                (None, Some(CicsPassTicketFailure::DefaultUser))
            }
            _ => return Err(HostProblem::ProviderFailure),
        };
        Ok(CicsPassTicketOutcome {
            ticket,
            failure,
            esm_response: i64::from(outcome.status.racf_return_code),
            esm_reason: i64::from(outcome.status.racf_reason_code),
        })
    }

    fn audit_signoff(
        &self,
        actor: &PrincipalId,
        session: &str,
        binding_digest: [u8; 32],
        idempotency_key: &str,
        tick: u64,
        allowed: bool,
    ) -> Result<(), HostProblem> {
        let mut digest = Sha256::new();
        digest.update(b"mainframe-env.cics-signon-terminal@1\0");
        digest.update(session.as_bytes());
        digest.update(binding_digest);
        let context = SafRequestContext::new(
            actor.clone(),
            None,
            None,
            idempotency_key,
            idempotency_key,
            tick,
        )?;
        let outcome = self.racf.racroute(
            &context,
            RacrouteRequest::Audit {
                action: "CICS-SIGNOFF".into(),
                resource_digest: format!("sha256:{:x}", digest.finalize()),
                decision: if allowed {
                    DecisionOutcome::Allow
                } else {
                    DecisionOutcome::Deny
                },
                fields: Default::default(),
            },
        )?;
        if outcome.status.reason == DecisionReason::Granted
            && matches!(outcome.result, Some(RacrouteResult::Audit { .. }))
        {
            Ok(())
        } else {
            Err(HostProblem::ProviderFailure)
        }
    }

    fn verify_token(
        &self,
        request: CicsTokenVerificationRequest<'_>,
    ) -> Result<CicsTokenVerification, HostProblem> {
        cics_token::verify(self, request)
    }
}

fn map_credential_outcome(
    outcome: mainframe_env_racf::RacrouteOutcome,
) -> Result<CicsCredentialVerification, HostProblem> {
    let (failure, details) = match outcome.result {
        Some(RacrouteResult::CredentialVerified {
            failure, details, ..
        }) => (failure, details),
        _ => return Err(HostProblem::ProviderFailure),
    };
    Ok(CicsCredentialVerification {
        failure: failure.map(|failure| match failure {
            CredentialFailure::UnknownUser => CicsCredentialFailure::UnknownUser,
            CredentialFailure::Revoked => CicsCredentialFailure::Revoked,
            CredentialFailure::NewCredentialRequired => {
                CicsCredentialFailure::NewCredentialRequired
            }
            CredentialFailure::InvalidCredential => CicsCredentialFailure::InvalidCredential,
            CredentialFailure::UnacceptableNewCredential => {
                CicsCredentialFailure::UnacceptableNewCredential
            }
            CredentialFailure::MismatchedCredentialKind => {
                CicsCredentialFailure::MismatchedCredentialKind
            }
            CredentialFailure::UnknownGroup => CicsCredentialFailure::UnknownGroup,
            CredentialFailure::GroupNotConnected => CicsCredentialFailure::GroupNotConnected,
            CredentialFailure::GroupRevoked => CicsCredentialFailure::GroupRevoked,
            CredentialFailure::PolicyUnavailable => CicsCredentialFailure::PolicyUnavailable,
        }),
        details: details.map(|value| CicsCredentialDetails {
            changed_tick: value.changed_tick,
            days_left: value.days_left,
            expiry_tick: value.expiry_tick,
            invalid_count: value.invalid_count,
            last_use_tick: value.last_use_tick,
        }),
        esm_response: i64::from(outcome.status.racf_return_code),
        esm_reason: i64::from(outcome.status.racf_reason_code),
    })
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
        let adapter = RacfCicsSecurityAuthority::new(racf.clone(), secrets);
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
