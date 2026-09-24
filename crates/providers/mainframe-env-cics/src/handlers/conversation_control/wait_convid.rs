//! WAIT CONVID confirms mapped APPC process and SEND transmission.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationState, ConversationTransmitOutcome,
    load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const DECIMAL_SCHEMA: &str = "mainframe-env.cics.decimal@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
    let value = request
        .arguments
        .get("CONVID")
        .ok_or(HostProblem::Malformed)?;
    let token: [u8; 4] = value
        .bytes()
        .try_into()
        .map_err(|_| condition("NOTALLOC", 61, 0))?;
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
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
            load_conversation_replay(service.store.as_ref(), mutation.idempotency_key.as_str())
                .map_err(store_error)?
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
        let before = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let record = before
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        if record.kind != ConversationKind::AppcMapped || record.data.terminal_error() {
            return Err(condition("INVREQ", 16, 0));
        }
        match service.flush_conversation_send(run, token)? {
            ConversationTransmitOutcome::Pending => {
                return service.response(
                    run,
                    CicsDisposition::Suspended,
                    "NORMAL",
                    0,
                    0,
                    None,
                    None,
                    Vec::new(),
                );
            }
            ConversationTransmitOutcome::Confirmed | ConversationTransmitOutcome::Rejected(_) => {}
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let record = current
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        if current
            .exchanges
            .get(&u32::from_be_bytes(token).to_string())
            .is_some_and(|exchange| exchange.pending_outbound() != 0)
        {
            continue;
        }
        let state = record.state;
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(state),
            token: Some(token),
            outputs: BTreeMap::from([("STATE".into(), state.cvda().to_string().into_bytes())]),
        };
        let replay = ConversationReplay {
            schema_version: 1,
            effect_key: mutation.idempotency_key.as_str().into(),
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
        let mut next = current.clone();
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
    if request.operation != CicsOperation::WaitConvid
        || !request.arguments.contains_key("CONVID")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID" | "STATE" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.condition != "NORMAL" || reply.response != 0 || reply.response2 != 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let bytes = reply
        .outputs
        .get("STATE")
        .ok_or(HostProblem::InfrastructureFailure)?;
    if std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.parse::<i32>().ok())
        != reply.state.map(ConversationState::cvda)
    {
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
    result.outputs.insert(
        "STATE".into(),
        BoundedPayload::new(DECIMAL_SCHEMA, bytes.clone(), InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(result)
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

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
