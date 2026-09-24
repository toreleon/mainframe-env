//! Explicit APPC mapped/MRO send-then-receive over durable peer frames.

use super::{
    ConversationKind, ConversationLedger, ConversationOutboundFrame, ConversationOwner,
    ConversationProblem, ConversationReplay, ConversationReply, MAX_EXCHANGE_FRAME_BYTES,
    definitions, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, CicsResponse,
    HostProblem, HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const PAYLOAD_SCHEMA: &str = "mainframe-env.cics.payload@1";
const DECIMAL_SCHEMA: &str = "mainframe-env.cics.decimal@1";
const FLAG_SCHEMA: &str = "mainframe-env.cics.eib-flag@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::Converse {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
    let owner = ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    };
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let token = select_token(&initial, &owner, request)?;
    let record = initial
        .conversation(token)
        .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
    let system = record.system.clone();
    let kind = record.kind;
    if kind == ConversationKind::AppcBasic {
        return Err(condition("INVREQ", 16, 0));
    }
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{system}"),
        AccessIntent::Execute,
    )?;
    let attach_id = if let Some(value) = request.arguments.get("ATTACHID") {
        if kind != ConversationKind::Mro {
            return Err(condition("INVREQ", 16, 0));
        }
        let name = super::allocate::name(value.bytes(), 8)?;
        service.authorize(
            run,
            "ATTACH",
            &format!("CICS.ATTACH.{name}"),
            AccessIntent::Read,
        )?;
        if initial.attach(&owner, &name).is_none() {
            return Err(condition("CBIDERR", 62, 0));
        }
        Some(name)
    } else {
        None
    };
    if kind != ConversationKind::Mro
        && (request.arguments.contains_key("OPTION.FMH")
            || request.arguments.contains_key("OPTION.DEFRESP"))
    {
        return Err(condition("INVREQ", 16, 0));
    }
    let data = outbound_data(request)?;
    let profile = definitions::load_profile(
        service.store.as_ref(),
        record.effective_processing_profile(),
        kind,
    )?
    .ok_or_else(|| condition("INVREQ", 16, 0))?;
    if data.len() > profile.maximum_data_bytes as usize {
        return Err(condition("LENGERR", 22, 0));
    }
    let capacity = receive_capacity(request)?;
    let outbound = ConversationOutboundFrame {
        data,
        attach_id,
        fmh: request.arguments.contains_key("OPTION.FMH"),
        definite_response: request.arguments.contains_key("OPTION.DEFRESP"),
    };
    outbound
        .validate()
        .map_err(|_| condition("LENGERR", 22, 0))?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let effect_key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let principal = run.invocation.principal.id().as_str().to_owned();
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
            return response(service, run, &request.condition_policy, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = current.clone();
        let record = next
            .conversation_mut(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        if record.system != system || record.kind != kind {
            return Err(HostProblem::IdempotencyConflict);
        }
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        let Some(exchange) = next.exchange_mut(token) else {
            return suspended(service, run);
        };
        if !exchange.retained.is_empty() {
            return Err(HostProblem::Unsupported);
        }
        let Some(frame) = exchange.inbound.first().cloned() else {
            return suspended(service, run);
        };
        if kind == ConversationKind::Mro && frame.signal {
            return Err(HostProblem::Unsupported);
        }
        let received_len = capacity.min(frame.data.len());
        let overlong = frame.data.len() > capacity;
        let notruncate = request.arguments.contains_key("OPTION.NOTRUNCATE");
        let reported_len = if overlong && !notruncate {
            frame.data.len()
        } else {
            received_len
        };
        if request.arguments.contains_key("TOLENGTH") && reported_len > i16::MAX as usize {
            return Err(HostProblem::Unsupported);
        }
        let condition_name = if kind == ConversationKind::Mro && frame.inbound_fmh {
            "INBFMH"
        } else if overlong && !notruncate {
            "LENGERR"
        } else if kind == ConversationKind::AppcMapped && frame.signal {
            "SIGNAL"
        } else if frame.end_of_chain {
            "EOC"
        } else {
            "NORMAL"
        };
        let mut outputs = BTreeMap::from([
            ("EIBEOC".into(), vec![flag(frame.end_of_chain)]),
            ("EIBFMH".into(), vec![flag(frame.inbound_fmh)]),
            ("EIBSIG".into(), vec![flag(frame.signal)]),
        ]);
        let output = if request.arguments.contains_key("INTO") {
            "INTO"
        } else {
            "SET"
        };
        outputs.insert(output.into(), frame.data[..received_len].to_vec());
        for name in ["TOLENGTH", "TOFLENGTH"] {
            if request.arguments.contains_key(name) {
                outputs.insert(name.into(), reported_len.to_string().into_bytes());
            }
        }
        if request.arguments.contains_key("STATE") {
            outputs.insert("STATE".into(), super::state_cvda::bytes(frame.next_state));
        }
        let reply = ConversationReply {
            condition: condition_name.into(),
            response: condition_code(condition_name),
            response2: 0,
            state: Some(frame.next_state),
            token: Some(token),
            outputs,
        };
        next.conversation_mut(token)
            .ok_or(HostProblem::InfrastructureFailure)?
            .complete_converse(&owner, super::context(run)?, frame.next_state)
            .map_err(map_problem)?;
        let exchange = next
            .exchange_mut(token)
            .ok_or(HostProblem::InfrastructureFailure)?;
        exchange.inbound.remove(0);
        exchange
            .record_outbound(outbound.clone())
            .map_err(|problem| match problem {
                ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
                _ => HostProblem::Malformed,
            })?;
        if overlong && notruncate {
            exchange.retained = frame.data[received_len..].to_vec();
        }
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
            return response(service, run, &request.condition_policy, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    let has = |name: &str| request.arguments.contains_key(name);
    if has("CONVID") && has("SESSION")
        || !has("FROM") && !has("ATTACHID")
        || has("INTO") == has("SET")
        || has("FROMLENGTH") && has("FROMFLENGTH")
        || has("MAXLENGTH") && has("MAXFLENGTH")
        || has("TOLENGTH") && has("TOFLENGTH")
        || (has("FROMLENGTH") || has("FROMFLENGTH")) && !has("FROM")
        || has("INTO") && !has("CAPACITY.INTO")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID"
                    | "SESSION"
                    | "ATTACHID"
                    | "FROM"
                    | "FROMLENGTH"
                    | "FROMFLENGTH"
                    | "INTO"
                    | "SET"
                    | "TOLENGTH"
                    | "TOFLENGTH"
                    | "MAXLENGTH"
                    | "MAXFLENGTH"
                    | "CAPACITY.INTO"
                    | "CAPACITY.TOLENGTH"
                    | "CAPACITY.TOFLENGTH"
                    | "STATE"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOTRUNCATE"
                    | "OPTION.DEFRESP"
                    | "OPTION.FMH"
                    | "OPTION.NOHANDLE"
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

fn outbound_data(request: &CicsRequest) -> Result<Vec<u8>, HostProblem> {
    let Some(value) = request.arguments.get("FROM") else {
        return Ok(Vec::new());
    };
    let length = if request.arguments.contains_key("FROMLENGTH") {
        signed_number(request, "FROMLENGTH", 2)?
    } else if request.arguments.contains_key("FROMFLENGTH") {
        signed_number(request, "FROMFLENGTH", 4)?
    } else {
        i64::try_from(value.bytes().len()).map_err(|_| condition("LENGERR", 22, 0))?
    };
    if length < 0
        || length as usize > value.bytes().len()
        || length as usize > MAX_EXCHANGE_FRAME_BYTES
    {
        return Err(condition("LENGERR", 22, 0));
    }
    Ok(value.bytes()[..length as usize].to_vec())
}

fn receive_capacity(request: &CicsRequest) -> Result<usize, HostProblem> {
    let into_capacity = if request.arguments.contains_key("INTO") {
        capacity_argument(request, "CAPACITY.INTO")?
    } else {
        MAX_EXCHANGE_FRAME_BYTES
    };
    let selected = if request.arguments.contains_key("MAXLENGTH") {
        signed_number(request, "MAXLENGTH", 2)?.max(0) as usize
    } else if request.arguments.contains_key("MAXFLENGTH") {
        signed_number(request, "MAXFLENGTH", 4)?.max(0) as usize
    } else if request.arguments.contains_key("TOLENGTH")
        && request.arguments.contains_key("CAPACITY.TOLENGTH")
    {
        capacity_argument(request, "CAPACITY.TOLENGTH")?
    } else if request.arguments.contains_key("TOFLENGTH")
        && request.arguments.contains_key("CAPACITY.TOFLENGTH")
    {
        capacity_argument(request, "CAPACITY.TOFLENGTH")?
    } else {
        into_capacity
    };
    if selected > MAX_EXCHANGE_FRAME_BYTES || into_capacity > MAX_EXCHANGE_FRAME_BYTES {
        return Err(condition("LENGERR", 22, 0));
    }
    Ok(selected.min(into_capacity))
}

fn capacity_argument(request: &CicsRequest, name: &str) -> Result<usize, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    if value.schema() != DECIMAL_SCHEMA {
        return Err(HostProblem::Malformed);
    }
    let number = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<i64>()
        .map_err(|_| HostProblem::Malformed)?;
    Ok(number.max(0) as usize)
}

fn signed_number(request: &CicsRequest, name: &str, width: usize) -> Result<i64, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    match (value.schema(), width, value.bytes()) {
        (DECIMAL_SCHEMA, 2, bytes) => std::str::from_utf8(bytes)
            .map_err(|_| HostProblem::Malformed)?
            .parse::<i16>()
            .map(i64::from)
            .map_err(|_| condition("LENGERR", 22, 0)),
        (DECIMAL_SCHEMA, 4, bytes) => std::str::from_utf8(bytes)
            .map_err(|_| HostProblem::Malformed)?
            .parse::<i32>()
            .map(i64::from)
            .map_err(|_| condition("LENGERR", 22, 0)),
        ("mainframe-env.cics.storage-value@1", 2, [first, second]) => {
            Ok(i64::from(i16::from_be_bytes([*first, *second])))
        }
        ("mainframe-env.cics.storage-value@1", 4, [a, b, c, d]) => {
            Ok(i64::from(i32::from_be_bytes([*a, *b, *c, *d])))
        }
        _ => Err(HostProblem::Malformed),
    }
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

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn condition_code(name: &str) -> i32 {
    match name {
        "EOC" => 6,
        "INBFMH" => 7,
        "LENGERR" => 22,
        "SIGNAL" => 24,
        _ => 0,
    }
}

fn flag(value: bool) -> u8 {
    if value { 0xff } else { 0 }
}

fn suspended(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Suspended,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn response(
    service: &CicsService,
    run: &Run,
    policy: &CicsConditionPolicy,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    let mut result = if reply.condition == "NORMAL" {
        service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        )?
    } else {
        super::super::condition(
            service,
            run,
            policy,
            condition(&reply.condition, reply.response, reply.response2),
        )?
    };
    for (name, bytes) in &reply.outputs {
        let schema = match name.as_str() {
            "INTO" | "SET" => PAYLOAD_SCHEMA,
            "TOLENGTH" | "TOFLENGTH" => DECIMAL_SCHEMA,
            "STATE" if bytes.len() == 4 => "mainframe-env.cics.cvda@1",
            "EIBEOC" | "EIBFMH" | "EIBSIG" if matches!(bytes.as_slice(), [0] | [0xff]) => {
                FLAG_SCHEMA
            }
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        result.outputs.insert(
            name.clone(),
            BoundedPayload::new(schema, bytes.clone(), InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(result)
}
