//! Actual online machine construction, drive and unchanged completion cleanup owner.
use super::*;

impl ProductServer {
    pub(super) fn run_online_exchange(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let mut saved = self.preflight_online_continuation(session, None)?;
        if saved.is_none() {
            self.preflight_online_program(program)?;
        }
        let mut exchange = self.online_exchange(session)?;
        self.recover_pending_online_transfer_if_present(
            session,
            &mut saved,
            &mut exchange,
            now_tick,
        )?;
        if let Some(state) = exchange.as_ref() {
            self.preflight_online_exchange_state(state)?;
            let expected_program = saved
                .as_ref()
                .map_or(program, |continuation| continuation.program.as_str());
            if state.principal != principal.as_str()
                || state.program != normalize_online_name(expected_program, 128)?
            {
                return Err(HostProblem::Unauthorized);
            }
            if let Some(recovered) =
                self.recover_terminal_online_exchange(session, principal, state, now_tick)?
            {
                return match recovered {
                    TerminalExchangeRecovery::Completed
                    | TerminalExchangeRecovery::HandoffCompleted => Ok(()),
                    TerminalExchangeRecovery::Cancelled => Err(HostProblem::Cancelled),
                    TerminalExchangeRecovery::TimedOut => Err(HostProblem::TimedOut),
                    TerminalExchangeRecovery::Failed => Err(HostProblem::ProviderFailure),
                };
            }
        }
        let blocked = exchange
            .as_ref()
            .map(|state| self.online_exchange_blocked(state))
            .transpose()?
            .unwrap_or(false);
        if blocked
            && !self.reconcile_online_exchange(
                session,
                exchange
                    .as_mut()
                    .ok_or(HostProblem::InfrastructureFailure)?,
            )?
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let context = match exchange.as_ref() {
            Some(state) => {
                let mut invocation = self.online_exchange_invocation(state)?;
                self.restore_bridge_binding(&mut invocation)?;
                continuation::restore_online_machine_priority(&mut invocation, saved.as_ref());
                self.restore_online_cics_run(state, invocation.clone(), session, now_tick)?;
                CicsTerminalExecution {
                    invocation,
                    transaction: state.transaction.clone(),
                    commarea: state.commarea.clone(),
                    aid: state.aid,
                }
            }
            None => {
                let context = self.cics.terminal_execution(session, principal, now_tick)?;
                let exchange_program = saved
                    .as_ref()
                    .map_or(program, |continuation| continuation.program.as_str());
                exchange = Some(self.begin_online_exchange(session, exchange_program, &context)?);
                context
            }
        };
        #[cfg(feature = "assurance-observers")]
        let terminal_admission =
            cics_read_copyback::TerminalAdmission::capture(&context.invocation);
        let mut invocation = context.invocation;
        if let Some(continuation) = saved.as_ref() {
            invocation
                .provider_generations
                .clone_from(&continuation.provider_generations);
        }
        continuation::restore_online_machine_context(&mut invocation, saved.as_ref())?;
        invocation.bindings.insert(
            "cics.commarea".into(),
            BoundedPayload::new(
                "mainframe-env.cics.commarea@1",
                context.commarea,
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        invocation.bindings.insert(
            "cics.aid".into(),
            BoundedPayload::new(
                "mainframe-env.cics.aid@1",
                vec![context.aid],
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        invocation.bindings.insert(
            "cics.transaction".into(),
            BoundedPayload::new(
                "mainframe-env.cics.transaction@1",
                context.transaction.into_bytes(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
        bind_compatible_runtime_services(&mut invocation)?;
        let mut saved_version = saved.as_ref().map(|saved| saved.version);
        let mut saved_checkpoint = saved.as_ref().map(|saved| saved.checkpoint.clone());
        let mut current = normalize_online_name(
            saved
                .as_ref()
                .map_or(program, |saved| saved.program.as_str()),
            128,
        )?;
        let mut root_idempotency = invocation.idempotency_key.as_str().to_string();
        for frame in 0..invocation.limits.max_frames {
            let artifact = self
                .online_programs
                .lock()
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .get(&current)
                .cloned()
                .ok_or(HostProblem::NotFound)?;
            let record = self
                .artifacts
                .get_artifact(&artifact)
                .map_err(store_error)?
                .ok_or(HostProblem::NotFound)?;
            if frame == 0
                && saved
                    .as_ref()
                    .is_some_and(|continuation| continuation.artifact != artifact)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            let executable = admit_executable_artifact(&record)?;
            invocation.selector =
                Selector::new(format!("program:{current}"), InvocationLimits::default())
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            invocation.artifact = artifact.clone();
            invocation.idempotency_key = IdempotencyKey::new(
                format!("{root_idempotency}:{current}"),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?;
            #[cfg(feature = "assurance-observers")]
            let read_observation = self.bind_cics_read_observation(
                session,
                principal,
                &current,
                &record,
                &invocation,
                terminal_admission.as_ref().filter(|_| frame == 0),
            );
            #[cfg(feature = "assurance-observers")]
            let db2_observation = self.bind_db2_copyback_observation(
                session,
                principal,
                &current,
                &record,
                &invocation,
            );
            let mut machine = ReferenceMachine::from_binary(
                executable.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .map_err(|_| HostProblem::ProviderFailure)?;
            if let Some(checkpoint) = saved_checkpoint.take() {
                machine
                    .restore_checkpoint(&checkpoint)
                    .map_err(|_| HostProblem::InfrastructureFailure)?;
            }
            #[cfg(feature = "assurance-observers")]
            if let Some(observation) = &read_observation {
                observation.enable(&mut machine);
            }
            #[cfg(feature = "assurance-observers")]
            if let Some(observation) = &db2_observation {
                observation.enable(&mut machine);
            }
            let coordinator = ExecutionCoordinator::durable(
                self.host.clone(),
                self.store.clone(),
                CoordinatorLimits {
                    max_quanta: 100,
                    ..CoordinatorLimits::default()
                },
            );
            match coordinator.execute_resumable_with_control(&mut machine, &invocation, || {
                self.program.observe_execution_control(&invocation)
            }) {
                ExecutionOutcome::Completed(_) => {
                    self.program.finish_run_unit(&invocation)?;
                    self.clear_online_machine_continuation(session, saved_version)?;
                    self.finish_online_machine_run(session, principal, now_tick)?;
                    self.clear_online_exchange(
                        session,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    #[cfg(feature = "assurance-observers")]
                    if let Some(observation) = read_observation {
                        observation.finish(self, &invocation, &mut machine);
                    }
                    #[cfg(feature = "assurance-observers")]
                    if let Some(observation) = db2_observation {
                        observation.finish(self, &invocation, &mut machine);
                    }
                    return Ok(());
                }
                ExecutionOutcome::Suspended(suspension) => {
                    return self.finish_online_suspension(
                        session,
                        principal,
                        &current,
                        &artifact,
                        &invocation,
                        &machine,
                        saved_version,
                        &suspension,
                        &coordinator,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        now_tick,
                    );
                }
                ExecutionOutcome::Transfer(transfer) if transfer.replace_frame => {
                    let (next_program, next_artifact) =
                        self.preflight_online_program(transfer.selector.as_str())?;
                    let (next, checkpoint, next_version) = self.stage_online_program_transfer(
                        session,
                        &next_program,
                        &next_artifact,
                        &invocation,
                        &transfer.payload,
                        saved_version,
                        exchange
                            .as_mut()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                        &coordinator,
                        now_tick,
                    )?;
                    self.online_traces
                        .lock()
                        .map_err(|_| HostProblem::InfrastructureFailure)?
                        .entry(session.as_str().into())
                        .or_default()
                        .push(CicsTraceEntry {
                            operation: mainframe_env_host_api::CicsOperation::Retrieve,
                            outcome: format!("TRANSFER {}", transfer.selector.as_str()),
                            response: 0,
                            response2: 0,
                            payload_bytes: 0,
                        });
                    current = next_program;
                    invocation = next;
                    root_idempotency = invocation.idempotency_key.as_str().into();
                    saved_checkpoint = Some(checkpoint);
                    saved_version = Some(next_version);
                }
                ExecutionOutcome::Condition(condition) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: condition.name,
                        response: condition.response,
                        response2: condition.response2,
                    });
                }
                ExecutionOutcome::Abend(abend) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: abend.code,
                        response: -1,
                        response2: 0,
                    });
                }
                ExecutionOutcome::TimedOut => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::TimedOut);
                }
                ExecutionOutcome::Cancelled => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Cancelled);
                }
                ExecutionOutcome::ResourceExhausted(problem) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: format!(
                            "{} at {}",
                            problem.public_message,
                            machine.position_summary()
                        ),
                        response: -3,
                        response2: 0,
                    });
                }
                ExecutionOutcome::ProviderFailure(problem) => {
                    if problem.has_unknown_outcome() {
                        let key = IdempotencyKey::new(
                            format!(
                                "{}:{}",
                                invocation.idempotency_key,
                                machine.effect_sequence()
                            ),
                            InvocationLimits::default(),
                        )
                        .map_err(|_| HostProblem::InfrastructureFailure)?;
                        if self
                            .store
                            .effect(&key)
                            .map_err(store_error)?
                            .is_some_and(|effect| {
                                matches!(
                                    effect.state,
                                    EffectState::Intent | EffectState::UnknownOutcome
                                )
                            })
                            && let Some(state) = exchange.as_mut()
                        {
                            state.blocking_effect = Some(key.as_str().into());
                            let _ = self.persist_online_exchange(session, state);
                        }
                        return Err(HostProblem::UnknownOutcome);
                    }
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: problem.public_message,
                        response: -4,
                        response2: 0,
                    });
                }
                ExecutionOutcome::InfrastructureFailure(_) => {
                    return Err(HostProblem::InfrastructureFailure);
                }
                ExecutionOutcome::Rejected(problem) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Condition {
                        name: format!(
                            "{} at {}",
                            problem.public_message,
                            machine.position_summary()
                        ),
                        response: -2,
                        response2: 0,
                    });
                }
                ExecutionOutcome::Invoke(_) | ExecutionOutcome::Transfer(_) => {
                    self.finish_known_online_failure(
                        session,
                        principal,
                        &invocation,
                        now_tick,
                        saved_version,
                        exchange
                            .as_ref()
                            .ok_or(HostProblem::InfrastructureFailure)?,
                    )?;
                    return Err(HostProblem::Unsupported);
                }
            }
        }
        Err(HostProblem::ResourceExhausted)
    }
}
