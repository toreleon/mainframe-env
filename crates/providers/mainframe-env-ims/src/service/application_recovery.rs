//! Bounded selected application LOG adapter. RecoverySession remains the only
//! log authority; the existing bridge owns atomic selected-generation publication.

use super::*;
use crate::recovery::{LogRequest, RecoveryLimits, RecoveryProblem, RecoverySession};
use mainframe_env_host_api::{
    HostLimits, ImsPcbMetadata, ImsRecoveryCall, ImsRecoveryRequest, ImsRecoveryResult,
};
use mainframe_env_store_api::{EffectDigestFormat, EffectState, IdempotencyStore};

/// Compose the existing read/write providers with the typed application LOG route.
/// `effects` must be the canonical journal from the same product store authority.
/// The original constructor remains available; it rejects recovery requests.
pub fn ims_providers_with_recovery(
    service: Arc<ImsService>,
    effects: Arc<dyn IdempotencyStore>,
    limits: InvocationLimits,
) -> Vec<Arc<dyn HostProvider>> {
    ims_providers(service.clone(), limits)
        .into_iter()
        .map(|inner| {
            Arc::new(ApplicationRecoveryProvider {
                inner,
                service: service.clone(),
                effects: effects.clone(),
            }) as Arc<dyn HostProvider>
        })
        .collect()
}

struct ApplicationRecoveryProvider {
    inner: Arc<dyn HostProvider>,
    service: Arc<ImsService>,
    effects: Arc<dyn IdempotencyStore>,
}

impl HostProvider for ApplicationRecoveryProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.inner.descriptor()
    }

    fn invoke(&self, invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let HostRequest::ImsRecovery(request) = &effect.request else {
            return self.inner.invoke(invocation, effect);
        };
        let outcome = (|| {
            effect.validate(HostLimits::default())?;
            if effect.run_unit != invocation.run_unit_id {
                return Err(HostProblem::Malformed);
            }
            if invocation.cancellation_requested() {
                return Err(HostProblem::Cancelled);
            }
            let capability = effect
                .request
                .required_capability(InvocationLimits::default());
            if self.descriptor().capability != capability
                || !invocation.principal.has_grant(&capability)
            {
                return Err(HostProblem::Unauthorized);
            }
            if invocation.service_class != ServiceClass::Batch {
                return Err(HostProblem::Unsupported);
            }
            let digest = mainframe_env_host_api::canonical_request_digest(&effect.request)?;
            self.service
                .application_log(invocation, request, &*self.effects, digest)
                .map(HostResult::ImsRecovery)
        })();
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

impl ImsService {
    fn application_log(
        &self,
        invocation: &Invocation,
        request: &ImsRecoveryRequest,
        effects: &dyn IdempotencyStore,
        digest: [u8; 32],
    ) -> Result<ImsRecoveryResult, HostProblem> {
        // Unlike compatibility constructors, this new route requires the typed
        // production authorization authority even when invoked without ScopedHost.
        let authorizer = self.authorizer.as_ref().ok_or(HostProblem::Unauthorized)?;
        let resource = EnterpriseResource::new(
            EnterpriseResourceClass::ImsPsb,
            request.psb.clone(),
            AccessIntent::Update,
        )?;
        authorizer.authorize(invocation.principal.id(), &resource)?;
        self.recovery_database_resource(
            invocation,
            &request.application,
            &request.package_identity,
            &request.database,
        )
        .map_err(recovery_error)?;
        let selected = self
            .selected_metadata_generation(&request.application)?
            .ok_or(HostProblem::NotFound)?;
        if selected.package_identity != request.package_identity {
            return Err(HostProblem::IdempotencyConflict);
        }
        let psb = selected
            .catalog
            .psbs
            .iter()
            .find(|psb| psb.name == request.psb)
            .ok_or(HostProblem::NotFound)?;
        if !psb.pcbs.iter().any(|pcb| {
            matches!(pcb,
            ImsPcbMetadata::Database(pcb) if pcb.database == request.database)
        }) {
            return Err(HostProblem::Malformed);
        }
        let key = &request.mutation.idempotency_key;
        let effect = effects
            .effect(key)
            .map_err(store_error)?
            .ok_or(HostProblem::MissingIdempotency)?;
        if effect.digest_format != EffectDigestFormat::CanonicalHostV1
            || effect.execution_id != invocation.execution_id
            || effect.run_unit_id != invocation.run_unit_id
            || effect.sequence != request.mutation.sequence
            || effect.request_digest != digest
            || effect.intent.owner != invocation.execution_id
            || effect.intent.attempt != invocation.attempt
            || effect.intent.capability.as_ref().map(CapabilityId::as_str) != Some("host.ims.write")
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        if effect.intent.recovery_lease.is_some() || effect.state == EffectState::UnknownOutcome {
            return Err(HostProblem::UnknownOutcome);
        }
        if effect.state == EffectState::Failed {
            return Err(HostProblem::IdempotencyConflict);
        }
        if effect.intent.created_tick == 0 || effect.intent.created_tick >= invocation.deadline_tick
        {
            return Err(HostProblem::TimedOut);
        }
        // Address the existing recovery row by an unambiguous selected binding.
        // This avoids collisions with other PSBs/generations using the same run.
        let mut run_material = Sha256::new();
        run_material.update(b"mainframe-env.ims-application-recovery-run@1\0");
        for value in [
            &selected.application,
            &request.package_identity,
            &request.psb,
            &request.database,
            invocation.run_unit_id.as_str(),
            invocation.principal.id().as_str(),
        ] {
            run_material.update((value.len() as u64).to_le_bytes());
            run_material.update(value.as_bytes());
        }
        let run = format!("{:x}", run_material.finalize());
        let recovery = RecoverySession::load(&*self.store, &run, RecoveryLimits::default())
            .map_err(recovery_error)?;
        let mut effect_material = Sha256::new();
        effect_material.update(b"mainframe-env.ims-application-recovery-effect@1\0");
        for value in [invocation.execution_id.as_str(), key.as_str()] {
            effect_material.update((value.len() as u64).to_le_bytes());
            effect_material.update(value.as_bytes());
        }
        effect_material.update(request.mutation.sequence.to_le_bytes());
        let effect_id = format!("{:x}", effect_material.finalize());
        let ImsRecoveryCall::Log { code, data } = &request.call;
        let transition = recovery
            .log(
                &effect_id,
                LogRequest {
                    code: *code,
                    data: data.clone(),
                },
            )
            .map_err(recovery_error)?;
        let result = ImsRecoveryResult::Logged {
            status: "  ".into(),
            sequence: transition.sequence(),
        };
        self.publish_database_recovery_transition(
            invocation,
            &request.application,
            &request.package_identity,
            &request.database,
            transition,
            effects,
            key,
            digest,
        )
        .map_err(recovery_error)?;
        Ok(result)
    }
}

fn recovery_error(problem: RecoveryProblem) -> HostProblem {
    match problem {
        RecoveryProblem::InvalidRequest => HostProblem::Malformed,
        RecoveryProblem::Unauthorized => HostProblem::Unauthorized,
        RecoveryProblem::LimitExceeded => HostProblem::ResourceExhausted,
        RecoveryProblem::Unsupported => HostProblem::Unsupported,
        RecoveryProblem::NotFound => HostProblem::NotFound,
        RecoveryProblem::Conflict => HostProblem::IdempotencyConflict,
        RecoveryProblem::CorruptImage => HostProblem::ProviderFailure,
        RecoveryProblem::InfrastructureFailure => HostProblem::InfrastructureFailure,
        RecoveryProblem::UnknownOutcome => HostProblem::UnknownOutcome,
    }
}
