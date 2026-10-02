//! Observed installed admission provenance; never a process/registry lease.

use super::*;
use crate::cobol::artifact::AdmittedProgramProvenance;
use mainframe_env_compiler_api::ArtifactManifest;
use mainframe_env_host_api::{canonical_audit_resource_digest, canonical_request_digest};
use mainframe_env_store_api::{
    EffectDigestFormat, EffectRecord, EffectState, ExecutableArtifactMetadata, ExecutionRecord,
    ExecutionState,
};

/// Server-constructed only after validated artifact admission and a winning
/// original CALL reservation. Observations do not attest an MQ host root or SAF.
/// No Clone, serialization or caller-supplied lease constructor is provided.
pub struct InstalledBatchAdmission<'a> {
    parent: &'a Invocation,
    child: &'a Invocation,
    admitted: &'a AdmittedProgram,
    call: &'a replay::WinningInstalledCall<'a>,
    store: &'a Arc<dyn PlatformStore>,
    control: &'a Arc<dyn ProgramExecutionControl>,
    host: &'a Arc<ScopedHostService>,
    artifacts: &'a Arc<dyn ArtifactStore>,
    core: EffectRecord,
    execution: ExecutionRecord,
    observed: ExecutionControl,
}

impl InstalledBatchAdmission<'_> {
    pub fn parent(&self) -> &Invocation {
        self.parent
    }
    pub fn child(&self) -> &Invocation {
        self.child
    }
    pub fn artifact(&self) -> &ArtifactRef {
        &self.admitted.artifact
    }
    pub fn content_digest(&self) -> [u8; 32] {
        *self.admitted.executable.content_id().as_bytes()
    }
    pub fn manifest(&self) -> &ArtifactManifest {
        self.admitted.executable.manifest()
    }
    pub fn artifact_metadata(&self) -> &ExecutableArtifactMetadata {
        &self.admitted.metadata
    }
    pub fn catalog_record(&self) -> Option<&ProviderStateRecord> {
        match &self.admitted.provenance {
            AdmittedProgramProvenance::Catalog(record) => Some(record),
            _ => None,
        }
    }
    pub fn selection(&self) -> Option<&mainframe_env_host_api::ProgramLinkSelection> {
        match &self.admitted.provenance {
            AdmittedProgramProvenance::Selected(selection) => Some(selection),
            _ => None,
        }
    }
    pub fn original_call(&self) -> &EffectRequest {
        self.call.effect()
    }
    pub fn call_reservation(&self) -> &ProviderStateRecord {
        self.call.reservation()
    }
    pub fn core_intent(&self) -> &EffectRecord {
        &self.core
    }
    pub fn running_parent(&self) -> &ExecutionRecord {
        &self.execution
    }
    /// The actual frozen physical adapter. Equal rows in another store are not identity.
    pub fn store(&self) -> &Arc<dyn PlatformStore> {
        self.store
    }
    pub fn execution_control(&self) -> &Arc<dyn ProgramExecutionControl> {
        self.control
    }
    pub fn host_runtime(&self) -> &Arc<ScopedHostService> {
        self.host
    }
    pub fn artifact_store(&self) -> &Arc<dyn ArtifactStore> {
        self.artifacts
    }
    pub fn observed_control(&self) -> ExecutionControl {
        self.observed
    }
}

/// Existing core observation protocol; not an atomic publication or lease permit.
pub(super) fn observe_parent(
    program: &CobolProgram,
    parent: &Invocation,
    effect: &EffectRequest,
    now: u64,
) -> Result<(EffectRecord, ExecutionRecord), HostProblem> {
    let store = program
        .store
        .get()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let key = effect
        .idempotency_key
        .as_ref()
        .ok_or(HostProblem::Malformed)?;
    let capability = effect
        .request
        .required_capability(InvocationLimits::default());
    let core = store
        .effect(key)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .ok_or(HostProblem::Unauthorized)?;
    let metadata = &core.intent;
    if now == 0
        || now > i64::MAX as u64
        || effect.run_unit != parent.run_unit_id
        || effect.sequence == 0
        || effect.sequence > u64::from(parent.limits.max_effects)
        || !matches!(
            &effect.request,
            HostRequest::Program(ProgramRequest::Call { service: None, .. })
        )
        || core.key != *key
        || core.execution_id != parent.execution_id
        || core.run_unit_id != parent.run_unit_id
        || core.sequence != effect.sequence
        || core.digest_format != EffectDigestFormat::CanonicalHostV1
        || core.request_digest != canonical_request_digest(&effect.request)?
        || core.state != EffectState::Intent
        || core.result_digest.is_some()
        || core.resolved_tick.is_some()
        || metadata.owner != parent.execution_id
        || metadata.attempt == 0
        || metadata.attempt != parent.attempt
        || metadata.capability.as_ref() != Some(&capability)
        || metadata.audit_resource != Some(canonical_audit_resource_digest(&effect.request))
        || metadata.audit_invocation_key.as_ref() != Some(&parent.idempotency_key)
        || metadata.created_tick == 0
        || metadata.created_tick > now
        || metadata.recovery_after_tick != parent.deadline_tick.min(effect.deadline_tick)
        || metadata.recovery_after_tick > i64::MAX as u64
        || now >= metadata.recovery_after_tick
        || metadata.epoch == 0
        || metadata.recovery_lease.is_some()
    {
        return Err(HostProblem::Unauthorized);
    }
    let execution = store
        .get_execution(&parent.execution_id)
        .map_err(|_| HostProblem::InfrastructureFailure)?
        .ok_or(HostProblem::Unauthorized)?;
    if execution.execution_id != parent.execution_id
        || execution.run_unit_id != parent.run_unit_id
        || execution.principal != *parent.principal.id()
        || execution.attempt != parent.attempt
        || execution.selector != parent.selector
        || execution.artifact != parent.artifact
        || execution.version == 0
        || execution.version > i64::MAX as u64
        || execution.state != ExecutionState::Running
        || execution.terminal_tick.is_some()
        || execution.lease_expiry_tick.is_some_and(|tick| tick <= now)
        || store
            .effect(key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
            .as_ref()
            != Some(&core)
    {
        return Err(HostProblem::Unauthorized);
    }
    Ok((core, execution))
}

pub(super) fn admitted<'a>(
    program: &'a CobolProgram,
    parent: &'a Invocation,
    child: &'a Invocation,
    artifact: &'a AdmittedProgram,
    call: &'a replay::WinningInstalledCall<'a>,
    store: &'a Arc<dyn PlatformStore>,
    control: &'a Arc<dyn ProgramExecutionControl>,
    observed: ExecutionControl,
) -> Result<InstalledBatchAdmission<'a>, HostProblem> {
    if !Arc::ptr_eq(store, call.store())
        || call.parent() != parent
        || child.parent_execution_id.as_ref() != Some(&parent.execution_id)
        || child.run_unit_id != parent.run_unit_id
        || child.principal != parent.principal
        || child.deadline_tick != parent.deadline_tick
        || child.limits != parent.limits
        || child.provider_generations != parent.provider_generations
        || child.cancellation_probe != parent.cancellation_probe
        || child.cancellation != parent.cancellation
        || child.attempt != parent.attempt
        || child.service_class != parent.service_class
        || child.priority != parent.priority
        || child.artifact != artifact.artifact
        || child.execution_id.as_str() != call.child_execution()
    {
        return Err(HostProblem::Unauthorized);
    }
    let HostRequest::Program(ProgramRequest::Call {
        program: name,
        payload,
        service: None,
    }) = &call.effect().request
    else {
        return Err(HostProblem::Unsupported);
    };
    if !name.as_str().eq_ignore_ascii_case(&artifact.name)
        || payload.schema() != "mainframe-env.program.input@1"
    {
        return Err(HostProblem::Unauthorized);
    }
    call.recheck()?;
    match &artifact.provenance {
        AdmittedProgramProvenance::Catalog(record) => {
            if record.namespace != "batch-program"
                || record.key != artifact.name
                || record.version == 0
                || record.version > i64::MAX as u64
                || record.payload != artifact.artifact.as_str().as_bytes()
                || store
                    .get_provider_state(&record.namespace, &record.key)
                    .map_err(|_| HostProblem::InfrastructureFailure)?
                    .as_ref()
                    != Some(record)
            {
                return Err(HostProblem::Unauthorized);
            }
        }
        // Selected CICS/online provenance is retained, but not impersonated as batch.
        AdmittedProgramProvenance::Selected(_) => return Err(HostProblem::Unsupported),
    }
    let (core, execution) = observe_parent(program, parent, call.effect(), observed.now_tick)?;
    Ok(InstalledBatchAdmission {
        parent,
        child,
        admitted: artifact,
        call,
        store,
        control,
        host: program
            .host
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?,
        artifacts: program
            .artifacts
            .get()
            .ok_or(HostProblem::InfrastructureFailure)?,
        core,
        execution,
        observed,
    })
}
