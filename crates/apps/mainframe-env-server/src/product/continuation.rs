use super::{OnlineExchangeState, ProductServer, normalize_online_name, store_error};
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, Invocation, InvocationLimits, Machine, PrincipalId,
    Suspension,
};
use mainframe_env_host_api::{HostProblem, ScopedHostService, SessionId};
use mainframe_env_interpreter::{ExecutionCoordinator, ReferenceMachine};
use mainframe_env_store_api::ProviderStateRecord;
use std::collections::BTreeMap;

pub(super) const ONLINE_PROVIDER_CAPABILITIES: [&str; 12] = [
    "host.security.authorize",
    "host.cics.execute",
    "host.dataset.read",
    "host.dataset.write",
    "host.db2.read",
    "host.db2.write",
    "host.ims.read",
    "host.ims.write",
    "host.mq.read",
    "host.mq.write",
    "host.program.invoke",
    "host.clock",
];

pub(super) struct OnlineMachineContinuation {
    pub(super) program: String,
    pub(super) artifact: ArtifactRef,
    pub(super) provider_generations: BTreeMap<CapabilityId, String>,
    pub(super) checkpoint: BoundedPayload,
    pub(super) version: u64,
}

impl ProductServer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_online_suspension(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        program: &str,
        artifact: &ArtifactRef,
        invocation: &Invocation,
        machine: &ReferenceMachine,
        current_version: Option<u64>,
        suspension: &Suspension,
        coordinator: &ExecutionCoordinator,
        exchange: &OnlineExchangeState,
        now_tick: u64,
    ) -> Result<(), HostProblem> {
        let checkpoint = machine.checkpoint().ok_or(HostProblem::ProviderFailure)?;
        self.persist_online_machine_continuation(
            session,
            program,
            artifact,
            &invocation.provider_generations,
            &checkpoint,
            current_version,
        )?;
        match suspension.kind.as_str() {
            "cics-enqueue" => return Ok(()),
            "cics-terminal" => {}
            _ => return Err(HostProblem::InfrastructureFailure),
        }
        let control = self
            .program
            .observe_execution_control(invocation)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        coordinator
            .complete_suspended_handoff(invocation, control.now_tick)
            .map_err(store_error)?;
        self.program.finish_run_unit(invocation)?;
        self.finish_online_machine_run(session, principal, now_tick)?;
        self.clear_online_exchange(session, exchange)
    }
}

pub(super) fn encode_online_machine_continuation(
    program: &str,
    artifact: &ArtifactRef,
    provider_generations: &BTreeMap<CapabilityId, String>,
    checkpoint: &BoundedPayload,
) -> Result<Vec<u8>, HostProblem> {
    let generations = encode_provider_generations(provider_generations)?;
    let mut encoded = b"MEOM2".to_vec();
    for value in [
        program.as_bytes(),
        artifact.as_str().as_bytes(),
        &generations,
        checkpoint.schema().as_bytes(),
        checkpoint.bytes(),
    ] {
        encoded.extend_from_slice(
            &u32::try_from(value.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        encoded.extend_from_slice(value);
    }
    Ok(encoded)
}

pub(super) fn decode_online_machine_continuation(
    record: &ProviderStateRecord,
) -> Result<OnlineMachineContinuation, HostProblem> {
    if !record.payload.starts_with(b"MEOM2") {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 5usize;
    let mut next = || -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(u32::from_be_bytes(
            record
                .payload
                .get(at..at + 4)
                .ok_or(HostProblem::InfrastructureFailure)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        at += 4;
        let end = at
            .checked_add(length)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = record
            .payload
            .get(at..end)
            .ok_or(HostProblem::InfrastructureFailure)?
            .to_vec();
        at = end;
        Ok(value)
    };
    let program = String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let artifact = ArtifactRef::new(
        String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?,
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    let provider_generations = decode_provider_generations(&next()?)?;
    let schema = String::from_utf8(next()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    let bytes = next()?;
    if at != record.payload.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(OnlineMachineContinuation {
        program: normalize_online_name(&program, 128)?,
        artifact,
        provider_generations,
        checkpoint: BoundedPayload::new(
            schema,
            bytes,
            InvocationLimits {
                max_payload_bytes: 64 * 1024 * 1024,
                ..InvocationLimits::default()
            },
        )
        .map_err(|_| HostProblem::InfrastructureFailure)?,
        version: record.version,
    })
}

fn encode_provider_generations(
    generations: &BTreeMap<CapabilityId, String>,
) -> Result<Vec<u8>, HostProblem> {
    let limits = InvocationLimits::default();
    if !has_exact_provider_generation_shape(generations)
        || generations
            .values()
            .any(|generation| generation.is_empty() || generation.len() > limits.max_identity_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    serde_json::to_vec(
        &generations
            .iter()
            .map(|(capability, generation)| (capability.as_str().to_string(), generation.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn decode_provider_generations(
    bytes: &[u8],
) -> Result<BTreeMap<CapabilityId, String>, HostProblem> {
    let values: BTreeMap<String, String> =
        serde_json::from_slice(bytes).map_err(|_| HostProblem::InfrastructureFailure)?;
    if serde_json::to_vec(&values).map_err(|_| HostProblem::InfrastructureFailure)? != bytes {
        return Err(HostProblem::InfrastructureFailure);
    }
    let limits = InvocationLimits::default();
    if values.len() > limits.max_capabilities {
        return Err(HostProblem::InfrastructureFailure);
    }
    let generations = values
        .into_iter()
        .map(|(capability, generation)| {
            if generation.is_empty() || generation.len() > limits.max_identity_bytes {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok((
                CapabilityId::new(capability, limits)
                    .map_err(|_| HostProblem::InfrastructureFailure)?,
                generation,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    if !has_exact_provider_generation_shape(&generations) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(generations)
}

pub(super) fn preflight_provider_generations(
    host: &ScopedHostService,
    generations: &BTreeMap<CapabilityId, String>,
) -> Result<(), HostProblem> {
    if !has_exact_provider_generation_shape(generations) {
        return Err(HostProblem::ProviderFailure);
    }
    host.validate_provider_generations(generations)
}

fn has_exact_provider_generation_shape(generations: &BTreeMap<CapabilityId, String>) -> bool {
    generations.len() == ONLINE_PROVIDER_CAPABILITIES.len()
        && ONLINE_PROVIDER_CAPABILITIES.iter().all(|expected| {
            generations
                .keys()
                .any(|capability| capability.as_str() == *expected)
        })
}
