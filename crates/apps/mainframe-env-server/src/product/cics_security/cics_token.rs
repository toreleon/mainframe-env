//! Narrow SAF-backed token verification for CICS application commands.

use super::*;
use base64::Engine;

pub(super) fn verify(
    authority: &RacfCicsSecurityAuthority,
    request: CicsTokenVerificationRequest<'_>,
) -> Result<CicsTokenVerification, HostProblem> {
    match request.kind {
        CicsSecurityTokenKind::BasicAuth => basic_auth(authority, request),
        CicsSecurityTokenKind::Jwt => mapped_token(authority, request, false),
        CicsSecurityTokenKind::Kerberos => mapped_token(authority, request, true),
    }
}

fn basic_auth(
    authority: &RacfCicsSecurityAuthority,
    request: CicsTokenVerificationRequest<'_>,
) -> Result<CicsTokenVerification, HostProblem> {
    let Ok(value) = std::str::from_utf8(request.token) else {
        return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
    };
    let Some((user, credential)) = value.split_once(':') else {
        return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
    };
    if user.is_empty()
        || user.len() > 8
        || credential.is_empty()
        || credential.len() > 100
        || user.trim() != user
        || credential.trim() != credential
        || user.contains(' ')
    {
        return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
    }
    let user = PrincipalId::new(user.to_ascii_uppercase(), InvocationLimits::default())
        .map_err(|_| HostProblem::Malformed)?;
    let mut identity = Sha256::new();
    identity.update(b"mainframe-env.cics-token-secret-ref@1\0");
    identity.update(request.idempotency_key.as_bytes());
    let reference = SecretRef::new(
        format!("cics:token:{:x}", identity.finalize()),
        HostLimits::default(),
    )?;
    let _scope = authority
        .secrets
        .scoped(&reference, credential.as_bytes().to_vec())?;
    let context = SafRequestContext::new(
        request.actor.clone(),
        None,
        None,
        request.idempotency_key,
        request.correlation,
        request.tick,
    )?;
    let outcome = if authority.racf.has_active_passticket(
        &user,
        request.application,
        credential.as_bytes(),
        request.tick,
    )? {
        authority.racf.racroute(
            &context,
            RacrouteRequest::RedeemPassTicket {
                user: user.clone(),
                application: request.application.into(),
                ticket_reference: reference,
                binding_digest: request.binding_digest,
            },
        )?
    } else {
        authority.racf.racroute(
            &context,
            RacrouteRequest::VerifyCredential {
                user: user.clone(),
                credential_reference: reference,
                kind: if credential.len() > 8 {
                    CredentialKind::Phrase
                } else {
                    CredentialKind::Password
                },
                group: None,
                binding_digest: request.binding_digest,
            },
        )?
    };
    let failure = match outcome.result {
        Some(RacrouteResult::CredentialVerified { failure, .. })
        | Some(RacrouteResult::PassTicketRedeemed { failure, .. }) => failure,
        _ => return Err(HostProblem::ProviderFailure),
    };
    Ok(CicsTokenVerification {
        user: failure.is_none().then(|| user.as_str().into()),
        failure: failure.map(map_failure),
        confidential: false,
        mutual: false,
        out_token: None,
        esm_response: i64::from(outcome.status.racf_return_code),
        esm_reason: i64::from(outcome.status.racf_reason_code),
    })
}

fn mapped_token(
    authority: &RacfCicsSecurityAuthority,
    request: CicsTokenVerificationRequest<'_>,
    kerberos: bool,
) -> Result<CicsTokenVerification, HostProblem> {
    let (confidential, mutual) = if kerberos {
        let confidential = request.token.starts_with(b"KRB5:CONF:");
        if !confidential && !request.token.starts_with(b"KRB5:PLAIN:") {
            return Ok(rejected(CicsTokenFailure::MalformedKerberos, 8, 50));
        }
        (
            confidential,
            request.token.starts_with(b"KRB5:CONF:MUTUAL:"),
        )
    } else {
        let Ok(text) = std::str::from_utf8(request.token) else {
            return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
        };
        let parts = text.split('.').collect::<Vec<_>>();
        if parts.len() != 3 || parts[2].is_empty() {
            return Ok(rejected(CicsTokenFailure::UnsignedJwt, 8, 103));
        }
        let Ok(header) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[0]) else {
            return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
        };
        let Ok(header): Result<serde_json::Value, _> = serde_json::from_slice(&header) else {
            return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
        };
        if header.get("alg").and_then(serde_json::Value::as_str) != Some("HS256") {
            return Ok(rejected(CicsTokenFailure::UnsignedJwt, 8, 103));
        }
        let Ok(claims) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) else {
            return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
        };
        let Ok(claims): Result<serde_json::Value, _> = serde_json::from_slice(&claims) else {
            return Ok(rejected(CicsTokenFailure::Malformed, 8, 60));
        };
        if claims
            .get("exp")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|expiry| expiry <= request.tick)
        {
            return Ok(rejected(CicsTokenFailure::Rejected, 8, 61));
        }
        (false, false)
    };
    let context = SafRequestContext::new(
        request.actor.clone(),
        None,
        None,
        request.idempotency_key,
        request.correlation,
        request.tick,
    )?;
    let outcome = authority.racf.racroute(
        &context,
        RacrouteRequest::Tokenmap {
            token_digest: format!("sha256:{:x}", Sha256::digest(request.token)),
        },
    )?;
    let user = match outcome.result {
        Some(RacrouteResult::TokenMapped { acee, kind, .. })
            if kind
                == if kerberos {
                    TokenKind::Custom
                } else {
                    TokenKind::JwtReference
                } =>
        {
            Some(acee.principal)
        }
        _ => None,
    };
    let failure = user.is_none().then(|| match outcome.status.reason {
        DecisionReason::PrincipalInactive => CicsTokenFailure::Revoked,
        DecisionReason::PrincipalNotFound => CicsTokenFailure::UnknownUser,
        DecisionReason::PolicyUnavailable | DecisionReason::StoreUnavailable => {
            CicsTokenFailure::PolicyUnavailable
        }
        _ => CicsTokenFailure::Rejected,
    });
    let out_token = if user.is_some() && mutual {
        let mut token = b"KRB5:APREP:".to_vec();
        token.extend_from_slice(format!("{:x}", Sha256::digest(request.token)).as_bytes());
        Some(
            BoundedPayload::new(
                "mainframe-env.cics.secret@1",
                token,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        )
    } else {
        None
    };
    Ok(CicsTokenVerification {
        failure,
        user,
        confidential,
        mutual,
        out_token,
        esm_response: if failure.is_some() {
            8
        } else {
            i64::from(outcome.status.racf_return_code)
        },
        esm_reason: if failure.is_some() {
            61
        } else {
            i64::from(outcome.status.racf_reason_code)
        },
    })
}

fn rejected(failure: CicsTokenFailure, response: i64, reason: i64) -> CicsTokenVerification {
    CicsTokenVerification {
        user: None,
        failure: Some(failure),
        confidential: false,
        mutual: false,
        out_token: None,
        esm_response: response,
        esm_reason: reason,
    }
}

fn map_failure(failure: CredentialFailure) -> CicsTokenFailure {
    match failure {
        CredentialFailure::UnknownUser => CicsTokenFailure::UnknownUser,
        CredentialFailure::Revoked => CicsTokenFailure::Revoked,
        CredentialFailure::PolicyUnavailable => CicsTokenFailure::PolicyUnavailable,
        _ => CicsTokenFailure::Rejected,
    }
}
