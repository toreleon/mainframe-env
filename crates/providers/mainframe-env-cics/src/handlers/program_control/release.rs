use super::super::super::{CicsService, Run, argument_text, mutation_problem};
use super::super::store_error;
use super::load::{PROGRAM_LOAD_NAMESPACE, encode_state};
use super::{CicsJavaStatus, ProgramReader, condition, field, normalize_program_name};
use mainframe_env_execution_api::InvocationLimits;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::{ProviderStateMutation, ProviderStateRecord, ProviderStateWrite};

const RELEASE_NAMESPACE: &str = "cics-program-release-v1";
const RELEASE_MAGIC: &[u8; 7] = b"MECPRL1";

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let name = normalize_program_name(&argument_text(request, "PROGRAM")?)?;
    let effect_key = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?
        .idempotency_key
        .as_str();
    let owner = run.invocation.run_unit_id.as_str().to_string();
    if let Some(record) = service
        .store
        .get_provider_state(RELEASE_NAMESPACE, effect_key)
        .map_err(store_error)?
    {
        let (released_name, released_owner) = decode_receipt(&record)?;
        if released_name != name || released_owner != owner {
            return Err(HostProblem::IdempotencyConflict);
        }
        return response(service, run, name);
    }
    if service.artifacts.get().is_none() {
        return Err(condition("INVREQ", 16, 30));
    }

    let event_key = {
        let state = service.lock()?;
        let generations = state
            .program_definitions
            .get(&name)
            .ok_or_else(|| condition("PGMIDERR", 27, 1))?;
        let load = state.program_loads.get(&name);
        let event = load.and_then(|load| {
            load.events
                .iter()
                .rev()
                .find(|event| event.owner_run_unit == owner)
                .or_else(|| load.events.iter().rev().find(|event| event.hold))
        });
        let program = event
            .and_then(|event| generations.get(&event.generation))
            .or_else(|| generations.last_key_value().map(|(_, program)| program))
            .ok_or_else(|| condition("PGMIDERR", 27, 1))?;
        if !program.enabled {
            return Err(condition("PGMIDERR", 27, 2));
        }
        if program.remote {
            return Err(condition("PGMIDERR", 27, 9));
        }
        if program.java_status != CicsJavaStatus::NotJava {
            return Err(condition("PGMIDERR", 27, 42));
        }
        if program.reload {
            return Err(condition("INVREQ", 16, 17));
        }
        let event = event.ok_or_else(|| {
            condition(
                "INVREQ",
                16,
                if run.current_program.current.as_deref() == Some(name.as_str()) {
                    5
                } else if load.is_some_and(|load| !load.events.is_empty()) {
                    7
                } else {
                    6
                },
            )
        })?;
        if program.artifact != event.artifact {
            return Err(HostProblem::InfrastructureFailure);
        }
        event.effect_key.clone()
    };
    service
        .authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{name}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| match problem {
            HostProblem::Unauthorized => condition("NOTAUTH", 70, 0),
            problem => problem,
        })?;

    let mut state = service.lock()?;
    let current = state
        .program_loads
        .get(&name)
        .cloned()
        .ok_or(HostProblem::UnknownOutcome)?;
    let mut next = current.clone();
    let index = next
        .events
        .iter()
        .position(|event| event.effect_key == event_key)
        .ok_or(HostProblem::UnknownOutcome)?;
    next.events.remove(index);
    let mut mutations = Vec::with_capacity(2);
    if next.events.is_empty() {
        mutations.push(ProviderStateMutation::Delete {
            namespace: PROGRAM_LOAD_NAMESPACE.into(),
            key: name.clone(),
            expected_version: current.version,
        });
    } else {
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: PROGRAM_LOAD_NAMESPACE.into(),
                key: name.clone(),
                version: next.version,
                payload: encode_state(&next)?,
            },
            expected_version: Some(current.version),
        }));
    }
    mutations.push(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: RELEASE_NAMESPACE.into(),
            key: effect_key.into(),
            version: 1,
            payload: encode_receipt(&name, &owner)?,
        },
        expected_version: None,
    }));
    service
        .store
        .mutate_provider_states_atomic(mutations)
        .map_err(store_error)
        .map_err(mutation_problem)?;
    if next.events.is_empty() {
        state.program_loads.remove(&name);
    } else {
        state.program_loads.insert(name.clone(), next);
    }
    drop(state);
    response(service, run, name)
}

fn response(service: &CicsService, run: &Run, name: String) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        Some(name),
        None,
        Vec::new(),
    )
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = ["OPTION.NOHANDLE", "PROGRAM", "RESP", "RESP2"];
    if !request.arguments.contains_key("PROGRAM")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || name == "PROGRAM"
                    && !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    )
                || name == "OPTION.NOHANDLE"
                    && (value.schema() != "mainframe-env.cics.option@1"
                        || !value.bytes().is_empty())
                || matches!(name.as_str(), "RESP" | "RESP2")
                    && value.schema() != "mainframe-env.cics.argument@1"
        })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn encode_receipt(name: &str, owner: &str) -> Result<Vec<u8>, HostProblem> {
    let mut bytes = RELEASE_MAGIC.to_vec();
    field(&mut bytes, name.as_bytes())?;
    field(&mut bytes, owner.as_bytes())?;
    Ok(bytes)
}

fn decode_receipt(record: &ProviderStateRecord) -> Result<(String, String), HostProblem> {
    if record.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut reader = ProgramReader::new(&record.payload, RELEASE_MAGIC)?;
    let name = reader.text(8)?;
    let owner = reader.text(InvocationLimits::default().max_binding_bytes)?;
    if normalize_program_name(&name).ok().as_deref() != Some(name.as_str())
        || owner.is_empty()
        || !reader.done()
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok((name, owner))
}
