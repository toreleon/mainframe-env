//! Initial compiled selection under the actual terminal task's command custody.
//! No protected storage-root, coordinator Intent or MQ authority is minted here.
use super::*;
use crate::service::handlers::program_control;
use crate::service::handlers::transaction_definition;
use mainframe_env_execution_api::{IdempotencyKey, Selector};

#[cfg(test)]
mod tests;

fn terminal_ready(run: &Run, original: &Invocation, now_tick: u64) -> Result<(), HostProblem> {
    if run.program_abend.is_some() {
        return Err(HostProblem::UnknownOutcome);
    }
    if original.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if now_tick >= original.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    let limits = InvocationLimits::default();
    if original.bindings.len() > limits.max_bindings
        || original.bindings.iter().any(|(name, value)| {
            name.is_empty()
                || name.len() > limits.max_binding_bytes
                || value.bytes().len() > limits.max_payload_bytes
        })
    {
        return Err(HostProblem::ResourceExhausted);
    }
    if run.invocation != *original
        || run.current_program.effect_invocation != *original
        || run.current_program.current.is_some()
        || run.current_program.program_occurrence != 0
        || run.current_program.logical_level != 1
        || run.current_program.parent_execution_id.is_some()
        || original.parent_execution_id.is_some()
        || original.selector.as_str() != format!("cics:{}", run.transaction)
        || original
            .bindings
            .contains_key(CICS_NESTED_EFFECT_ORIGIN_BINDING)
        || original
            .bindings
            .contains_key(CICS_OUTER_EFFECT_ORIGIN_BINDING)
        || original
            .bindings
            .get("cics.execution-context")
            .is_none_or(|value| {
                value.schema() != "mainframe-env.cics.execution-context@1"
                    || value.bytes() != b"local"
            })
        || original.bindings.get("cics.session").is_none_or(|value| {
            value.schema() != "mainframe-env.cics.session@1"
                || value.bytes() != run.session.as_bytes()
        })
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok(())
}

fn enrich(
    run: &Run,
    session: &Session,
    program: &str,
    selection: &ProgramLinkSelection,
) -> Result<Invocation, HostProblem> {
    let limits = InvocationLimits::default();
    let added = [
        "cics.program-entry",
        "cics.commarea",
        "cics.aid",
        "cics.transaction",
        "cobol.runtime-service.CEEDAYS",
        "cobol.runtime-service.COBDATFT",
        "cobol.runtime-service.MVSWAIT",
        "cobol.runtime-service.CEE3ABD",
    ]
    .into_iter()
    .filter(|name| !run.invocation.bindings.contains_key(*name))
    .count();
    if run.retrieve.len() > limits.max_payload_bytes
        || run
            .invocation
            .bindings
            .len()
            .checked_add(added)
            .is_none_or(|total| total > limits.max_bindings)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut expected = run.invocation.clone();
    expected.selector = Selector::new(format!("program:{program}"), limits)
        .map_err(|_| HostProblem::ResourceExhausted)?;
    expected.artifact = selection.artifact.clone();
    expected.idempotency_key = IdempotencyKey::new(
        format!("{}:{program}", run.invocation.idempotency_key.as_str()),
        limits,
    )
    .map_err(|_| HostProblem::ResourceExhausted)?;
    // These are exactly the initial online_machine/continuation bindings.
    // They are reconstructed from the held Run and actual session, never an
    // offered replacement context or an arbitrary binding allow-list.
    for (name, schema, bytes) in [
        (
            "cics.program-entry",
            "mainframe-env.cics.program-entry@1",
            b"initial".to_vec(),
        ),
        (
            "cics.commarea",
            "mainframe-env.cics.commarea@1",
            run.retrieve.clone(),
        ),
        ("cics.aid", "mainframe-env.cics.aid@1", vec![session.aid]),
        (
            "cics.transaction",
            "mainframe-env.cics.transaction@1",
            run.transaction.as_bytes().to_vec(),
        ),
    ] {
        expected.bindings.insert(
            name.into(),
            BoundedPayload::new(schema, bytes, limits)
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    // Existing finite compatible runtime selectors. No grant, generation or
    // runtime provider is installed or inferred by admitting these bytes.
    for name in ["CEEDAYS", "COBDATFT", "MVSWAIT", "CEE3ABD"] {
        let key = format!("cobol.runtime-service.{name}");
        let bytes = format!("le:{name}:1").into_bytes();
        if let Some(value) = expected.bindings.get(&key) {
            if value.schema() != "mainframe-env.runtime-service-selector@1"
                || value.bytes() != bytes
            {
                return Err(HostProblem::Malformed);
            }
        } else {
            expected.bindings.insert(
                key,
                BoundedPayload::new("mainframe-env.runtime-service-selector@1", bytes, limits)
                    .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
    }
    if expected.bindings.len() > limits.max_bindings {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(expected)
}

impl CicsService {
    /// Enter the installed initial program while retaining the actual terminal Run.
    ///
    /// This opt-in boundary accepts only a fresh root terminal entry. It resolves
    /// the existing durable transaction/program rows and actual immutable program
    /// definition/artifact, then compares the whole offered machine actor to the
    /// finite ordinary initial-entry transformation of the real held Run. It
    /// never registers a program/task, replaces the Run, admits an XCTL/LINK or
    /// creates a protected driver claim. The selection argument is observation
    /// of the frozen installed generation, not an independent authority token.
    ///
    /// Use only around one non-CICS external dispatch. The actual command claim
    /// intentionally refuses ordinary CICS command reentrancy. Unknown, panic or
    /// failed restoration fences the session before that claim is released.
    pub fn with_initial_terminal_program_loan<T, F>(
        &self,
        original: &Invocation,
        machine_actor: &Invocation,
        now_tick: u64,
        body: F,
    ) -> Result<T, HostProblem>
    where
        F: for<'a> FnOnce(
            &CicsCurrentProgramLoan<'a>,
            &ProgramLinkSelection,
        ) -> Result<T, HostProblem>,
    {
        {
            let state = self.lock()?;
            let run = state
                .runs
                .get(&original.run_unit_id)
                .ok_or(HostProblem::Unauthorized)?;
            terminal_ready(run, original, now_tick)?;
            command_ready(&state, &original.run_unit_id)?;
            if state.task_dispatch.active(&original.run_unit_id) {
                return Err(HostProblem::Unauthorized);
            }
        }
        let mut command = CommandLease::acquire_with_origin(self, &original.run_unit_id, None)?;
        terminal_ready(&command, original, now_tick)?;
        let program = self.local_transaction_program(&command.transaction)?;
        let installed = transaction_definition::installed_program_artifact(self, &program)?
            .ok_or(HostProblem::Unsupported)?;
        let selection = program_control::local_link_selection(self, &program)?
            .ok_or(HostProblem::Unsupported)?;
        if selection.artifact != installed {
            return Err(HostProblem::IdempotencyConflict);
        }
        let (session, definition) = {
            let state = self.lock()?;
            let session = state
                .sessions
                .get(&command.session)
                .cloned()
                .ok_or(HostProblem::NotFound)?;
            if !session.connected
                || session.principal != original.principal.id().as_str()
                || session.run_unit != original.run_unit_id.as_str()
                || session.transaction != command.transaction
            {
                return Err(HostProblem::Unauthorized);
            }
            if now_tick >= session.expires_at_tick {
                return Err(HostProblem::TimedOut);
            }
            let definition = state
                .program_definitions
                .get(&program)
                .and_then(|generations| generations.last_key_value())
                .map(|(_, definition)| definition.clone())
                .ok_or(HostProblem::Unsupported)?;
            if definition.artifact != selection.artifact
                || definition.generation != selection.generation
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            (session, definition)
        };
        let expected = enrich(&command, &session, &program, &selection)?;
        if expected != *machine_actor {
            return Err(HostProblem::Unauthorized);
        }
        if self.local_transaction_program(&command.transaction)? != program
            || transaction_definition::installed_program_artifact(self, &program)?.as_ref()
                != Some(&selection.artifact)
            || program_control::local_link_selection(self, &program)?.as_ref() != Some(&selection)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        {
            let state = self.lock()?;
            state
                .task_dispatch
                .require_available_session(&command.session)?;
            if state
                .sessions
                .get(&command.session)
                .is_none_or(|current| current.version != session.version)
                || state
                    .program_definitions
                    .get(&program)
                    .and_then(|generations| generations.last_key_value())
                    .is_none_or(|(_, current)| *current != definition)
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            // The real initial frame is bound before the first external effect.
            // Every unrelated Run/frame field is retained byte-for-byte.
            command.current_program.effect_invocation = expected;
            command.current_program.current = Some(program);
            command.current_program.initial_entry = true;
        }
        let mut fence = ExitFence {
            service: self,
            session: command.session.clone(),
            armed: true,
        };
        let result = {
            let loan = CicsCurrentProgramLoan {
                service: self,
                run: &command,
                _thread_confined: PhantomData,
            };
            loan.require_current(machine_actor, now_tick)?;
            body(&loan, &selection)
        };
        let unknown = matches!(&result, Err(HostProblem::UnknownOutcome));
        if unknown {
            fence.retain()?;
        }
        #[cfg(test)]
        if let Some(observe) = BEFORE_RESTORE.with(|slot| slot.borrow_mut().take()) {
            observe(self, machine_actor);
        }
        if command.restore().is_err() {
            let _ = fence.retain();
            return Err(HostProblem::UnknownOutcome);
        }
        if !unknown {
            fence.armed = false;
        }
        result
    }
}
