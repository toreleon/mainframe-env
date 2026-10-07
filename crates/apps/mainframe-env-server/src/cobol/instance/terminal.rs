//! One COBOL terminal planner; native closure supplies already captured rows.
use super::*;
use mainframe_env_store_api::{RootClosureSnapshot, TerminalRowDependency};

pub(super) fn plan_end(
    invocation: &Invocation,
    mut state: RunState,
    version: Option<u64>,
    records: Vec<ProviderStateRecord>,
    ended_tick: u64,
    already_terminal: bool,
    mut verify: impl FnMut(&ProviderStateRecord, &Instance) -> Result<(), HostProblem>,
) -> Result<Vec<ProviderStateMutation>, HostProblem> {
    if ended_tick == 0 || records.len() > MAX_INSTANCES {
        return Err(HostProblem::ResourceExhausted);
    }
    let key = run_key(invocation);
    let scope = namespace(&key);
    let mut mutations = Vec::new();
    for record in records {
        if record.namespace != scope || !state.programs.contains(&record.key) {
            return Err(HostProblem::UnknownOutcome);
        }
        let value = load_instance(&record)?;
        if value.busy || value.open_files {
            return Err(HostProblem::Unsupported);
        }
        verify(&record, &value)?;
        mutations.push(ProviderStateMutation::Delete {
            namespace: scope.clone(),
            key: record.key,
            expected_version: record.version,
        });
    }
    if !already_terminal {
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
    Ok(mutations)
}

fn captured<'a>(
    closure: &'a RootClosureSnapshot,
    scope: &str,
    key: &str,
) -> Result<Option<&'a ProviderStateRecord>, HostProblem> {
    closure
        .provider_dependencies
        .iter()
        .find_map(|dependency| match dependency {
            TerminalRowDependency::Exact(row) if row.namespace == scope && row.key == key => {
                Some(Ok(Some(row)))
            }
            TerminalRowDependency::Absent {
                namespace,
                key: absent,
            } if namespace == scope && absent == key => Some(Ok(None)),
            _ => None,
        })
        .unwrap_or(Err(HostProblem::UnknownOutcome))
}

impl CobolProgram {
    /// Same parser/planner as legacy cleanup, with no reads/writes after Closing.
    /// The genuine root driver supplies its complete captured ownership scope.
    pub(in super::super) fn prepare_native_run_end(
        &self,
        invocation: &Invocation,
        closure: &RootClosureSnapshot,
        ended_tick: u64,
    ) -> Result<Vec<ProviderStateMutation>, HostProblem> {
        closure
            .validate_bounds()
            .map_err(|_| HostProblem::ResourceExhausted)?;
        super::super::replay::validate_native_calls(closure)?;
        let key = run_key(invocation);
        let row = captured(closure, RUN_STATE_NAMESPACE, &key)?;
        let (mut state, version) = match row {
            Some(row) => (
                decode_run_state(row).map_err(|_| HostProblem::UnknownOutcome)?,
                Some(row.version),
            ),
            None => (
                RunState {
                    schema_version: 2,
                    ..Default::default()
                },
                None,
            ),
        };
        let already_terminal = state.schema_version == 2 && state.ended;
        adopt_run_owner(&mut state, invocation)?;
        if state.ended || state.active != 0 {
            return Err(HostProblem::UnknownOutcome);
        }
        let scope = namespace(&key);
        if !closure
            .claim
            .admission()
            .provider_namespaces
            .contains(&scope)
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let records: Vec<_> = closure
            .provider_dependencies
            .iter()
            .filter_map(|dependency| match dependency {
                TerminalRowDependency::Exact(row) if row.namespace == scope => Some(row),
                _ => None,
            })
            .collect();
        if records.len() != state.instances
            || records.len() > MAX_INSTANCES
            || records.iter().any(|row| !state.programs.contains(&row.key))
        {
            return Err(HostProblem::UnknownOutcome);
        }
        let mut mutations = plan_end(
            invocation,
            state,
            version,
            records.into_iter().cloned().collect(),
            ended_tick,
            already_terminal,
            |row, value| abend::verify_captured_cleanup(closure, invocation, row, value),
        )?;
        if let Some(protocol) = super::super::replay::protocol_terminal_mutation_from(
            captured(
                closure,
                CALL_PROTOCOL_NAMESPACE,
                &protocol_key(invocation.run_unit_id.as_str()),
            )?,
            invocation,
            ended_tick,
        )? {
            mutations.push(protocol);
        }
        Ok(mutations)
    }
}
