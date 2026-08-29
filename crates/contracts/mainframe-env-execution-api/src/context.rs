use crate::{
    ArtifactRef, CancellationId, CapabilityId, ExecutionId, IdempotencyKey, PrincipalId, RequestId,
    RunUnitId, Selector, TraceId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationLimits {
    pub max_identity_bytes: usize,
    pub max_capabilities: usize,
    pub max_payload_bytes: usize,
    pub max_bindings: usize,
    pub max_binding_bytes: usize,
}

impl Default for InvocationLimits {
    fn default() -> Self {
        Self {
            max_identity_bytes: 128,
            max_capabilities: 128,
            max_payload_bytes: 1024 * 1024,
            max_bindings: 128,
            max_binding_bytes: 4096,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceLimits {
    pub max_steps: u64,
    pub max_storage_bytes: u64,
    pub max_output_bytes: u64,
    pub max_frames: u32,
    pub max_effects: u64,
    pub max_events: u64,
}

impl ResourceLimits {
    pub fn validate(self) -> Result<Self, InvocationProblem> {
        if self.max_steps == 0
            || self.max_storage_bytes == 0
            || self.max_output_bytes == 0
            || self.max_frames == 0
            || self.max_effects == 0
            || self.max_events == 0
        {
            Err(InvocationProblem::InvalidResourceLimits)
        } else {
            Ok(self)
        }
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_storage_bytes: 64 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            max_frames: 256,
            max_effects: 65_536,
            max_events: 65_536,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedPayload {
    schema: String,
    bytes: Vec<u8>,
}

impl BoundedPayload {
    pub fn new(
        schema: impl Into<String>,
        bytes: Vec<u8>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        let schema = schema.into();
        if schema.is_empty() || schema.len() > limits.max_identity_bytes {
            return Err(InvocationProblem::InvalidPayloadSchema);
        }
        if bytes.len() > limits.max_payload_bytes {
            return Err(InvocationProblem::PayloadLimitExceeded);
        }
        Ok(Self { schema, bytes })
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Principal {
    id: PrincipalId,
    grants: BTreeSet<CapabilityId>,
}

impl Principal {
    pub fn new(
        id: PrincipalId,
        grants: BTreeSet<CapabilityId>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        if grants.len() > limits.max_capabilities {
            return Err(InvocationProblem::CapabilityLimitExceeded);
        }
        Ok(Self { id, grants })
    }

    #[must_use]
    pub fn id(&self) -> &PrincipalId {
        &self.id
    }
    #[must_use]
    pub fn grants(&self) -> &BTreeSet<CapabilityId> {
        &self.grants
    }
    #[must_use]
    pub fn has_grant(&self, capability: &CapabilityId) -> bool {
        self.grants.contains(capability)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ServiceClass {
    Interactive,
    Batch,
    Compiler,
    Blocking,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cancellation {
    pub id: CancellationId,
    pub reason: String,
    pub requested_at_tick: u64,
}

impl Cancellation {
    pub fn new(
        id: CancellationId,
        reason: impl Into<String>,
        requested_at_tick: u64,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        let reason = reason.into();
        if reason.is_empty() || reason.len() > limits.max_binding_bytes {
            return Err(InvocationProblem::InvalidCancellation);
        }
        Ok(Self {
            id,
            reason,
            requested_at_tick,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub request_id: RequestId,
    pub execution_id: ExecutionId,
    pub run_unit_id: RunUnitId,
    pub parent_execution_id: Option<ExecutionId>,
    pub selector: Selector,
    pub artifact: ArtifactRef,
    pub principal: Principal,
    pub service_class: ServiceClass,
    pub priority: u8,
    pub deadline_tick: u64,
    pub trace_id: TraceId,
    pub idempotency_key: IdempotencyKey,
    pub attempt: u32,
    pub limits: ResourceLimits,
    pub bindings: BTreeMap<String, BoundedPayload>,
    pub cancellation: Option<Cancellation>,
    pub provider_generations: BTreeMap<CapabilityId, String>,
    pub audit_correlation: String,
}

impl Invocation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: RequestId,
        execution_id: ExecutionId,
        run_unit_id: RunUnitId,
        parent_execution_id: Option<ExecutionId>,
        selector: Selector,
        artifact: ArtifactRef,
        principal: Principal,
        service_class: ServiceClass,
        priority: u8,
        deadline_tick: u64,
        trace_id: TraceId,
        idempotency_key: IdempotencyKey,
        attempt: u32,
        resource_limits: ResourceLimits,
        bindings: BTreeMap<String, BoundedPayload>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        if attempt == 0 {
            return Err(InvocationProblem::InvalidAttempt);
        }
        if deadline_tick == 0 {
            return Err(InvocationProblem::InvalidDeadline);
        }
        if bindings.len() > limits.max_bindings
            || bindings.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > limits.max_binding_bytes
                    || value.bytes.len() > limits.max_payload_bytes
            })
        {
            return Err(InvocationProblem::BindingLimitExceeded);
        }
        let audit_correlation = trace_id.as_str().to_string();
        Ok(Self {
            request_id,
            execution_id,
            run_unit_id,
            parent_execution_id,
            selector,
            artifact,
            principal,
            service_class,
            priority,
            deadline_tick,
            trace_id,
            idempotency_key,
            attempt,
            limits: resource_limits.validate()?,
            bindings,
            cancellation: None,
            provider_generations: BTreeMap::new(),
            audit_correlation,
        })
    }

    pub fn with_provider_generations(
        mut self,
        generations: BTreeMap<CapabilityId, String>,
        limits: InvocationLimits,
    ) -> Result<Self, InvocationProblem> {
        if generations.len() > limits.max_capabilities
            || generations.iter().any(|(capability, generation)| {
                !self.principal.has_grant(capability)
                    || generation.is_empty()
                    || generation.len() > limits.max_identity_bytes
            })
        {
            return Err(InvocationProblem::InvalidProviderGeneration);
        }
        self.provider_generations = generations;
        Ok(self)
    }

    #[must_use]
    pub fn with_cancellation(mut self, cancellation: Cancellation) -> Self {
        self.cancellation = Some(cancellation);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationProblem {
    InvalidResourceLimits,
    InvalidPayloadSchema,
    PayloadLimitExceeded,
    CapabilityLimitExceeded,
    InvalidCancellation,
    InvalidAttempt,
    InvalidDeadline,
    BindingLimitExceeded,
    InvalidProviderGeneration,
}

impl fmt::Display for InvocationProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invocation contract failed: {self:?}")
    }
}
impl std::error::Error for InvocationProblem {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityProblem;

    #[test]
    fn zero_attempt_and_unbounded_payload_fail() {
        let limits = InvocationLimits {
            max_payload_bytes: 1,
            ..InvocationLimits::default()
        };
        assert_eq!(
            BoundedPayload::new("test@1", vec![1, 2], limits),
            Err(InvocationProblem::PayloadLimitExceeded)
        );
        assert_eq!(
            PrincipalId::new("bad space", limits),
            Err(IdentityProblem::Invalid)
        );
    }

    #[test]
    fn grants_are_exact_and_bounded() {
        let limits = InvocationLimits {
            max_capabilities: 1,
            ..InvocationLimits::default()
        };
        let id = PrincipalId::new("IBMUSER", limits).unwrap();
        let read = CapabilityId::new("host.dataset.read", limits).unwrap();
        let principal = Principal::new(id, BTreeSet::from([read.clone()]), limits).unwrap();
        assert!(principal.has_grant(&read));
        let other = CapabilityId::new("host.dataset.write", limits).unwrap();
        assert!(!principal.has_grant(&other));
    }
}
