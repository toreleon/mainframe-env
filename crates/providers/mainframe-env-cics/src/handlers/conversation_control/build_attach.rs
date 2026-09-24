//! Task-owned MRO attach FMH construction; no transport send occurs here.

use super::{
    ConversationAttachHeader, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, allocate, load_conversation_replay,
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
    if request.operation != CicsOperation::BuildAttach {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
    let name = allocate::name(
        request
            .arguments
            .get("ATTACHID")
            .ok_or(HostProblem::Malformed)?
            .bytes(),
        8,
    )?;
    service.authorize(
        run,
        "ATTACH",
        &format!("CICS.ATTACH.{name}"),
        AccessIntent::Update,
    )?;
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
    let principal = run.invocation.principal.id().as_str();
    let header = header(request, owner.clone(), name)?;
    let reply = ConversationReply {
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        state: None,
        token: None,
        outputs: BTreeMap::new(),
    };
    for _ in 0..MAX_CAS_RETRIES {
        super::deadline(service, run)?;
        if let Some(saved) =
            load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?
        {
            let saved = saved
                .matches_request(
                    &owner.execution,
                    &owner.run_unit,
                    principal,
                    owner.lease_epoch,
                    mutation.sequence,
                    digest,
                )
                .map_err(store_error)?;
            return response(service, run, saved);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = current.clone();
        next.set_attach(header.clone())
            .map_err(|problem| match problem {
                ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
                _ => HostProblem::Malformed,
            })?;
        let replay = ConversationReplay {
            schema_version: 1,
            effect_key: effect_key.into(),
            owner_execution: owner.execution.clone(),
            owner_run_unit: owner.run_unit.clone(),
            owner_principal: principal.into(),
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
    if !request.arguments.contains_key("ATTACHID")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "ATTACHID"
                    | "PROCESS"
                    | "RESOURCE"
                    | "RPROCESS"
                    | "RRESOURCE"
                    | "QUEUE"
                    | "IUTYPE"
                    | "DATASTR"
                    | "RECFM"
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

fn header(
    request: &CicsRequest,
    owner: ConversationOwner,
    name: String,
) -> Result<ConversationAttachHeader, HostProblem> {
    let bytes = |key: &str| {
        request
            .arguments
            .get(key)
            .map(|value| value.bytes().to_vec())
            .unwrap_or_default()
    };
    let numeric = |key: &str, default: u16, mask: u16| -> Result<u16, HostProblem> {
        request
            .arguments
            .get(key)
            .map(|value| parse_halfword(value.schema(), value.bytes()).map(|value| value & mask))
            .transpose()
            .map(|value| value.unwrap_or(default))
    };
    let header = ConversationAttachHeader {
        owner,
        name,
        process: bytes("PROCESS"),
        resource: bytes("RESOURCE"),
        return_process: bytes("RPROCESS"),
        return_resource: bytes("RRESOURCE"),
        queue: bytes("QUEUE"),
        iu_type: numeric("IUTYPE", 0, 0x7f)?,
        data_stream: numeric("DATASTR", 0, 0xff)?,
        record_format: numeric("RECFM", 4, 0xff)?,
    };
    header.validate().map_err(|_| HostProblem::Malformed)?;
    Ok(header)
}

pub(super) fn parse_halfword(schema: &str, bytes: &[u8]) -> Result<u16, HostProblem> {
    match schema {
        "mainframe-env.cics.decimal@1" => {
            let text = std::str::from_utf8(bytes).map_err(|_| HostProblem::Malformed)?;
            let signed = text.parse::<i16>().map_err(|_| HostProblem::Malformed)?;
            Ok(signed as u16)
        }
        "mainframe-env.cics.storage-value@1" if bytes.len() == 2 => {
            Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
        }
        _ => Err(HostProblem::Malformed),
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
    service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}
