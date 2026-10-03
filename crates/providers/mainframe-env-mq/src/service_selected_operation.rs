//! Private ordinary batch MQI composition under MqService's sole authority.
//!
//! Host dispatch must already have been admitted independently of its bindings.
//! This entry is not a public provider, new queue engine or accepted participant.
//! Lossless receipt payload composition is supplied by the owned MQI replay codec.

use super::super::*;
use crate::host_context::decode_host_context;
use crate::mqi_admission::{MqMqiAdmission, MqMqiServiceScope, admit_mqi};
use crate::mqi_lifecycle::LogicalBatchOwner;
use crate::mqi_lifecycle::{FrameLease, LifecycleLimits, MqLifecycleDirectory, ProcessLease};
use crate::service_mqi_intent::{MqIntentProblem, bind_core_intent};
use mainframe_env_execution_api::{AuditDecision, AuditRecord};
use mainframe_env_host_api::mq_mqi::*;
use mainframe_env_host_api::mq_status::MqReviewedStatus;
use mainframe_env_host_api::{
    HostLimits, MqMqiEffectOccurrence, MqMqiHostResult, canonical_audit_resource_digest,
};
use mainframe_env_host_api::{
    MqGetDisposition, MqHandleOwner, MqHostEnvironment, MqSyncpointOwner, MqTruncationDisposition,
};
use mainframe_env_store_api::{EffectDigestFormat, EffectState};

#[path = "service_selected_operation/authorization.rs"]
mod authorization;
#[path = "service_selected_operation/batch_child.rs"]
mod batch_child;
#[path = "service_selected_operation/explicit_context.rs"]
mod explicit_context;
#[path = "service_selected_operation/ownership.rs"]
mod ownership;
#[path = "service_selected_operation/producer.rs"]
pub(in crate::service) mod producer;
#[path = "service_selected_operation/property.rs"]
mod property;
#[path = "service_selected_operation/receipt.rs"]
pub(in crate::service) mod receipt;
#[path = "service_selected_operation/rows.rs"]
pub(in crate::service) mod rows;
#[path = "service_selected_operation/transition.rs"]
mod transition;
#[path = "service_selected_operation/trusted_embedding.rs"]
mod trusted_embedding;

use ownership::Control;

/// Lives inside RichStoredState, under the SAME mutex as catalog/delivery rows.
/// Cold restoration constructs none of these volatile authorities.
pub(in crate::service) struct SelectedRuntime {
    pub(super) directory: MqLifecycleDirectory,
    pub(super) handles: crate::MqPubsubKernel,
    pub(super) control: Control,
    connections: Vec<transition::ConnectionBinding>,
    objects: Vec<transition::ObjectBinding>,
    replies: BTreeMap<String, EffectResult>,
    reply_bytes: usize,
    fenced: bool,
}

impl SelectedRuntime {
    fn new(state: &rich_state::RichStoredState, limits: MqLimits) -> Result<Self, HostProblem> {
        let (generation, fence) = state.marker.identity.generation_and_fence();
        let control = match &state.ownership.control {
            Some(control) => control.next_incarnation()?,
            None => Control::initial(generation, fence)?,
        };
        Ok(Self {
            directory: MqLifecycleDirectory::new(LifecycleLimits::default())?,
            handles: crate::MqPubsubKernel::new(
                (*state.catalog).clone(),
                Default::default(),
                control.registry_epoch,
                limits.max_handles,
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
            control,
            connections: Vec::new(),
            objects: Vec::new(),
            replies: BTreeMap::new(),
            reply_bytes: 0,
            fenced: false,
        })
    }
}

impl MqService {
    /// Read-only projection for the independently admitted host producer before
    /// constructing an original effect. The returned scalar is an assertion,
    /// never a UOW permit; execution rechecks retained provenance and exact CAS.
    pub(crate) fn selected_local_unit(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        connection: mainframe_env_host_api::MqHconn,
    ) -> Result<MqMqiUnitOfWork, HostProblem> {
        if !invocation.principal.has_grant(
            &CapabilityId::new("host.mq.write", InvocationLimits::default())
                .map_err(|_| HostProblem::Malformed)?,
        ) {
            return Err(HostProblem::Unauthorized);
        }
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let runtime = state.runtime.as_mut().ok_or(HostProblem::Unauthorized)?;
        if runtime.fenced {
            return Err(HostProblem::UnknownOutcome);
        }
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        let owner = runtime.directory.owner_for(frame, invocation, now)?;
        let logical = runtime
            .directory
            .logical_batch_owner(frame, invocation, now)?;
        runtime
            .handles
            .handles_mut()
            .validate_connection(owner, connection)
            .map_err(|_| HostProblem::Malformed)?;
        let binding = runtime
            .connections
            .iter()
            .find(|b| b.connection == connection)
            .ok_or(HostProblem::Malformed)?;
        state
            .ownership
            .units
            .get(&binding.unit)
            .ok_or(HostProblem::Malformed)?
            .require_owner(&logical, &binding.key, &runtime.control, binding.unit)?;
        Ok(MqMqiUnitOfWork::Local { unit: binding.unit })
    }

    /// One original host occurrence and independently minted opaque frame.
    /// Host attestation is a precondition of private lease minting, not a property
    /// inferred from Invocation binding assertions by this entry point.
    pub(crate) fn execute_selected_mqi(
        &self,
        frame: FrameLease,
        invocation: &Invocation,
        original: MqMqiEffectOccurrence<'_>,
        provider: &CapabilityDescriptor,
        host_limits: HostLimits,
    ) -> Result<EffectResult, HostProblem> {
        let store = self
            .selected_store
            .as_ref()
            .ok_or(HostProblem::Unsupported)?;
        let clock = self.replay_clock.as_ref().ok_or(HostProblem::Unsupported)?;
        let authorizer = self.authorizer.as_ref().ok_or(HostProblem::Unsupported)?;
        let original_sequence = original.effect().sequence;
        let original_limits = original.envelope().limits;
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let now = clock.now_tick()?;
        let mut runtime = state.runtime.take().ok_or(HostProblem::Unauthorized)?;
        let result = (|| {
            if runtime.fenced {
                return Err(HostProblem::UnknownOutcome);
            }
            let owner = runtime.directory.owner_for(frame, invocation, now)?;
            let logical = runtime
                .directory
                .logical_batch_owner(frame, invocation, now)?;
            let context = runtime.directory.context_for(frame, invocation, now)?;
            let scope = MqMqiServiceScope::for_directory_dispatch(
                invocation,
                owner,
                original,
                provider,
                host_limits,
                context,
            );
            let admission = admit_mqi(&scope, invocation, now)?;
            let admitted = match &admission {
                MqMqiAdmission::ServiceValidation(value) => value,
                MqMqiAdmission::Pending { .. } => return Err(HostProblem::Unsupported),
                MqMqiAdmission::ForbiddenContext(result) => {
                    return Ok(EffectResult {
                        sequence: original_sequence,
                        outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                            limits: original_limits,
                            result: result.clone(),
                        })),
                    });
                }
            };
            let key = admitted.mutation.idempotency_key.as_str();
            // A shared transaction is not this separately retained local MQ
            // owner. Real participant/coordinator integration remains required.
            if admitted.mutation.transaction.is_some() {
                return Err(HostProblem::Unsupported);
            }
            admitted.recheck_controls(clock.now_tick()?)?;
            // Refresh exact physical receipt before any kernel/SAF mutation.
            // A receipt from another writer cannot be adopted as a live token.
            if let Some(record) = store
                .get_provider_state(receipt::NAMESPACE, key)
                .map_err(store_error)?
            {
                let physical_control = store
                    .get_provider_state(ownership::CONTROL_NAMESPACE, ownership::CONTROL_KEY)
                    .map_err(store_error)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                state
                    .ownership
                    .require_physical_control(&physical_control)?;
                let stored: ObjectRow<receipt::OccurrenceReceipt> =
                    serde_json::from_slice(&record.payload).map_err(|_| HostProblem::Malformed)?;
                if stored.schema_version != OBJECT_ROW_SCHEMA
                    || stored.object_key != key
                    || record.version != 1
                {
                    return Err(HostProblem::Malformed);
                }
                stored.value.validate(key, &runtime.control)?;
                stored.value.require_observed_time(now)?;
                if !stored.value.matches(admitted) {
                    return Err(HostProblem::IdempotencyConflict);
                }
                let effect = store
                    .effect(&admitted.mutation.idempotency_key)
                    .map_err(store_error)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                if effect.execution_id != invocation.execution_id
                    || effect.run_unit_id != invocation.run_unit_id
                    || effect.sequence != admitted.effect().sequence
                    || effect.digest_format != EffectDigestFormat::CanonicalHostV1
                    || effect.request_digest != admitted.host_request_digest
                    || !matches!(effect.state, EffectState::Intent | EffectState::Completed)
                    || (effect.state == EffectState::Completed
                        && effect.result_digest != Some(stored.value.result_digest))
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                let metadata = &effect.intent;
                if metadata.owner != invocation.execution_id
                    || metadata.attempt != invocation.attempt
                    || metadata.capability.as_ref() != Some(&admitted.capability)
                    || metadata.audit_resource
                        != Some(canonical_audit_resource_digest(&admitted.effect().request))
                    || metadata.audit_invocation_key.as_ref() != Some(&invocation.idempotency_key)
                    || metadata.created_tick == 0
                    || metadata.created_tick > now
                    || metadata.recovery_after_tick
                        != invocation
                            .deadline_tick
                            .min(admitted.effect().deadline_tick)
                    || metadata.epoch == 0
                    || metadata.recovery_lease.is_some()
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                let execution = store
                    .get_execution(&invocation.execution_id)
                    .map_err(store_error)?
                    .ok_or(HostProblem::UnknownOutcome)?;
                if execution.execution_id != invocation.execution_id
                    || execution.run_unit_id != invocation.run_unit_id
                    || execution.principal != *invocation.principal.id()
                    || execution.attempt != invocation.attempt
                    || execution.selector != invocation.selector
                    || execution.artifact != invocation.artifact
                    || execution.state != mainframe_env_store_api::ExecutionState::Running
                    || execution.terminal_tick.is_some()
                    || execution.lease_expiry_tick.is_some_and(|tick| tick <= now)
                {
                    return Err(HostProblem::UnknownOutcome);
                }
                if effect.state == EffectState::Intent {
                    bind_core_intent(&admission, &**store, clock.now_tick()?)
                        .map_err(intent_error)?;
                }
                stored.value.authorize_replay(&**authorizer, invocation)?;
                admitted.recheck_controls(clock.now_tick()?)?;
                let mut reply = stored.value.replay(host_limits, admitted.envelope.limits)?;
                let preflight = admitted.preflight_result(&reply, clock.now_tick()?)?;
                if preflight.host_result_digest != stored.value.result_digest {
                    return Err(HostProblem::UnknownOutcome);
                }
                if transition::has_handle_reply(&reply)
                    || matches!(&admitted.envelope.request, MqMqiRequest::Property(_))
                {
                    // This runtime originally adopted the reply only after the
                    // exact atomic publication. A coherent substituted handle
                    // observation is not that reply, even if another live entry
                    // exists under the same owner. Cold caches grant no permit.
                    let issued = runtime
                        .replies
                        .get(key)
                        .ok_or(HostProblem::UnknownOutcome)?;
                    if issued.sequence != admitted.effect().sequence
                        || mainframe_env_host_api::canonical_result_digest(&issued.outcome)?
                            != stored.value.result_digest
                    {
                        return Err(HostProblem::UnknownOutcome);
                    }
                }
                // Receipt/core/frame/limits/SAF proof above precedes every
                // observation lookup. Resolution never allocates or resurrects.
                transition::resolve_reply(
                    state,
                    &mut runtime,
                    &logical,
                    owner,
                    &admitted.envelope.request,
                    &mut reply,
                    invocation,
                    &**authorizer,
                )?;
                let resolved = admitted.preflight_result(&reply, clock.now_tick()?)?;
                if resolved.host_result_digest != stored.value.result_digest {
                    return Err(HostProblem::UnknownOutcome);
                }
                producer::recheck(
                    self,
                    frame,
                    invocation,
                    admitted,
                    &runtime.directory,
                    clock.now_tick()?,
                )
                .map_err(|_| HostProblem::UnknownOutcome)?;
                return Ok(reply);
            }
            if state.receipts.contains_key(key) || runtime.replies.contains_key(key) {
                return Err(HostProblem::UnknownOutcome);
            }
            let mut binding = bind_core_intent(&admission, &**store, now).map_err(intent_error)?;
            admitted.recheck_controls(clock.now_tick()?)?;
            let authorized = authorization::Capture::new(&**authorizer);
            let mut candidate = match transition::prepare(
                state,
                &mut runtime,
                invocation,
                &logical,
                owner,
                &admitted.envelope.request,
                key,
                now,
                &authorized,
                self.limits,
                self,
                frame,
                admitted,
            ) {
                Ok(candidate) => candidate,
                Err(HostProblem::Unauthorized) => {
                    let decision_tick = clock.now_tick()?;
                    binding
                        .prepare(
                            audit(admitted, decision_tick, AuditDecision::Deny),
                            Vec::new(),
                            decision_tick,
                        )
                        .map_err(intent_error)?
                        .publish(decision_tick)
                        .map_err(intent_error)?;
                    return Err(HostProblem::Unauthorized);
                }
                Err(
                    error @ (HostProblem::ProviderFailure | HostProblem::InfrastructureFailure),
                ) if matches!(
                    admitted.envelope.request,
                    MqMqiRequest::FullGet(_)
                        | MqMqiRequest::FullPut { .. }
                        | MqMqiRequest::FullPutOne { .. }
                ) || (error == HostProblem::InfrastructureFailure
                    && matches!(admitted.envelope.request, MqMqiRequest::Property(_))) =>
                {
                    // This finite prepare error occurred before either complete
                    // delivery or property state could publish. Use the SAME bound
                    // original intent, never a sequential audit fallback.
                    let decision = if error == HostProblem::ProviderFailure {
                        AuditDecision::ProviderFailure
                    } else {
                        AuditDecision::InfrastructureFailure
                    };
                    let decision_tick = clock.now_tick()?;
                    binding
                        .prepare(
                            audit(admitted, decision_tick, decision),
                            Vec::new(),
                            decision_tick,
                        )
                        .map_err(intent_error)?
                        .publish(decision_tick)
                        .map_err(intent_error)?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            let attempt = (|| {
                let mut access = if candidate.property.is_some() {
                    Some(runtime.handles.message_handles_mut())
                } else {
                    None
                };
                let stage = match (&mut access, &candidate.property) {
                    (Some(access), Some(request)) => Some(
                        access
                            .stage_property(owner, request, admitted.envelope.limits)
                            .map_err(property::kernel_error)?,
                    ),
                    _ => None,
                };
                if let Some(stage) = &stage {
                    candidate.output = stage.output.clone();
                    candidate.reviewed_status = Some(stage.status);
                }
                let result = if let Some(status) = candidate.reviewed_status {
                    MqMqiResult::reviewed_output(
                        status,
                        candidate.output.clone(),
                        admitted.envelope,
                    )
                    .map_err(|_| HostProblem::Malformed)?
                } else if let MqMqiOutput::Got { disposition, .. } = &candidate.output {
                    let (completion, reason) = match disposition {
                        MqGetDisposition::Message(MqTruncationDisposition::Complete { .. }) => {
                            ("MQCC_OK", "MQRC_NONE")
                        }
                        MqGetDisposition::Message(
                            MqTruncationDisposition::AcceptedRemoved { .. }
                            | MqTruncationDisposition::AcceptedBrowsed { .. },
                        ) => ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_ACCEPTED"),
                        MqGetDisposition::Message(MqTruncationDisposition::RejectedRetained {
                            ..
                        }) => ("MQCC_WARNING", "MQRC_TRUNCATED_MSG_FAILED"),
                        MqGetDisposition::NoMessage | MqGetDisposition::WaitExpired => {
                            ("MQCC_FAILED", "MQRC_NO_MSG_AVAILABLE")
                        }
                        MqGetDisposition::UnknownOutcome => {
                            return Err(HostProblem::UnknownOutcome);
                        }
                    };
                    let status = MqReviewedStatus::from_symbols(MqMqiCall::Get, completion, reason)
                        .map_err(|_| HostProblem::Unsupported)?;
                    // Source-pair legality describes output, never SAF/UOW permission.
                    MqMqiResult::reviewed_output(
                        status,
                        candidate.output.clone(),
                        admitted.envelope,
                    )
                    .map_err(|_| HostProblem::Malformed)?
                } else {
                    MqMqiResult {
                        call: admitted.envelope.request.call(),
                        outcome: MqMqiOutcome::Completed {
                            status: MqMqiStatus::OkNone,
                            output: candidate.output.clone(),
                        },
                    }
                };
                let mut reply = EffectResult {
                    sequence: admitted.effect().sequence,
                    outcome: Ok(HostResult::MqMqi(MqMqiHostResult {
                        limits: admitted.envelope.limits,
                        result,
                    })),
                };
                let decision_tick = clock.now_tick()?;
                producer::recheck(
                    self,
                    frame,
                    invocation,
                    admitted,
                    &runtime.directory,
                    decision_tick,
                )?;
                let preflight = admitted.preflight_result(&reply, decision_tick)?;
                let bytes = runtime
                    .reply_bytes
                    .checked_add(preflight.host_result_bytes)
                    .ok_or(HostProblem::ResourceExhausted)?;
                if bytes > self.limits.max_state_bytes
                    || runtime.replies.len() >= self.limits.max_replays
                {
                    return Err(HostProblem::ResourceExhausted);
                }
                let receipt = receipt::OccurrenceReceipt::capture(
                    admitted,
                    &reply,
                    &candidate.control,
                    decision_tick,
                    host_limits,
                    self.limits
                        .max_state_bytes
                        .min(admitted.envelope.limits.canonical_bytes),
                    authorized.into_resources()?,
                )?;
                let mut additions = state.ownership.admission_changes(
                    &candidate.control,
                    &candidate.units,
                    &candidate.unit_dependencies,
                    self.limits,
                )?;
                additions.push(receipt.insertion(self.limits)?);
                let plan = state
                    .plan_selected_delivery(
                        &candidate.delivery,
                        additions,
                        rich_state::publication::PublicationLimits::default(),
                    )
                    .map_err(|_| HostProblem::Malformed)?;
                let (mutations, next) = plan.into_parts();
                let publish = binding
                    .prepare(
                        // Known host publication succeeded, including a lossless
                        // MQ failure observation. Its exact MQCC/MQRC remains in
                        // the original result; core failure audits publish no rows.
                        audit(admitted, decision_tick, AuditDecision::Success),
                        mutations,
                        decision_tick,
                    )
                    .map_err(intent_error)?
                    .publish(decision_tick)
                    .map_err(intent_error);
                if let Err(error) = publish {
                    return Err(error);
                }
                // All physical rows and audit committed. Only now adopt delivery,
                // owner/bindings and cached reply; core completion stays external.
                *state = next;
                if stage.is_some() && self.unknown_after_persist.swap(false, Ordering::SeqCst) {
                    // Durable receipt exists, but no live/provisional property
                    // mutation may be adopted on an uncertain publication boundary.
                    return Err(HostProblem::UnknownOutcome);
                }
                if let Some(stage) = stage {
                    if let Some(live) = stage.adopt() {
                        let Ok(HostResult::MqMqi(value)) = &mut reply.outcome else {
                            return Err(HostProblem::UnknownOutcome);
                        };
                        let MqMqiOutcome::ReviewedOutput { output, .. } = &mut value.result.outcome
                        else {
                            return Err(HostProblem::UnknownOutcome);
                        };
                        *output = MqMqiOutput::MessageHandle(live);
                    }
                    if mainframe_env_host_api::canonical_result_digest(&reply.outcome)
                        .map_err(|_| HostProblem::UnknownOutcome)?
                        != receipt.result_digest
                    {
                        return Err(HostProblem::UnknownOutcome);
                    }
                }
                drop(access);
                transition::adopt(&mut runtime, owner, &mut candidate)?;
                runtime.control = candidate.control.clone();
                runtime.connections = candidate.connections.clone();
                runtime.objects = candidate.objects.clone();
                runtime.reply_bytes = bytes;
                runtime.replies.insert(key.into(), reply.clone());
                if self.unknown_after_persist.swap(false, Ordering::SeqCst) {
                    return Err(HostProblem::UnknownOutcome);
                }
                admitted
                    .recheck_controls(clock.now_tick()?)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
                producer::recheck(
                    self,
                    frame,
                    invocation,
                    admitted,
                    &runtime.directory,
                    clock.now_tick()?,
                )
                .map_err(|_| HostProblem::UnknownOutcome)?;
                Ok(reply)
            })();
            if attempt.is_err() {
                transition::discard(&mut runtime, owner, &mut candidate)?;
            }
            attempt
        })();
        if matches!(&result, Err(HostProblem::UnknownOutcome)) {
            runtime.fenced = true;
        }
        state.runtime = Some(runtime);
        result
    }

    /// Construction precondition: the real host already admitted this original
    /// Invocation and selected its process topology. This parser/directory does
    /// not attest arbitrary application bindings. No new token is exposed until
    /// its independently retained registry incarnation is atomically published.
    pub(crate) fn mint_selected_process(
        &self,
        invocation: &Invocation,
    ) -> Result<ProcessLease, HostProblem> {
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        let context = decode_host_context(invocation)?.ok_or(HostProblem::Malformed)?;
        if context.environment != MqHostEnvironment::ZosBatch
            || context.owner != MqSyncpointOwner::QueueManager
        {
            return Err(HostProblem::Unsupported);
        }
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        if state.runtime.is_none() {
            state.runtime = Some(SelectedRuntime::new(state, self.limits)?);
        }
        state
            .runtime
            .as_mut()
            .expect("runtime initialized under authority lock")
            .directory
            .mint_process(invocation, now)
    }

    /// Opaque leases have no numeric reconstruction constructor. The caller is
    /// the same independently admitted host as mint_selected_process.
    pub(crate) fn bind_selected_root(
        &self,
        process: ProcessLease,
        invocation: &Invocation,
    ) -> Result<(FrameLease, MqHandleOwner), HostProblem> {
        let now = self
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::Unsupported)?
            .now_tick()?;
        let mut guard = self.lock_selected()?;
        let rich_state::StoredAuthority::Rich(state) = &mut *guard else {
            return Err(HostProblem::Unsupported);
        };
        let runtime = state.runtime.as_mut().ok_or(HostProblem::Unauthorized)?;
        let lease = runtime.directory.bind_root(process, invocation, now)?;
        let owner = runtime.directory.owner_for(lease, invocation, now)?;
        Ok((lease, owner))
    }
}

fn intent_error(problem: MqIntentProblem) -> HostProblem {
    match problem {
        MqIntentProblem::Host(problem) => problem,
        MqIntentProblem::Store(StoreError::Infrastructure(_) | StoreError::Poisoned) => {
            HostProblem::UnknownOutcome
        }
        MqIntentProblem::Store(problem) => store_error(problem),
        MqIntentProblem::NotServiceValidation | MqIntentProblem::NestedCompositionPending => {
            HostProblem::Unsupported
        }
    }
}

fn audit(
    admitted: &crate::mqi_admission::MqMqiAdmitted<'_>,
    tick: u64,
    decision: AuditDecision,
) -> AuditRecord {
    let invocation = admitted.invocation();
    AuditRecord {
        execution_id: invocation.execution_id.clone(),
        run_unit_id: invocation.run_unit_id.clone(),
        attempt: invocation.attempt,
        effect_sequence: admitted.effect().sequence,
        observed_tick: tick,
        principal: invocation.principal.id().clone(),
        invocation_key: invocation.idempotency_key.clone(),
        capability: admitted.capability.clone(),
        resource: canonical_audit_resource_digest(&admitted.effect().request),
        decision,
    }
}

#[cfg(test)]
#[path = "service_selected_operation/tests.rs"]
mod tests;
