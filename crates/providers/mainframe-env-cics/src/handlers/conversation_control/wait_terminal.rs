//! WAIT TERMINAL on the task's terminal or owned APPC/MRO facility.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationTransmitOutcome, DataCondition,
    load_conversation_replay,
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
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let principal = run.invocation.principal.id().as_str().to_owned();
    let saved = load_conversation_replay(service.store.as_ref(), mutation.idempotency_key.as_str())
        .map_err(store_error)?;
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
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
    } else if request.arguments.contains_key("CONVID") || request.arguments.contains_key("SESSION")
    {
        Some(super::receive::select_token(&initial, &owner, request)?)
    } else {
        initial
            .conversations
            .values()
            .find(|record| {
                record.principal_facility
                    && !record.released
                    && record.owner.execution == owner.execution
                    && record.owner.run_unit == owner.run_unit
            })
            .map(|record| record.token)
    };
    let system = if let Some(token) = token {
        let record = initial
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        if request.arguments.contains_key("SESSION") && record.kind != ConversationKind::Mro
            || request.arguments.contains_key("CONVID") && record.kind == ConversationKind::Mro
        {
            return Err(condition("NOTALLOC", 61, 0));
        }
        service.authorize(
            run,
            "CONNECTION",
            &format!("CICS.CONNECTION.{}", record.system),
            AccessIntent::Execute,
        )?;
        Some(record.system.clone())
    } else {
        service.authorize(run, "FACILITY", "CICS.TERMINAL.WAIT", AccessIntent::Execute)?;
        None
    };
    if let Some(saved) = saved {
        return super::receive::response(
            service,
            run,
            saved
                .matches_request(
                    &owner.execution,
                    &owner.run_unit,
                    &principal,
                    owner.lease_epoch,
                    mutation.sequence,
                    digest,
                )
                .map_err(store_error)?,
        );
    }
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
            return super::receive::response(service, run, reply);
        }
        let before = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = before.clone();
        let mut indicator = None;
        let state = if let Some(token) = token {
            let record = before
                .conversation(token)
                .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
            if Some(record.system.as_str()) != system.as_deref() {
                return Err(HostProblem::IdempotencyConflict);
            }
            record
                .check_owner(&owner, super::context(run)?)
                .map_err(map_problem)?;
            if record.data.terminal_error() {
                return Err(condition("TERMERR", 81, 0));
            }
            let pending = if record.kind == ConversationKind::AppcBasic {
                record.data.pending_outbound()
            } else {
                before
                    .exchanges
                    .get(&u32::from_be_bytes(token).to_string())
                    .map_or(0, super::ConversationExchangeState::pending_outbound)
            };
            if pending != 0 {
                let outcome = if record.kind == ConversationKind::AppcBasic {
                    service.flush_basic_conversation_send(run, token)?
                } else {
                    service.flush_conversation_send(run, token)?
                };
                if outcome == ConversationTransmitOutcome::Pending {
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
                continue;
            }
            if record.kind == ConversationKind::AppcBasic {
                indicator = next
                    .conversation_mut(token)
                    .ok_or(HostProblem::InfrastructureFailure)?
                    .observe_basic_wait_terminal(&owner, super::context(run)?)
                    .map_err(map_problem)?;
            } else if let Some(exchange) = next
                .exchanges
                .get_mut(&u32::from_be_bytes(token).to_string())
            {
                indicator = exchange.observe_wait_terminal();
                if indicator.is_some() {
                    next.conversation_mut(token)
                        .ok_or(HostProblem::InfrastructureFailure)?
                        .next_sequence()
                        .map_err(map_problem)?;
                }
            }
            Some(record.state)
        } else {
            None
        };
        let (name, response) = match indicator {
            Some(DataCondition::Signal) => ("SIGNAL", 24),
            Some(DataCondition::EndOfChain) => ("EOC", 6),
            _ => ("NORMAL", 0),
        };
        let mut reply = ConversationReply {
            condition: name.into(),
            response,
            response2: 0,
            state,
            token,
            outputs: BTreeMap::from([(
                "EIBEOC".into(),
                vec![u8::from(indicator == Some(DataCondition::EndOfChain)) * 0xff],
            )]),
        };
        super::receive::capture_disposition(service, run, request, &mut reply)?;
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
        if before
            .persist_with_replay(&mut next, &replay, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return super::receive::response(service, run, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::WaitTerminal
        || request.arguments.contains_key("CONVID") && request.arguments.contains_key("SESSION")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID" | "SESSION" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn map_problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned | ConversationProblem::Malformed => {
            condition("NOTALLOC", 61, 0)
        }
        ConversationProblem::DplPrincipal => condition("INVREQ", 16, 200),
        ConversationProblem::WrongState | ConversationProblem::WrongKind => {
            condition("NOTALLOC", 61, 0)
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
