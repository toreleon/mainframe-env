use super::{ONLINE_EXCHANGE_NAMESPACE, ProductServer, decode_online_exchange, store_error};
use crate::jes_worker::process_cics_work;
use mainframe_env_cics::{CICS_DELAY_WORK_GENERATION, CicsLimits};
use mainframe_env_execution_api::{InvocationLimits, PrincipalId};
use mainframe_env_host_api::{HostProblem, SessionId};
use mainframe_env_store_api::WorkRecord;

impl ProductServer {
    pub(super) fn process_interval_work(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<bool, HostProblem> {
        if !process_cics_work(&self.cics, work, now_tick)? {
            return Ok(false);
        }
        if work.required_generation == CICS_DELAY_WORK_GENERATION {
            self.wake_delayed_online_task(work, now_tick)?;
        }
        Ok(true)
    }

    fn wake_delayed_online_task(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let delay_id = std::str::from_utf8(&work.payload).map_err(|_| HostProblem::Malformed)?;
        let (run_unit, position) = delay_id.rsplit_once(':').ok_or(HostProblem::Malformed)?;
        if run_unit.is_empty()
            || position.is_empty()
            || !position.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(HostProblem::Malformed);
        }
        let maximum = CicsLimits::default().max_sessions;
        let rows = self
            .store
            .list_provider_state(ONLINE_EXCHANGE_NAMESPACE, maximum.saturating_add(1))
            .map_err(store_error)?;
        if rows.len() > maximum {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut selected = None;
        for row in rows {
            let state = decode_online_exchange(&row)?;
            if state.run_unit_id != run_unit {
                continue;
            }
            if selected.is_some() {
                return Err(HostProblem::InfrastructureFailure);
            }
            selected = Some((row.key, state));
        }
        let Some((session, state)) = selected else {
            // Completion clears the exchange before the worker completion CAS.
            return Ok(());
        };
        let session =
            SessionId::new(session, 64).map_err(|_| HostProblem::InfrastructureFailure)?;
        let principal = PrincipalId::new(&state.principal, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        self.run_online_exchange(&session, &principal, &state.program, now_tick)
    }
}
