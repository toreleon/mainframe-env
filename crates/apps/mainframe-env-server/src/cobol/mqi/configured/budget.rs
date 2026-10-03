//! Conservative retained-host-memory charge, not a canonical encoding/digest.
use super::*;

pub(super) fn charge(original: &Invocation, ceiling: usize) -> Result<usize, HostProblem> {
    let mut bytes = std::mem::size_of::<Invocation>();
    let mut add = |length: usize| -> Result<(), HostProblem> {
        bytes = bytes
            .checked_add(length)
            .ok_or(HostProblem::ResourceExhausted)?;
        if bytes > ceiling {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(())
    };
    for value in [
        original.request_id.as_str(),
        original.execution_id.as_str(),
        original.run_unit_id.as_str(),
        original.selector.as_str(),
        original.artifact.as_str(),
        original.principal.id().as_str(),
        original.trace_id.as_str(),
        original.idempotency_key.as_str(),
        original.audit_correlation.as_str(),
    ] {
        add(value.len())?;
    }
    if let Some(parent) = &original.parent_execution_id {
        add(parent.as_str().len())?;
    }
    if let Some(cancellation) = &original.cancellation {
        add(cancellation.id.as_str().len())?;
        add(cancellation.reason.len())?;
    }
    for capability in original.principal.grants() {
        add(128)?;
        add(capability.as_str().len())?;
    }
    for (capability, generation) in &original.provider_generations {
        add(128)?;
        add(capability.as_str().len())?;
        add(generation.len())?;
    }
    for (name, payload) in &original.bindings {
        add(128)?;
        add(name.len())?;
        add(payload.schema().len())?;
        add(payload.bytes().len())?;
    }
    // Cover the host/facet's bounded snapshot copies and tree/Arc overhead.
    // Shared payload bytes are charged conservatively, never deep-cloned here.
    let charged = bytes
        .checked_add(512)
        .and_then(|n| n.checked_mul(8))
        .ok_or(HostProblem::ResourceExhausted)?;
    if charged > ceiling {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(charged)
}
impl Topology {
    pub(super) fn reserve_bytes(
        &mut self,
        charge: usize,
        ceiling: usize,
    ) -> Result<(), HostProblem> {
        let next = self
            .bytes
            .checked_add(charge)
            .ok_or(HostProblem::ResourceExhausted)?;
        if next > ceiling {
            return Err(HostProblem::ResourceExhausted);
        }
        self.bytes = next;
        Ok(())
    }
    pub(super) fn release_bytes(&mut self, charge: usize) -> Result<(), HostProblem> {
        self.bytes = self
            .bytes
            .checked_sub(charge)
            .ok_or(HostProblem::InfrastructureFailure)?;
        Ok(())
    }
}
