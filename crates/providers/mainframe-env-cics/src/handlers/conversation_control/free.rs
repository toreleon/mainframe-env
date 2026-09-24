//! Source-bounded mapped APPC and MRO FREE over task-owned sessions.

use super::{
    ConversationLedger, ConversationOwner, ConversationProblem, ConversationReplay,
    ConversationReply, ConversationState, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const STATE_SCHEMA: &str = "mainframe-env.cics.cvda@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::FreeConversation {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let principal = run.invocation.principal.id().as_str().to_owned();
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let saved =
        load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?;
    let token = if let Some(saved) = &saved {
        saved
            .matches_request(
                &owner.execution,
                &owner.run_unit,
                &principal,
                owner.lease_epoch,
                mutation.sequence,
                digest,
            )
            .map_err(store_error)?
            .token
            .ok_or(HostProblem::InfrastructureFailure)?
    } else {
        select_token(&initial, &owner, request)?
    };
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
        if record.released {
            return Err(condition("NOTALLOC", 61, 0));
        }
        record
            .release(&owner, super::context(run)?, false)
            .map_err(map_problem)?;
        next.remove_exchange(token);
        let outputs = if request.arguments.contains_key("STATE") {
            BTreeMap::from([("STATE".into(), vec![0; 4])])
        } else {
            BTreeMap::new()
        };
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(ConversationState::Free),
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
    if request.arguments.contains_key("SESSION")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID" | "STATE" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}

fn select_token(
    ledger: &ConversationLedger,
    owner: &ConversationOwner,
    request: &CicsRequest,
) -> Result<[u8; 4], HostProblem> {
    if let Some(value) = request
        .arguments
        .get("CONVID")
        .or_else(|| request.arguments.get("SESSION"))
    {
        return value
            .bytes()
            .try_into()
            .map_err(|_| condition("NOTALLOC", 61, 0));
    }
    ledger
        .conversations
        .values()
        .find(|record| {
            record.principal_facility
                && !record.released
                && record.owner.execution == owner.execution
                && record.owner.run_unit == owner.run_unit
        })
        .map(|record| record.token)
        .ok_or_else(|| condition("NOTALLOC", 61, 0))
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
        ConversationProblem::StaleOwner => HostProblem::IdempotencyConflict,
        ConversationProblem::Length => HostProblem::Malformed,
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
    if let Some(bytes) = reply.outputs.get("STATE") {
        if bytes != &[0; 4] {
            return Err(HostProblem::InfrastructureFailure);
        }
        result.outputs.insert(
            "STATE".into(),
            BoundedPayload::new(STATE_SCHEMA, bytes.clone(), InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(result)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
