//! Bind selected source requests to actual completed CICS actors and UOW rows.
use super::*;
use mainframe_env_cics::{
    CicsLimits, CicsReplayCodecVersion, CicsReplayRetentionState, CicsTraceEntry,
    CicsUowCodecVersion, CicsUowDependencyState, CicsUowRowDescriptor, CicsUowState,
    describe_cics_replay_row, describe_cics_uow_row,
};
use mainframe_env_execution_api::{
    AuditDecision, AuditResourceDigestFormat, LifecycleEvent, LifecycleEventKind,
};
use mainframe_env_host_api::{
    HostRequest, Mutation, canonical_audit_resource_digest, canonical_request_digest,
};
use mainframe_env_store_api::{EffectRecord, ExecutionRecord, ExecutionState, ProviderStateRecord};

const MAX_ROWS: usize = 512;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const REPLAY: &str = "cics-effect-replay-v1";
const UOW: &str = "cics-uow";

pub(super) struct Before {
    session: String,
    replay: BTreeMap<String, ProviderStateRecord>,
    uow: BTreeMap<String, ProviderStateRecord>,
    trace: Vec<CicsTraceEntry>,
}
#[derive(Clone, Copy)]
pub(super) enum Stage {
    Rewrite,
    Commit,
}

fn rows(
    store: &dyn PlatformStore,
    namespace: &str,
) -> Result<BTreeMap<String, ProviderStateRecord>, CorpusProblem> {
    let rows = store
        .list_provider_state_bounded(namespace, MAX_ROWS + 1, MAX_BYTES)
        .map_err(|_| refused())?;
    require(rows.len() <= MAX_ROWS)?;
    let count = rows.len();
    let result = rows
        .into_iter()
        .map(|row| (row.key.clone(), row))
        .collect::<BTreeMap<_, _>>();
    require(result.len() == count)?;
    Ok(result)
}
pub(super) fn before(context: &Context<'_>, session: &str) -> Result<Before, CorpusProblem> {
    let trace = context
        .server
        .online_trace(session)
        .map_err(|_| refused())?;
    require(trace.len() <= MAX_ROWS)?;
    Ok(Before {
        session: session.into(),
        replay: rows(context.store, REPLAY)?,
        uow: rows(context.store, UOW)?,
        trace,
    })
}
fn fresh<'a>(
    old: &BTreeMap<String, ProviderStateRecord>,
    new: &'a BTreeMap<String, ProviderStateRecord>,
) -> Result<Vec<&'a ProviderStateRecord>, CorpusProblem> {
    require(old.iter().all(|(key, row)| new.get(key) == Some(row)))?;
    Ok(new
        .iter()
        .filter(|(key, _)| !old.contains_key(*key))
        .map(|(_, row)| row)
        .collect())
}
fn argument(schema: &str, value: &[u8]) -> BoundedPayload {
    BoundedPayload::new(schema, value.to_vec(), InvocationLimits::default())
        .expect("frozen bounded source operand")
}
fn mutation(effect: &EffectRecord) -> Mutation {
    Mutation {
        sequence: effect.sequence,
        idempotency_key: effect.key.clone(),
        transaction: Some("CAUP".into()),
    }
}
pub(super) fn rewrite_request(effect: &EffectRecord, customer: bool) -> HostRequest {
    let mut arguments = BTreeMap::new();
    for (name, value) in [
        (
            "FILE",
            if customer {
                b"CUSTDAT ".as_slice()
            } else {
                b"ACCTDAT ".as_slice()
            },
        ),
        (
            "FROM",
            if customer {
                oracles::CUSTOMER_FROM.as_slice()
            } else {
                oracles::ACCOUNT_FROM.as_slice()
            },
        ),
    ] {
        arguments.insert(
            name.into(),
            argument("mainframe-env.cics.storage-value@1", value),
        );
    }
    arguments.insert(
        "LENGTH".into(),
        argument(
            "mainframe-env.cics.decimal@1",
            if customer { b"500" } else { b"300" },
        ),
    );
    let response = "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-RESP-CD";
    let response2 = "WS-MISC-STORAGE.WS-CICS-PROCESSNG-VARS.WS-REAS-CD";
    for (name, field) in [("RESP", response), ("RESP2", response2)] {
        arguments.insert(
            name.into(),
            argument("mainframe-env.cics.argument@1", field.as_bytes()),
        );
    }
    HostRequest::Cics(CicsRequest {
        operation: CicsOperation::Rewrite,
        arguments,
        condition_policy: CicsConditionPolicy::Respond {
            response_field: response.into(),
            response2_field: Some(response2.into()),
        },
        mutation: Some(mutation(effect)),
    })
}
pub(super) fn commit_request(effect: &EffectRecord) -> HostRequest {
    HostRequest::Cics(CicsRequest {
        operation: CicsOperation::Syncpoint,
        arguments: BTreeMap::new(),
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(mutation(effect)),
    })
}
pub(super) fn same_request(effect: &EffectRecord, request: &HostRequest) -> bool {
    let resource = canonical_audit_resource_digest(request);
    resource.format == AuditResourceDigestFormat::CanonicalHostResourceV1
        && effect.intent.audit_resource == Some(resource)
        && canonical_request_digest(request).is_ok_and(|digest| digest == effect.request_digest)
}
pub(super) fn owner_matches(
    owner: &ExecutionRecord,
    effect: &EffectRecord,
    installed: &BTreeMap<String, ArtifactRef>,
    run: &RunUnitId,
    commit: bool,
) -> bool {
    let Some(program) = owner.selector.as_str().strip_prefix("program:") else {
        return false;
    };
    (program == "COACTUPC" || commit && program == "COMEN01C")
        && installed.get(program) == Some(&owner.artifact)
        && owner.execution_id == effect.execution_id
        && owner.run_unit_id == *run
        && effect.run_unit_id == *run
        && owner.principal.as_str() == "WEBUSER"
        && owner.attempt > 0
        && owner.state == ExecutionState::Completed
        && owner.terminal_tick.is_some_and(|tick| tick > 0)
        && effect.intent.owner == owner.execution_id
        && effect.intent.attempt == owner.attempt
        && effect.intent.capability.as_ref().map(CapabilityId::as_str) == Some("host.cics.execute")
}
pub(super) fn terminal_event_matches(
    owner: &ExecutionRecord,
    event: &LifecycleEvent,
    commit: bool,
) -> bool {
    let kind_matches = if commit && owner.selector.as_str() == "program:COACTUPC" {
        matches!(event.kind, LifecycleEventKind::HandoffCompleted)
    } else {
        matches!(event.kind, LifecycleEventKind::Completed { return_code: 0 })
    };
    event.validate()
        && event.execution_id == owner.execution_id
        && event.run_unit_id == owner.run_unit_id
        && event.attempt == owner.attempt
        && event.sequence == owner.version
        && Some(event.tick) == owner.terminal_tick
        && kind_matches
}
fn check_owner(
    context: &Context<'_>,
    effect: &EffectRecord,
    run: &RunUnitId,
    commit: bool,
) -> Result<ExecutionRecord, CorpusProblem> {
    let owner = context
        .store
        .get_execution(&effect.execution_id)
        .map_err(|_| refused())?
        .ok_or_else(refused)?;
    require(owner_matches(
        &owner,
        effect,
        context.installed,
        run,
        commit,
    ))?;
    let events = context
        .store
        .events(&owner.execution_id, owner.version, 2)
        .map_err(|_| refused())?;
    require(matches!(events.as_slice(), [event] if terminal_event_matches(&owner, event, commit)))?;
    Ok(owner)
}
fn check_audit(
    context: &Context<'_>,
    effect: &EffectRecord,
    owner: &ExecutionRecord,
) -> Result<(), CorpusProblem> {
    let invocation = effect
        .intent
        .audit_invocation_key
        .as_ref()
        .ok_or_else(refused)?;
    let resource = effect.intent.audit_resource.as_ref().ok_or_else(refused)?;
    let audits = context
        .store
        .audit_records(&owner.execution_id, 0, MAX_ROWS + 1)
        .map_err(|_| refused())?;
    require(audits.len() <= MAX_ROWS)?;
    let matching = audits
        .iter()
        .filter(|audit| {
            audit.capability.as_str() == "host.cics.execute"
                && audit.effect_sequence == effect.sequence
        })
        .collect::<Vec<_>>();
    require(
        matches!(matching.as_slice(), [audit] if audit.execution_id == owner.execution_id
        && audit.run_unit_id == owner.run_unit_id && audit.principal == owner.principal
        && audit.attempt == owner.attempt && audit.invocation_key == *invocation
        && audit.resource == *resource && audit.decision == AuditDecision::Success),
    )
}
pub(super) fn require_uow(
    row: &CicsUowRowDescriptor,
    owner: &ExecutionRecord,
    effect: &EffectRecord,
) -> Result<(), CorpusProblem> {
    require(
        row.namespace == UOW
            && matches!(
                row.codec,
                CicsUowCodecVersion::RetentionV2 | CicsUowCodecVersion::ProgramFrameV3
            )
            && row.state == CicsUowState::Committed
            && row.dependency == CicsUowDependencyState::Clear
            && row.row_version == 2
            && row.transaction == "CAUP"
            && row.effect_key.as_deref() == Some(effect.key.as_str())
            && row.key == effect.key.as_str()
            && row.owner_execution.as_deref() == Some(owner.execution_id.as_str())
            && row.owner_run_unit.as_deref() == Some(owner.run_unit_id.as_str())
            && row.deadline_tick.is_some_and(|tick| tick > 0)
            && row
                .terminal_tick
                .zip(row.deadline_tick)
                .is_some_and(|(terminal, deadline)| terminal >= deadline),
    )
}
pub(super) fn trace_matches(trace: &[CicsTraceEntry], stage: Stage) -> Result<(), CorpusProblem> {
    let actual = trace
        .iter()
        .filter(|entry| {
            matches!(
                entry.operation,
                CicsOperation::Read
                    | CicsOperation::Rewrite
                    | CicsOperation::Write
                    | CicsOperation::Delete
                    | CicsOperation::Syncpoint
                    | CicsOperation::Xctl
            )
        })
        .map(|entry| {
            (
                entry.operation,
                entry.outcome.as_str(),
                entry.response,
                entry.response2,
                entry.payload_bytes,
            )
        })
        .collect::<Vec<_>>();
    let expected = match stage {
        Stage::Rewrite => vec![
            (CicsOperation::Read, 300),
            (CicsOperation::Read, 500),
            (CicsOperation::Rewrite, 0),
            (CicsOperation::Rewrite, 0),
        ],
        Stage::Commit => vec![(CicsOperation::Syncpoint, 0), (CicsOperation::Xctl, 160)],
    }
    .into_iter()
    .map(|(operation, bytes)| (operation, "NORMAL", 0, 0, bytes))
    .collect::<Vec<_>>();
    require(actual == expected)
}
// The selected F5 rewrite and PF3 commit both consume supplied input.
// COACTUPC returns normally after F5; PF3 replaces it with COMEN01C,
// whose ordinary RETURN also completes without an empty-input RECEIVE.
pub(super) fn terminal_surface_matches(
    terminal: &serde_json::Value,
    session: &str,
    stage: Stage,
) -> bool {
    let (mapset, map) = match stage {
        Stage::Rewrite => ("COACTUP", "CACTUPA"),
        Stage::Commit => ("COMEN01", "COMEN1A"),
    };
    terminal["session"] == session
        && terminal["principal"] == "WEBUSER"
        && terminal["connected"] == true
        && terminal["suspended"] == false
        && terminal["mapset"] == mapset
        && terminal["map"] == map
}
pub(super) fn verify(
    context: &Context<'_>,
    old: &Before,
    terminal: &serde_json::Value,
    stage: Stage,
) -> Result<(), CorpusProblem> {
    require(terminal_surface_matches(terminal, &old.session, stage))?;
    let run = RunUnitId::new(
        terminal["run_unit"].as_str().ok_or_else(refused)?,
        InvocationLimits::default(),
    )
    .map_err(|_| refused())?;
    let new = before(context, &old.session)?;
    require(new.trace.starts_with(&old.trace))?;
    trace_matches(&new.trace[old.trace.len()..], stage)?;
    let changed = fresh(&old.replay, &new.replay)?;
    require(!changed.is_empty())?;
    let commit = matches!(stage, Stage::Commit);
    let mut offered = Vec::new();
    for row in changed {
        let key =
            IdempotencyKey::new(&row.key, InvocationLimits::default()).map_err(|_| refused())?;
        let effect = context
            .store
            .effect(&key)
            .map_err(|_| refused())?
            .ok_or_else(refused)?;
        let descriptor = describe_cics_replay_row(row, Some(&effect), CicsLimits::default())
            .map_err(|_| refused())?;
        require(
            descriptor.codec == CicsReplayCodecVersion::RetentionV3
                && descriptor.retention == CicsReplayRetentionState::Terminal
                && descriptor.owner_run_unit.as_deref() == Some(run.as_str()),
        )?;
        let owner = check_owner(context, &effect, &run, commit)?;
        if owner.selector.as_str() != "program:COACTUPC" {
            continue;
        }
        let requests = if commit {
            vec![commit_request(&effect)]
        } else {
            vec![
                rewrite_request(&effect, false),
                rewrite_request(&effect, true),
            ]
        };
        for (which, request) in requests.into_iter().enumerate() {
            if same_request(&effect, &request) {
                check_audit(context, &effect, &owner)?;
                offered.push((effect.sequence, which, effect.clone(), owner.clone()));
            }
        }
    }
    offered.sort_by_key(|(sequence, _, _, _)| *sequence);
    require(if commit {
        offered.len() == 1 && offered[0].1 == 0
    } else {
        offered.len() == 2
            && offered[0].1 == 0
            && offered[1].1 == 1
            && offered[0].0 < offered[1].0
            && offered[0].3 == offered[1].3
    })?;
    let fresh_uow = fresh(&old.uow, &new.uow)?;
    if commit {
        require(fresh_uow.len() == 1)?;
        let (_, _, effect, owner) = &offered[0];
        let undo = context
            .store
            .get_provider_state("cics-uow-undo", run.as_str())
            .map_err(|_| refused())?;
        let descriptor =
            describe_cics_uow_row(fresh_uow[0], undo.as_ref()).map_err(|_| refused())?;
        require_uow(&descriptor, owner, effect)?;
        if let Some(task) = descriptor.task_owner_execution {
            let task =
                ExecutionId::new(task, InvocationLimits::default()).map_err(|_| refused())?;
            let task = context
                .store
                .get_execution(&task)
                .map_err(|_| refused())?
                .ok_or_else(refused)?;
            require(task.run_unit_id == run && task.principal.as_str() == "WEBUSER")?;
        }
    } else {
        require(fresh_uow.is_empty())?;
    }
    Ok(())
}
