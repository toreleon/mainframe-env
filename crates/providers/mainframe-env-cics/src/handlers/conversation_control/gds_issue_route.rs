//! APPC basic ISSUE controls over the one durable conversation ledger.

use super::{
    ConversationKind, ConversationLedger, ConversationOwner, ConversationProblem,
    ConversationReply, ConversationTransmitOutcome, GdsIssueFailure, GdsIssueFlow,
    IssueRequestIdentity, load_conversation_replay,
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
    let flow = flow(request.operation).ok_or(HostProblem::Unsupported)?;
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
    let identity = IssueRequestIdentity {
        principal: run.invocation.principal.id().as_str().into(),
        mutation_sequence: mutation.sequence,
        digest,
        deadline_tick: run.invocation.deadline_tick,
        retain_until_tick: retention_tick,
        state_output: request.arguments.contains_key("STATE"),
        convdata_output: request.arguments.contains_key("CONVDATA"),
        retcode_output: true,
    };
    identity.validate().map_err(|_| HostProblem::Malformed)?;
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
    let saved =
        load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?;
    let token = if let Some(saved) = &saved {
        saved
            .matches_request(
                &owner.execution,
                &owner.run_unit,
                &identity.principal,
                owner.lease_epoch,
                identity.mutation_sequence,
                identity.digest,
            )
            .map_err(store_error)?
            .token
    } else {
        select_token(&initial, &owner, request)
    };
    let Some(token) = token else {
        return response(service, run, &failure(GdsIssueFailure::NotOwned, flow));
    };
    let Some(record) = initial.conversation(token) else {
        return response(service, run, &failure(GdsIssueFailure::NotOwned, flow));
    };
    let system = record.system.clone();
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{system}"),
        AccessIntent::Update,
    )?;
    if let Some(saved) = &saved {
        return response(service, run, &saved.reply);
    }
    for _ in 0..MAX_CAS_RETRIES {
        super::deadline(service, run)?;
        if let Some(saved) =
            load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?
        {
            let reply = saved
                .matches_request(
                    &owner.execution,
                    &owner.run_unit,
                    &identity.principal,
                    owner.lease_epoch,
                    identity.mutation_sequence,
                    identity.digest,
                )
                .map_err(store_error)?;
            return response(service, run, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = current.clone();
        let Some(record) = next.conversation_mut(token) else {
            return response(service, run, &failure(GdsIssueFailure::NotOwned, flow));
        };
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.released {
            return response(service, run, &failure(GdsIssueFailure::NotOwned, flow));
        }
        let kind = record.kind;
        let context = super::context(run)?;
        if let Err(problem) = record.check_owner(&owner, context) {
            return mapped_failure(service, run, flow, kind, problem);
        }
        if kind != ConversationKind::AppcBasic {
            let failure = if kind == ConversationKind::AppcMapped {
                GdsIssueFailure::NotBasic
            } else {
                GdsIssueFailure::NotAppc
            };
            return response(service, run, &self::failure(failure, flow));
        }
        if let Some(pending) = &record.pending_issue {
            if pending.flow != flow || pending.matches_request(effect_key, &identity).is_err() {
                return Err(HostProblem::IdempotencyConflict);
            }
            match service.flush_issue_control(run, token, effect_key, pending.id)? {
                ConversationTransmitOutcome::Confirmed => continue,
                ConversationTransmitOutcome::Pending => return suspended(service, run),
                ConversationTransmitOutcome::Rejected(_) => {
                    return Err(HostProblem::UnknownOutcome);
                }
            }
        }
        let control_id = match record.stage_issue_request(
            &owner,
            context,
            true,
            flow,
            effect_key,
            identity.clone(),
        ) {
            Ok(id) => id,
            Err(problem) => {
                let Some(failure) = GdsIssueFailure::from_validation(problem, kind) else {
                    return Err(HostProblem::InfrastructureFailure);
                };
                return response(service, run, &self::failure(failure, flow));
            }
        };
        if current
            .persist(&mut next, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            match service.flush_issue_control(run, token, effect_key, control_id)? {
                ConversationTransmitOutcome::Confirmed => continue,
                ConversationTransmitOutcome::Pending => return suspended(service, run),
                ConversationTransmitOutcome::Rejected(_) => {
                    return Err(HostProblem::UnknownOutcome);
                }
            }
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn flow(operation: CicsOperation) -> Option<GdsIssueFlow> {
    match operation {
        CicsOperation::GdsIssueAbend => Some(GdsIssueFlow::Abend),
        CicsOperation::GdsIssueConfirmation => Some(GdsIssueFlow::Confirmation),
        CicsOperation::GdsIssueError => Some(GdsIssueFlow::Error),
        CicsOperation::GdsIssuePrepare => Some(GdsIssueFlow::Prepare),
        CicsOperation::GdsIssueSignal => Some(GdsIssueFlow::Signal),
        _ => None,
    }
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP") {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "CONVID" => {
                matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ) && value.bytes().len() == 4
            }
            "CONVDATA" | "RETCODE" | "STATE" | "RESP" | "RESP2" => {
                value.schema() == "mainframe-env.cics.argument@1"
            }
            "OPTION.NOHANDLE" => {
                value.schema() == "mainframe-env.cics.option@1" && value.bytes().is_empty()
            }
            _ => false,
        };
        if !valid {
            return Err(HostProblem::Malformed);
        }
    }
    Ok(())
}

fn select_token(
    ledger: &ConversationLedger,
    owner: &ConversationOwner,
    request: &CicsRequest,
) -> Option<[u8; 4]> {
    if let Some(value) = request.arguments.get("CONVID") {
        return value.bytes().try_into().ok();
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
}

fn mapped_failure(
    service: &CicsService,
    run: &Run,
    flow: GdsIssueFlow,
    kind: ConversationKind,
    problem: ConversationProblem,
) -> Result<CicsResponse, HostProblem> {
    let Some(failure) = GdsIssueFailure::from_problem(problem, kind) else {
        return Err(HostProblem::InfrastructureFailure);
    };
    response(service, run, &self::failure(failure, flow))
}

fn failure(failure: GdsIssueFailure, flow: GdsIssueFlow) -> ConversationReply {
    ConversationReply {
        condition: "NORMAL".into(),
        response: 0,
        response2: 0,
        state: None,
        token: None,
        outputs: BTreeMap::from([("RETCODE".into(), failure.retcode(flow).0.to_vec())]),
    }
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
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.condition != "NORMAL"
        || reply.response != 0
        || reply.response2 != 0
        || reply.outputs.keys().any(|key| {
            !matches!(
                key.as_str(),
                "CONTROL_ID" | "RETCODE" | "CONVDATA" | "STATE"
            )
        })
        || reply
            .outputs
            .get("RETCODE")
            .is_none_or(|code| code.len() != 6)
        || reply
            .outputs
            .get("CONTROL_ID")
            .is_some_and(|id| id.len() != 8)
        || reply
            .outputs
            .get("CONVDATA")
            .is_some_and(|block| block.len() != 24)
        || reply.outputs.get("STATE").is_some_and(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| text.parse::<i32>().ok())
                != reply.state.map(super::ConversationState::cvda)
        })
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
    for (name, bytes) in &reply.outputs {
        let schema = match name.as_str() {
            "CONTROL_ID" => continue,
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
