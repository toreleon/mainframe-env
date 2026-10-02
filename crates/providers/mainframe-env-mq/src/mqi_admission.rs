//! Private pre-state admission for the next typed MQI service integration.
//!
//! This is neither a dispatcher nor a participant permit. The service must
//! supply its host-minted numeric owner and the admitted execution's original
//! invocation, envelope, mutation and effect occurrence. In particular it must
//! resolve execution/run/principal to that owner through its trusted lifecycle
//! authority; this helper cannot attest an arbitrary `Invocation` or binding.
//! No owner is derived from request assertions or hashed string identities.
//!
//! `ServiceValidation` permits only the next service validation phase: live
//! handle/UOW checks, typed SAF/audit, supported execution and atomic fenced
//! effect/provider-row persistence remain with their existing authorities.
//! `PublicDispatch` review is not an execution permit. Explicit pending forms
//! cannot reach that phase through this boundary.
//! The retained request digest is the standalone MQI canonical domain, not the
//! shared journal's `CanonicalHostV1` effect digest. The manager-owned route
//! must encode the full original host effect through that existing authority.
//!
//! Source baseline: ibm-mq-9.4-mqi-2026-08-31, rows 0001/0002/0007,
//! 0008/0009, 0004/0005/0011 (offline manifest-hash-verified review).

use crate::host_context::decode_host_context;
use crate::retention::{MqReplayOwnerKind, origin_for};
use mainframe_env_execution_api::{
    CapabilityId, IdempotencyKey, Invocation, InvocationLimits, PrincipalId, RunUnitId,
};
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_object_route::MqRouteContextIntent;
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, HostLimits, HostProblem, MqContextDisposition,
    MqHandleOwner, MqHostEnvironment, MqSyncpointCall, MqSyncpointOwner, Mutation,
    mq_syncpoint_context_disposition,
};

/// Borrowed projection of the existing host-effect occurrence, not a second
/// effect protocol. The future manager-owned route must extract the MQI envelope
/// from this SAME original effect before constructing the service scope.
/// Until that route exists, projecting metadata does not register any payload.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MqMqiEffectOccurrence<'a> {
    run_unit: &'a RunUnitId,
    sequence: u64,
    deadline_tick: u64,
    idempotency_key: Option<&'a IdempotencyKey>,
}

impl<'a> MqMqiEffectOccurrence<'a> {
    pub(crate) fn from_effect(effect: &'a EffectRequest) -> Self {
        Self {
            run_unit: &effect.run_unit,
            sequence: effect.sequence,
            deadline_tick: effect.deadline_tick,
            idempotency_key: effect.idempotency_key.as_ref(),
        }
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }
    pub(crate) const fn deadline_tick(self) -> u64 {
        self.deadline_tick
    }
    pub(crate) const fn run_unit(self) -> &'a RunUnitId {
        self.run_unit
    }
    pub(crate) const fn idempotency_key(self) -> Option<&'a IdempotencyKey> {
        self.idempotency_key
    }
}

/// Trusted service-owned reference scope, deliberately unavailable to callers
/// outside this provider crate. Construction records a precondition, not proof
/// that application-provided bindings were host-minted. The service must obtain
/// every reference from its admitted dispatch/lifecycle authorities, never the
/// incoming envelope's owner. Keep this scope within one synchronous dispatch.
pub(crate) struct MqMqiServiceScope<'a> {
    invocation: &'a Invocation,
    owner: MqHandleOwner,
    envelope: &'a MqMqiRequestEnvelope,
    mutation: &'a Mutation,
    effect: MqMqiEffectOccurrence<'a>,
    provider: &'a CapabilityDescriptor,
}

impl<'a> MqMqiServiceScope<'a> {
    pub(crate) fn for_host_dispatch(
        invocation: &'a Invocation,
        owner: MqHandleOwner,
        envelope: &'a MqMqiRequestEnvelope,
        mutation: &'a Mutation,
        effect: MqMqiEffectOccurrence<'a>,
        provider: &'a CapabilityDescriptor,
    ) -> Self {
        Self {
            invocation,
            owner,
            envelope,
            mutation,
            effect,
            provider,
        }
    }
}

/// Bounded borrowed identity binds the exact canonical intent to its existing
/// effect and principal. There is no allocation of a request or state snapshot.
#[derive(Debug)]
pub(crate) struct MqMqiAdmitted<'a> {
    invocation: &'a Invocation,
    pub(crate) envelope: &'a MqMqiRequestEnvelope,
    pub(crate) mutation: &'a Mutation,
    pub(crate) owner: MqHandleOwner,
    pub(crate) request_digest: [u8; 32],
    pub(crate) request_bytes: usize,
    pub(crate) observed_tick: u64,
    pub(crate) origin: MqReplayOwnerKind,
    pub(crate) outer_effect_key: Option<String>,
    pub(crate) capability: CapabilityId,
    effect: MqMqiEffectOccurrence<'a>,
}

impl MqMqiAdmitted<'_> {
    pub(crate) fn invocation(&self) -> &Invocation {
        self.invocation
    }
    pub(crate) fn principal(&self) -> &PrincipalId {
        self.invocation.principal.id()
    }
    pub(crate) fn effect(&self) -> MqMqiEffectOccurrence<'_> {
        self.effect
    }

    /// Recheck immediately before SAF/state/mutation and at subsequent owned
    /// dispatch boundaries. A successful earlier check never freezes the probe.
    pub(crate) fn recheck_controls(&self, now_tick: u64) -> Result<(), HostProblem> {
        if now_tick < self.observed_tick {
            return Err(HostProblem::Malformed);
        }
        controls(self.invocation, self.effect, now_tick)
    }
}

#[derive(Debug)]
#[must_use = "admission does not execute an MQI call or authorize resource mutation"]
pub(crate) enum MqMqiAdmission<'a> {
    ServiceValidation(MqMqiAdmitted<'a>),
    Pending {
        identity: MqMqiAdmitted<'a>,
        reason: MqMqiPending,
    },
    /// Source-exact direct-call condition. No provider state was inspected.
    ForbiddenContext(MqMqiResult),
}

fn controls(
    invocation: &Invocation,
    effect: MqMqiEffectOccurrence<'_>,
    now_tick: u64,
) -> Result<(), HostProblem> {
    if invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    if now_tick == 0
        || [invocation.deadline_tick, effect.deadline_tick]
            .iter()
            .any(|tick| *tick == 0 || *tick == u64::MAX)
    {
        return Err(HostProblem::Malformed);
    }
    if now_tick >= invocation.deadline_tick || now_tick >= effect.deadline_tick {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

fn bounded_invocation(value: &Invocation) -> Result<(), HostProblem> {
    let limits = InvocationLimits::default();
    value
        .limits
        .validate()
        .map_err(|_| HostProblem::Malformed)?;
    if value.attempt == 0
        || value.bindings.len() > limits.max_bindings
        || value.principal.grants().len() > limits.max_capabilities
        || value.provider_generations.len() > limits.max_capabilities
        || value.bindings.iter().any(|(name, binding)| {
            name.is_empty()
                || name.len() > limits.max_identity_bytes
                || binding.schema().len() > limits.max_identity_bytes
                || binding.bytes().len() > limits.max_binding_bytes
        })
        || value
            .provider_generations
            .values()
            .any(|generation| generation.is_empty() || generation.len() > limits.max_identity_bytes)
        || [
            value.execution_id.as_str(),
            value.run_unit_id.as_str(),
            value.principal.id().as_str(),
            value.idempotency_key.as_str(),
        ]
        .iter()
        .any(|text| text.len() > limits.max_identity_bytes)
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

/// Validate the occurrence exactly as the existing effect/mutation contract,
/// plus the service's immutable admitted occurrence. An effect key is not the
/// invocation key: nested/ordinary effects legitimately have their own key.
fn occurrence(
    invocation: &Invocation,
    mutation: &Mutation,
    effect: MqMqiEffectOccurrence<'_>,
) -> Result<(), HostProblem> {
    mutation.validate(HostLimits::default())?;
    let key = effect
        .idempotency_key
        .ok_or(HostProblem::MissingIdempotency)?;
    if effect.run_unit != &invocation.run_unit_id {
        return Err(HostProblem::Malformed);
    }
    if effect.sequence != mutation.sequence || key != &mutation.idempotency_key {
        return Err(HostProblem::IdempotencyConflict);
    }
    if effect.sequence > invocation.limits.max_effects
        || key.as_str().len() > InvocationLimits::default().max_identity_bytes
    {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(())
}

pub(crate) fn admit_mqi<'a>(
    scope: &MqMqiServiceScope<'_>,
    invocation: &'a Invocation,
    envelope: &'a MqMqiRequestEnvelope,
    mutation: &'a Mutation,
    effect: MqMqiEffectOccurrence<'a>,
    now_tick: u64,
) -> Result<MqMqiAdmission<'a>, HostProblem> {
    controls(invocation, effect, now_tick)?;
    controls(scope.invocation, scope.effect, now_tick)?;
    bounded_invocation(invocation)?;
    bounded_invocation(scope.invocation)?;
    occurrence(invocation, mutation, effect)?;
    occurrence(scope.invocation, scope.mutation, scope.effect)?;
    if invocation.execution_id != scope.invocation.execution_id
        || invocation.run_unit_id != scope.invocation.run_unit_id
        || invocation.principal.id() != scope.invocation.principal.id()
        || invocation.idempotency_key != scope.invocation.idempotency_key
        || invocation.attempt != scope.invocation.attempt
        || invocation.limits != scope.invocation.limits
        || invocation.cancellation_probe != scope.invocation.cancellation_probe
        || invocation.bindings != scope.invocation.bindings
        || invocation.provider_generations != scope.invocation.provider_generations
        || invocation.deadline_tick > scope.invocation.deadline_tick
        || effect.deadline_tick > scope.effect.deadline_tick
        || mutation != scope.mutation
        || effect.run_unit != scope.effect.run_unit
        || effect.sequence != scope.effect.sequence
        || effect.idempotency_key != scope.effect.idempotency_key
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    // Keep the current MQ host route's capability authority. No new public
    // read/write classification or handler registration is inferred here.
    let capability = CapabilityId::new("host.mq.write", InvocationLimits::default())
        .expect("existing static MQ routing grant");
    if !invocation.principal.has_grant(&capability)
        || !scope.invocation.principal.has_grant(&capability)
        || !invocation
            .principal
            .grants()
            .is_subset(scope.invocation.principal.grants())
    {
        return Err(HostProblem::Unauthorized);
    }
    let provider = scope.provider;
    provider
        .validate(InvocationLimits::default())
        .map_err(|_| HostProblem::ProviderFailure)?;
    if !provider.ready
        || provider.capability != capability
        || provider.provider_id != "mainframe-env-mq"
        || invocation
            .provider_generations
            .get(&capability)
            .is_some_and(|generation| generation != &provider.generation)
    {
        return Err(HostProblem::ProviderFailure);
    }
    let request_bytes = mq_mqi_request_size(envelope).map_err(contract_problem)?;
    if request_bytes > provider.max_request_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let request_digest = mq_mqi_request_digest(envelope).map_err(contract_problem)?;
    let original_bytes = mq_mqi_request_size(scope.envelope).map_err(contract_problem)?;
    if original_bytes > provider.max_request_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    if request_digest != mq_mqi_request_digest(scope.envelope).map_err(contract_problem)? {
        return Err(HostProblem::IdempotencyConflict);
    }
    let context = decode_host_context(invocation)?.ok_or(HostProblem::Malformed)?;
    if envelope.context.owner != scope.owner
        || scope.owner.environment != context.environment
        || envelope.context.syncpoint_owner != context.owner
    {
        return Err(HostProblem::Malformed);
    }
    // Reuse exact existing nested/outer/run/sequence/key validation. A valid
    // origin is provenance, NEVER authority to select an internal coordinator.
    let (origin, outer_effect_key) = origin_for(
        invocation,
        mutation.idempotency_key.as_str(),
        mutation.sequence,
    )?;
    if matches!(envelope.request, MqMqiRequest::CallbackFunction { .. }) {
        return Err(HostProblem::Unsupported);
    }
    let call = match envelope.request {
        MqMqiRequest::Back { .. } => Some(MqSyncpointCall::Back),
        MqMqiRequest::Begin { .. } => Some(MqSyncpointCall::Begin),
        MqMqiRequest::Commit { .. } => Some(MqSyncpointCall::Commit),
        _ => None,
    };
    if let Some(call) = call
        && matches!(
            mq_syncpoint_context_disposition(call, context.environment, context.owner),
            MqContextDisposition::Rejected { .. }
        )
    {
        controls(invocation, effect, now_tick)?;
        return Ok(MqMqiAdmission::ForbiddenContext(MqMqiResult {
            call: envelope.request.call(),
            outcome: MqMqiOutcome::Completed {
                status: MqMqiStatus::FailedEnvironment,
                output: MqMqiOutput::NoOutput,
            },
        }));
    }
    let identity = MqMqiAdmitted {
        invocation,
        envelope,
        mutation,
        owner: scope.owner,
        request_digest,
        request_bytes,
        observed_tick: now_tick,
        origin,
        outer_effect_key,
        capability,
        effect,
    };
    identity.recheck_controls(now_tick)?;
    match pending_form(&envelope.request, context.environment, context.owner) {
        Some(reason) => Ok(MqMqiAdmission::Pending { identity, reason }),
        None => Ok(MqMqiAdmission::ServiceValidation(identity)),
    }
}

fn contract_problem(problem: MqMqiProblem) -> HostProblem {
    match problem {
        MqMqiProblem::CanonicalLimit
        | MqMqiProblem::Allocation
        | MqMqiProblem::Limits
        | MqMqiProblem::SelectorCount
        | MqMqiProblem::AttributeCount
        | MqMqiProblem::Buffer => HostProblem::ResourceExhausted,
        _ => HostProblem::Malformed,
    }
}

/// Only explicit unsupported forms are classified here. Legality of supported
/// kernel intent still belongs to the real kernel/SAF/participant authorities.
fn pending_form(
    request: &MqMqiRequest,
    environment: MqHostEnvironment,
    coordinator: MqSyncpointOwner,
) -> Option<MqMqiPending> {
    use MqMqiPending as P;
    use MqMqiRequest as R;
    let options = |value| {
        if matches!(value, MqMqiOptions::PendingStructure { .. }) {
            Some(P::StructureAndWireMapping)
        } else {
            None
        }
    };
    let unit = |value| {
        if matches!(value, MqMqiUnitOfWork::ExternalPending { .. })
            || (coordinator == MqSyncpointOwner::HostCoordinator
                && matches!(value, MqMqiUnitOfWork::Local { .. }))
        {
            Some(P::ExternalUnitOfWork)
        } else {
            None
        }
    };
    let put = |value: &MqMqiPut| {
        options(value.options)
            .or_else(|| unit(value.unit))
            .or_else(|| {
                (!matches!(value.context, MqMqiMessageContext::Default))
                    .then_some(P::TrustedContextAndAuthorization)
            })
    };
    match request {
        R::Connect(value) | R::ConnectExtended(value) => options(value.options),
        // MQBEGIN's source global-coordination semantics are not the private
        // delivery kernel's local begin. Its participant mapping stays pending.
        R::Begin { options: value, .. } => options(*value).or(Some(P::ExternalUnitOfWork)),
        R::BufferToHandle(value) | R::HandleToBuffer(value) => {
            options(value.options).or_else(|| {
                matches!(value.format, MqMqiBufferFormat::MqRfh2Pending)
                    .then_some(P::StructureAndWireMapping)
            })
        }
        R::Callback {
            operation,
            options: value,
            ..
        } => options(*value).or_else(|| {
            (matches!(operation, MqMqiCallbackOperation::EventHandlerPending)
                || matches!(
                    environment,
                    MqHostEnvironment::ZosIms | MqHostEnvironment::ZosImsBatchDli
                ))
            .then_some(P::CallbackContext)
        }),
        R::Control {
            operation,
            options: value,
            ..
        } => options(*value).or_else(|| {
            (matches!(
                operation,
                MqMqiControl::StartWaitPending | MqMqiControl::Quiesce
            ) || matches!(
                environment,
                MqHostEnvironment::ZosIms | MqHostEnvironment::ZosImsBatchDli
            ) || (environment == MqHostEnvironment::ZosCics && *operation == MqMqiControl::Start))
                .then_some(P::CallbackContext)
        }),
        R::Get(value) => options(value.options).or_else(|| unit(value.unit)),
        R::Put { put: value, .. } => put(value),
        R::PutOne {
            put: value,
            alternate_user,
            ..
        } => put(value).or_else(|| {
            alternate_user
                .is_some()
                .then_some(P::TrustedContextAndAuthorization)
        }),
        R::Inquire(_) | R::Set(_) => Some(P::SelectorAndAttributeMapping),
        R::InquireProperty(value) => options(value.options),
        R::CreateMessageHandle { options: value, .. }
        | R::DeleteMessageHandle { options: value, .. }
        | R::DeleteProperty { options: value, .. }
        | R::SetProperty { options: value, .. } => options(*value),
        R::Stat { .. } => Some(P::StatusMapping),
        R::Subscribe(value) => options(value.options).or_else(|| {
            (matches!(value.mode, MqMqiSubscriptionMode::AlterPending)
                || !matches!(value.destination, MqMqiSubscriptionDestination::Catalog))
            .then_some(P::StructureAndWireMapping)
        }),
        R::SubscriptionRequest {
            options: value,
            unit: uow,
            ..
        } => options(*value).or_else(|| unit(*uow)),
        R::Open(value) => (value.modifiers().alternate_user.is_some()
            || value.modifiers().context != MqRouteContextIntent::default())
        .then_some(P::TrustedContextAndAuthorization),
        R::CallbackFunction { .. } => Some(P::CallbackContext),
        R::Back { .. } | R::Commit { .. } | R::Close(_) | R::Disconnect { .. } => None,
    }
}

#[cfg(test)]
mod tests;
