//! APPC basic GDS WAIT over the carrier-confirmed process ledger.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationTransmitOutcome, GdsReturnCode,
    GdsWaitFailure, load_conversation_replay,
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
const DECIMAL_SCHEMA: &str = "mainframe-env.cics.decimal@1";
const PAYLOAD_SCHEMA: &str = "mainframe-env.cics.payload@1";

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
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let token = if let Some(value) = request.arguments.get("CONVID") {
        let Ok(token): Result<[u8; 4], _> = value.bytes().try_into() else {
            return response(service, run, &failure_reply(GdsWaitFailure::NotOwned));
        };
        token
    } else if let Some(record) = initial.conversations.values().find(|record| {
        record.principal_facility
            && record.kind == ConversationKind::AppcBasic
            && !record.released
            && record.owner.execution == owner.execution
            && record.owner.run_unit == owner.run_unit
    }) {
        record.token
    } else {
        return response(service, run, &failure_reply(GdsWaitFailure::NotOwned));
    };
    let Some(record) = initial.conversation(token) else {
        return response(service, run, &failure_reply(GdsWaitFailure::NotOwned));
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
        let Some(record) = before.conversation(token) else {
            return response(service, run, &failure_reply(GdsWaitFailure::NotOwned));
        };
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.kind == ConversationKind::Mro {
            return response(service, run, &failure_reply(GdsWaitFailure::NotAppc));
        }
        if record.kind != ConversationKind::AppcBasic {
            return response(service, run, &failure_reply(GdsWaitFailure::NotBasic));
        }
        if let Err(problem) = record.wait_transmitted(&owner, super::context(run)?, true) {
            return response(service, run, &failure_reply(map_problem(problem)));
        }
        if record.data.pending_outbound() != 0 {
            match service.flush_basic_conversation_send(run, token)? {
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
                ConversationTransmitOutcome::Confirmed
                | ConversationTransmitOutcome::Rejected(_) => continue,
            }
        }
        let block = record
            .gds_convdata(false)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let state = record.state;
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(state),
            token: Some(token),
            outputs: BTreeMap::from([
                ("RETCODE".into(), GdsReturnCode::NORMAL.0.to_vec()),
                ("CONVDATA".into(), block.to_vec()),
                ("STATE".into(), state.cvda().to_string().into_bytes()),
            ]),
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
        let mut next = before.clone();
        if before
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
    if request.operation != CicsOperation::GdsWaitConversation
        || !request.arguments.contains_key("RETCODE")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID" | "CONVDATA" | "RETCODE" | "STATE" | "RESP" | "RESP2" | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn map_problem(problem: ConversationProblem) -> GdsWaitFailure {
    match problem {
        ConversationProblem::WrongKind => GdsWaitFailure::NotBasic,
        ConversationProblem::WrongState => GdsWaitFailure::StateCheck,
        ConversationProblem::NotOwned
        | ConversationProblem::DplPrincipal
        | ConversationProblem::StaleOwner
        | ConversationProblem::Malformed
        | ConversationProblem::Length
        | ConversationProblem::Exhausted => GdsWaitFailure::NotOwned,
    }
}

fn failure_reply(failure: GdsWaitFailure) -> ConversationReply {
    ConversationReply {
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        state: None,
        token: None,
        outputs: BTreeMap::from([("RETCODE".into(), failure.retcode().0.to_vec())]),
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
    for (name, bytes) in &reply.outputs {
        let schema = match name.as_str() {
            "RETCODE" => RETCODE_SCHEMA,
            "STATE" => DECIMAL_SCHEMA,
            _ => PAYLOAD_SCHEMA,
        };
        result.outputs.insert(
            name.clone(),
            BoundedPayload::new(schema, bytes.clone(), InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(result)
}
