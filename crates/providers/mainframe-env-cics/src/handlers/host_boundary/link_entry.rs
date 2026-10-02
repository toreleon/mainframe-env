//! Read-only proof of the current selected local LINK loan, never dispatch authority.
use super::*;

#[derive(Debug)]
pub(super) struct SelectedEntry {
    pub(super) origin: Option<CicsOperation>,
    pub(super) source_execution_id: ExecutionId,
    pub(super) source_level: u32,
    pub(super) selection: ProgramLinkSelection,
    pub(super) call: Option<SelectedCall>,
}

#[derive(Debug)]
pub(super) struct SelectedCall {
    pub(super) effect: EffectRequest,
    pub(super) outer_effect_key: IdempotencyKey,
    pub(super) occurrence: u64,
}

/// Immutable observation of the exact effect dispatched at a live local LINK loan.
///
/// This nonserializable value retains request provenance, not the live loan. It
/// grants no admission, store write, dispatch, completion, cleanup or recovery
/// authority. The embedding must independently match the original outer key to
/// its current canonical core Intent and validate its current source lease.
#[derive(Debug)]
pub struct CicsLocalLinkCallAttestation {
    entry: CicsLocalLinkEntryAttestation,
    effect: EffectRequest,
    outer_effect_key: IdempotencyKey,
}

impl CicsLocalLinkCallAttestation {
    /// Return the observed selected entry boundary and its original source.
    pub fn entry(&self) -> &CicsLocalLinkEntryAttestation {
        &self.entry
    }
    /// Return the full actual nested effect observed immediately before host dispatch.
    pub fn effect(&self) -> &EffectRequest {
        &self.effect
    }
    /// Return the caller's original outer CICS effect key, before loan mutation.
    pub fn outer_effect_key(&self) -> &IdempotencyKey {
        &self.outer_effect_key
    }
}

/// Immutable observation of a live, selected local CICS LINK entry boundary.
///
/// This nonserializable observation admits no actor and authorizes no dispatch,
/// storage reset, replacement, CALL completion or recovery. The embedding must
/// separately validate durable execution, instance and CALL authority before use.
/// Retaining this value does not keep its observed loan alive.
#[derive(Debug)]
pub struct CicsLocalLinkEntryAttestation {
    root_invocation: Invocation,
    source_invocation: Invocation,
    target_invocation: Invocation,
    selection: ProgramLinkSelection,
    logical_level: u32,
}

impl CicsLocalLinkEntryAttestation {
    /// Return the originating CICS task invocation, distinct from nested sources.
    pub fn root_invocation(&self) -> &Invocation {
        &self.root_invocation
    }
    /// Return the exact loan parent before entry COMMAREA enrichment.
    pub fn source_invocation(&self) -> &Invocation {
        &self.source_invocation
    }
    /// Return the validated candidate child invocation; no actor is admitted.
    pub fn target_invocation(&self) -> &Invocation {
        &self.target_invocation
    }
    /// Return the exact immutable tuple carried by the actual selected LINK request.
    pub fn selection(&self) -> &ProgramLinkSelection {
        &self.selection
    }
    /// Return the current lower CICS logical level of the observed loan.
    pub fn logical_level(&self) -> u32 {
        self.logical_level
    }
}

fn preserves_controls(source: &Invocation, target: &Invocation) -> bool {
    let bounds = InvocationLimits::default();
    target.execution_id != source.execution_id
        && target.parent_execution_id.as_ref() == Some(&source.execution_id)
        && target.run_unit_id == source.run_unit_id
        && target.principal == source.principal
        && target.attempt == source.attempt && target.attempt != 0
        && target.service_class == source.service_class && target.priority == source.priority
        && target.provider_generations == source.provider_generations
        && target.audit_correlation == source.audit_correlation
        && target.cancellation == source.cancellation
        && target.cancellation_probe == source.cancellation_probe
        && target.deadline_tick != 0 && target.deadline_tick <= source.deadline_tick
        && target.limits.validate().is_ok()
        && target.limits.max_frames <= source.limits.max_frames
        && target.limits.max_steps <= source.limits.max_steps
        && target.limits.max_storage_bytes <= source.limits.max_storage_bytes
        && target.limits.max_output_bytes <= source.limits.max_output_bytes
        && target.limits.max_effects <= source.limits.max_effects
        && target.limits.max_events <= source.limits.max_events
        && target.bindings.len() <= bounds.max_bindings
        && target.bindings.iter().all(|(name, value)| !name.is_empty()
            && name.len() <= bounds.max_binding_bytes && value.bytes().len() <= bounds.max_payload_bytes)
        // Only LINK's entry COMMAREA may differ in the CICS execution envelope.
        // Other subsystem bindings are not attested by this provider.
        && source.bindings.iter().chain(target.bindings.iter()).all(|(name, _)|
            !name.starts_with("cics.") || name == "cics.commarea"
            || source.bindings.get(name) == target.bindings.get(name))
        && target.bindings.get("cics.commarea").is_none_or(|value|
            value.schema() == "mainframe-env.cics.commarea@1")
}

impl CicsService {
    /// Observe the exact selected LINK effect on the current same-thread top loan.
    ///
    /// The source must equal the original caller before entry enrichment; the
    /// entire effect, including its payload, selection, key, sequence and deadline,
    /// must equal the actual request retained by `nested` before host dispatch.
    /// Manual fixture loans without that provenance fail even when their existing
    /// entry proof succeeds. Expired, exited, foreign-thread and cold loans fail.
    /// This read-only proof grants no admission, publication or cleanup authority;
    /// the embedding still owns current core Intent and source lease validation.
    pub fn attest_local_link_call(
        &self,
        source: &Invocation,
        target: &Invocation,
        effect: &EffectRequest,
    ) -> Result<super::CicsLocalLinkCallAttestation, HostProblem> {
        let HostRequest::Program(ProgramRequest::Link {
            selection: Some(selection),
            ..
        }) = &effect.request
        else {
            return Err(HostProblem::Unauthorized);
        };
        let attestation = self.attest_local_link_entry(source, target, selection)?;
        let state = self.lock()?;
        let task = state
            .runs
            .get(&source.run_unit_id)
            .ok_or(HostProblem::Unauthorized)?;
        state
            .task_dispatch
            .require_available_session(&task.session)?;
        let claim = state
            .task_dispatch
            .claims
            .get(&source.run_unit_id)
            .ok_or(HostProblem::Unauthorized)?;
        let loan = claim.loans.last().ok_or(HostProblem::Unauthorized)?;
        let entry = loan.entry.as_ref().ok_or(HostProblem::Unauthorized)?;
        let call = entry.call.as_ref().ok_or(HostProblem::Unauthorized)?;
        if claim.thread != std::thread::current().id()
            || claim.commands != claim.loans.len()
            || claim.command_origin != Some(CicsOperation::Link)
            || claim.session != task.session
            || task.program_abend.is_some()
            || loan.parent != *source
            || entry.source_execution_id != source.execution_id
            || entry.origin != Some(CicsOperation::Link)
            || entry.selection != *selection
            || task.current_program.logical_level != attestation.logical_level
            || task.invocation != attestation.root_invocation
            || loan.actor.as_ref().is_some_and(|actor| actor != target)
            || task.current_program.effect_invocation != *loan.actor.as_ref().unwrap_or(source)
            || call.occurrence == 0
            || call.occurrence > u64::from(source.limits.max_effects)
            || call.effect.sequence != call.occurrence
            || call.effect.run_unit != source.run_unit_id
            || call.effect.deadline_tick != source.deadline_tick
            || call.effect.idempotency_key.is_none()
            || call.effect != *effect
        {
            return Err(HostProblem::Unauthorized);
        }
        Ok(CicsLocalLinkCallAttestation {
            entry: attestation,
            effect: call.effect.clone(),
            outer_effect_key: call.outer_effect_key.clone(),
        })
    }

    /// Observe the live selected local LINK loan without changing task, claim or rows.
    ///
    /// The source must be the original loan parent; entry COMMAREA enrichment is
    /// permitted only on the target. Manual/unselected, foreign-thread, outstanding,
    /// expired and uncertain loans fail closed. No cold state can recreate a loan.
    /// This observation does not grant dispatch or durable/instance/CALL authority.
    pub fn attest_local_link_entry(
        &self,
        source: &Invocation,
        target: &Invocation,
        selection: &ProgramLinkSelection,
    ) -> Result<CicsLocalLinkEntryAttestation, HostProblem> {
        if !preserves_controls(source, target) || target.artifact != selection.artifact {
            return Err(HostProblem::Unauthorized);
        }
        if source.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        if let Some(clock) = &self.replay_clock {
            let tick = clock.now_tick()?;
            if tick == 0 {
                return Err(HostProblem::InfrastructureFailure);
            }
            if tick >= target.deadline_tick {
                return Err(HostProblem::TimedOut);
            }
        }
        if source
            .bindings
            .get("cics.execution-context")
            .is_some_and(|value| {
                value.schema() != "mainframe-env.cics.execution-context@1"
                    || value.bytes() != b"local"
            })
        {
            return Err(HostProblem::Unauthorized);
        }
        let state = self.lock()?;
        let task = state
            .runs
            .get(&source.run_unit_id)
            .ok_or(HostProblem::Unauthorized)?;
        state
            .task_dispatch
            .require_available_session(&task.session)?;
        if task.program_abend.is_some() {
            return Err(HostProblem::UnknownOutcome);
        }
        let claim = state
            .task_dispatch
            .claims
            .get(&source.run_unit_id)
            .ok_or(HostProblem::Unauthorized)?;
        if claim.thread != std::thread::current().id()
            || claim.commands != claim.loans.len()
            || claim.command_origin != Some(CicsOperation::Link)
            || claim.session != task.session
        {
            return Err(HostProblem::Unauthorized);
        }
        let loan = claim.loans.last().ok_or(HostProblem::Unauthorized)?;
        let entry = loan.entry.as_ref().ok_or(HostProblem::Unauthorized)?;
        if entry.origin != Some(CicsOperation::Link)
            || loan.parent != *source
            || entry.source_execution_id != source.execution_id
            || entry.selection != *selection
            || loan.artifact.as_ref() != Some(&selection.artifact)
            || super::super::task_context::current_program(target).as_ref() != Some(&loan.program)
            || target.selector.as_str() != format!("program:{}", loan.program)
            || task.current_program.current.as_ref() != Some(&loan.program)
            || entry.source_level.checked_add(1) != Some(task.current_program.logical_level)
            || task.current_program.logical_level as usize != claim.loans.len() + 1
            || task.current_program.parent_execution_id.as_ref() != Some(&source.execution_id)
            || task.invocation.run_unit_id != source.run_unit_id
            || task.invocation.principal != source.principal
            || task.current_program.initial_entry
            || target.bindings.get("cics.session").is_some_and(|value| {
                value.schema() != "mainframe-env.cics.session@1"
                    || value.bytes() != task.session.as_bytes()
            })
        {
            return Err(HostProblem::Unauthorized);
        }
        match &loan.actor {
            Some(actor) if actor != target || task.current_program.effect_invocation != *actor => {
                return Err(HostProblem::IdempotencyConflict);
            }
            None if task.current_program.effect_invocation != *source => {
                return Err(HostProblem::Unauthorized);
            }
            _ => {}
        }
        Ok(CicsLocalLinkEntryAttestation {
            root_invocation: task.invocation.clone(),
            source_invocation: source.clone(),
            target_invocation: target.clone(),
            selection: selection.clone(),
            logical_level: task.current_program.logical_level,
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod call_tests;
