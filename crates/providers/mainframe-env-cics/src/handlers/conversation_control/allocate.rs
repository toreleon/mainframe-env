//! Source-bounded ALLOCATE over one durable APPC/MRO session group.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationState, definitions,
    load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsConditionPolicy, CicsDisposition, CicsOperation, CicsRequest, CicsResponse,
    HostProblem, HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const EIBRSRCE_SCHEMA: &str = "mainframe-env.cics.eib-rsrce@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::AllocateConversation {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    let (sysid, selected_profile) = selector(service, run, request)?;
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{sysid}"),
        AccessIntent::Execute,
    )?;
    if run.invocation.cancellation_requested() {
        return Err(HostProblem::Cancelled);
    }
    check_deadline(service, run)?;
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
    for _ in 0..MAX_CAS_RETRIES {
        if let Some(saved) =
            load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?
        {
            let reply = saved
                .matches_request(
                    &owner.execution,
                    &owner.run_unit,
                    principal,
                    owner.lease_epoch,
                    mutation.sequence,
                    digest,
                )
                .map_err(store_error)?;
            return response(service, run, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let system = current
            .systems
            .get(&sysid)
            .ok_or_else(|| condition("SYSIDERR", 53, 0))?;
        if !system.enabled {
            return Err(condition("SYSIDERR", 53, 0));
        }
        let kind = system.kind;
        if kind == ConversationKind::AppcBasic {
            return Err(condition("INVREQ", 16, 0));
        }
        let profile = selected_profile.as_deref().unwrap_or("DFHCICSA");
        if definitions::load_profile(service.store.as_ref(), profile, kind)?.is_none() {
            return Err(condition("CBIDERR", 62, 0));
        }
        let mut next = current.clone();
        let allocated = match next.allocate_with_profile(&sysid, kind, owner.clone(), profile) {
            Ok(record) => record,
            Err(ConversationProblem::Exhausted) => {
                let immediate = request.arguments.contains_key("OPTION.NOQUEUE")
                    || matches!(request.condition_policy, CicsConditionPolicy::Default)
                        && run.handlers.contains_key("SYSBUSY");
                return if immediate {
                    Err(condition("SYSBUSY", 59, 0))
                } else {
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
                };
            }
            Err(ConversationProblem::WrongKind) => return Err(condition("INVREQ", 16, 0)),
            Err(ConversationProblem::WrongState) => return Err(condition("SYSIDERR", 53, 0)),
            Err(ConversationProblem::Malformed) => return Err(condition("SYSIDERR", 53, 0)),
            Err(_) => return Err(HostProblem::InfrastructureFailure),
        };
        let mut eibrsrce = [b' '; 8];
        eibrsrce[..4].copy_from_slice(&allocated.token);
        let mut outputs = BTreeMap::from([("EIBRSRCE".into(), eibrsrce.to_vec())]);
        if request.arguments.contains_key("STATE") {
            outputs.insert(
                "STATE".into(),
                super::state_cvda::bytes(ConversationState::Allocated),
            );
        }
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(ConversationState::Allocated),
            token: Some(allocated.token),
            outputs,
        };
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
            if run.invocation.cancellation_requested() || check_deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return response(service, run, &reply);
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    let has_sysid = request.arguments.contains_key("SYSID");
    let has_partner = request.arguments.contains_key("PARTNER");
    if has_sysid == has_partner
        || request.arguments.contains_key("PROFILE") && !has_sysid
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "SYSID"
                    | "PARTNER"
                    | "PROFILE"
                    | "STATE"
                    | "RESP"
                    | "RESP2"
                    | "OPTION.NOQUEUE"
                    | "OPTION.NOHANDLE"
            )
        })
    {
        return Err(HostProblem::Unsupported);
    }
    Ok(())
}

fn selector(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<(String, Option<String>), HostProblem> {
    if let Some(partner) = request.arguments.get("PARTNER") {
        let name = name(partner.bytes(), 8)?;
        service.authorize(
            run,
            "PARTNER",
            &format!("CICS.PARTNER.{name}"),
            AccessIntent::Execute,
        )?;
        let definition = definitions::load_partner(service.store.as_ref(), &name)?
            .ok_or_else(|| condition("PARTNERIDERR", 97, 0))?;
        return Ok((definition.sysid, Some(definition.profile)));
    }
    let sysid = name(
        request
            .arguments
            .get("SYSID")
            .ok_or(HostProblem::Malformed)?
            .bytes(),
        4,
    )?;
    let profile = request
        .arguments
        .get("PROFILE")
        .map(|value| name(value.bytes(), 8))
        .transpose()?;
    Ok((sysid, profile))
}

pub(super) fn name(bytes: &[u8], max: usize) -> Result<String, HostProblem> {
    let value = bytes.trim_ascii_end();
    if value.is_empty()
        || value.len() > max
        || !value
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"$#@".contains(byte))
    {
        return Err(HostProblem::Malformed);
    }
    String::from_utf8(value.to_vec()).map_err(|_| HostProblem::Malformed)
}

fn check_deadline(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if let Some(clock) = &service.replay_clock
        && clock.now_tick()? >= run.invocation.deadline_tick
    {
        return Err(HostProblem::TimedOut);
    }
    Ok(())
}

fn response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    let mut result = service.response(
        run,
        CicsDisposition::Complete,
        &reply.condition,
        reply.response,
        reply.response2,
        None,
        None,
        Vec::new(),
    )?;
    let eibrsrce = reply
        .outputs
        .get("EIBRSRCE")
        .ok_or(HostProblem::InfrastructureFailure)?;
    if eibrsrce.len() != 8 {
        return Err(HostProblem::InfrastructureFailure);
    }
    result.outputs.insert(
        "EIBRSRCE".into(),
        BoundedPayload::new(
            EIBRSRCE_SCHEMA,
            eibrsrce.clone(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    super::state_cvda::insert_output(&mut result, reply.outputs.get("STATE"))?;
    Ok(result)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
