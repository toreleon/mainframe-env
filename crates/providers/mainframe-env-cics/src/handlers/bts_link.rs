//! Synchronous BTS LINK over the shared process/activity CAS authority.

use super::super::{CicsService, Run, store_error};
use super::bts_lifecycle::{BtsLifecycleStore, BtsMode, BtsReply};
use super::event_control::{self, EventKind};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits, RunUnitId};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, HostResult, ProgramLinkSelection, ProgramName, ProgramRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::ProviderStateRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CONTEXT_NS: &str = "cics-bts-link-context-v2";
const FRAME_NS: &str = "cics-bts-link-frame-v1";
const MAX_CONTEXT_BYTES: usize = 1024;
const MAX_LINK_DEPTH: usize = 16;

/// Task-local reference to an active BTS activity, supplied by the shared RUN authority.
/// Acquired targets are always resolved from its versioned UOW acquisition row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsBtsLinkContext {
    /// Opaque identity of the activity currently activated by this task.
    pub active_activity_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredLinkContext {
    active_activity_id: Option<String>,
    owner_execution: String,
    owner_principal: String,
}

impl CicsService {
    /// Bind an active activity ID for LINK ACTIVITY child-name selection.
    pub fn bind_bts_link_context(
        &self,
        run_unit: &RunUnitId,
        context: CicsBtsLinkContext,
    ) -> Result<(), HostProblem> {
        let (owner_execution, owner_principal) = {
            let state = self.lock()?;
            let run = state.runs.get(run_unit).ok_or(HostProblem::Unauthorized)?;
            (
                run.invocation.execution_id.as_str().to_string(),
                run.invocation.principal.id().as_str().to_string(),
            )
        };
        if let Some(id) = &context.active_activity_id {
            let authority = BtsLifecycleStore::new(self.store.as_ref());
            let index = authority
                .load_activity_index(id)?
                .ok_or(HostProblem::NotFound)?;
            let process = authority
                .load_process(&index.process_type, &index.process_name)?
                .ok_or(HostProblem::NotFound)?;
            if !process.visible_to(run_unit.as_str())
                || process
                    .activities
                    .get(id)
                    .is_none_or(|activity| activity.mode != BtsMode::Active)
            {
                return Err(HostProblem::Unauthorized);
            }
        }
        let payload = serde_json::to_vec(&StoredLinkContext {
            active_activity_id: context.active_activity_id,
            owner_execution,
            owner_principal,
        })
        .map_err(|_| HostProblem::ResourceExhausted)?;
        if payload.len() > MAX_CONTEXT_BYTES {
            return Err(HostProblem::ResourceExhausted);
        }
        if let Some(row) = self
            .store
            .get_provider_state(CONTEXT_NS, run_unit.as_str())
            .map_err(store_error)?
        {
            return if row.version == 1 && row.payload == payload {
                Ok(())
            } else {
                Err(HostProblem::IdempotencyConflict)
            };
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: CONTEXT_NS.into(),
                    key: run_unit.as_str().into(),
                    version: 1,
                    payload,
                },
                None,
            )
            .map_err(store_error)
    }
}

#[derive(Clone)]
struct Target {
    process_type: String,
    process_name: String,
    activity_id: String,
    process_link: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkFrame {
    owner_execution: String,
    owner_principal: String,
    process_type: String,
    process_name: String,
    activity_id: String,
    activation_epoch: u64,
}

fn frames(
    service: &CicsService,
    run_unit: &str,
) -> Result<(Vec<LinkFrame>, Option<u64>), HostProblem> {
    let row = service
        .store
        .get_provider_state(FRAME_NS, run_unit)
        .map_err(store_error)?;
    let Some(row) = row else {
        return Ok((Vec::new(), None));
    };
    if row.payload.len() > 4096 || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let frames: Vec<LinkFrame> =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if frames.is_empty() || frames.len() > MAX_LINK_DEPTH {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok((frames, Some(row.version)))
}

fn push_frame(
    service: &CicsService,
    run: &Run,
    target: &Target,
    epoch: u64,
) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    let (mut stack, prior) = frames(service, key)?;
    if stack.len() == MAX_LINK_DEPTH {
        return Err(HostProblem::ResourceExhausted);
    }
    stack.push(LinkFrame {
        owner_execution: run.invocation.execution_id.as_str().into(),
        owner_principal: run.invocation.principal.id().as_str().into(),
        process_type: target.process_type.clone(),
        process_name: target.process_name.clone(),
        activity_id: target.activity_id.clone(),
        activation_epoch: epoch,
    });
    let payload = serde_json::to_vec(&stack).map_err(|_| HostProblem::ResourceExhausted)?;
    if payload.len() > 4096 {
        return Err(HostProblem::ResourceExhausted);
    }
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: FRAME_NS.into(),
                key: key.into(),
                version: prior.unwrap_or(0) + 1,
                payload,
            },
            prior,
        )
        .map_err(store_error)
}

fn pop_frame(service: &CicsService, run: &Run, target: &Target) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    let (mut stack, prior) = frames(service, key)?;
    let Some(last) = stack.last() else {
        return Ok(());
    };
    if last.owner_execution != run.invocation.execution_id.as_str()
        || last.owner_principal != run.invocation.principal.id().as_str()
        || last.activity_id != target.activity_id
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    stack.pop();
    let version = prior.ok_or(HostProblem::InfrastructureFailure)?;
    if stack.is_empty() {
        service
            .store
            .delete_provider_state(FRAME_NS, key, version)
            .map_err(store_error)
    } else {
        let payload = serde_json::to_vec(&stack).map_err(|_| HostProblem::ResourceExhausted)?;
        service
            .store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: FRAME_NS.into(),
                    key: key.into(),
                    version: version + 1,
                    payload,
                },
                Some(version),
            )
            .map_err(store_error)
    }
}

/// Resolve a selected child execution to its active lifecycle identity.
pub(super) fn nested_activity_scope(
    service: &CicsService,
    run: &Run,
) -> Result<Option<String>, HostProblem> {
    let Some(parent) = run.invocation.parent_execution_id.as_ref() else {
        return Ok(None);
    };
    let (stack, _) = frames(service, run.invocation.run_unit_id.as_str())?;
    let Some(frame) = stack
        .iter()
        .rev()
        .find(|frame| frame.owner_execution == parent.as_str())
    else {
        return Ok(None);
    };
    if frame.owner_principal != run.invocation.principal.id().as_str() {
        return Err(HostProblem::IdempotencyConflict);
    }
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let process = authority
        .load_process(&frame.process_type, &frame.process_name)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    if !process.visible_to(run.invocation.run_unit_id.as_str())
        || process
            .activities
            .get(&frame.activity_id)
            .is_none_or(|activity| {
                activity.mode != BtsMode::Active
                    || activity.activation_epoch != frame.activation_epoch
            })
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Some(frame.activity_id.clone()))
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
    retention_tick: u64,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    super::bts_live(service, run, retention_tick)?;
    let authority = BtsLifecycleStore::new(service.store.as_ref());
    let active = active_activity(service, run)?;
    let target = resolve_target(&authority, run, request, active.as_deref())?;
    let input = request
        .arguments
        .get("INPUTEVENT")
        .map(|_| argument_name(request, "INPUTEVENT"))
        .transpose()?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let intent_key = replay_key("intent", mutation.idempotency_key.as_str());
    let done_key = replay_key("done", mutation.idempotency_key.as_str());
    let process = authority
        .load_process(&target.process_type, &target.process_name)?
        .ok_or_else(|| target_error(&target, 8))?;
    if !process.visible_to(run.invocation.run_unit_id.as_str()) {
        return Err(target_error(&target, 8));
    }
    if let Some(replay) = process.replays.get(&done_key) {
        check_replay(replay, run, digest)?;
        pop_frame(service, run, &target)?;
        return normal(service, run);
    }
    if let Some(replay) = process.replays.get(&intent_key) {
        check_replay(replay, run, digest)?;
        return Err(HostProblem::UnknownOutcome);
    }
    let activity = process
        .activities
        .get(&target.activity_id)
        .ok_or(HostProblem::InfrastructureFailure)?;
    if activity.suspended {
        return Err(condition(
            "INVREQ",
            16,
            if target.process_link { 23 } else { 21 },
        ));
    }
    if activity.mode == BtsMode::Active {
        return Err(if target.process_link {
            condition("PROCESSBUSY", 106, 13)
        } else {
            condition("LOCKED", 100, 0)
        });
    }
    if !matches!(activity.mode, BtsMode::Initial | BtsMode::Dormant)
        || activity.mode == BtsMode::Initial && input.is_some()
        || activity.mode == BtsMode::Dormant && input.is_none()
    {
        return Err(target_error(&target, 14));
    }
    if let Some(name) = &input {
        let events = event_control::load_activity(service, &target.activity_id)?;
        if !events
            .events
            .get(name)
            .is_some_and(|event| matches!(event.kind, EventKind::Input) && !event.fired)
        {
            return Err(condition("EVENTERR", 111, 7));
        }
    }
    let resource = if target.process_link {
        format!(
            "CICS.BTS.PROCESS.{}.{}",
            target.process_type, target.process_name
        )
    } else {
        format!("CICS.BTS.ACTIVITY.{}", target.activity_id)
    };
    service
        .authorize(
            run,
            if target.process_link {
                "BTSPROCESS"
            } else {
                "BTSACTIVITY"
            },
            &resource,
            AccessIntent::Execute,
        )
        .map_err(|problem| {
            if problem == HostProblem::Unauthorized {
                condition("NOTAUTH", 70, 101)
            } else {
                problem
            }
        })?;
    let program = activity.program.clone();
    service
        .authorize(
            run,
            "FACILITY",
            &format!("CICS.PROGRAM.{program}"),
            AccessIntent::Execute,
        )
        .map_err(|problem| {
            if problem == HostProblem::Unauthorized {
                condition("NOTAUTH", 70, 101)
            } else {
                problem
            }
        })?;
    let definitions = service.lock()?.program_definitions.get(&program).cloned();
    let installed = definitions
        .as_ref()
        .and_then(|generations| generations.iter().next_back())
        .map(|(_, definition)| definition)
        .ok_or_else(|| condition("PGMIDERR", 27, 1))?;
    if installed.remote {
        return Err(condition("INVREQ", 16, 40));
    }
    if !installed.enabled {
        return Err(condition("PGMIDERR", 27, 2));
    }
    let selected = ProgramLinkSelection {
        artifact: installed.artifact.clone(),
        generation: installed.generation,
        content_identity: installed.artifact.as_str().into(),
    };
    let owner_run = run.invocation.run_unit_id.as_str().to_string();
    let owner_execution = run.invocation.execution_id.as_str().to_string();
    let owner_principal = run.invocation.principal.id().as_str().to_string();
    let intent = authority.mutate_process(
        &target.process_type,
        &target.process_name,
        &owner_run,
        &owner_execution,
        &owner_principal,
        &intent_key,
        digest,
        |process| {
            let activity = process
                .activities
                .get_mut(&target.activity_id)
                .ok_or(HostProblem::InfrastructureFailure)?;
            if !matches!(activity.mode, BtsMode::Initial | BtsMode::Dormant) || activity.suspended {
                return Err(if target.process_link {
                    condition("PROCESSBUSY", 106, 13)
                } else {
                    condition("LOCKED", 100, 0)
                });
            }
            activity.mode = BtsMode::Active;
            activity.activation_epoch = activity
                .activation_epoch
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            let mut reply = BtsReply::normal();
            reply.outputs.insert(
                "ACTIVATION.EPOCH".into(),
                activity.activation_epoch.to_be_bytes().to_vec(),
            );
            Ok(reply)
        },
    )?;
    let epoch: [u8; 8] = intent
        .outputs
        .get("ACTIVATION.EPOCH")
        .and_then(|bytes| bytes.as_slice().try_into().ok())
        .ok_or(HostProblem::InfrastructureFailure)?;
    push_frame(service, run, &target, u64::from_be_bytes(epoch))
        .map_err(|_| HostProblem::UnknownOutcome)?;
    if let Some(name) = &input {
        service
            .post_input_event(&target.activity_id, name)
            .map_err(|_| HostProblem::UnknownOutcome)?;
    }
    let name = ProgramName::new(program, 128).map_err(|_| HostProblem::Malformed)?;
    let payload = BoundedPayload::new(
        "mainframe-env.cics.channel@1",
        Vec::new(),
        InvocationLimits::default(),
    )
    .map_err(|_| HostProblem::ResourceExhausted)?;
    match service.nested(
        run,
        HostRequest::Program(ProgramRequest::Link {
            program: name,
            payload,
            selection: Some(selected),
        }),
    ) {
        Ok(HostResult::Program(_)) => {}
        _ => return Err(HostProblem::UnknownOutcome),
    }
    super::bts_live(service, run, retention_tick).map_err(|_| HostProblem::UnknownOutcome)?;
    authority
        .mutate_process(
            &target.process_type,
            &target.process_name,
            &owner_run,
            &owner_execution,
            &owner_principal,
            &done_key,
            digest,
            |process| {
                let activity = process
                    .activities
                    .get_mut(&target.activity_id)
                    .ok_or(HostProblem::InfrastructureFailure)?;
                if activity.mode != BtsMode::Active {
                    return Err(HostProblem::UnknownOutcome);
                }
                activity.mode = BtsMode::Dormant;
                Ok(BtsReply::normal())
            },
        )
        .map_err(|_| HostProblem::UnknownOutcome)?;
    pop_frame(service, run, &target).map_err(|_| HostProblem::UnknownOutcome)?;
    normal(service, run)
}

fn resolve_target(
    authority: &BtsLifecycleStore<'_>,
    run: &Run,
    request: &CicsRequest,
    active: Option<&str>,
) -> Result<Target, HostProblem> {
    let run_unit = run.invocation.run_unit_id.as_str();
    match request.operation {
        CicsOperation::LinkActivity => {
            let active = active.ok_or_else(|| condition("INVREQ", 16, 4))?;
            let name = argument_name(request, "ACTIVITY")?;
            let index = authority
                .load_activity_index(active)?
                .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?;
            let process = authority
                .load_process(&index.process_type, &index.process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if !process.visible_to(run_unit) {
                return Err(condition("ACTIVITYERR", 109, 8));
            }
            let child = process
                .child(active, &name)
                .ok_or_else(|| condition("ACTIVITYERR", 109, 8))?;
            let child_index = authority
                .load_activity_index(&child.id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if child_index.process_type != index.process_type
                || child_index.process_name != index.process_name
                || child_index.parent_id.as_deref() != Some(active)
                || child_index
                    .pending_uow
                    .as_deref()
                    .is_some_and(|owner| owner != run_unit)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(Target {
                process_type: index.process_type,
                process_name: index.process_name,
                activity_id: child.id.clone(),
                process_link: false,
            })
        }
        CicsOperation::LinkAcqActivity | CicsOperation::LinkAcqProcess => {
            if active.is_some() {
                return Err(if request.operation == CicsOperation::LinkAcqProcess {
                    condition("PROCESSERR", 108, 6)
                } else {
                    condition("INVREQ", 16, 4)
                });
            }
            let acquisition = authority
                .load_acquisition(run_unit)?
                .ok_or_else(|| missing_acquisition(request.operation))?;
            if acquisition.owner_execution != run.invocation.execution_id.as_str()
                || acquisition.owner_principal != run.invocation.principal.id().as_str()
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            let id = acquisition
                .activity_id
                .ok_or_else(|| missing_acquisition(request.operation))?;
            let index = authority
                .load_activity_index(&id)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let process = authority
                .load_process(&index.process_type, &index.process_name)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            let process_link = request.operation == CicsOperation::LinkAcqProcess;
            if process_link != (id == process.root_id) {
                return Err(missing_acquisition(request.operation));
            }
            if acquisition.process_type.as_deref() != Some(index.process_type.as_str())
                || acquisition.process_name.as_deref() != Some(index.process_name.as_str())
                || process
                    .activities
                    .get(&id)
                    .is_none_or(|activity| activity.acquired_by.as_deref() != Some(run_unit))
            {
                return Err(HostProblem::IdempotencyConflict);
            }
            Ok(Target {
                process_type: index.process_type,
                process_name: index.process_name,
                activity_id: id,
                process_link,
            })
        }
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

fn active_activity(service: &CicsService, run: &Run) -> Result<Option<String>, HostProblem> {
    if let Some(activity) = nested_activity_scope(service, run)? {
        return Ok(Some(activity));
    }
    let row = service
        .store
        .get_provider_state(CONTEXT_NS, run.invocation.run_unit_id.as_str())
        .map_err(store_error)?;
    let Some(row) = row else { return Ok(None) };
    if row.version != 1 || row.payload.len() > MAX_CONTEXT_BYTES {
        return Err(HostProblem::InfrastructureFailure);
    }
    let context: StoredLinkContext =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if context.owner_execution != run.invocation.execution_id.as_str()
        || context.owner_principal != run.invocation.principal.id().as_str()
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    if let Some(id) = &context.active_activity_id {
        let authority = BtsLifecycleStore::new(service.store.as_ref());
        let index = authority
            .load_activity_index(id)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let process = authority
            .load_process(&index.process_type, &index.process_name)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if !process.visible_to(run.invocation.run_unit_id.as_str())
            || process
                .activities
                .get(id)
                .is_none_or(|activity| activity.mode != BtsMode::Active)
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(context.active_activity_id)
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    if super::selected_link_return(run) {
        return Ok(());
    }
    let key = run.invocation.run_unit_id.as_str();
    if let Some(row) = service
        .store
        .get_provider_state(CONTEXT_NS, key)
        .map_err(store_error)?
    {
        service
            .store
            .delete_provider_state(CONTEXT_NS, key, row.version)
            .map_err(store_error)?;
    }
    if let Some(row) = service
        .store
        .get_provider_state(FRAME_NS, key)
        .map_err(store_error)?
    {
        service
            .store
            .delete_provider_state(FRAME_NS, key, row.version)
            .map_err(store_error)?;
    }
    Ok(())
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let allowed = match request.operation {
        CicsOperation::LinkActivity => {
            &["ACTIVITY", "INPUTEVENT", "RESP", "RESP2", "OPTION.NOHANDLE"][..]
        }
        CicsOperation::LinkAcqActivity => &[
            "INPUTEVENT",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
            "OPTION.ACQACTIVITY",
        ][..],
        CicsOperation::LinkAcqProcess => &[
            "INPUTEVENT",
            "RESP",
            "RESP2",
            "OPTION.NOHANDLE",
            "OPTION.ACQPROCESS",
        ][..],
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if request.arguments.iter().any(|(name, value)| {
        !allowed.contains(&name.as_str())
            || if name.starts_with("OPTION.") {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            } else if name == "RESP" || name == "RESP2" {
                value.schema() != "mainframe-env.cics.argument@1"
            } else {
                !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                        | "mainframe-env.cics.argument@1"
                )
            }
    }) || request.operation == CicsOperation::LinkActivity
        && !request.arguments.contains_key("ACTIVITY")
        || request.operation == CicsOperation::LinkAcqActivity
            && !request.arguments.contains_key("OPTION.ACQACTIVITY")
        || request.operation == CicsOperation::LinkAcqProcess
            && !request.arguments.contains_key("OPTION.ACQPROCESS")
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn argument_name(request: &CicsRequest, name: &str) -> Result<String, HostProblem> {
    let value = request.arguments.get(name).ok_or(HostProblem::Malformed)?;
    let name = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim_end_matches(' ');
    if name.is_empty()
        || name.len() > 16
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'$' | b'@' | b'#' | b'.' | b'-' | b'_')
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name.into())
}

fn check_replay(
    replay: &super::bts_lifecycle::BtsReplay,
    run: &Run,
    digest: [u8; 32],
) -> Result<(), HostProblem> {
    if replay.owner_run_unit != run.invocation.run_unit_id.as_str()
        || replay.owner_execution != run.invocation.execution_id.as_str()
        || replay.owner_principal != run.invocation.principal.id().as_str()
        || replay.request_digest != digest
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}
fn replay_key(phase: &str, key: &str) -> String {
    format!("bts-link-{phase}-{:x}", Sha256::digest(key.as_bytes()))
}
fn missing_acquisition(operation: CicsOperation) -> HostProblem {
    condition(
        "INVREQ",
        16,
        if operation == CicsOperation::LinkAcqProcess {
            15
        } else {
            24
        },
    )
}
fn target_error(target: &Target, code: i32) -> HostProblem {
    if target.process_link {
        condition("PROCESSERR", 108, if code == 8 { 9 } else { code })
    } else {
        condition("ACTIVITYERR", 109, code)
    }
}
fn normal(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
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
fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}
