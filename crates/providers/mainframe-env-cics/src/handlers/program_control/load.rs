use super::super::super::{CicsLimits, CicsService, Run, argument_text, mutation_problem};
use super::super::{field, store_error};
use super::{
    CicsJavaStatus, CicsProgramDefinition, ProgramReader, condition, decimal_usize,
    normalize_program_name, validate_program_artifact,
};
use mainframe_env_execution_api::{ArtifactRef, BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore};
use std::collections::{BTreeMap, BTreeSet};

const PROGRAM_LOAD_NAMESPACE: &str = "cics-program-load-v1";
const PROGRAM_LOAD_MAGIC: &[u8; 7] = b"MECPLD1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct ProgramLoadState {
    pub(in crate::service) events: Vec<ProgramLoadEvent>,
    pub(in crate::service) version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::service) struct ProgramLoadEvent {
    pub(in crate::service) effect_key: String,
    pub(in crate::service) owner_run_unit: String,
    pub(in crate::service) generation: u64,
    pub(in crate::service) artifact: ArtifactRef,
    pub(in crate::service) hold: bool,
    pub(in crate::service) output_mask: u8,
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let program_name = normalize_program_name(&argument_text(request, "PROGRAM")?)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let output_mask = output_mask(request);
    let hold = request.arguments.contains_key("OPTION.HOLD");

    if let Some((persisted_name, event, program)) = {
        let state = service.lock()?;
        state.program_loads.iter().find_map(|(name, load)| {
            load.events
                .iter()
                .find(|event| event.effect_key == effect_key)
                .map(|event| {
                    let program = state
                        .program_definitions
                        .get(name)
                        .and_then(|generations| generations.get(&event.generation))
                        .cloned();
                    (name.clone(), event.clone(), program)
                })
        })
    } {
        if persisted_name != program_name
            || event.owner_run_unit != run.invocation.run_unit_id.as_str()
            || event.hold != hold
            || event.output_mask != output_mask
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let program = program.ok_or(HostProblem::InfrastructureFailure)?;
        if program.artifact != event.artifact {
            return Err(HostProblem::InfrastructureFailure);
        }
        let content = program_content(service, &program)?;
        return response(service, run, request, &program, &content);
    }

    let program = {
        let state = service.lock()?;
        state
            .program_definitions
            .get(&program_name)
            .and_then(|generations| generations.last_key_value().map(|(_, value)| value.clone()))
            .ok_or_else(|| condition("PGMIDERR", 27, 1))?
    };
    if !program.enabled {
        return Err(condition("PGMIDERR", 27, 2));
    }
    if program.remote {
        return Err(condition("PGMIDERR", 27, 9));
    }
    if program.java_status != CicsJavaStatus::NotJava {
        return Err(condition("PGMIDERR", 27, 42));
    }
    let content = program_content(service, &program)?;
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.PROGRAM.{program_name}"),
        AccessIntent::Execute,
    )?;
    validate_outputs(request, output_mask, content.len())?;

    let event = ProgramLoadEvent {
        effect_key: effect_key.into(),
        owner_run_unit: run.invocation.run_unit_id.as_str().into(),
        generation: program.generation,
        artifact: program.artifact.clone(),
        hold,
        output_mask,
    };
    {
        let mut state = service.lock()?;
        let total = state
            .program_loads
            .values()
            .map(|load| load.events.len())
            .sum::<usize>();
        if total >= service.limits.max_queue_records {
            return Err(HostProblem::ResourceExhausted);
        }
        let current = state.program_loads.get(&program_name).cloned();
        let mut next = current.clone().unwrap_or(ProgramLoadState {
            events: Vec::new(),
            version: 0,
        });
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.events.push(event);
        service
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: PROGRAM_LOAD_NAMESPACE.into(),
                    key: program_name.clone(),
                    version: next.version,
                    payload: encode_state(&next)?,
                },
                current.as_ref().map(|value| value.version),
            )
            .map_err(store_error)
            .map_err(mutation_problem)?;
        state.program_loads.insert(program_name, next);
    }
    response(service, run, request, &program, &content)
}

pub(in crate::service) fn load_program_loads(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<BTreeMap<String, ProgramLoadState>, HostProblem> {
    let mut loads = BTreeMap::new();
    let mut effect_keys = BTreeSet::new();
    let mut total = 0usize;
    for row in store
        .list_provider_state(PROGRAM_LOAD_NAMESPACE, limits.max_programs)
        .map_err(store_error)?
    {
        let name =
            normalize_program_name(&row.key).map_err(|_| HostProblem::InfrastructureFailure)?;
        let state = decode_state(&row.payload, row.version, limits)?;
        total = total
            .checked_add(state.events.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        if name != row.key
            || state.events.is_empty()
            || total > limits.max_queue_records
            || state
                .events
                .iter()
                .any(|event| !effect_keys.insert(event.effect_key.clone()))
            || loads.insert(name, state).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(loads)
}

pub(in crate::service) fn release_task_program_loads(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    let owner = run.invocation.run_unit_id.as_str();
    let mut state = service.lock()?;
    let names = state.program_loads.keys().cloned().collect::<Vec<_>>();
    for name in names {
        let Some(current) = state.program_loads.get(&name).cloned() else {
            continue;
        };
        let mut next = current.clone();
        next.events
            .retain(|event| event.hold || event.owner_run_unit != owner);
        if next.events.len() == current.events.len() {
            continue;
        }
        if next.events.is_empty() {
            service
                .store
                .delete_provider_state(PROGRAM_LOAD_NAMESPACE, &name, current.version)
                .map_err(store_error)?;
            state.program_loads.remove(&name);
        } else {
            next.version = next
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            service
                .store
                .put_provider_state(
                    ProviderStateRecord {
                        namespace: PROGRAM_LOAD_NAMESPACE.into(),
                        key: name.clone(),
                        version: next.version,
                        payload: encode_state(&next)?,
                    },
                    Some(current.version),
                )
                .map_err(store_error)?;
            state.program_loads.insert(name, next);
        }
    }
    Ok(())
}

fn program_content(
    service: &CicsService,
    program: &CicsProgramDefinition,
) -> Result<Vec<u8>, HostProblem> {
    let artifacts = service
        .artifacts
        .get()
        .ok_or_else(|| condition("INVREQ", 16, 30))?;
    validate_program_artifact(artifacts.as_ref(), program)
        .map_err(|_| condition("PGMIDERR", 27, 3))?;
    artifacts
        .get_artifact(&program.artifact)
        .map_err(store_error)?
        .map(|artifact| artifact.payload)
        .ok_or_else(|| condition("PGMIDERR", 27, 3))
}

fn validate_outputs(
    request: &CicsRequest,
    output_mask: u8,
    content_length: usize,
) -> Result<(), HostProblem> {
    if output_mask & 4 != 0 && content_length > i16::MAX as usize {
        return Err(condition("LENGERR", 22, 19));
    }
    if output_mask & 3 != 0 {
        let capacity = request
            .arguments
            .get("SET.MAXLENGTH")
            .and_then(decimal_usize)
            .ok_or(HostProblem::Malformed)?;
        if content_length > capacity {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(())
}

fn response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    program: &CicsProgramDefinition,
    content: &[u8],
) -> Result<CicsResponse, HostProblem> {
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        Some(program.name.clone()),
        None,
        Vec::new(),
    )?;
    let output_mask = output_mask(request);
    if output_mask & 3 != 0 {
        response.outputs.insert(
            "LOAD.CONTENT".into(),
            BoundedPayload::new(
                "mainframe-env.cics.payload@1",
                content.to_vec(),
                InvocationLimits::default(),
            )
            .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    for (bit, name, offset) in [
        (1, "SET", 0usize),
        (2, "ENTRY", program.entry_offset as usize),
    ] {
        if output_mask & bit != 0 {
            response.outputs.insert(
                name.into(),
                payload("mainframe-env.cics.load-offset@1", offset.to_string())?,
            );
        }
    }
    for (bit, name) in [(4, "LENGTH"), (8, "FLENGTH")] {
        if output_mask & bit != 0 {
            response.outputs.insert(
                name.into(),
                payload("mainframe-env.cics.decimal@1", content.len().to_string())?,
            );
        }
    }
    response.outputs.insert(
        "PROGRAM.CONTENT".into(),
        payload(
            "mainframe-env.cics.content-identity@1",
            program.artifact.as_str().to_string(),
        )?,
    );
    response.outputs.insert(
        "PROGRAM.GENERATION".into(),
        payload(
            "mainframe-env.cics.decimal@1",
            program.generation.to_string(),
        )?,
    );
    Ok(response)
}

fn payload(schema: &str, value: String) -> Result<BoundedPayload, HostProblem> {
    BoundedPayload::new(schema, value.into_bytes(), InvocationLimits::default())
        .map_err(|_| HostProblem::ResourceExhausted)
}

fn output_mask(request: &CicsRequest) -> u8 {
    [("SET", 1), ("ENTRY", 2), ("LENGTH", 4), ("FLENGTH", 8)]
        .into_iter()
        .fold(0, |mask, (name, bit)| {
            if request.arguments.contains_key(name) {
                mask | bit
            } else {
                mask
            }
        })
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = [
        "ENTRY",
        "FLENGTH",
        "LENGTH",
        "OPTION.HOLD",
        "OPTION.NOHANDLE",
        "PROGRAM",
        "RESP",
        "RESP2",
        "SET",
        "SET.MAXLENGTH",
    ];
    let output_argument = |name: &str| {
        request
            .arguments
            .get(name)
            .is_none_or(|value| value.schema() == "mainframe-env.cics.argument@1")
    };
    if !request.arguments.contains_key("PROGRAM")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("LENGTH") && request.arguments.contains_key("FLENGTH")
        || request.arguments.iter().any(|(name, value)| {
            !allowed.contains(&name.as_str())
                || name.starts_with("OPTION.")
                    && (value.schema() != "mainframe-env.cics.option@1"
                        || !value.bytes().is_empty())
        })
        || ["ENTRY", "FLENGTH", "LENGTH", "RESP", "RESP2", "SET"]
            .into_iter()
            .any(|name| !output_argument(name))
        || request
            .arguments
            .get("SET.MAXLENGTH")
            .is_some_and(|value| decimal_usize(value).is_none())
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn encode_state(state: &ProgramLoadState) -> Result<Vec<u8>, HostProblem> {
    let mut out = PROGRAM_LOAD_MAGIC.to_vec();
    out.extend_from_slice(
        &u32::try_from(state.events.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for event in &state.events {
        field(&mut out, event.effect_key.as_bytes())?;
        field(&mut out, event.owner_run_unit.as_bytes())?;
        out.extend_from_slice(&event.generation.to_be_bytes());
        field(&mut out, event.artifact.as_str().as_bytes())?;
        out.push(u8::from(event.hold));
        out.push(event.output_mask);
    }
    Ok(out)
}

fn decode_state(
    bytes: &[u8],
    version: u64,
    limits: CicsLimits,
) -> Result<ProgramLoadState, HostProblem> {
    let mut reader = ProgramReader::new(bytes, PROGRAM_LOAD_MAGIC)?;
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
    if version == 0 || count > limits.max_queue_records {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut events = Vec::with_capacity(count);
    let mut keys = BTreeSet::new();
    for _ in 0..count {
        let effect_key = reader.text(InvocationLimits::default().max_binding_bytes)?;
        let owner_run_unit = reader.text(InvocationLimits::default().max_binding_bytes)?;
        let generation = reader.u64()?;
        let artifact = ArtifactRef::new(reader.text(80)?, InvocationLimits::default())
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let hold = flag(reader.take(1)?[0])?;
        let output_mask = reader.take(1)?[0];
        if effect_key.is_empty()
            || owner_run_unit.is_empty()
            || generation == 0
            || output_mask & !0x0f != 0
            || !keys.insert(effect_key.clone())
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        events.push(ProgramLoadEvent {
            effect_key,
            owner_run_unit,
            generation,
            artifact,
            hold,
            output_mask,
        });
    }
    if !reader.done() {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(ProgramLoadState { events, version })
}

fn flag(value: u8) -> Result<bool, HostProblem> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}
