//! Run-unit-owned last-used instances. A busy instance is never reset on retry.
use super::*;
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateWrite, StoreError};
use serde::{Deserialize, Serialize};

mod abend;
mod transfer;

use super::retention::{
    CALL_PROTOCOL_NAMESPACE, CANCEL_NAMESPACE, CobolRetentionRowDescriptor, CobolRetentionRowKind,
    CobolRetentionState, CobolRetentionValidationError, INSTANCE_NAMESPACE_PREFIX,
    RUN_STATE_NAMESPACE, owner_dependencies, protocol_key, provider_dependency, valid_digest,
    valid_identity, validate_row_identity,
};

const MAX_INSTANCES: usize = 256;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RunState {
    schema_version: u32,
    #[serde(default)]
    owner_execution: Option<String>,
    #[serde(default)]
    owner_run_unit: Option<String>,
    #[serde(default)]
    owner_principal: Option<String>,
    #[serde(default)]
    metadata_digest: Option<String>,
    active: usize,
    instances: usize,
    programs: std::collections::BTreeSet<String>,
    ended: bool,
    #[serde(default)]
    ended_tick: Option<u64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Instance {
    schema_version: u32,
    artifact: String,
    busy: bool,
    open_files: bool,
    state: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    abend: Option<abend::AbendProof>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CancelReceipt {
    schema_version: u32,
    cancel_key: String,
    fingerprint: String,
    owner_execution: String,
    owner_run_unit: String,
    owner_principal: String,
    protocol_key: String,
    run_state_key: String,
    metadata_digest: String,
    cancelled_tick: u64,
}

fn cancel_metadata_digest(receipt: &CancelReceipt) -> String {
    let tick = receipt.cancelled_tick.to_be_bytes();
    super::replay::digest(&[
        b"cancel-metadata",
        receipt.cancel_key.as_bytes(),
        receipt.fingerprint.as_bytes(),
        receipt.owner_execution.as_bytes(),
        receipt.owner_run_unit.as_bytes(),
        receipt.owner_principal.as_bytes(),
        receipt.protocol_key.as_bytes(),
        receipt.run_state_key.as_bytes(),
        &tick,
    ])
}

pub(super) struct Lease {
    invocation: Invocation,
    run: String,
    namespace: String,
    name: String,
    version: u64,
    initial: bool,
    instance: Instance,
}

pub(super) fn run_key(invocation: &Invocation) -> String {
    super::retention::run_state_key(
        invocation.run_unit_id.as_str(),
        invocation.principal.id().as_str(),
    )
}
fn namespace(key: &str) -> String {
    format!("{INSTANCE_NAMESPACE_PREFIX}{key}")
}

fn decode_run_state(
    record: &ProviderStateRecord,
) -> Result<RunState, CobolRetentionValidationError> {
    validate_row_identity(record, RUN_STATE_NAMESPACE)?;
    if !valid_digest(&record.key) {
        return Err(CobolRetentionValidationError::InvalidIdentity);
    }
    let value: RunState = serde_json::from_slice(&record.payload)
        .map_err(|_| CobolRetentionValidationError::CorruptPayload)?;
    if !matches!(value.schema_version, 1 | 2)
        || value.instances > MAX_INSTANCES
        || value.instances != value.programs.len()
        || value.active > value.instances
        || value.programs.iter().any(|program| !valid_program(program))
        || value.schema_version == 1
            && (value.owner_execution.is_some()
                || value.owner_run_unit.is_some()
                || value.owner_principal.is_some()
                || value.metadata_digest.is_some()
                || value.ended_tick.is_some())
        || value.schema_version == 2
            && (!value.owner_execution.as_deref().is_some_and(valid_identity)
                || !value.owner_run_unit.as_deref().is_some_and(valid_identity)
                || !value.owner_principal.as_deref().is_some_and(valid_identity)
                || value
                    .owner_run_unit
                    .as_deref()
                    .zip(value.owner_principal.as_deref())
                    .is_none_or(|(run_unit, principal)| {
                        super::retention::run_state_key(run_unit, principal) != record.key
                    })
                || !valid_run_metadata(&value, &record.key)
                || value.ended != value.ended_tick.is_some()
                || value.ended_tick == Some(0))
        || value.ended && (value.active != 0 || value.instances != 0 || !value.programs.is_empty())
    {
        return Err(CobolRetentionValidationError::InconsistentState);
    }
    Ok(value)
}

fn run_metadata_digest(state: &RunState, key: &str) -> String {
    let mut parts = vec![
        b"run-state-metadata".as_slice(),
        key.as_bytes(),
        state
            .owner_execution
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        state
            .owner_run_unit
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        state
            .owner_principal
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    ];
    let active = (state.active as u64).to_be_bytes();
    let instances = (state.instances as u64).to_be_bytes();
    let ended = [u8::from(state.ended)];
    let ended_tick = state.ended_tick.unwrap_or(0).to_be_bytes();
    parts.extend([
        active.as_slice(),
        instances.as_slice(),
        ended.as_slice(),
        ended_tick.as_slice(),
    ]);
    let mut hash = sha2::Sha256::new();
    use sha2::Digest as _;
    hash.update(b"mainframe-env.installed-call@1\0");
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    for program in &state.programs {
        hash.update((program.len() as u64).to_be_bytes());
        hash.update(program.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn valid_run_metadata(state: &RunState, key: &str) -> bool {
    state
        .metadata_digest
        .as_deref()
        .is_some_and(|digest| digest == run_metadata_digest(state, key))
}

fn refresh_run_metadata(state: &mut RunState, key: &str) {
    state.metadata_digest = None;
    state.metadata_digest = Some(run_metadata_digest(state, key));
}

fn load_run(store: &dyn PlatformStore, key: &str) -> Result<(RunState, Option<u64>), HostProblem> {
    match store
        .get_provider_state(RUN_STATE_NAMESPACE, key)
        .map_err(|_| HostProblem::InfrastructureFailure)?
    {
        None => Ok((
            RunState {
                schema_version: 2,
                ..Default::default()
            },
            None,
        )),
        Some(record) => Ok((
            decode_run_state(&record).map_err(|_| HostProblem::UnknownOutcome)?,
            Some(record.version),
        )),
    }
}
fn write<T: Serialize>(
    namespace: &str,
    key: &str,
    value: &T,
    expected: Option<u64>,
) -> Result<ProviderStateWrite, HostProblem> {
    Ok(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: namespace.into(),
            key: key.into(),
            version: expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload: serde_json::to_vec(value).map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: expected,
    })
}
fn decode_instance(
    record: &ProviderStateRecord,
) -> Result<Instance, CobolRetentionValidationError> {
    let value: Instance = serde_json::from_slice(&record.payload)
        .map_err(|_| CobolRetentionValidationError::CorruptPayload)?;
    if !abend::valid_instance(record, &value)
        || !valid_instance_namespace(&record.namespace)
        || !valid_program(&record.key)
        || record.version == 0
        || record.version > i64::MAX as u64
        || value.artifact.is_empty() && (value.busy || value.open_files || value.state.is_some())
        || !value.artifact.is_empty() && !valid_identity(&value.artifact)
    {
        return Err(CobolRetentionValidationError::InconsistentState);
    }
    Ok(value)
}

fn load_instance(record: &ProviderStateRecord) -> Result<Instance, HostProblem> {
    decode_instance(record).map_err(|_| HostProblem::UnknownOutcome)
}

fn valid_program(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 246
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_')
        })
}

fn valid_instance_namespace(value: &str) -> bool {
    value
        .strip_prefix(INSTANCE_NAMESPACE_PREFIX)
        .is_some_and(valid_digest)
}

fn adopt_run_owner(state: &mut RunState, invocation: &Invocation) -> Result<(), HostProblem> {
    if state.schema_version == 1 {
        state.schema_version = 2;
        state.owner_execution = Some(super::replay::protocol_owner_execution(invocation)?);
        state.owner_run_unit = Some(invocation.run_unit_id.as_str().into());
        state.owner_principal = Some(invocation.principal.id().as_str().into());
    } else {
        let expected_execution = super::replay::protocol_owner_execution(invocation)?;
        if state
            .owner_execution
            .as_deref()
            .is_some_and(|owner| owner != expected_execution.as_str())
            || state
                .owner_run_unit
                .as_deref()
                .is_some_and(|owner| owner != invocation.run_unit_id.as_str())
            || state
                .owner_principal
                .as_deref()
                .is_some_and(|owner| owner != invocation.principal.id().as_str())
        {
            return Err(HostProblem::UnknownOutcome);
        }
    }
    if state.owner_execution.is_none() {
        state.owner_execution = Some(super::replay::protocol_owner_execution(invocation)?);
    }
    if state.owner_run_unit.is_none() {
        state.owner_run_unit = Some(invocation.run_unit_id.as_str().into());
    }
    if state.owner_principal.is_none() {
        state.owner_principal = Some(invocation.principal.id().as_str().into());
    }
    refresh_run_metadata(state, &run_key(invocation));
    Ok(())
}

#[allow(dead_code, reason = "R-11 product integration seam")]
pub(super) fn describe_run_state_row(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    let state = decode_run_state(record)?;
    if state.schema_version == 1 {
        return Ok(CobolRetentionRowDescriptor {
            namespace: record.namespace.clone(),
            key: record.key.clone(),
            row_version: record.version,
            kind: CobolRetentionRowKind::RunState,
            state: if state.ended {
                CobolRetentionState::LegacyProtected
            } else {
                CobolRetentionState::Active
            },
            owner_execution: None,
            owner_run_unit: None,
            terminal_tick: None,
            dependencies: Vec::new(),
        });
    }
    let owner_execution = state
        .owner_execution
        .ok_or(CobolRetentionValidationError::InconsistentState)?;
    let owner_run_unit = state
        .owner_run_unit
        .ok_or(CobolRetentionValidationError::InconsistentState)?;
    let dependencies = owner_dependencies(&owner_execution, &owner_run_unit);
    Ok(CobolRetentionRowDescriptor {
        namespace: record.namespace.clone(),
        key: record.key.clone(),
        row_version: record.version,
        kind: CobolRetentionRowKind::RunState,
        state: if state.ended {
            CobolRetentionState::Terminal
        } else {
            CobolRetentionState::Active
        },
        owner_execution: Some(owner_execution),
        owner_run_unit: Some(owner_run_unit),
        terminal_tick: state.ended_tick,
        dependencies,
    })
}

#[allow(dead_code, reason = "R-11 product integration seam")]
pub(super) fn describe_instance_row(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    let instance = decode_instance(record)?;
    let run_key = record
        .namespace
        .strip_prefix(INSTANCE_NAMESPACE_PREFIX)
        .ok_or(CobolRetentionValidationError::WrongNamespace)?;
    let mut dependencies = vec![provider_dependency(RUN_STATE_NAMESPACE, run_key)];
    if let Some(proof) = instance.abend {
        if proof.owner_execution != proof.execution {
            dependencies.push(super::retention::CobolRetentionDependency::Execution(
                proof.owner_execution,
            ));
        }
        dependencies.push(super::retention::CobolRetentionDependency::Execution(
            proof.execution,
        ));
    }
    Ok(CobolRetentionRowDescriptor {
        namespace: record.namespace.clone(),
        key: record.key.clone(),
        row_version: record.version,
        kind: CobolRetentionRowKind::Instance,
        state: CobolRetentionState::Active,
        owner_execution: None,
        owner_run_unit: None,
        terminal_tick: None,
        dependencies,
    })
}

fn decode_cancel_receipt(
    record: &ProviderStateRecord,
) -> Result<Option<CancelReceipt>, CobolRetentionValidationError> {
    validate_row_identity(record, CANCEL_NAMESPACE)?;
    if !valid_digest(&record.key) || record.version != 1 {
        return Err(CobolRetentionValidationError::InvalidIdentity);
    }
    let Ok(receipt) = serde_json::from_slice::<CancelReceipt>(&record.payload) else {
        // Legacy rows contain the canonical request tuple directly. Decode the
        // complete tuple so arbitrary corrupt bytes cannot gain legacy status.
        let legacy: (u32, u64, std::collections::BTreeSet<String>) =
            serde_json::from_slice(&record.payload)
                .map_err(|_| CobolRetentionValidationError::CorruptPayload)?;
        if legacy.0 != 1
            || legacy.1 == 0
            || legacy.2.is_empty()
            || legacy.2.len() > MAX_INSTANCES
            || legacy.2.iter().any(|program| !valid_program(program))
            || serde_json::to_vec(&legacy).ok().as_deref() != Some(record.payload.as_slice())
        {
            return Err(CobolRetentionValidationError::InconsistentState);
        }
        return Ok(None);
    };
    if receipt.schema_version != 2
        || receipt.cancel_key != record.key
        || !valid_digest(&receipt.fingerprint)
        || !valid_identity(&receipt.owner_execution)
        || !valid_identity(&receipt.owner_run_unit)
        || !valid_identity(&receipt.owner_principal)
        || receipt.protocol_key != protocol_key(&receipt.owner_run_unit)
        || receipt.run_state_key
            != super::retention::run_state_key(&receipt.owner_run_unit, &receipt.owner_principal)
        || receipt.metadata_digest != cancel_metadata_digest(&receipt)
        || receipt.cancelled_tick == 0
    {
        return Err(CobolRetentionValidationError::InconsistentState);
    }
    Ok(Some(receipt))
}

fn cancel_receipt_matches(
    record: &ProviderStateRecord,
    legacy_fingerprint: &[u8],
    fingerprint: &str,
    parent: &Invocation,
) -> bool {
    match decode_cancel_receipt(record) {
        Ok(None) => record.payload == legacy_fingerprint,
        Ok(Some(receipt)) => {
            receipt.fingerprint == fingerprint
                && receipt.owner_execution == parent.execution_id.as_str()
                && receipt.owner_run_unit == parent.run_unit_id.as_str()
                && receipt.owner_principal == parent.principal.id().as_str()
                && receipt.protocol_key == protocol_key(parent.run_unit_id.as_str())
                && receipt.run_state_key == run_key(parent)
        }
        Err(_) => false,
    }
}

#[allow(dead_code, reason = "R-11 product integration seam")]
pub(super) fn describe_cancel_row(
    record: &ProviderStateRecord,
) -> Result<CobolRetentionRowDescriptor, CobolRetentionValidationError> {
    let Some(receipt) = decode_cancel_receipt(record)? else {
        return Ok(CobolRetentionRowDescriptor {
            namespace: record.namespace.clone(),
            key: record.key.clone(),
            row_version: record.version,
            kind: CobolRetentionRowKind::Cancel,
            state: CobolRetentionState::LegacyProtected,
            owner_execution: None,
            owner_run_unit: None,
            terminal_tick: None,
            dependencies: Vec::new(),
        });
    };
    let mut dependencies = owner_dependencies(&receipt.owner_execution, &receipt.owner_run_unit);
    dependencies.push(provider_dependency(
        CALL_PROTOCOL_NAMESPACE,
        receipt.protocol_key,
    ));
    dependencies.push(provider_dependency(
        RUN_STATE_NAMESPACE,
        receipt.run_state_key,
    ));
    Ok(CobolRetentionRowDescriptor {
        namespace: record.namespace.clone(),
        key: record.key.clone(),
        row_version: record.version,
        kind: CobolRetentionRowKind::Cancel,
        state: CobolRetentionState::Terminal,
        owner_execution: Some(receipt.owner_execution),
        owner_run_unit: Some(receipt.owner_run_unit),
        terminal_tick: Some(receipt.cancelled_tick),
        dependencies,
    })
}
fn state_problem(problem: mainframe_env_interpreter::MachineProblem) -> HostProblem {
    match problem {
        mainframe_env_interpreter::MachineProblem::UnsupportedForm => HostProblem::Unsupported,
        mainframe_env_interpreter::MachineProblem::ResourceExhausted => {
            HostProblem::ResourceExhausted
        }
        _ => HostProblem::UnknownOutcome,
    }
}

impl Lease {
    pub(super) fn acquire(
        store: &dyn PlatformStore,
        invocation: &Invocation,
        program: &str,
        machine: &mut ReferenceMachine,
    ) -> Result<Self, HostProblem> {
        let initial = machine.installed_call_is_initial().map_err(state_problem)?;
        let run = run_key(invocation);
        let (mut state, expected_run) = load_run(store, &run)?;
        adopt_run_owner(&mut state, invocation)?;
        if state.ended {
            return Err(HostProblem::Condition {
                name: "COBOL-RUN-ENDED".into(),
                response: -9,
                response2: 0,
            });
        }
        let namespace = namespace(&run);
        if expected_run.is_none()
            && !store
                .list_provider_state(&namespace, 1)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .is_empty()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let name = program.to_ascii_uppercase();
        let existing = store
            .get_provider_state(&namespace, &name)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let expected = existing.as_ref().map(|record| record.version);
        let mut instance = match existing {
            Some(record) => {
                let value = load_instance(&record)?;
                if value.busy || value.abend.is_some() {
                    return Err(HostProblem::UnknownOutcome);
                }
                if !value.artifact.is_empty() && value.artifact != invocation.artifact.as_str() {
                    return Err(HostProblem::IdempotencyConflict);
                }
                value
            }
            None => {
                if state.instances >= MAX_INSTANCES {
                    return Err(HostProblem::ResourceExhausted);
                }
                state.instances += 1;
                state.programs.insert(name.clone());
                Instance {
                    schema_version: 1,
                    artifact: String::new(),
                    busy: false,
                    open_files: false,
                    state: None,
                    abend: None,
                }
            }
        };
        if !initial && let Some(payload) = &instance.state {
            machine
                .install_retained_program_state(payload)
                .map_err(state_problem)?;
        }
        instance.artifact = invocation.artifact.as_str().into();
        instance.busy = true;
        state.active += 1;
        refresh_run_metadata(&mut state, &run);
        let instance_write = write(&namespace, &name, &instance, expected)?;
        let version = instance_write.record.version;
        store
            .put_provider_states_atomic(vec![
                write(RUN_STATE_NAMESPACE, &run, &state, expected_run)?,
                instance_write,
            ])
            .map_err(|error| match error {
                StoreError::CapacityExceeded | StoreError::PayloadTooLarge => {
                    HostProblem::ResourceExhausted
                }
                StoreError::Conflict | StoreError::AlreadyExists => {
                    HostProblem::IdempotencyConflict
                }
                _ => HostProblem::InfrastructureFailure,
            })?;
        Ok(Self {
            invocation: invocation.clone(),
            run,
            namespace,
            name,
            version,
            initial,
            instance,
        })
    }

    // These writes MUST commit in the same transaction as the cached CALL reply.
    // Otherwise replay could apply the state transition twice or lose it.
    pub(super) fn completed(
        mut self,
        store: &dyn PlatformStore,
        machine: &ReferenceMachine,
    ) -> Result<Vec<ProviderStateWrite>, HostProblem> {
        // Validate return-time obligations even for INITIAL (whose saved state
        // is discarded). Unsupported live cursors/control cannot become success.
        let retained = machine
            .retained_program_state()
            .map_err(|_| HostProblem::UnknownOutcome)?;
        self.instance.state = if self.initial { None } else { Some(retained) };
        self.instance.busy = false;
        self.instance.open_files = !machine.dataset_cursors().is_empty();
        let (mut state, expected) = load_run(store, &self.run)?;
        if state.ended || state.active == 0 {
            return Err(HostProblem::UnknownOutcome);
        }
        state.active -= 1;
        refresh_run_metadata(&mut state, &self.run);
        Ok(vec![
            write(RUN_STATE_NAMESPACE, &self.run, &state, expected)?,
            write(
                &self.namespace,
                &self.name,
                &self.instance,
                Some(self.version),
            )?,
        ])
    }
}

impl CobolProgram {
    pub(super) fn cancel_instances(
        &self,
        parent: &Invocation,
        effect: &EffectRequest,
        programs: &[mainframe_env_host_api::ProgramName],
    ) -> Result<HostResult, HostProblem> {
        if effect.run_unit != parent.run_unit_id
            || programs.is_empty()
            || programs.len() > MAX_INSTANCES
        {
            return Err(HostProblem::Malformed);
        }
        let store = self.store.get().ok_or(HostProblem::InfrastructureFailure)?;
        let run = run_key(parent);
        let key = super::replay::digest(&[
            b"cancel",
            run.as_bytes(),
            parent.execution_id.as_str().as_bytes(),
            effect
                .idempotency_key
                .as_ref()
                .ok_or(HostProblem::Malformed)?
                .as_str()
                .as_bytes(),
        ]);
        let names: std::collections::BTreeSet<_> = programs
            .iter()
            .map(|name| name.as_str().to_ascii_uppercase())
            .collect();
        let legacy_fingerprint = serde_json::to_vec(&(1_u32, effect.sequence, &names))
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let fingerprint = super::replay::digest(&[b"cancel-fingerprint", &legacy_fingerprint]);
        if let Some(record) = store
            .get_provider_state(CANCEL_NAMESPACE, &key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            if !cancel_receipt_matches(&record, &legacy_fingerprint, &fingerprint, parent) {
                return Err(HostProblem::IdempotencyConflict);
            }
            return cancel_reply();
        }
        self.ensure_call_protocol(parent)?;
        let (mut state, expected_run) = load_run(store.as_ref(), &run)?;
        adopt_run_owner(&mut state, parent)?;
        if state.ended {
            return Err(HostProblem::Unsupported);
        }
        let namespace = namespace(&run);
        if expected_run.is_none()
            && !store
                .list_provider_state(&namespace, 1)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .is_empty()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut writes = vec![write(RUN_STATE_NAMESPACE, &run, &state, expected_run)?];
        for name in names {
            if let Some(record) = store
                .get_provider_state(&namespace, &name)
                .map_err(|_| HostProblem::InfrastructureFailure)?
            {
                let value = load_instance(&record)?;
                // Recursive/abandoned-frame CANCEL and implicit closing of open
                // files are not implemented. Validate all targets before writes.
                if value.busy || value.open_files || value.abend.is_some() {
                    return Err(HostProblem::Unsupported);
                }
                let reset = Instance {
                    schema_version: 1,
                    artifact: String::new(),
                    busy: false,
                    open_files: false,
                    state: None,
                    abend: None,
                };
                writes.push(write(&namespace, &name, &reset, Some(record.version))?);
            }
        }
        let cancelled_tick = super::replay::retention_observation_tick(self, parent)?;
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: CANCEL_NAMESPACE.into(),
                key: key.clone(),
                version: 1,
                payload: {
                    let mut receipt = CancelReceipt {
                        schema_version: 2,
                        cancel_key: key.clone(),
                        fingerprint: fingerprint.clone(),
                        owner_execution: parent.execution_id.as_str().into(),
                        owner_run_unit: parent.run_unit_id.as_str().into(),
                        owner_principal: parent.principal.id().as_str().into(),
                        protocol_key: protocol_key(parent.run_unit_id.as_str()),
                        run_state_key: run.clone(),
                        metadata_digest: String::new(),
                        cancelled_tick,
                    };
                    receipt.metadata_digest = cancel_metadata_digest(&receipt);
                    serde_json::to_vec(&receipt).map_err(|_| HostProblem::InfrastructureFailure)?
                },
            },
            expected_version: None,
        });
        match store.put_provider_states_atomic(writes) {
            Ok(()) => cancel_reply(),
            Err(StoreError::Conflict | StoreError::AlreadyExists) => {
                match store.get_provider_state(CANCEL_NAMESPACE, &key) {
                    Ok(Some(record))
                        if cancel_receipt_matches(
                            &record,
                            &legacy_fingerprint,
                            &fingerprint,
                            parent,
                        ) =>
                    {
                        cancel_reply()
                    }
                    _ => Err(HostProblem::IdempotencyConflict),
                }
            }
            Err(_) => Err(HostProblem::UnknownOutcome),
        }
    }

    pub(super) fn finish_run_unit(&self, invocation: &Invocation) -> Result<(), HostProblem> {
        let Some(store) = self.store.get() else {
            return Ok(());
        };
        let key = run_key(invocation);
        let (mut state, version) = load_run(store.as_ref(), &key)?;
        if version.is_none()
            && !store
                .list_provider_state(&namespace(&key), 1)
                .map_err(|_| HostProblem::InfrastructureFailure)?
                .is_empty()
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let already_attributed_terminal = state.schema_version == 2 && state.ended;
        adopt_run_owner(&mut state, invocation)?;
        if !state.ended && state.active != 0 {
            return Err(HostProblem::UnknownOutcome);
        }
        let namespace = namespace(&key);
        let records = if state.ended {
            Vec::new()
        } else {
            state
                .programs
                .iter()
                .map(|name| {
                    store
                        .get_provider_state(&namespace, name)
                        .map_err(|_| HostProblem::InfrastructureFailure)?
                        .ok_or(HostProblem::UnknownOutcome)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut mutations = Vec::new();
        for record in records {
            let value = load_instance(&record)?;
            if value.busy || value.open_files {
                return Err(HostProblem::Unsupported);
            }
            abend::verify_for_cleanup(store.as_ref(), invocation, &record, &value)?;
            mutations.push(ProviderStateMutation::Delete {
                namespace: namespace.clone(),
                key: record.key,
                expected_version: record.version,
            });
        }
        let ended_tick = match state.ended_tick {
            Some(tick) => tick,
            None => super::replay::retention_observation_tick(self, invocation)?,
        };
        if !already_attributed_terminal {
            state.ended = true;
            state.ended_tick = Some(ended_tick);
            state.instances = 0;
            state.programs.clear();
            refresh_run_metadata(&mut state, &key);
            mutations.push(ProviderStateMutation::Put(write(
                RUN_STATE_NAMESPACE,
                &key,
                &state,
                version,
            )?));
        }
        if let Some(protocol) =
            super::replay::protocol_terminal_mutation(store.as_ref(), invocation, ended_tick)?
        {
            mutations.push(protocol);
        }
        if mutations.is_empty() {
            return Ok(());
        }
        store
            .mutate_provider_states_atomic(mutations)
            .map_err(|_| HostProblem::UnknownOutcome)
    }
}

impl DefaultProgramRouter {
    /// Release last-used instances at a terminal run boundary. Suspended or
    /// uncertain runs must not call this API. Replay receipts remain durable.
    pub fn finish_run_unit(&self, invocation: &Invocation) -> Result<(), HostProblem> {
        self.cobol.finish_run_unit(invocation)
    }
}
fn cancel_reply() -> Result<HostResult, HostProblem> {
    BoundedPayload::new(
        "mainframe-env.program.cancel@1",
        Vec::new(),
        InvocationLimits::default(),
    )
    .map(HostResult::Program)
    .map_err(|_| HostProblem::InfrastructureFailure)
}
