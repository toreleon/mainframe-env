use super::{
    JesWorkOutcome, ONLINE_EXCHANGE_NAMESPACE, ProductServer, decode_online_exchange,
    normalize_online_name, store_error,
};
use crate::jes_worker::{CicsWorkOutcome, process_cics_work};
use mainframe_env_cics::bts_lifecycle::{BtsCompletion, BtsRunRecord};
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
const BTS_TASK_SESSION_PREFIX: &str = "cics-bts-task-";

impl ProductServer {
    pub(super) fn process_interval_work(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<Option<JesWorkOutcome>, HostProblem> {
        Ok(match process_cics_work(&self.cics, work, now_tick)? {
            Some(CicsWorkOutcome::Start(task)) => {
                Some(self.launch_started_task(work, &task, now_tick)?)
            }
            Some(CicsWorkOutcome::BtsRun(Some(task))) => {
                Some(self.launch_bts_run_task(work, &task, now_tick)?)
            }
            Some(CicsWorkOutcome::BtsRun(None)) => Some(JesWorkOutcome::Completed),
            Some(CicsWorkOutcome::Delay) => {
                self.wake_delayed_online_task(work, now_tick)?;
                Some(JesWorkOutcome::Completed)
            }
            None => None,
        })
    }

    fn launch_started_task(
        &self,
        work: &WorkRecord,
        task: &CicsStartTask,
        now_tick: u64,
    ) -> Result<JesWorkOutcome, HostProblem> {
        let transaction = normalize_online_name(&task.transaction, 16)?;
        let Some(program) = self
            .online_transactions
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&transaction)
            .cloned()
        else {
            return Ok(JesWorkOutcome::Completed);
        };
        let Some(artifact) = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&program)
            .cloned()
        else {
            return Ok(JesWorkOutcome::Completed);
        };
        let invocation = started_task_invocation(work, task, artifact)?;
        let principal = invocation.principal.id().clone();
        let terminal = match task.terminal.as_deref() {
            Some(terminal) => match self.cics.resolve_start_terminal(terminal)? {
                Some(terminal) if terminal.principal == principal => Some(terminal),
                Some(_) | None => return Ok(JesWorkOutcome::Completed),
            },
            None => None,
        };
        let background = terminal.is_none();
        let session = match &terminal {
            Some(terminal) => terminal.session.clone(),
            None => SessionId::new(
                format!("{START_TASK_SESSION_PREFIX}{}", work.execution_id),
                InvocationLimits::default().max_binding_bytes,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        };
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
                return Ok(JesWorkOutcome::Completed);
            }
            if execution.state.terminal() && self.online_exchange(&session)?.is_none() {
                if background {
                    self.cleanup_started_task_if_idle(&session, &principal)?;
                }
                return Ok(JesWorkOutcome::Completed);
            }
        }
        let exchange = self.online_exchange(&session)?;
        if let Some(exchange) = &exchange
            && exchange.execution_id != invocation.execution_id.as_str()
        {
            return Ok(JesWorkOutcome::Deferred);
        }
        if exchange.is_none() {
            let launched = match &terminal {
                Some(terminal) if !terminal.available => return Ok(JesWorkOutcome::Deferred),
                Some(_) => self.cics.launch_started_terminal_task(
                    invocation.clone(),
                    &session,
                    &transaction,
                    now_tick,
                ),
                None => {
                    self.cics
                        .launch_background_task(invocation.clone(), &session, &transaction)
                }
            };
            match launched {
                Ok(()) => {}
                Err(HostProblem::IdempotencyConflict) if terminal.is_some() => {
                    return Ok(JesWorkOutcome::Deferred);
                }
                Err(HostProblem::NotFound | HostProblem::Unauthorized | HostProblem::TimedOut) => {
                    return Ok(JesWorkOutcome::Completed);
                }
                Err(problem) => return Err(problem),
            }
        }
        let result = self.run_online_exchange(&session, &principal, &program, now_tick);
        self.settle_started_exchange(
            &session,
            &principal,
            &invocation.execution_id,
            background,
            result,
        )?;
        Ok(JesWorkOutcome::Completed)
    }

    fn launch_bts_run_task(
        &self,
        work: &WorkRecord,
        task: &BtsRunRecord,
        now_tick: u64,
    ) -> Result<JesWorkOutcome, HostProblem> {
        let program = normalize_online_name(&task.program, 8)?;
        let artifact = self
            .online_programs
            .lock()
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .get(&program)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let invocation = bts_task_invocation(work, task, artifact)?;
        let principal = invocation.principal.id().clone();
        let session = SessionId::new(
            format!("{BTS_TASK_SESSION_PREFIX}{}", work.execution_id),
            InvocationLimits::default().max_binding_bytes,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if let Some(execution) = self
            .store
            .get_execution(&invocation.execution_id)
            .map_err(store_error)?
        {
            let selector = Selector::new(format!("program:{program}"), InvocationLimits::default())
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if execution.run_unit_id != invocation.run_unit_id
                || execution.selector != selector
                || execution.artifact != invocation.artifact
                || execution.principal != principal
                || execution.attempt != invocation.attempt
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            if execution.state.terminal() && self.online_exchange(&session)?.is_none() {
                return self.settle_bts_run_task(
                    work,
                    task,
                    &invocation,
                    &session,
                    &principal,
                    now_tick,
                    None,
                );
            }
        }
        let exchange = self.online_exchange(&session)?;
        if exchange
            .as_ref()
            .is_some_and(|state| state.execution_id != invocation.execution_id.as_str())
        {
            return Ok(JesWorkOutcome::Deferred);
        }
        if exchange.is_none() {
            self.cics
                .launch_background_task(invocation.clone(), &session, &task.transaction)?;
        }
        self.cics.bind_bts_activity_context(
            &invocation.run_unit_id,
            &task.process_type,
            &task.process_name,
            &task.activity_id,
            task.activation_epoch,
            work.lease_epoch,
        )?;
        let result = self.run_online_exchange(&session, &principal, &program, now_tick);
        self.settle_bts_run_task(
            work,
            task,
            &invocation,
            &session,
            &principal,
            now_tick,
            Some(result),
        )
    }

    fn settle_bts_run_task(
        &self,
        work: &WorkRecord,
        task: &BtsRunRecord,
        invocation: &Invocation,
        session: &SessionId,
        principal: &PrincipalId,
        now_tick: u64,
        result: Option<Result<(), HostProblem>>,
    ) -> Result<JesWorkOutcome, HostProblem> {
        let execution = self
            .store
            .get_execution(&invocation.execution_id)
            .map_err(store_error)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let failure = result.and_then(Result::err);
        match execution.state {
            ExecutionState::Completed => {
                if let Some(problem) = failure {
                    return Err(problem);
                }
                self.cics
                    .complete_bts_run_work(work, BtsCompletion::Normal, None, None)?;
            }
            ExecutionState::Cancelled | ExecutionState::TimedOut => {
                self.cics
                    .complete_bts_run_work(work, BtsCompletion::Forced, None, None)?;
            }
            ExecutionState::Failed => {
                let (code, program) = self
                    .cics
                    .bts_run_abend(&invocation.run_unit_id)?
                    .ok_or_else(|| failure.unwrap_or(HostProblem::UnknownOutcome))?;
                self.cics.complete_bts_run_work(
                    work,
                    BtsCompletion::Abend,
                    Some(&code),
                    Some(&program),
                )?;
            }
            ExecutionState::Admitted
            | ExecutionState::Queued
            | ExecutionState::Running
            | ExecutionState::Suspended
            | ExecutionState::Completing => {
                return match failure {
                    Some(problem) => Err(problem),
                    None => Ok(JesWorkOutcome::Deferred),
                };
            }
            ExecutionState::DeadLetter => return Err(HostProblem::UnknownOutcome),
        }
        self.cics.close_bts_run_context(work)?;
        self.cleanup_started_task_if_idle(session, principal)?;
        if task.synchronous {
            self.wake_run_unit_online_task(&task.owner_run_unit, now_tick)?;
        }
        Ok(JesWorkOutcome::Completed)
    }

    fn settle_started_exchange(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        execution_id: &ExecutionId,
        background: bool,
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
        if background {
            self.cleanup_started_task_if_idle(session, principal)?;
        }
        Ok(())
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
        self.wake_run_unit_online_task(run_unit, now_tick)
    }

    fn wake_run_unit_online_task(&self, run_unit: &str, now_tick: u64) -> Result<(), HostProblem> {
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
            return self.settle_started_exchange(&session, &principal, &execution, true, result);
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

fn bts_task_invocation(
    work: &WorkRecord,
    task: &BtsRunRecord,
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
    let mut bindings = BTreeMap::from([
        (
            "cics.bts-run-id".into(),
            BoundedPayload::new(
                "mainframe-env.cics.bts-run-id@1",
                task.run_id.as_bytes().to_vec(),
                limits,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        ),
        (
            "cics.bts-input-event".into(),
            BoundedPayload::new(
                "mainframe-env.cics.bts-input-event@1",
                task.input_event.as_bytes().to_vec(),
                limits,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        ),
    ]);
    if let Some(token) = task.facility_token {
        bindings.insert(
            "cics.bts-facility-token".into(),
            BoundedPayload::new(
                "mainframe-env.cics.bts-facility-token@1",
                token.to_vec(),
                limits,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
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
            PrincipalId::new(&task.userid, limits).map_err(|_| HostProblem::Unauthorized)?,
            grants,
            limits,
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?,
        ServiceClass::Interactive,
        work.priority,
        work.deadline_tick,
        TraceId::new(format!("trace-{execution}"), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        IdempotencyKey::new(format!("bts-{execution}"), limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        1,
        ResourceLimits::default(),
        bindings,
        limits,
    )
    .and_then(|invocation| invocation.with_provider_generations(generations, limits))
    .map_err(|_| HostProblem::InfrastructureFailure)
}
