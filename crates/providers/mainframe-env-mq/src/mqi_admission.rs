//! Private pre-state admission bound to one original typed host effect.
//!
//! This is neither a dispatcher nor a participant permit. Scope construction
//! requires the already-admitted host Invocation, the service lifecycle's
//! independently minted owner, provider descriptor and HostLimits. It cannot
//! attest arbitrary bindings or derive an owner from request assertions.
//! The sole payload source is the validated host-api MQI occurrence. No separate
//! envelope, mutation or metadata-only occurrence can replace its original.
//!
//! ServiceValidation permits only the next service validation phase: live
//! registry/handle/UOW checks, typed SAF/audit and atomic fenced publication
//! remain with their existing authorities. Result preflight is bounded shape
//! and copied-capacity validation, never a returned-handle or mutation permit.
//! PublicDispatch review and typed pending observations are not execution success.
//! Full host request/result canonical identities use the shared journal codec;
//! no standalone MQI digest, private journal or new outcome protocol is used.
//!
//! Source baseline: ibm-mq-9.4-mqi-2026-08-31, rows 0001-0026,
//! 26 identities/27 source positions (offline manifest-hash-verified review).

use crate::host_context::decode_host_context;
use crate::mqi_lifecycle::DirectoryHostContext;
use crate::retention::{MqReplayOwnerKind, origin_for};
use mainframe_env_execution_api::{CapabilityId, Invocation, InvocationLimits, PrincipalId};
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_object_route::MqRouteContextIntent;
use mainframe_env_host_api::{
    CapabilityDescriptor, EffectRequest, HostLimits, HostProblem, MAX_CANONICAL_EFFECT_BYTES,
    MqContextDisposition, MqHandleOwner, MqHostEnvironment, MqMqiEffectOccurrence, MqSyncpointCall,
    MqSyncpointOwner, Mutation, canonical_request_digest, canonical_request_size,
    mq_syncpoint_context_disposition,
};

mod result;

/// Trusted service-owned scope. Host admission/lifecycle provenance is an
/// explicit construction precondition, not a claim that this helper attests it.
/// Keep this scope within one dispatch; bindings never grant coordinator authority.
pub(crate) struct MqMqiServiceScope<'a> {
    invocation: &'a Invocation,
    owner: MqHandleOwner,
    original: MqMqiEffectOccurrence<'a>,
    provider: &'a CapabilityDescriptor,
    host_limits: HostLimits,
    directory_context: Option<DirectoryHostContext>,
}

impl<'a> MqMqiServiceScope<'a> {
    pub(crate) fn for_host_dispatch(
        invocation: &'a Invocation,
        owner: MqHandleOwner,
        original: MqMqiEffectOccurrence<'a>,
        provider: &'a CapabilityDescriptor,
        host_limits: HostLimits,
    ) -> Self {
        Self {
            invocation,
            owner,
            original,
            provider,
            host_limits,
            directory_context: None,
        }
    }

    /// Proof must come from the same locked directory's exact original frame.
    /// It supplies context only; no binding or original occurrence is rewritten.
    pub(crate) fn for_directory_dispatch(
        invocation: &'a Invocation,
        owner: MqHandleOwner,
        original: MqMqiEffectOccurrence<'a>,
        provider: &'a CapabilityDescriptor,
        host_limits: HostLimits,
        context: DirectoryHostContext,
    ) -> Self {
        let mut scope = Self::for_host_dispatch(invocation, owner, original, provider, host_limits);
        scope.directory_context = Some(context);
        scope
    }
}

/// Borrowed original identity; no request/state snapshot or replacement payload.
#[derive(Debug)]
pub(crate) struct MqMqiAdmitted<'a> {
    invocation: &'a Invocation,
    trusted_invocation: &'a Invocation,
    pub(crate) envelope: &'a MqMqiRequestEnvelope,
    pub(crate) mutation: &'a Mutation,
    pub(crate) owner: MqHandleOwner,
    pub(crate) host_request_digest: [u8; 32],
    pub(crate) host_request_bytes: usize,
    pub(crate) observed_tick: u64,
    pub(crate) origin: MqReplayOwnerKind,
    pub(crate) outer_effect_key: Option<String>,
    pub(crate) capability: CapabilityId,
    effect: &'a EffectRequest,
    provider: &'a CapabilityDescriptor,
    host_limits: HostLimits,
}

impl MqMqiAdmitted<'_> {
    pub(crate) fn invocation(&self) -> &Invocation {
        self.invocation
    }
    pub(crate) fn principal(&self) -> &PrincipalId {
        self.invocation.principal.id()
    }
    pub(crate) fn effect(&self) -> &EffectRequest {
        self.effect
    }

    /// Recheck immediately before SAF/state/mutation and subsequent owned
    /// boundaries. A successful earlier check never freezes the live probe.
    pub(crate) fn recheck_controls(&self, now_tick: u64) -> Result<(), HostProblem> {
        if now_tick < self.observed_tick {
            return Err(HostProblem::Malformed);
        }
        controls(self.invocation, self.effect, now_tick)?;
        controls(self.trusted_invocation, self.effect, now_tick)
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
    effect: &EffectRequest,
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
            .iter()
            .any(|(capability, generation)| {
                capability.as_str().len() > limits.max_identity_bytes
                    || generation.is_empty()
                    || generation.len() > limits.max_identity_bytes
            })
        || value
            .principal
            .grants()
            .iter()
            .any(|grant| grant.as_str().len() > limits.max_identity_bytes)
        || value
            .parent_execution_id
            .as_ref()
            .is_some_and(|id| id.as_str().len() > limits.max_identity_bytes)
        || [
            value.request_id.as_str(),
            value.execution_id.as_str(),
            value.run_unit_id.as_str(),
            value.selector.as_str(),
            value.artifact.as_str(),
            value.principal.id().as_str(),
            value.trace_id.as_str(),
            value.idempotency_key.as_str(),
            &value.audit_correlation,
        ]
        .iter()
        .any(|text| text.len() > limits.max_identity_bytes)
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

pub(crate) fn admit_mqi<'a>(
    scope: &'a MqMqiServiceScope<'a>,
    invocation: &'a Invocation,
    now_tick: u64,
) -> Result<MqMqiAdmission<'a>, HostProblem> {
    let effect = scope.original.effect();
    let envelope = scope.original.envelope();
    let mutation = scope.original.mutation();
    controls(invocation, effect, now_tick)?;
    controls(scope.invocation, effect, now_tick)?;
    bounded_invocation(invocation)?;
    bounded_invocation(scope.invocation)?;
    let capability = effect
        .request
        .required_capability(InvocationLimits::default());
    if capability.as_str() != "host.mq.write" {
        return Err(HostProblem::Malformed);
    }
    if !invocation.principal.has_grant(&capability)
        || !scope.invocation.principal.has_grant(&capability)
    {
        return Err(HostProblem::Unauthorized);
    }
    // Complete equality also preserves request/parent/artifact/trace/audit,
    // service class, cancellation identity, grants and every generation binding.
    if invocation != scope.invocation {
        return Err(HostProblem::IdempotencyConflict);
    }
    if effect.run_unit != invocation.run_unit_id {
        return Err(HostProblem::Malformed);
    }
    if effect.sequence > invocation.limits.max_effects {
        return Err(HostProblem::ResourceExhausted);
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
    // The actual host preimage includes the original Mutation and envelope.
    // Preflight provider/product/hard budgets before any field-cloning validator.
    let host_request_bytes = canonical_request_size(
        &effect.request,
        provider
            .max_request_bytes
            .min(envelope.limits.canonical_bytes)
            .min(MAX_CANONICAL_EFFECT_BYTES),
    )?;
    effect.validate(scope.host_limits)?;
    let host_request_digest = canonical_request_digest(&effect.request)?;
    let context = match &scope.directory_context {
        Some(proof) => proof.require(invocation, scope.owner)?,
        None => decode_host_context(invocation)?.ok_or(HostProblem::Malformed)?,
    };
    if envelope.context.owner != scope.owner
        || scope.owner.environment != context.environment
        || envelope.context.syncpoint_owner != context.owner
    {
        return Err(HostProblem::Malformed);
    }
    // Reuse full nested/outer/run/sequence/key validation; origin is provenance,
    // never authority to select an internal coordinator for application calls.
    let (origin, outer_effect_key) = origin_for(
        invocation,
        mutation.idempotency_key.as_str(),
        mutation.sequence,
    )?;
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
        trusted_invocation: scope.invocation,
        envelope,
        mutation,
        owner: scope.owner,
        host_request_digest,
        host_request_bytes,
        observed_tick: now_tick,
        origin,
        outer_effect_key,
        capability,
        effect,
        provider,
        host_limits: scope.host_limits,
    };
    identity.recheck_controls(now_tick)?;
    match pending_form(&envelope.request, context.environment, context.owner) {
        Some(reason) => Ok(MqMqiAdmission::Pending { identity, reason }),
        None => Ok(MqMqiAdmission::ServiceValidation(identity)),
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
        // Full GET has one finite private selected profile. ContractDefault is
        // kernel intent, NEVER evidence for arbitrary native MQGMO option bits.
        R::FullGet(value) => options(value.options)
            .or_else(|| unit(value.unit))
            .or_else(|| {
                (environment != MqHostEnvironment::ZosBatch
                    || coordinator != MqSyncpointOwner::QueueManager
                    || value.message_handle.is_some()
                    || value.wait != mainframe_env_host_api::MqWait::NoWait
                    || value.mode != mainframe_env_host_api::MqGetMode::Remove)
                    .then_some(P::StructureAndWireMapping)
            }),
        R::FullPut { .. } | R::FullPutOne { .. } => Some(P::StructureAndWireMapping),
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
