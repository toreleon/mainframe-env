//! APPC basic GDS FREE after a peer-confirmed FREE protocol state.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationState, GdsFreeFailure, GdsReturnCode,
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
const RETCODE_SCHEMA: &str = "mainframe-env.cics.gds-retcode@1";
const STATE_SCHEMA: &str = "mainframe-env.cics.cvda@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::GdsFreeConversation {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
    let value = request
        .arguments
        .get("CONVID")
        .ok_or(HostProblem::Malformed)?;
    let Ok(token): Result<[u8; 4], _> = value.bytes().try_into() else {
        return response(service, run, &failure_reply(GdsFreeFailure::NotOwned));
    };
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
    let Some(record) = initial.conversation(token) else {
        return response(service, run, &failure_reply(GdsFreeFailure::NotOwned));
    };
    let system = record.system.clone();
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
        let Some(record) = next.conversation_mut(token) else {
            return response(service, run, &failure_reply(GdsFreeFailure::NotOwned));
        };
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.released {
            return response(service, run, &failure_reply(GdsFreeFailure::NotOwned));
        }
        if record.kind == ConversationKind::Mro {
            return response(service, run, &failure_reply(GdsFreeFailure::NotAppc));
        }
        if record.kind != ConversationKind::AppcBasic {
            return response(service, run, &failure_reply(GdsFreeFailure::NotBasic));
        }
        match record.release(&owner, super::context(run)?, true) {
            Ok(()) => {}
            Err(ConversationProblem::StaleOwner) => return Err(HostProblem::IdempotencyConflict),
            Err(problem) => return response(service, run, &failure_reply(map_problem(problem))),
        }
        let mut outputs = BTreeMap::from([("RETCODE".into(), GdsReturnCode::NORMAL.0.to_vec())]);
        if request.arguments.contains_key("STATE") {
            outputs.insert("STATE".into(), vec![0; 4]);
        }
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
    if !request.arguments.contains_key("CONVID")
        || !request.arguments.contains_key("RETCODE")
        || request.arguments.contains_key("CONVDATA")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID" | "RETCODE" | "STATE" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}

fn map_problem(problem: ConversationProblem) -> GdsFreeFailure {
    match problem {
        ConversationProblem::WrongKind => GdsFreeFailure::NotBasic,
        ConversationProblem::WrongState => GdsFreeFailure::StateCheck,
        ConversationProblem::NotOwned
        | ConversationProblem::DplPrincipal
        | ConversationProblem::Malformed
        | ConversationProblem::Exhausted
        | ConversationProblem::Length
        | ConversationProblem::StaleOwner => GdsFreeFailure::NotOwned,
    }
}

fn failure_reply(problem: GdsFreeFailure) -> ConversationReply {
    ConversationReply {
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        state: None,
        token: None,
        outputs: BTreeMap::from([("RETCODE".into(), problem.retcode().0.to_vec())]),
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
    let code = reply
        .outputs
        .get("RETCODE")
        .ok_or(HostProblem::InfrastructureFailure)?;
    if code.len() != 6 {
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
        "RETCODE".into(),
        BoundedPayload::new(RETCODE_SCHEMA, code.clone(), InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?,
    );
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
