//! Read-only prerequisite proof, never a frame lease or a durable handoff.
use super::*;

fn attest_controls(invocation: &Invocation, control: ExecutionControl) -> Result<(), HostProblem> {
    if control.now_tick == 0
        || invocation.deadline_tick == u64::MAX
        || control.now_tick >= invocation.deadline_tick
        || control.cancellation_requested
        || invocation.cancellation_requested()
    {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(())
}

impl TargetStage {
    fn attest_constructor(
        &self,
        receipt: &Receipt,
        source: &Invocation,
        observed: &Transfer,
        selection: &mainframe_env_host_api::ProgramLinkSelection,
        executable: &mainframe_env_compiler_api::ValidatedArtifact,
    ) -> Result<(Invocation, ReferenceMachine), HostProblem> {
        if !self.valid(receipt)
            || self.generation != selection.generation
            || self.content_identity != selection.content_identity
            || self.artifact != selection.artifact.as_str()
            || self.artifact != executable.content_id().to_reference()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let invocation = self.invocation.restore(source.cancellation_probe.clone())?;
        let expected = target_invocation(source, receipt, selection, observed)?;
        if StagedInvocation::capture(&expected)?.digest()? != self.context_digest {
            return Err(HostProblem::UnknownOutcome);
        }
        let machine = ReferenceMachine::from_binary(
            executable.payload(),
            invocation.clone(),
            CodecLimits::default(),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
        let constructor = machine.checkpoint().ok_or(HostProblem::UnknownOutcome)?;
        if constructor.schema() != self.checkpoint_schema
            || STANDARD.encode(constructor.bytes()) != self.checkpoint
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok((invocation, machine))
    }
}

impl CobolProgram {
    /// The embedding supplies the trusted source invocation and its restored
    /// machine. This rechecks retained core/CICS/artifact authorities and exact
    /// inherited context without writes, leases, dispatch, or fence changes.
    /// A returned constructor still requires the CICS logical-frame owner and
    /// installed-instance owner to admit it under their current fences.
    pub(in crate::cobol) fn attest_staged_transfer(
        &self,
        row: &ProviderStateRecord,
        source: &Invocation,
        machine: &ReferenceMachine,
        observed: &Transfer,
    ) -> Result<(Invocation, ReferenceMachine), HostProblem> {
        let DecodedReceipt::Current(receipt) =
            decode_receipt(row).map_err(|_| HostProblem::UnknownOutcome)?
        else {
            return Err(HostProblem::UnknownOutcome);
        };
        if receipt.schema_version != 4
            || row.version != 3
            || receipt.child_execution != source.execution_id.as_str()
            || source.parent_execution_id.as_ref().map(ExecutionId::as_str)
                != Some(receipt.owner_execution.as_str())
            || receipt.owner_run_unit != source.run_unit_id.as_str()
            || receipt.owner_principal != source.principal.id().as_str()
            || source.idempotency_key.as_str() != format!("online-call-effect-{}", row.key)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let store = self.store.get().ok_or(HostProblem::UnknownOutcome)?;
        let execution = store
            .get_execution(&source.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let checkpoint = store
            .get_checkpoint(&source.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let events = store
            .events(&source.execution_id, execution.version, 1)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        receipt
            .transfer
            .as_ref()
            .ok_or(HostProblem::UnknownOutcome)?
            .attest_source(
                source,
                &execution,
                &checkpoint,
                events.first().ok_or(HostProblem::UnknownOutcome)?,
                machine,
                observed,
            )?;
        let key = IdempotencyKey::new(
            format!("{}:{}", source.idempotency_key, machine.effect_sequence()),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
        let effect = store
            .effect(&key)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .ok_or(HostProblem::UnknownOutcome)?;
        let cics = self
            .transfer_owner
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or(HostProblem::UnknownOutcome)?;
        let selection = cics.attested_program_transfer_selection(source, &effect, observed)?;
        let admitted = self
            .preflight_selected_program(observed.selector.as_str(), &selection)
            .map_err(|_| HostProblem::UnknownOutcome)?;
        let target = receipt.target.as_ref().ok_or(HostProblem::UnknownOutcome)?;
        let prepared = target.attest_constructor(
            &receipt,
            source,
            observed,
            &selection,
            &admitted.executable,
        )?;
        attest_controls(
            &prepared.0,
            self.observe_execution_control(source)
                .map_err(|_| HostProblem::UnknownOutcome)?,
        )?;
        // This seam handles only pre-admission, never uncertain target recovery.
        if store
            .get_execution(&prepared.0.execution_id)
            .map_err(|_| HostProblem::UnknownOutcome)?
            .is_some()
            || store
                .get_checkpoint(&prepared.0.execution_id)
                .map_err(|_| HostProblem::UnknownOutcome)?
                .is_some()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        Ok(prepared)
    }
}

#[cfg(test)]
mod tests;
