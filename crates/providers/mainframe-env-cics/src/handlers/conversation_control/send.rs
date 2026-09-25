//! Mapped APPC/MRO SEND staged in the shared exchange and confirmed by carrier.

use super::{
    ConversationDataFrame, ConversationKind, ConversationLedger, ConversationOwner,
    ConversationProblem, ConversationReplay, ConversationReply, ConversationState,
    ConversationTransmitOutcome, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const MAX_SEND_BYTES: usize = 32_767;
const DECIMAL_SCHEMA: &str = "mainframe-env.cics.decimal@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_shape(request)?;
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
    let saved = load_conversation_replay(service.store.as_ref(), mutation.idempotency_key.as_str())
        .map_err(store_error)?;
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
        super::receive::select_token(&initial, &owner, request)?
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
        AccessIntent::Update,
    )?;
    let wait = request.arguments.contains_key("OPTION.WAIT")
        || request.arguments.contains_key("OPTION.CONFIRM")
        || request.arguments.contains_key("OPTION.DEFRESP");
    if let Some(saved) = saved {
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
        return finish(service, run, token, wait, reply);
    }
    let frame = frame(request)?;
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
            return finish(service, run, token, wait, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let record = current
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        if record.kind == ConversationKind::AppcBasic
            || record.kind == ConversationKind::Mro && request.arguments.contains_key("CONVID")
            || record.kind == ConversationKind::AppcMapped
                && (frame.fmh || frame.defresp || frame.attach_id.is_some())
        {
            return Err(condition("INVREQ", 16, 0));
        }
        if record.data.terminal_error() {
            return Err(condition("TERMERR", 81, 0));
        }
        if frame
            .attach_id
            .as_ref()
            .is_some_and(|name| current.attach(&owner, name).is_none())
        {
            return Err(condition("CBIDERR", 62, 0));
        }
        let mut next = current.clone();
        next.ensure_mapped_connect_staged(token, &owner, super::context(run)?)
            .map_err(map_problem)?;
        let signal = next
            .consume_mapped_signal(token, &owner, super::context(run)?)
            .map_err(map_problem)?;
        let send_id = next
            .stage_mapped_send(token, &owner, super::context(run)?, frame.clone())
            .map_err(map_problem)?;
        let staged_state = next
            .conversation(token)
            .ok_or(HostProblem::InfrastructureFailure)?
            .state;
        let state = if wait && staged_state == ConversationState::PendReceive {
            ConversationState::Receive
        } else {
            staged_state
        };
        let mut reply = ConversationReply {
            condition: if signal { "SIGNAL" } else { "NORMAL" }.into(),
            response: if signal { 24 } else { 0 },
            response2: 0,
            state: Some(state),
            token: Some(token),
            outputs: BTreeMap::from([
                ("STATE".into(), state.cvda().to_string().into_bytes()),
                ("SEND_ID".into(), send_id.to_be_bytes().to_vec()),
            ]),
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
        if current
            .persist_with_replay(&mut next, &replay, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return finish(service, run, token, wait, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn finish(
    service: &CicsService,
    run: &Run,
    token: [u8; 4],
    wait: bool,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if wait {
        match service.flush_conversation_send(run, token)? {
            ConversationTransmitOutcome::Confirmed | ConversationTransmitOutcome::Rejected(_) => {}
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
        }
    }
    if wait {
        let send_id = u64::from_be_bytes(
            reply
                .outputs
                .get("SEND_ID")
                .ok_or(HostProblem::InfrastructureFailure)?
                .as_slice()
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        );
        let ledger = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        if let Some(code) = ledger
            .exchanges
            .get(&u32::from_be_bytes(token).to_string())
            .and_then(|exchange| exchange.negative_responses.get(&send_id))
        {
            let mut rejected = reply.clone();
            rejected.outputs.insert("EIBERR".into(), vec![0xff]);
            rejected.outputs.insert("EIBERRCD".into(), code.to_vec());
            return response(service, run, &rejected);
        }
    }
    response(service, run, reply)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    let has = |name: &str| request.arguments.contains_key(name);
    if request.operation != CicsOperation::SendConversation
        || has("CONVID") && has("SESSION")
        || has("RESP2") && !has("RESP")
        || has("LENGTH") && has("FLENGTH")
        || has("OPTION.INVITE") && has("OPTION.LAST")
        || has("OPTION.CONFIRM") && has("OPTION.DEFRESP")
        || has("FROM") != (has("LENGTH") || has("FLENGTH"))
            && !(has("OPTION.INVITE") && !has("FROM") && !has("LENGTH") && !has("FLENGTH"))
        || !has("FROM") && !has("OPTION.INVITE")
        || !has("FROM") && (has("ATTACHID") || has("OPTION.FMH") || has("OPTION.DEFRESP"))
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID"
                    | "SESSION"
                    | "FROM"
                    | "LENGTH"
                    | "FLENGTH"
                    | "ATTACHID"
                    | "STATE"
                    | "OPTION.INVITE"
                    | "OPTION.LAST"
                    | "OPTION.CONFIRM"
                    | "OPTION.WAIT"
                    | "OPTION.FMH"
                    | "OPTION.DEFRESP"
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

fn frame(request: &CicsRequest) -> Result<ConversationDataFrame, HostProblem> {
    let bytes = if let Some(from) = request.arguments.get("FROM") {
        let (name, width) = if request.arguments.contains_key("LENGTH") {
            ("LENGTH", 2)
        } else {
            ("FLENGTH", 4)
        };
        let length = super::receive::parse_signed(
            request.arguments.get(name).ok_or(HostProblem::Malformed)?,
            width,
        )?;
        if length < 0 || length as usize > MAX_SEND_BYTES || length as usize > from.bytes().len() {
            return Err(condition("LENGERR", 22, 0));
        }
        from.bytes()[..length as usize].to_vec()
    } else {
        Vec::new()
    };
    let attach_id = request
        .arguments
        .get("ATTACHID")
        .map(|value| {
            std::str::from_utf8(value.bytes())
                .map(str::trim_end)
                .map(str::to_ascii_uppercase)
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()?;
    let frame = ConversationDataFrame {
        bytes,
        end_of_chain: request.arguments.contains_key("OPTION.LAST"),
        fmh: request.arguments.contains_key("OPTION.FMH"),
        signal: false,
        end_structured_field: true,
        error_code: None,
        invite: request.arguments.contains_key("OPTION.INVITE"),
        confirm: request.arguments.contains_key("OPTION.CONFIRM"),
        defresp: request.arguments.contains_key("OPTION.DEFRESP"),
        attach_id,
        ..Default::default()
    };
    frame.validate().map_err(map_problem)?;
    Ok(frame)
}

fn response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if !matches!(reply.condition.as_str(), "NORMAL" | "SIGNAL")
        || reply.response != if reply.condition == "SIGNAL" { 24 } else { 0 }
        || reply.response2 != 0
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let disposition = match reply.outputs.get("DISPOSITION").map(Vec::as_slice) {
        Some(b"C") => CicsDisposition::Complete,
        Some(b"I") => CicsDisposition::Ignored,
        Some(b"H") => CicsDisposition::Handler,
        Some(b"A") => CicsDisposition::Abended,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let target = reply
        .outputs
        .get("TARGET")
        .map(|bytes| {
            String::from_utf8(bytes.clone()).map_err(|_| HostProblem::InfrastructureFailure)
        })
        .transpose()?;
    if (disposition == CicsDisposition::Handler) != target.is_some() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let state = reply
        .outputs
        .get("STATE")
        .ok_or(HostProblem::InfrastructureFailure)?;
    if std::str::from_utf8(state)
        .ok()
        .and_then(|text| text.parse::<i32>().ok())
        != reply.state.map(ConversationState::cvda)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut result = service.response(
        run,
        disposition,
        &reply.condition,
        reply.response,
        0,
        target,
        None,
        Vec::new(),
    )?;
    result.outputs.insert(
        "STATE".into(),
        BoundedPayload::new(DECIMAL_SCHEMA, state.clone(), InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    for name in ["EIBERR", "EIBERRCD"] {
        if let Some(bytes) = reply.outputs.get(name) {
            result.outputs.insert(
                name.into(),
                BoundedPayload::new(
                    "mainframe-env.cics.payload@1",
                    bytes.clone(),
                    InvocationLimits::default(),
                )
                .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
    }
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
