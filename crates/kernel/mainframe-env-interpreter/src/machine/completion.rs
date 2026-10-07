use super::*;

impl ReferenceMachine {
    pub(super) fn checkpoint_bytes(&self) -> Option<Vec<u8>> {
        if self.pending.is_some() || self.mqi.is_some() {
            return None;
        }
        snapshot_codec::encode_snapshot(&self.snapshot())
    }

    pub(super) fn scoped_completion_checkpoint(&self) -> Option<BoundedPayload> {
        // Presence is opt-in only. Storage authority is validated by the owner.
        let binding = self.invocation.bindings.get("cobol.storage-entry")?;
        if binding.schema() != "mainframe-env.cobol.storage-entry@1" {
            return None;
        }
        self.attest_installed_program_return().ok()?;
        self.checkpoint()
    }

    pub(super) fn complete_installed_step(
        &mut self,
        operation: &Operation,
    ) -> Result<MachineDrive<EffectRequest>, MachineProblem> {
        let completion = self.complete()?;
        self.normal_return = normal_return::NormalReturnMarker::completed(
            operation,
            self.pc,
            self.executed_steps,
            completion.return_code,
        );
        Ok(MachineDrive::Completed(completion))
    }

    pub(super) fn interrupted_drive(
        &mut self,
        category: FailureCategory,
    ) -> MachineDrive<EffectRequest> {
        self.release_storage64_task();
        failure_drive(
            category,
            match category {
                FailureCategory::Cancelled => "execution cancelled",
                FailureCategory::TimedOut => "execution timed out",
                _ => "execution interrupted",
            },
        )
    }

    pub(super) fn complete(&mut self) -> Result<Completion, MachineProblem> {
        let limits = InvocationLimits {
            max_payload_bytes: self.invocation.limits.max_output_bytes as usize,
            ..InvocationLimits::default()
        };
        let completion = Completion {
            return_code: match self.implicit.get("RETURN-CODE") {
                Some(CobolValue::Decimal(value)) if value.scale == 0 => {
                    i32::try_from(value.coefficient).map_err(|_| MachineProblem::SizeError)?
                }
                _ => 0,
            },
            output: BoundedPayload::new("mainframe-env.output@1", self.output.clone(), limits)
                .map_err(|_| MachineProblem::ResourceExhausted)?,
        };
        self.release_storage64_task();
        typed_cics::container_set::release_all(self);
        Ok(completion)
    }
}
