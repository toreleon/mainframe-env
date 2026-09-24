//! APPC basic GDS CONNECT PROCESS with source-specific RETCODE outcomes.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationState, GdsConnectFailure, GdsReturnCode,
    build_attach, definitions, load_conversation_replay,
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

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::GdsConnectProcess {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
    let token = request
        .arguments
        .get("CONVID")
        .ok_or(HostProblem::Malformed)?
        .bytes();
    let Ok(token): Result<[u8; 4], _> = token.try_into() else {
        return response(service, run, &failure_reply(GdsConnectFailure::NotOwned));
    };
    let process = if let Some(partner) = request.arguments.get("PARTNER") {
        let name = super::allocate::name(partner.bytes(), 8)?;
        service.authorize(
            run,
            "PARTNER",
            &format!("CICS.PARTNER.{name}"),
            AccessIntent::Execute,
        )?;
        if definitions::load_partner(service.store.as_ref(), &name)?.is_none() {
            return response(
                service,
                run,
                &failure_reply(GdsConnectFailure::UnknownPartner),
            );
        }
        let Some(definition) = definitions::load_partner_process(service.store.as_ref(), &name)?
        else {
            return response(
                service,
                run,
                &failure_reply(GdsConnectFailure::UnknownPartner),
            );
        };
        definition.process
    } else {
        let value = request
            .arguments
            .get("PROCNAME")
            .ok_or(HostProblem::Malformed)?
            .bytes();
        let length = halfword(request, "PROCLENGTH")? as usize;
        if !(1..=64).contains(&length) || value.len() < length {
            return response(
                service,
                run,
                &failure_reply(GdsConnectFailure::InvalidProcessLength),
            );
        }
        value[..length].to_vec()
    };
    let pip = if let Some(value) = request.arguments.get("PIPLIST") {
        let length = halfword(request, "PIPLENGTH")? as usize;
        if !(4..=super::MAX_BASIC_PIP_BYTES).contains(&length) || value.bytes().len() < length {
            return response(
                service,
                run,
                &failure_reply(GdsConnectFailure::InvalidPipLength),
            );
        }
        value.bytes()[..length].to_vec()
    } else {
        Vec::new()
    };
    let sync_level = if let Some(value) = request.arguments.get("SYNCLEVEL") {
        let value = build_attach::parse_halfword(value.schema(), value.bytes())?;
        if value > 2 {
            return response(
                service,
                run,
                &failure_reply(GdsConnectFailure::InvalidSyncLevel),
            );
        }
        value as u8
    } else {
        0
    };
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
    let Some(record) = initial.conversation(token) else {
        return response(service, run, &failure_reply(GdsConnectFailure::NotOwned));
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
            return response(service, run, &failure_reply(GdsConnectFailure::NotOwned));
        };
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.kind == ConversationKind::Mro {
            return response(service, run, &failure_reply(GdsConnectFailure::NotAppc));
        }
        if record.kind != ConversationKind::AppcBasic {
            return response(service, run, &failure_reply(GdsConnectFailure::NotBasic));
        }
        match record.connect(
            &owner,
            super::context(run)?,
            true,
            process.clone(),
            pip.clone(),
            sync_level,
        ) {
            Ok(()) => {}
            Err(ConversationProblem::StaleOwner) => return Err(HostProblem::IdempotencyConflict),
            Err(problem) => return response(service, run, &failure_reply(map_problem(problem))),
        }
        record
            .stage_basic_connect(&owner, super::context(run)?)
            .map_err(|_| HostProblem::InfrastructureFailure)?;
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(ConversationState::Send),
            token: Some(token),
            outputs: BTreeMap::from([("RETCODE".into(), GdsReturnCode::NORMAL.0.to_vec())]),
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
    if !has("CONVID")
        || !has("RETCODE")
        || has("PROCNAME") == has("PARTNER")
        || has("PROCNAME") != has("PROCLENGTH")
        || has("PIPLIST") != has("PIPLENGTH")
        || has("STATE")
        || has("CONVDATA")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "CONVID"
                    | "PROCNAME"
                    | "PROCLENGTH"
                    | "PARTNER"
                    | "PIPLIST"
                    | "PIPLENGTH"
                    | "SYNCLEVEL"
                    | "RETCODE"
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

fn halfword(request: &CicsRequest, name: &str) -> Result<u16, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    build_attach::parse_halfword(value.schema(), value.bytes())
}

fn map_problem(problem: ConversationProblem) -> GdsConnectFailure {
    match problem {
        ConversationProblem::NotOwned
        | ConversationProblem::DplPrincipal
        | ConversationProblem::Malformed
        | ConversationProblem::Exhausted => GdsConnectFailure::NotOwned,
        ConversationProblem::WrongKind => GdsConnectFailure::NotBasic,
        ConversationProblem::WrongState => GdsConnectFailure::StateCheck,
        ConversationProblem::Length => GdsConnectFailure::InvalidPipLength,
        ConversationProblem::StaleOwner => GdsConnectFailure::NotOwned,
    }
}

fn failure_reply(problem: GdsConnectFailure) -> ConversationReply {
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
    let code = reply
        .outputs
        .get("RETCODE")
        .ok_or(HostProblem::InfrastructureFailure)?;
    if code.len() != 6 || reply.condition != "NORMAL" || reply.response != 0 || reply.response2 != 0
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
        "RETCODE".into(),
        BoundedPayload::new(RETCODE_SCHEMA, code.clone(), InvocationLimits::default())
            .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(result)
}
