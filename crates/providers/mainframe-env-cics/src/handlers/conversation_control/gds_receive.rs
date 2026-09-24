//! APPC basic GDS RECEIVE with six-byte RETCODE and DFHCDBLK indicators.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, GdsReceiveFailure, GdsReturnCode,
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
    let value = request
        .arguments
        .get("CONVID")
        .ok_or(HostProblem::Malformed)?;
    let Ok(token): Result<[u8; 4], _> = value.bytes().try_into() else {
        return response(service, run, &failure_reply(GdsReceiveFailure::NotOwned));
    };
    let max_length = match maximum(request)? {
        Ok(value) => value,
        Err(failure) => return response(service, run, &failure_reply(failure)),
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
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let principal = run.invocation.principal.id().as_str().to_owned();
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let Some(record) = initial.conversation(token) else {
        return response(service, run, &failure_reply(GdsReceiveFailure::NotOwned));
    };
    let system = record.system.clone();
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{system}"),
        AccessIntent::Read,
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
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = current.clone();
        let Some(record) = next.conversation_mut(token) else {
            return response(service, run, &failure_reply(GdsReceiveFailure::NotOwned));
        };
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.kind == ConversationKind::Mro {
            return response(service, run, &failure_reply(GdsReceiveFailure::NotAppc));
        }
        if record.kind != ConversationKind::AppcBasic {
            return response(service, run, &failure_reply(GdsReceiveFailure::NotBasic));
        }
        let received = match record.receive_data(
            &owner,
            super::context(run)?,
            true,
            max_length,
            false,
            !request.arguments.contains_key("OPTION.BUFFER"),
        ) {
            Ok(reply) => reply,
            Err(ConversationProblem::StaleOwner) => return Err(HostProblem::IdempotencyConflict),
            Err(problem) => return response(service, run, &failure_reply(map_problem(problem))),
        };
        let Some(received) = received else {
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
        };
        let block = record
            .gds_receive_convdata(&received)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(received.state),
            token: Some(token),
            outputs: BTreeMap::from([
                ("RETCODE".into(), GdsReturnCode::NORMAL.0.to_vec()),
                ("INTO".into(), received.bytes.clone()),
                ("SET".into(), received.bytes),
                (
                    "FLENGTH".into(),
                    received.returned_length.to_string().into_bytes(),
                ),
                ("CONVDATA".into(), block.to_vec()),
                (
                    "STATE".into(),
                    received.state.cvda().to_string().into_bytes(),
                ),
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
    if request.operation != CicsOperation::GdsReceiveConversation
        || !has("CONVID")
        || !has("MAXFLENGTH")
        || !has("FLENGTH")
        || !has("RETCODE")
        || has("INTO") == has("SET")
        || has("OPTION.BUFFER") && has("OPTION.LLID")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID"
                    | "MAXFLENGTH"
                    | "FLENGTH"
                    | "RETCODE"
                    | "CONVDATA"
                    | "STATE"
                    | "INTO"
                    | "SET"
                    | "INTO.MAXLENGTH"
                    | "SET.MAXLENGTH"
                    | "OPTION.BUFFER"
                    | "OPTION.LLID"
                    | "OPTION.NOHANDLE"
                    | "RESP"
                    | "RESP2"
            )
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn maximum(request: &CicsRequest) -> Result<Result<usize, GdsReceiveFailure>, HostProblem> {
    let value = request
        .arguments
        .get("MAXFLENGTH")
        .ok_or(HostProblem::Malformed)?;
    let numeric = match value.schema() {
        DECIMAL_SCHEMA => std::str::from_utf8(value.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<i32>()
            .map_err(|_| HostProblem::Malformed)?,
        "mainframe-env.cics.storage-value@1" if value.bytes().len() == 4 => {
            i32::from_be_bytes(value.bytes().try_into().unwrap())
        }
        _ => return Err(HostProblem::Malformed),
    };
    if !(0..=32_767).contains(&numeric) {
        return Ok(Err(GdsReceiveFailure::InvalidMaxFullLength));
    }
    let maximum = numeric as usize;
    if let Some(capacity) = request
        .arguments
        .get(if request.arguments.contains_key("INTO") {
            "INTO.MAXLENGTH"
        } else {
            "SET.MAXLENGTH"
        })
    {
        let available = std::str::from_utf8(capacity.bytes())
            .map_err(|_| HostProblem::Malformed)?
            .parse::<usize>()
            .map_err(|_| HostProblem::Malformed)?;
        if capacity.schema() != DECIMAL_SCHEMA || available < maximum {
            return Ok(Err(GdsReceiveFailure::InvalidMaxFullLength));
        }
    }
    Ok(Ok(maximum))
}

fn map_problem(problem: ConversationProblem) -> GdsReceiveFailure {
    match problem {
        ConversationProblem::WrongKind => GdsReceiveFailure::NotBasic,
        ConversationProblem::WrongState => GdsReceiveFailure::StateCheck,
        ConversationProblem::Length => GdsReceiveFailure::InvalidMaxFullLength,
        ConversationProblem::NotOwned
        | ConversationProblem::DplPrincipal
        | ConversationProblem::Malformed
        | ConversationProblem::Exhausted
        | ConversationProblem::StaleOwner => GdsReceiveFailure::NotOwned,
    }
}

fn failure_reply(problem: GdsReceiveFailure) -> ConversationReply {
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
    for (name, bytes) in &reply.outputs {
        let schema = match name.as_str() {
            "RETCODE" => RETCODE_SCHEMA,
            "FLENGTH" | "STATE" => DECIMAL_SCHEMA,
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
