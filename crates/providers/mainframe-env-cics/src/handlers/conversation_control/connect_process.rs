//! Mapped APPC CONNECT PROCESS over one task-owned conversation.

use super::{
    ConversationLedger, ConversationOwner, ConversationProblem, ConversationReplay,
    ConversationReply, ConversationState, build_attach, definitions, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::ConnectProcess {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
    let token = token(request)?;
    let process = process(service, run, request)?;
    let pip = pip(request)?;
    let sync_level = sync_level(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    let principal = run.invocation.principal.id().as_str().to_owned();
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let system = initial
        .conversation(token)
        .ok_or_else(|| condition("NOTALLOC", 61, 0))?
        .system
        .clone();
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{system}"),
        AccessIntent::Execute,
    )?;
    for _ in 0..MAX_CAS_RETRIES {
        super::deadline(service, run)?;
        if let Some(saved) =
            load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?
        {
            let reply = saved
                .matches_request(
                    &owner.execution,
                    &owner.run_unit,
                    &principal,
                    owner.lease_epoch,
                    mutation.sequence,
                    digest,
                )
                .map_err(store_error)?;
            return response(service, run, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = current.clone();
        let record = next
            .conversation_mut(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        record
            .connect(
                &owner,
                super::context(run)?,
                false,
                process.clone(),
                pip.clone(),
                sync_level,
            )
            .map_err(map_problem)?;
        let mut outputs = BTreeMap::new();
        if request.arguments.contains_key("STATE") {
            outputs.insert(
                "STATE".into(),
                super::state_cvda::bytes(ConversationState::Send),
            );
        }
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(ConversationState::Send),
            token: Some(token),
            outputs,
        };
        let replay = ConversationReplay {
            schema_version: 1,
            effect_key: effect_key.into(),
            owner_execution: owner.execution.clone(),
            owner_run_unit: owner.run_unit.clone(),
            owner_principal: principal.clone(),
            owner_epoch: owner.lease_epoch,
            mutation_sequence: mutation.sequence,
            request_digest: digest,
            deadline_tick: retention_tick,
            retain_until_tick: retention_tick,
            reply: reply.clone(),
        };
        if current
            .persist_with_replay(&mut next, &replay, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return response(service, run, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    let has = |name: &str| request.arguments.contains_key(name);
    if has("CONVID") == has("SESSION")
        || has("PROCNAME") == has("PARTNER")
        || has("PROCNAME") != has("PROCLENGTH")
        || has("PIPLIST") != has("PIPLENGTH")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID"
                    | "SESSION"
                    | "PROCNAME"
                    | "PROCLENGTH"
                    | "PARTNER"
                    | "PIPLIST"
                    | "PIPLENGTH"
                    | "SYNCLEVEL"
                    | "STATE"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}

fn token(request: &CicsRequest) -> Result<[u8; 4], HostProblem> {
    let value = request
        .arguments
        .get("CONVID")
        .or_else(|| request.arguments.get("SESSION"))
        .ok_or(HostProblem::Malformed)?;
    value
        .bytes()
        .try_into()
        .map_err(|_| condition("NOTALLOC", 61, 0))
}

fn process(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<Vec<u8>, HostProblem> {
    if let Some(partner) = request.arguments.get("PARTNER") {
        let name = super::allocate::name(partner.bytes(), 8)?;
        service.authorize(
            run,
            "PARTNER",
            &format!("CICS.PARTNER.{name}"),
            AccessIntent::Execute,
        )?;
        if definitions::load_partner(service.store.as_ref(), &name)?.is_none() {
            return Err(condition("PARTNERIDERR", 97, 0));
        }
        return definitions::load_partner_process(service.store.as_ref(), &name)?
            .map(|definition| definition.process)
            .ok_or_else(|| condition("PARTNERIDERR", 97, 0));
    }
    let value = request
        .arguments
        .get("PROCNAME")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let length = halfword(request, "PROCLENGTH")? as usize;
    if !(1..=64).contains(&length) || value.len() < length {
        return Err(condition("LENGERR", 22, 0));
    }
    Ok(value[..length].to_vec())
}

fn pip(request: &CicsRequest) -> Result<Vec<u8>, HostProblem> {
    let Some(value) = request.arguments.get("PIPLIST") else {
        return Ok(Vec::new());
    };
    let length = halfword(request, "PIPLENGTH")? as usize;
    if length > super::MAX_PIP_BYTES || value.bytes().len() < length {
        return Err(condition("LENGERR", 22, 0));
    }
    Ok(value.bytes()[..length].to_vec())
}

fn sync_level(request: &CicsRequest) -> Result<u8, HostProblem> {
    let Some(value) = request.arguments.get("SYNCLEVEL") else {
        return Ok(0);
    };
    let level = build_attach::parse_halfword(value.schema(), value.bytes())?;
    u8::try_from(level)
        .ok()
        .filter(|level| *level <= 2)
        .ok_or_else(|| condition("INVREQ", 16, 0))
}

fn halfword(request: &CicsRequest, name: &str) -> Result<u16, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    build_attach::parse_halfword(value.schema(), value.bytes())
}

fn map_problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned | ConversationProblem::Malformed => {
            condition("NOTALLOC", 61, 0)
        }
        ConversationProblem::DplPrincipal => condition("INVREQ", 16, 200),
        ConversationProblem::WrongKind | ConversationProblem::WrongState => {
            condition("INVREQ", 16, 0)
        }
        ConversationProblem::Length => condition("LENGERR", 22, 0),
        ConversationProblem::StaleOwner => HostProblem::IdempotencyConflict,
        ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
    }
}

fn response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.condition != "NORMAL" || reply.response != 0 || reply.response2 != 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut result = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    super::state_cvda::insert_output(&mut result, reply.outputs.get("STATE"))?;
    Ok(result)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
