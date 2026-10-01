use super::*;

impl ReferenceMachine {
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
