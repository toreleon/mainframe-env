//! Existing CicsResume HTTP route owner and opt-in finite refusal stages.
use super::*;

impl ProductServer {
    pub(super) fn resume_cics_session(
        &self,
        session: &str,
        principal: &str,
        csrf_token: &str,
    ) -> Result<GatewayResponse, GatewayProblem> {
        let session = terminal_session_id(session).inspect_err(|_| {
            #[cfg(feature = "assurance-observers")]
            eprintln!("CICS resume refusal stage=session-id");
        })?;
        let principal_id = terminal_principal(principal).inspect_err(|_| {
            #[cfg(feature = "assurance-observers")]
            eprintln!("CICS resume refusal stage=principal-id");
        })?;
        self.preflight_online_continuation(&session, None)
            .inspect_err(|_| {
                #[cfg(feature = "assurance-observers")]
                eprintln!("CICS resume refusal stage=preflight-continuation");
            })
            .map_err(gateway_problem)?;
        let snapshot = self
            .online_exchange(&session)
            .inspect_err(|_| {
                #[cfg(feature = "assurance-observers")]
                eprintln!("CICS resume refusal stage=exchange-load");
            })
            .map_err(gateway_problem)?;
        let mut start_fresh_task = snapshot.is_none();
        if let Some(exchange) = snapshot {
            self.preflight_online_exchange_state(&exchange)
                .inspect_err(|_| {
                    #[cfg(feature = "assurance-observers")]
                    eprintln!("CICS resume refusal stage=exchange-preflight");
                })
                .map_err(gateway_problem)?;
            let tick = current_tick().inspect_err(|_| {
                #[cfg(feature = "assurance-observers")]
                eprintln!("CICS resume refusal stage=resume-clock");
            })?;
            self.cics
                .validate_terminal_resume(&session, &principal_id, csrf_token, tick)
                .inspect_err(|_| {
                    #[cfg(feature = "assurance-observers")]
                    eprintln!("CICS resume refusal stage=resume-validation");
                })
                .map_err(gateway_problem)?;
            match self
                .recover_terminal_online_exchange(&session, &principal_id, &exchange, tick)
                .inspect_err(|_| {
                    #[cfg(feature = "assurance-observers")]
                    eprintln!("CICS resume refusal stage=terminal-recovery");
                })
                .map_err(gateway_problem)?
            {
                Some(TerminalExchangeRecovery::HandoffCompleted) => {
                    start_fresh_task = true;
                }
                Some(TerminalExchangeRecovery::Completed) => {}
                Some(TerminalExchangeRecovery::Cancelled) => {
                    return Err(gateway_problem(HostProblem::Cancelled));
                }
                Some(TerminalExchangeRecovery::TimedOut) => {
                    return Err(gateway_problem(HostProblem::TimedOut));
                }
                Some(TerminalExchangeRecovery::Failed) => {
                    return Err(gateway_problem(HostProblem::ProviderFailure));
                }
                None => {
                    self.run_online_exchange(&session, &principal_id, &exchange.program, tick)
                        .inspect_err(|_| {
                            #[cfg(feature = "assurance-observers")]
                            eprintln!("CICS resume refusal stage=existing-run");
                        })
                        .map_err(gateway_problem)?;
                }
            }
        }
        if start_fresh_task {
            self.resume_fresh_online_task(&session, principal, &principal_id, csrf_token)
                .inspect_err(|_| {
                    #[cfg(feature = "assurance-observers")]
                    eprintln!("CICS resume refusal stage=fresh-task");
                })?;
        }
        let snapshot = self
            .cics
            .terminal_snapshot(
                &session,
                &principal_id,
                current_tick().inspect_err(|_| {
                    #[cfg(feature = "assurance-observers")]
                    eprintln!("CICS resume refusal stage=snapshot-clock");
                })?,
            )
            .inspect_err(|_| {
                #[cfg(feature = "assurance-observers")]
                eprintln!("CICS resume refusal stage=final-snapshot");
            })
            .map_err(gateway_problem)?;
        Ok(GatewayResponse::json(
            StatusCode::OK,
            terminal_json(&snapshot),
        ))
    }
}
