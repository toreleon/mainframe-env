//! Owner-fenced staging for mapped APPC ISSUE controls.
//!
//! This route is internal while the ISSUE catalog rows are unready. A staged
//! control suspends until the shared carrier confirms partner consumption.

use super::{
    ConversationLedger, ConversationOwner, ConversationProblem, ConversationReplay,
    ConversationReply, ConversationTransmitOutcome, GdsIssueFlow, GdsReturnCode,
    IssueRequestIdentity, IssueValidationProblem, load_conversation_replay,
};
use crate::service::{CicsService, Run, mutation_problem, store_error};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use std::collections::BTreeMap;

const MAX_CAS_RETRIES: usize = 32;
const STATE_SCHEMA: &str = "mainframe-env.cics.cvda@1";

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
        convdata_output: false,
        retcode_output: false,
    };
    identity.validate().map_err(map_problem)?;
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
            .ok_or(HostProblem::InfrastructureFailure)?
    } else {
        select_token(&initial, &owner, request)?
    };
    let record = initial
        .conversation(token)
        .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
    let system = record.system.clone();
    service.authorize(
        run,
        "CONNECTION",
        &format!("CICS.CONNECTION.{system}"),
        AccessIntent::Update,
    )?;
    if let Some(saved) = &saved {
        return complete_response(service, run, &saved.reply);
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
            return complete_response(service, run, reply);
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let mut next = current.clone();
        let record = next
            .conversation_mut(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        if record.system != system {
            return Err(HostProblem::IdempotencyConflict);
        }
        if record.released {
            return Err(condition("NOTALLOC", 61, 0));
        }
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
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
        record
            .stage_issue_request(
                &owner,
                super::context(run)?,
                false,
                flow,
                effect_key,
                identity.clone(),
            )
            .map_err(map_validation)?;
        if current
            .persist(&mut next, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            let control_id = next
                .conversation(token)
                .and_then(|record| record.pending_issue.as_ref())
                .ok_or(HostProblem::InfrastructureFailure)?
                .id;
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
        CicsOperation::IssueAbend => Some(GdsIssueFlow::Abend),
        CicsOperation::IssueConfirmation => Some(GdsIssueFlow::Confirmation),
        CicsOperation::IssueError => Some(GdsIssueFlow::Error),
        CicsOperation::IssuePrepare => Some(GdsIssueFlow::Prepare),
        CicsOperation::IssueSignal => Some(GdsIssueFlow::Signal),
        _ => None,
    }
}

fn validate_shape(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.arguments.contains_key("CONVID") && request.arguments.contains_key("SESSION")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
    {
        return Err(HostProblem::Malformed);
    }
    for (name, value) in &request.arguments {
        let valid = match name.as_str() {
            "CONVID" | "SESSION" => {
                matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ) && value.bytes().len() == 4
            }
            "STATE" | "RESP" | "RESP2" => value.schema() == "mainframe-env.cics.argument@1",
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

fn map_validation(problem: IssueValidationProblem) -> HostProblem {
    match problem {
        IssueValidationProblem::WrongSyncLevel => condition("INVREQ", 16, 0),
        IssueValidationProblem::Protocol(problem) => map_problem(problem),
    }
}

fn map_problem(problem: ConversationProblem) -> HostProblem {
    match problem {
        ConversationProblem::NotOwned => condition("NOTALLOC", 61, 0),
        ConversationProblem::DplPrincipal => condition("INVREQ", 16, 200),
        ConversationProblem::WrongKind | ConversationProblem::WrongState => {
            condition("INVREQ", 16, 0)
        }
        ConversationProblem::StaleOwner => HostProblem::IdempotencyConflict,
        ConversationProblem::Malformed | ConversationProblem::Length => HostProblem::Malformed,
        ConversationProblem::Exhausted => HostProblem::ResourceExhausted,
    }
}

fn complete_response(
    service: &CicsService,
    run: &Run,
    reply: &ConversationReply,
) -> Result<CicsResponse, HostProblem> {
    if reply.condition != "NORMAL"
        || reply.response != 0
        || reply.response2 != 0
        || reply
            .outputs
            .keys()
            .any(|name| !matches!(name.as_str(), "STATE" | "CONTROL_ID"))
        || reply
            .outputs
            .get("CONTROL_ID")
            .is_none_or(|id| id.len() != 8)
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        "NORMAL",
        0,
        0,
        None,
        None,
        Vec::new(),
    )?;
    if let Some(state) = reply.outputs.get("STATE") {
        if reply
            .state
            .is_none_or(|value| state != &super::state_cvda::bytes(value))
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        response.outputs.insert(
            "STATE".into(),
            BoundedPayload::new(STATE_SCHEMA, state.clone(), InvocationLimits::default())
                .map_err(|_| HostProblem::ResourceExhausted)?,
        );
    }
    Ok(response)
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

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

/// Persist the dispatch marker before calling the confirmed APPC carrier.
pub(in crate::service) fn mark_attempted(
    service: &CicsService,
    run: &mut Run,
    token: [u8; 4],
    effect_key: &str,
    id: u64,
) -> Result<(), HostProblem> {
    let owner = owner(run);
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
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
    for _ in 0..MAX_CAS_RETRIES {
        super::deadline(service, run)?;
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let record = current
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        let pending = record
            .pending_issue
            .as_ref()
            .ok_or(HostProblem::IdempotencyConflict)?;
        if pending.effect_key != effect_key || pending.id != id {
            return Err(HostProblem::IdempotencyConflict);
        }
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        if pending.attempted {
            return Ok(());
        }
        let mut next = current.clone();
        next.conversation_mut(token)
            .ok_or(HostProblem::InfrastructureFailure)?
            .mark_issue_attempted(&owner, super::context(run)?, effect_key, id)
            .map_err(map_problem)?;
        if current
            .persist(&mut next, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return Ok(());
        }
    }
    Err(HostProblem::UnknownOutcome)
}

/// Commit one explicitly confirmed control result and its exact final reply.
pub(in crate::service) fn confirm(
    service: &CicsService,
    run: &mut Run,
    token: [u8; 4],
    effect_key: &str,
    id: u64,
) -> Result<(), HostProblem> {
    let owner = owner(run);
    let initial = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
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
    for _ in 0..MAX_CAS_RETRIES {
        if let Some(saved) =
            load_conversation_replay(service.store.as_ref(), effect_key).map_err(store_error)?
        {
            if saved.owner_execution != owner.execution
                || saved.owner_run_unit != owner.run_unit
                || saved.owner_epoch != owner.lease_epoch
                || saved.owner_principal != run.invocation.principal.id().as_str()
                || saved.reply.token != Some(token)
                || saved.reply.outputs.get("CONTROL_ID").map(Vec::as_slice)
                    != Some(id.to_be_bytes().as_slice())
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            return Ok(());
        }
        let current = ConversationLedger::load(service.store.as_ref()).map_err(store_error)?;
        let record = current
            .conversation(token)
            .ok_or_else(|| condition("NOTALLOC", 61, 0))?;
        record
            .check_owner(&owner, super::context(run)?)
            .map_err(map_problem)?;
        let pending = record
            .pending_issue
            .as_ref()
            .ok_or(HostProblem::IdempotencyConflict)?;
        if pending.effect_key != effect_key || pending.id != id || !pending.attempted {
            return Err(HostProblem::IdempotencyConflict);
        }
        let request = pending
            .request
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?;
        request.validate().map_err(map_problem)?;
        if request.principal != run.invocation.principal.id().as_str()
            || (request.convdata_output || request.retcode_output)
                && record.kind != super::ConversationKind::AppcBasic
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut next = current.clone();
        let changed = next
            .conversation_mut(token)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let state = changed
            .confirm_issue(&owner, super::context(run)?, effect_key, id)
            .map_err(map_problem)?;
        let basic = changed.kind == super::ConversationKind::AppcBasic;
        let mut outputs = BTreeMap::from([("CONTROL_ID".into(), id.to_be_bytes().to_vec())]);
        if request.state_output {
            outputs.insert(
                "STATE".into(),
                if basic {
                    state.cvda().to_string().into_bytes()
                } else {
                    super::state_cvda::bytes(state)
                },
            );
        }
        if request.convdata_output {
            outputs.insert(
                "CONVDATA".into(),
                changed.gds_convdata(false).map_err(map_problem)?.to_vec(),
            );
        }
        if request.retcode_output {
            outputs.insert("RETCODE".into(), GdsReturnCode::NORMAL.0.to_vec());
        }
        let reply = ConversationReply {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            state: Some(state),
            token: Some(token),
            outputs,
        };
        let replay = ConversationReplay {
            schema_version: 1,
            effect_key: effect_key.into(),
            owner_execution: owner.execution.clone(),
            owner_run_unit: owner.run_unit.clone(),
            owner_principal: request.principal.clone(),
            owner_epoch: owner.lease_epoch,
            mutation_sequence: request.mutation_sequence,
            request_digest: request.digest,
            deadline_tick: request.deadline_tick,
            retain_until_tick: request.retain_until_tick,
            reply,
        };
        if current
            .persist_with_replay(&mut next, &replay, service.store.as_ref())
            .map_err(|error| mutation_problem(store_error(error)))?
        {
            if super::deadline(service, run).is_err() {
                return Err(HostProblem::UnknownOutcome);
            }
            return Ok(());
        }
    }
    Err(HostProblem::UnknownOutcome)
}

fn owner(run: &Run) -> ConversationOwner {
    ConversationOwner {
        execution: run.invocation.execution_id.as_str().into(),
        run_unit: run.invocation.run_unit_id.as_str().into(),
        lease_epoch: u64::from(run.invocation.attempt),
    }
}
