use super::{
    ONLINE_EXCHANGE_NAMESPACE, ProductServer, decode_online_exchange, normalize_online_name,
    store_error,
};
use crate::jes_worker::{CicsWorkOutcome, process_cics_work};
use mainframe_env_cics::{CicsLimits, CicsStartTask};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_host_api::{HostProblem, SessionId};
use mainframe_env_store_api::{ExecutionState, WorkRecord};
use std::collections::{BTreeMap, BTreeSet};

const START_TASK_SESSION_PREFIX: &str = "cics-start-task-";

impl ProductServer {
    pub(super) fn process_interval_work(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<bool, HostProblem> {
        match process_cics_work(&self.cics, work, now_tick)? {
            Some(CicsWorkOutcome::Start(task)) => {
                self.launch_started_task(work, &task, now_tick)?;
            }
            Some(CicsWorkOutcome::Delay) => self.wake_delayed_online_task(work, now_tick)?,
            None => return Ok(false),
        }
        Ok(true)
    }

    fn launch_started_task(
        &self,
        work: &WorkRecord,
        task: &CicsStartTask,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        if task.terminal.is_some() {
            return Ok(());
        }
        let transaction = normalize_online_name(&task.transaction, 16)?;
        let Some(program) = self
            .online_transactions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&transaction)
            .cloned()
        else {
            return Ok(());
        };
        let Some(artifact) = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&program)
            .cloned()
        else {
            return Ok(());
        };
        let invocation = started_task_invocation(work, task, artifact)?;
        let session = SessionId::new(
            format!("{START_TASK_SESSION_PREFIX}{}", work.execution_id),
            InvocationLimits::default().max_binding_bytes,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let principal = invocation.principal.id().clone();
        if let Some(execution) = self
            .store
            .get_execution(&invocation.execution_id)
            .map_err(store_error)?
        {
            let execution_selector =
                Selector::new(format!("program:{program}"), InvocationLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            if execution.run_unit_id != invocation.run_unit_id
                || execution.selector != execution_selector
                || execution.artifact != invocation.artifact
                || &execution.principal != invocation.principal.id()
                || execution.attempt != invocation.attempt
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            if execution.state == ExecutionState::Suspended {
                return Ok(());
            }
            if execution.state.terminal() && self.online_exchange(&session)?.is_none() {
                self.cleanup_started_task_if_idle(&session, &principal)?;
                return Ok(());
            }
        }
        match self
            .cics
            .launch_background_task(invocation.clone(), &session, &transaction)
        {
            Ok(()) => {}
            Err(HostProblem::NotFound | HostProblem::Unauthorized) => return Ok(()),
            Err(problem) => return Err(problem),
        }
        let result = self.run_online_exchange(&session, &principal, &program, now_tick);
        self.settle_started_exchange(&session, &principal, &invocation.execution_id, result)
    }

    fn settle_started_exchange(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        execution_id: &ExecutionId,
        result: Result<(), HostProblem>,
    ) -> Result<(), HostProblem> {
        if let Err(problem) = result
            && !self
                .store
                .get_execution(execution_id)
                .map_err(store_error)?
                .is_some_and(|execution| execution.state.terminal())
        {
            return Err(problem);
        }
        self.cleanup_started_task_if_idle(session, principal)
    }

    fn cleanup_started_task_if_idle(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
    ) -> Result<(), HostProblem> {
        if self.online_exchange(session)?.is_none()
            && self.online_machine_continuation(session)?.is_none()
        {
            self.cics.finish_background_task(session, principal)?;
        }
        Ok(())
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
        let result = self.run_online_exchange(&session, &principal, &state.program, now_tick);
        if session.as_str().starts_with(START_TASK_SESSION_PREFIX) {
            let execution =
                ExecutionId::new(state.execution_id.as_str(), InvocationLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            return self.settle_started_exchange(&session, &principal, &execution, result);
        }
        result
    }
}

fn started_task_invocation(
    work: &WorkRecord,
    task: &CicsStartTask,
    artifact: ArtifactRef,
) -> Result<Invocation, HostProblem> {
    let limits = InvocationLimits::default();
    let grants = super::continuation::ONLINE_PROVIDER_CAPABILITIES
        .iter()
        .map(|name| CapabilityId::new(*name, limits))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let generations = grants
        .iter()
        .cloned()
        .map(|capability| (capability, "1".into()))
        .collect();
    let execution = work.execution_id.as_str();
    let bindings = BTreeMap::from([(
        "cics.start-request".into(),
        BoundedPayload::new(
            "mainframe-env.cics.start-request@1",
            task.request_id.as_bytes().to_vec(),
            limits,
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    )]);
    Invocation::new(
        RequestId::new(format!("request-{execution}"), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        work.execution_id.clone(),
        RunUnitId::new(format!("run-{execution}"), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        None,
        Selector::new(format!("cics:{}", task.transaction), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        artifact,
        Principal::new(
            PrincipalId::new(&task.principal, limits).map_err(|_| HostProblem::Unauthorized)?,
            grants,
            limits,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?,
        ServiceClass::Interactive,
        work.priority,
        work.deadline_tick,
        TraceId::new(format!("trace-{execution}"), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        IdempotencyKey::new(format!("start-{execution}"), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        1,
        ResourceLimits::default(),
        bindings,
        limits,
    )
    .and_then(|invocation| invocation.with_provider_generations(generations, limits))
    .map_err(|_| HostProblem::InfrastructureFailure)
}
