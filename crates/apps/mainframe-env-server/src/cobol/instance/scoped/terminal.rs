//! Bind a fresh live native return to its exact core terminal publication.
//! This read observation cannot authorize a close or replace a publication CAS.
use super::live::LiveLease;
use super::*;
use mainframe_env_execution_api::{LifecycleEvent, LifecycleEventKind, Machine};
use mainframe_env_interpreter::{ExecutionControl, InstalledProgramReturn};
use mainframe_env_store_api::{CheckpointRecord, ExecutionRecord, ExecutionState};
use sha2::Digest;

#[cfg(test)]
mod tests;

pub(super) struct TerminalObservation {
    core: ExecutionRecord,
    checkpoint: CheckpointRecord,
    event: LifecycleEvent,
    witness: InstalledProgramReturn,
    member: ProviderStateRecord,
}

impl TerminalObservation {
    pub(super) fn capture(
        actor: &Invocation,
        machine: &ReferenceMachine,
        lease: &LiveLease,
        store: &dyn PlatformStore,
        control: ExecutionControl,
    ) -> Result<Self, HostProblem> {
        if actor != lease.invocation() {
            return Err(HostProblem::UnknownOutcome);
        }
        lease.entry().validate_for(actor)?;
        let member = lease.record()?;
        if store
            .get_provider_state(&member.namespace, &member.key)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .as_ref()
            != Some(&member)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let witness = machine
            .attest_installed_program_return()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if witness.invocation() != actor {
            return Err(HostProblem::UnknownOutcome);
        }
        if control.cancellation_requested || actor.cancellation_requested() {
            return Err(HostProblem::Cancelled);
        }
        if control.now_tick == 0 || control.now_tick > i64::MAX as u64 {
            return Err(HostProblem::UnknownOutcome);
        }
        if control.now_tick >= actor.deadline_tick {
            return Err(HostProblem::TimedOut);
        }
        let image = machine
            .completion_checkpoint()
            .ok_or(HostProblem::UnknownOutcome)?;
        let core = store
            .get_execution(&actor.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let tick = core.terminal_tick.ok_or(HostProblem::UnknownOutcome)?;
        if core.execution_id != actor.execution_id
            || core.run_unit_id != actor.run_unit_id
            || core.selector != actor.selector
            || core.artifact != actor.artifact
            || core.principal != *actor.principal.id()
            || core.attempt != actor.attempt
            || core.state != ExecutionState::Completed
            || core.version == 0
            || core.version > i64::MAX as u64
            || core.owner_lease.is_some()
            || core.lease_expiry_tick.is_some()
            || tick == 0
            || tick > control.now_tick
            || tick >= actor.deadline_tick
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let events = store
            .events(&actor.execution_id, core.version, 1)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        if events.len() != 1 {
            return Err(HostProblem::UnknownOutcome);
        }
        let event = events
            .into_iter()
            .next()
            .ok_or(HostProblem::UnknownOutcome)?;
        let return_code = witness.return_code();
        if event.execution_id != actor.execution_id
            || event.run_unit_id != actor.run_unit_id
            || event.sequence != core.version
            || event.attempt != actor.attempt
            || event.tick != tick
            || event.kind != (LifecycleEventKind::Completed { return_code })
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let checkpoint = store
            .get_checkpoint(&actor.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let expected = CheckpointRecord {
            execution_id: actor.execution_id.clone(),
            run_unit_id: actor.run_unit_id.clone(),
            session_id: None,
            schema_version: 1,
            machine_schema_version: 1,
            artifact: actor.artifact.clone(),
            provider_generation: mainframe_env_interpreter::INTERPRETER_GENERATION.into(),
            required_host_interfaces: BTreeMap::from([
                ("mainframe-env.execution-api".into(), "1".into()),
                ("mainframe-env.host-api".into(), "1".into()),
            ]),
            effect_sequence: machine.effect_sequence(),
            transaction: None,
            principal: actor.principal.id().clone(),
            security_classification: "application-data".into(),
            encryption_key_reference: None,
            payload_size: image.bytes().len() as u64,
            payload_digest: sha2::Sha256::digest(image.bytes()).into(),
            payload: image.bytes().to_vec(),
        };
        if checkpoint != expected {
            return Err(HostProblem::UnknownOutcome);
        }
        // Recheck the current lease after the complete bounded read set. A later
        // publisher must still validate its root/member/CALL CAS and quiescence.
        if lease.record()? != member {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(Self {
            core,
            checkpoint,
            event,
            witness,
            member,
        })
    }
    pub(super) fn core(&self) -> &ExecutionRecord {
        &self.core
    }
    pub(super) fn checkpoint(&self) -> &CheckpointRecord {
        &self.checkpoint
    }
    pub(super) fn event(&self) -> &LifecycleEvent {
        &self.event
    }
    pub(super) fn witness(&self) -> &InstalledProgramReturn {
        &self.witness
    }
    pub(super) fn member(&self) -> &ProviderStateRecord {
        &self.member
    }
}
