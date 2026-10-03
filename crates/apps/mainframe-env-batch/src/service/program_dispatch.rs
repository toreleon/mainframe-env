//! One original Program controller request builder for ordinary/opt-in dispatch.
use super::*;

impl BatchService {
    pub(super) fn execute_program_controller(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        program: &str,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        invocation.check()?;
        self.dispatch_program(invocation, job, step, input, effect_sequence, program, None)
    }

    pub(super) fn execute_program_controller_admitted(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        program: &str,
        dispatch: &mut ProgramDispatch<'_>,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        invocation.check()?;
        self.dispatch_program(
            invocation,
            job,
            step,
            input,
            effect_sequence,
            program,
            Some(dispatch),
        )
    }

    fn dispatch_program(
        &self,
        invocation: &(impl RunInput + ?Sized),
        job: &Job,
        step: &StepPlan,
        input: &ProgramInput,
        effect_sequence: &mut u64,
        program: &str,
        dispatch: Option<&mut ProgramDispatch<'_>>,
    ) -> Result<crate::ProgramOutput, HostProblem> {
        invocation.check()?;
        let payload = BoundedPayload::new(
            "mainframe-env.program.input@1",
            serde_json::to_vec(input).map_err(|_| HostProblem::ProviderFailure)?,
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?;
        let sequence = next_effect_sequence(invocation, effect_sequence)?;
        let key = effect_key(job, step, sequence)?;
        let mut program_invocation = invocation.original().clone();
        let limits = InvocationLimits::default();
        let binding = BoundedPayload::new(
            "mainframe-env.jes-work@1",
            format!("jes:{}", job.id).into_bytes(),
            limits,
        )
        .map_err(|_| HostProblem::ResourceExhausted)?;
        if let Some(existing) = program_invocation.bindings.get("jes.work-id") {
            if existing != &binding {
                return Err(HostProblem::Malformed);
            }
        } else if program_invocation.bindings.len() >= limits.max_bindings {
            return Err(HostProblem::ResourceExhausted);
        }
        program_invocation
            .bindings
            .insert("jes.work-id".into(), binding);
        let request = EffectRequest {
            run_unit: invocation.original().run_unit_id.clone(),
            sequence,
            deadline_tick: invocation.original().deadline_tick,
            idempotency_key: Some(key),
            request: HostRequest::Program(ProgramRequest::Call {
                program: ProgramName::new(program, 128).map_err(|_| HostProblem::Malformed)?,
                payload,
                service: None,
            }),
        };
        let result = if invocation.all_effects() {
            let owner = running_step::StepOwner::admit(self, invocation, job, step, program)?;
            let admission = owner.borrow();
            let result =
                effect_loan::dispatch(invocation, request, Some((&program_invocation, &admission)));
            drop(owner);
            result
        } else if let Some(dispatch) = dispatch {
            let owner = running_step::StepOwner::admit(self, invocation, job, step, program)?;
            let admission = owner.borrow();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                dispatch(&admission, &program_invocation, request)
            }))
            .unwrap_or(EffectResult {
                sequence,
                outcome: Err(HostProblem::UnknownOutcome),
            });
            // Irrevocable revocation precedes decode, output/disposition and retirement.
            // Drop also covers unwind; it performs no host action or durable decision.
            drop(owner);
            if result.sequence != sequence {
                return Err(invocation.poison(HostProblem::UnknownOutcome));
            }
            if result.outcome == Err(HostProblem::UnknownOutcome) {
                return Err(invocation.poison(HostProblem::UnknownOutcome));
            }
            if let Err(
                problem @ (HostProblem::Cancelled
                | HostProblem::TimedOut
                | HostProblem::InfrastructureFailure),
            ) = &result.outcome
            {
                return Err(invocation.poison(problem.clone()));
            }
            invocation.check()?;
            result
        } else {
            self.invoke_host(
                &run_stop::Projected {
                    original: &program_invocation,
                    scope: invocation,
                },
                invocation.original().deadline_tick.saturating_sub(1),
                false,
                request,
            )
        };
        match result.outcome? {
            HostResult::Program(payload) => decode_program_output(&payload),
            _ => Err(HostProblem::ProviderFailure),
        }
    }
}
