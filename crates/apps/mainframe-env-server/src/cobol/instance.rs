//! Run-unit-owned last-used instances. A busy instance is never reset on retry.
use super::*;
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateWrite, StoreError};
use serde::{Deserialize, Serialize};

const RUNS: &str = "cobol-run-state@1";
const CANCELS: &str = "cobol-cancel@1";
const MAX_INSTANCES: usize = 256;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RunState {
    schema_version: u32,
    active: usize,
    instances: usize,
    programs: std::collections::BTreeSet<String>,
    ended: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Instance {
    schema_version: u32,
    artifact: String,
    busy: bool,
    open_files: bool,
    state: Option<Vec<u8>>,
}

pub(super) struct Lease {
    run: String,
    namespace: String,
    name: String,
    version: u64,
    initial: bool,
    instance: Instance,
}

fn run_key(invocation: &Invocation) -> String {
    super::replay::digest(&[
        b"instance-owner",
        invocation.run_unit_id.as_str().as_bytes(),
        invocation.principal.id().as_str().as_bytes(),
    ])
}
fn namespace(key: &str) -> String {
    format!("cobol-instance@1:{key}")
}
fn load_run(store: &dyn PlatformStore, key: &str) -> Result<(RunState, Option<u64>), HostProblem> {
    match store
        .get_provider_state(RUNS, key)
        .map_err(|_| HostProblem::InfrastructureFailure)?
    {
        None => Ok((
            RunState {
                schema_version: 1,
                ..Default::default()
            },
            None,
        )),
        Some(record) => {
            let value: RunState =
                serde_json::from_slice(&record.payload).map_err(|_| HostProblem::UnknownOutcome)?;
            if value.schema_version != 1
                || value.instances > MAX_INSTANCES
                || value.instances != value.programs.len()
                || value.active > value.instances
            {
                return Err(HostProblem::UnknownOutcome);
            }
            Ok((value, Some(record.version)))
        }
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
fn load_instance(record: &ProviderStateRecord) -> Result<Instance, HostProblem> {
    let value: Instance =
        serde_json::from_slice(&record.payload).map_err(|_| HostProblem::UnknownOutcome)?;
    if value.schema_version != 1 {
        return Err(HostProblem::UnknownOutcome);
    }
    Ok(value)
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
        if state.ended {
            return Err(HostProblem::Condition {
                name: "COBOL-RUN-ENDED".into(),
                response: -9,
                response2: 0,
            });
        }
        let namespace = namespace(&run);
        let name = program.to_ascii_uppercase();
        let existing = store
            .get_provider_state(&namespace, &name)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let expected = existing.as_ref().map(|record| record.version);
        let mut instance = match existing {
            Some(record) => {
                let value = load_instance(&record)?;
                if value.busy {
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
        let instance_write = write(&namespace, &name, &instance, expected)?;
        let version = instance_write.record.version;
        store
            .put_provider_states_atomic(vec![
                write(RUNS, &run, &state, expected_run)?,
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
        Ok(vec![
            write(RUNS, &self.run, &state, expected)?,
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
        self.ensure_call_protocol(parent)?;
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
        let fingerprint = serde_json::to_vec(&(1_u32, effect.sequence, &names))
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        if let Some(record) = store
            .get_provider_state(CANCELS, &key)
            .map_err(|_| HostProblem::InfrastructureFailure)?
        {
            if record.version != 1 || record.payload != fingerprint {
                return Err(HostProblem::IdempotencyConflict);
            }
            return cancel_reply();
        }
        let (state, expected_run) = load_run(store.as_ref(), &run)?;
        if state.ended {
            return Err(HostProblem::Unsupported);
        }
        let namespace = namespace(&run);
        let mut writes = vec![write(RUNS, &run, &state, expected_run)?];
        for name in names {
            if let Some(record) = store
                .get_provider_state(&namespace, &name)
                .map_err(|_| HostProblem::InfrastructureFailure)?
            {
                let value = load_instance(&record)?;
                // Recursive active CANCEL and implicit closing of open files are
                // not implemented. Reject ALL targets before changing any target.
                if value.busy || value.open_files {
                    return Err(HostProblem::Unsupported);
                }
                let reset = Instance {
                    schema_version: 1,
                    artifact: String::new(),
                    busy: false,
                    open_files: false,
                    state: None,
                };
                writes.push(write(&namespace, &name, &reset, Some(record.version))?);
            }
        }
        writes.push(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: CANCELS.into(),
                key: key.clone(),
                version: 1,
                payload: fingerprint.clone(),
            },
            expected_version: None,
        });
        match store.put_provider_states_atomic(writes) {
            Ok(()) => cancel_reply(),
            Err(StoreError::Conflict | StoreError::AlreadyExists) => {
                match store.get_provider_state(CANCELS, &key) {
                    Ok(Some(record)) if record.payload == fingerprint && record.version == 1 => {
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
        if state.ended {
            return Ok(());
        }
        if state.active != 0 {
            return Err(HostProblem::UnknownOutcome);
        }
        let namespace = namespace(&key);
        let records = state
            .programs
            .iter()
            .map(|name| {
                store
                    .get_provider_state(&namespace, name)
                    .map_err(|_| HostProblem::InfrastructureFailure)?
                    .ok_or(HostProblem::UnknownOutcome)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut mutations = Vec::new();
        for record in records {
            let value = load_instance(&record)?;
            if value.busy || value.open_files {
                return Err(HostProblem::Unsupported);
            }
            mutations.push(ProviderStateMutation::Delete {
                namespace: namespace.clone(),
                key: record.key,
                expected_version: record.version,
            });
        }
        state.ended = true;
        state.instances = 0;
        state.programs.clear();
        mutations.push(ProviderStateMutation::Put(write(
            RUNS, &key, &state, version,
        )?));
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
