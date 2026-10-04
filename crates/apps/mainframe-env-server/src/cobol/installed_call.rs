//! Original admitted compiled CALL execution and optional guarded MQ session.
use super::*;

impl CobolProgram {
    pub(super) fn execute_admitted(
        &self,
        parent: &Invocation,
        program: &str,
        admitted: AdmittedProgram,
        payload: &BoundedPayload,
        identity: &str,
        writes: &mut Vec<ProviderStateWrite>,
        original_call: Option<&replay::WinningInstalledCall<'_>>,
    ) -> Result<BoundedPayload, HostProblem> {
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let name = admitted.name.clone();
        let call_values = decode_cobol_call_values(payload)?;
        let limits = InvocationLimits::default();
        let sequence = identity;
        let mut bindings = parent.bindings.clone();
        replay::bind_protocol_owner(parent, &mut bindings)?;
        bindings.insert("cobol.call.arguments".into(), payload.clone());
        let invocation = Invocation::new(
            RequestId::new(format!("online-call-request-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            ExecutionId::new(format!("online-call-execution-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.run_unit_id.clone(),
            Some(parent.execution_id.clone()),
            Selector::new(format!("program:{}", program.to_ascii_uppercase()), limits)
                .map_err(|_| HostProblem::Malformed)?,
            admitted.artifact.clone(),
            Principal::new(
                parent.principal.id().clone(),
                parent.principal.grants().clone(),
                limits,
            )
            .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.service_class,
            parent.priority,
            parent.deadline_tick,
            TraceId::new(format!("online-call-trace-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            IdempotencyKey::new(format!("online-call-effect-{sequence}"), limits)
                .map_err(|_| HostProblem::InfrastructureFailure)?,
            parent.attempt,
            parent.limits,
            bindings,
            limits,
        )
        .and_then(|invocation| {
            invocation.with_provider_generations(parent.provider_generations.clone(), limits)
        })
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        let mut invocation = with_compatible_runtime_services(invocation)?;
        invocation.cancellation = parent.cancellation.clone();
        invocation.cancellation_probe = parent.cancellation_probe.clone();
        let mut session = self.admit_batch_mqi(&mut invocation, Some(&admitted), original_call)?;
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut machine = ReferenceMachine::from_binary(
                admitted.executable.payload(),
                invocation.clone(),
                CodecLimits::default(),
            )
            .map_err(|_| HostProblem::ProviderFailure)?;
            if let Some(session) = &session {
                machine
                    .bind_mqi_program_frame(session.frame(&invocation)?)
                    .map_err(|problem| match problem {
                        mainframe_env_interpreter::MachineProblem::Host(problem) => problem,
                        _ => HostProblem::ProviderFailure,
                    })?;
            }
            let cursor_key = format!("{}:{name}", parent.run_unit_id.as_str());
            let cursor_record = store
                .get_provider_state("batch-file-cursor", &cursor_key)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let cursor_version = cursor_record.as_ref().map(|record| record.version);
            if original_call.is_some_and(replay::WinningInstalledCall::is_native)
                && cursor_record.is_some()
            {
                return Err(HostProblem::Unsupported);
            }
            let loaded_cursors = cursor_record
                .map(|record| {
                    serde_json::from_slice(&record.payload)
                        .map_err(|_| HostProblem::InfrastructureFailure)
                })
                .transpose()?
                .unwrap_or_default();
            machine
                .install_dataset_cursors(loaded_cursors)
                .map_err(|_| HostProblem::ResourceExhausted)?;
            let lease = instance::Lease::acquire(store.as_ref(), &invocation, &name, &mut machine)?;
            let coordinator = ExecutionCoordinator::durable(
                Arc::clone(self.host.get().ok_or(HostProblem::InfrastructureFailure)?),
                Arc::clone(store),
                CoordinatorLimits::default(),
            );
            Ok((machine, cursor_key, cursor_version, lease, coordinator))
        }))
        .unwrap_or(Err(HostProblem::UnknownOutcome));
        let (mut machine, cursor_key, cursor_version, lease, coordinator) = match prepared {
            Ok(prepared) => prepared,
            Err(problem) => {
                return Err(session
                    .as_mut()
                    .map_or(problem.clone(), |session| session.abort(problem)));
            }
        };
        let mut parent_tick = session.as_ref().map_or(0, |session| session.observed_tick);
        let observe = || {
            let control = self.observe_execution_control(&invocation)?;
            if let Some(call) = original_call
                && session.is_some()
            {
                mqi::recheck_driving_parent(
                    self,
                    call,
                    control,
                    &mut parent_tick,
                    session
                        .as_ref()
                        .and_then(|session| session.original_core.as_ref())
                        .ok_or(ExecutionControlError::Unavailable)?,
                )
                .map_err(|_| ExecutionControlError::Unavailable)?;
            }
            Ok(control)
        };
        let outcome = match original_call.and_then(replay::WinningInstalledCall::native_enrollment)
        {
            Some(enrollment) => coordinator.execute_enrolled_child_with_control(
                &mut machine,
                &invocation,
                enrollment,
                observe,
            ),
            None => coordinator.execute_with_control(&mut machine, &invocation, observe),
        };
        if let Some(session) = &mut session {
            // Observe untouched raw outcome before cursor/linkage/CALL mapping.
            session.finish(&outcome)?;
        }
        let cursor_result = persist_batch_file_cursors(
            store.as_ref(),
            &cursor_key,
            machine.dataset_cursors(),
            cursor_version,
        );
        // A secondary persistence failure must not erase an in-doubt effect.
        if matches!(&outcome, ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome())
        {
            return Err(HostProblem::UnknownOutcome);
        }
        cursor_result
            .map_err(|problem| replay::preserve_control_cursor_failure(&outcome, problem))?;
        match outcome {
            ExecutionOutcome::Completed(_) => {
                let mut values = machine
                    .linkage_values()
                    .map_err(|_| HostProblem::ProviderFailure)?;
                values.truncate(call_values.len());
                let result =
                    encode_cobol_call_result(&values).map_err(|_| HostProblem::UnknownOutcome)?;
                writes.extend(lease.completed(store.as_ref(), &machine)?);
                Ok(result)
            }
            ExecutionOutcome::Condition(condition) => Err(HostProblem::Condition {
                name: condition.name,
                response: condition.response,
                response2: condition.response2,
            }),
            ExecutionOutcome::Cancelled => Err(HostProblem::Cancelled),
            ExecutionOutcome::TimedOut => Err(HostProblem::TimedOut),
            ExecutionOutcome::ResourceExhausted(_) => Err(HostProblem::ResourceExhausted),
            ExecutionOutcome::ProviderFailure(problem) if problem.has_unknown_outcome() => {
                Err(HostProblem::UnknownOutcome)
            }
            ExecutionOutcome::ProviderFailure(problem) => Err(HostProblem::Condition {
                name: format!("BATCH-PROVIDER:{}", problem.public_message),
                response: -6,
                response2: 0,
            }),
            ExecutionOutcome::InfrastructureFailure(_) => Err(HostProblem::InfrastructureFailure),
            ExecutionOutcome::Abend(_) => {
                lease
                    .abended(store.as_ref(), &invocation, &machine)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
                Err(HostProblem::Condition {
                    name: "INSTALLED-CALL-ABEND".into(),
                    response: -1,
                    response2: 0,
                })
            }
            ExecutionOutcome::Rejected(problem) => Err(HostProblem::Condition {
                name: format!("INSTALLED-CALL-REJECTED:{}", problem.public_message),
                response: -2,
                response2: 0,
            }),
            ExecutionOutcome::Transfer(transfer) => {
                replay::persist_transfer_intent(
                    store.as_ref(),
                    &invocation,
                    identity,
                    &machine,
                    &transfer,
                )?;
                self.stage_transfer_target(&invocation, identity, &machine, &transfer)?;
                Err(HostProblem::UnknownOutcome)
            }
            // The durable child is suspended and its CALL/instance are still
            // unresolved. Without an owned continuation/replacement protocol,
            // this cannot be a known condition a caller may handle as a return.
            ExecutionOutcome::Suspended(_) | ExecutionOutcome::Invoke(_) => {
                Err(HostProblem::UnknownOutcome)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn execute_installed(
        &self,
        parent: &Invocation,
        program: &str,
        payload: &BoundedPayload,
        identity: &str,
        writes: &mut Vec<ProviderStateWrite>,
    ) -> Result<BoundedPayload, HostProblem> {
        let admitted = self.preflight_installed_program(program, false)?;
        self.execute_admitted(parent, program, admitted, payload, identity, writes, None)
    }
}
