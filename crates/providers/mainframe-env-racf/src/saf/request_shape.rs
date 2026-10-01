use super::*;

pub(super) fn normalized_preflight_result(
    request: &RacrouteRequest,
    reason: DecisionReason,
) -> Option<RacrouteResult> {
    if !matches!(
        reason,
        DecisionReason::CredentialInvalid | DecisionReason::PolicyUnavailable
    ) || !matches!(
        request,
        RacrouteRequest::Signon { .. }
            | RacrouteRequest::Verify { .. }
            | RacrouteRequest::VerifyCredential { .. }
            | RacrouteRequest::ChangeCredential { .. }
            | RacrouteRequest::IssuePassTicket { .. }
            | RacrouteRequest::RedeemPassTicket { .. }
            | RacrouteRequest::Verifyx { .. }
    ) {
        return None;
    }
    if matches!(
        request,
        RacrouteRequest::VerifyCredential { .. } | RacrouteRequest::ChangeCredential { .. }
    ) {
        Some(RacrouteResult::CredentialVerified {
            decision: decision(reason, AccessLevel::None, None, None),
            failure: Some(CredentialFailure::PolicyUnavailable),
            details: None,
        })
    } else if matches!(request, RacrouteRequest::IssuePassTicket { .. }) {
        Some(RacrouteResult::PassTicketIssued {
            decision: decision(reason, AccessLevel::None, None, None),
            origin_denied: false,
            ticket: None,
        })
    } else if let RacrouteRequest::RedeemPassTicket { user, .. } = request {
        Some(RacrouteResult::PassTicketRedeemed {
            decision: decision(reason, AccessLevel::None, None, None),
            failure: Some(CredentialFailure::PolicyUnavailable),
            user: user.as_str().into(),
        })
    } else {
        Some(RacrouteResult::Verified {
            decision: decision(reason, AccessLevel::None, None, None),
            acee: None,
        })
    }
}

pub(super) fn validate_environment(environment: &AccessEnvironment) -> Result<(), DecisionReason> {
    for value in [
        &environment.terminal,
        &environment.console,
        &environment.system,
        &environment.application,
    ]
    .into_iter()
    .flatten()
    {
        normalized_text(value, 246)?;
    }
    Ok(())
}
