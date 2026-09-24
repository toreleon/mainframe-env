//! APPC basic allocation with source-specific six-byte GDS return codes.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReplay, ConversationReply, ConversationState, GdsAllocateFailure, GdsReturnCode,
    definitions, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const CONVID_SCHEMA: &str = "mainframe-env.cics.convid@1";
const RETCODE_SCHEMA: &str = "mainframe-env.cics.gds-retcode@1";

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    if request.operation != CicsOperation::GdsAllocateConversation {
        return Err(HostProblem::Unsupported);
    }
    validate_shape(request)?;
    super::deadline(service, run)?;
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
    let selected = selector(service, run, request)?;
    let (sysid, mode) = match selected {
        Ok(pair) => pair,
        Err(failure) => return response(service, run, &failure_reply(failure)),
    };
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{sysid}"),
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
        let Some(system) = current.systems.get(&sysid) else {
            let failure = if request.arguments.contains_key("PARTNER") {
                GdsAllocateFailure::UnknownPartnerNetworkName
            } else {
                GdsAllocateFailure::UnknownSystem
            };
            return response(service, run, &failure_reply(failure));
        };
        if system.kind != ConversationKind::AppcMapped {
            return response(
                service,
                run,
                &failure_reply(GdsAllocateFailure::WrongConnectionKind),
            );
        }
        if !system.enabled {
            return response(
                service,
                run,
                &failure_reply(GdsAllocateFailure::UnusableConnection),
            );
        }
        if mode == "SNASVCMG" {
            return response(
                service,
                run,
                &failure_reply(GdsAllocateFailure::RestrictedMode),
            );
        }
        if definitions::load_profile(service.store.as_ref(), &mode, ConversationKind::AppcBasic)?
            .is_none()
        {
            let failure = if request.arguments.contains_key("PARTNER") {
                GdsAllocateFailure::UnknownPartnerProfile
            } else {
                GdsAllocateFailure::UnknownMode
            };
            return response(service, run, &failure_reply(failure));
        }
        let mut next = current.clone();
        let record = match next.allocate_with_profile(
            &sysid,
            ConversationKind::AppcBasic,
            owner.clone(),
            &mode,
        ) {
            Ok(record) => record,
            Err(ConversationProblem::Exhausted) => {
                if request.arguments.contains_key("OPTION.NOQUEUE") {
                    return response(
                        service,
                        run,
                        &failure_reply(GdsAllocateFailure::NoImmediateSession),
                    );
                }
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
            Err(ConversationProblem::WrongKind) => {
                return response(
                    service,
                    run,
                    &failure_reply(GdsAllocateFailure::WrongConnectionKind),
                );
            }
            Err(_) => return Err(HostProblem::InfrastructureFailure),
        };
        let mut outputs = BTreeMap::from([
            ("CONVID".into(), record.token.to_vec()),
            ("RETCODE".into(), GdsReturnCode::NORMAL.0.to_vec()),
        ]);
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
            token: Some(record.token),
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
    let has_sysid = request.arguments.contains_key("SYSID");
    let has_partner = request.arguments.contains_key("PARTNER");
    if has_sysid == has_partner
        || request.arguments.contains_key("MODENAME") && !has_sysid
        || !request.arguments.contains_key("CONVID")
        || !request.arguments.contains_key("RETCODE")
        || request.arguments.keys().any(|name| {
            !matches!(
                name.as_str(),
                "SYSID"
                    | "PARTNER"
                    | "MODENAME"
                    | "CONVID"
                    | "RETCODE"
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
) -> Result<Result<(String, String), GdsAllocateFailure>, HostProblem> {
    if let Some(partner) = request.arguments.get("PARTNER") {
        let name = super::allocate::name(partner.bytes(), 8)?;
        service.authorize(
            run,
            "PARTNER",
            &format!("CICS.PARTNER.{name}"),
            AccessIntent::Execute,
        )?;
        let Some(definition) = definitions::load_partner(service.store.as_ref(), &name)? else {
            return Ok(Err(GdsAllocateFailure::UnknownPartner));
        };
        return Ok(Ok((definition.sysid, definition.profile)));
    }
    let sysid = super::allocate::name(
        request
            .arguments
            .get("SYSID")
            .ok_or(HostProblem::Malformed)?
            .bytes(),
        4,
    )?;
    let mode = request
        .arguments
        .get("MODENAME")
        .map(|value| super::allocate::name(value.bytes(), 8))
        .transpose()?
        .unwrap_or_else(|| "DEFAULT".into());
    Ok(Ok((sysid, mode)))
}

fn failure_reply(failure: GdsAllocateFailure) -> ConversationReply {
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
    for (name, schema, len) in [("CONVID", CONVID_SCHEMA, 4), ("RETCODE", RETCODE_SCHEMA, 6)] {
        if let Some(bytes) = reply.outputs.get(name) {
            if bytes.len() != len {
                return Err(HostProblem::InfrastructureFailure);
            }
            result.outputs.insert(
                name.into(),
                BoundedPayload::new(schema, bytes.clone(), InvocationLimits::default())
                    .map_err(|_| HostProblem::ResourceExhausted)?,
            );
        }
    }
    if !result.outputs.contains_key("RETCODE") {
        return Err(HostProblem::InfrastructureFailure);
    }
    super::state_cvda::insert_output(&mut result, reply.outputs.get("STATE"))?;
    Ok(result)
}
