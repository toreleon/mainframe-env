//! Source-bounded WRITE OPERATOR dispatch and durable reply timeout work.

use super::{CicsService, active, authority};
use crate::service::{Run, bounded, store_error};
use mainframe_env_execution_api::{ArtifactRef, ExecutionId, InvocationLimits, Selector};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
};
use mainframe_env_store_api::{StoreError, WorkRecord, WorkState};

/// Work generation for a pending operator reply's finite timeout.
pub const CICS_OPERATOR_WORK_GENERATION: &str = "cics-operator-reply-v1";
const DEFAULT_OPERTIM_SECONDS: u64 = 30;

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let text = request.arguments["TEXT"].bytes();
    let with_reply = request.arguments.contains_key("REPLY");
    let maximum_text = if with_reply { 121 } else { 690 };
    let text_length = decimal(request, "TEXTLENGTH")?
        .unwrap_or(i64::try_from(text.len()).map_err(|_| HostProblem::ResourceExhausted)?);
    if text_length < 0 || text_length as usize > maximum_text || text_length as usize > text.len() {
        return Err(condition("INVREQ", 16, 1));
    }
    let text = text[..text_length as usize].to_vec();
    let console = console_name(request)?;
    let routes = route_codes(request, console.is_some())?;
    let action = action_code(request)?;
    let maximum_reply = if with_reply {
        let maximum = decimal(request, "MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
        let area = decimal(request, "REPLY.MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
        if !(1..=119).contains(&maximum) || maximum > area {
            return Err(condition("INVREQ", 16, 4));
        }
        Some(usize::try_from(maximum).map_err(|_| HostProblem::ResourceExhausted)?)
    } else {
        None
    };
    let timeout = if with_reply {
        let seconds = decimal(request, "TIMEOUT")?.unwrap_or(DEFAULT_OPERTIM_SECONDS as i64);
        if !(0..=86_400).contains(&seconds) {
            return Err(condition("INVREQ", 16, 5));
        }
        Some(u64::try_from(seconds).map_err(|_| HostProblem::ResourceExhausted)?)
    } else {
        None
    };
    let resource = format!("CONSOLE.{}", console.as_deref().unwrap_or("ROUTED"));
    service.authorize(run, "FACILITY", &resource, AccessIntent::Execute)?;
    let now_tick = if with_reply {
        service
            .replay_clock
            .as_ref()
            .ok_or(HostProblem::InfrastructureFailure)?
            .now_tick()?
    } else {
        0
    };
    if with_reply && now_tick == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    if with_reply && service.work_store.is_none() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let deadline_tick = timeout
        .map(|seconds| {
            now_tick
                .checked_add(
                    seconds
                        .checked_mul(1_000)
                        .ok_or(HostProblem::ResourceExhausted)?,
                )
                .ok_or(HostProblem::ResourceExhausted)
        })
        .transpose()?;
    let record = authority::OperatorMessage::new(
        run.invocation.execution_id.as_str(),
        run.invocation.run_unit_id.as_str(),
        run.invocation.principal.id().as_str(),
        text,
        console,
        routes,
        action,
        maximum_reply,
        deadline_tick,
        mutation.idempotency_key.as_str(),
        digest,
    )?;
    if !with_reply {
        authority::put(service.store.as_ref(), &record)?;
        return response(service, run, request, &record);
    }
    let id = operator_id(request, run)?;
    let mut semantic = request.clone();
    semantic.mutation = None;
    let semantic_digest = canonical_request_digest(&HostRequest::Cics(semantic))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    for _ in 0..8 {
        let current = active::read(service.store.as_ref(), &id)?;
        if let Some(current) = &current
            && (current.waiting() || current.consumed_by(mutation.idempotency_key.as_str(), digest))
        {
            if current.semantic_digest != semantic_digest {
                return Err(HostProblem::IdempotencyConflict);
            }
            let retained = authority::read(service.store.as_ref(), &current.message_key)?
                .ok_or(HostProblem::InfrastructureFailure)?;
            if retained.state == authority::MessageState::Waiting {
                enqueue_timeout(service, &retained, run.invocation.priority)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
                return response(service, run, request, &retained);
            }
            if current.waiting() {
                active::consume(
                    service.store.as_ref(),
                    &id,
                    &current.message_key,
                    mutation.idempotency_key.as_str(),
                    digest,
                )?;
            }
            return response(service, run, request, &retained);
        }
        let next = active::ActiveOperator::new(&id, &record.key, semantic_digest);
        let (_, pointer) = active::staged_replace(next, current.as_ref())?;
        match service
            .store
            .put_provider_states_atomic(vec![authority::staged_put(&record)?, pointer])
        {
            Ok(()) => {
                enqueue_timeout(service, &record, run.invocation.priority)
                    .map_err(|_| HostProblem::UnknownOutcome)?;
                return response(service, run, request, &record);
            }
            Err(StoreError::AlreadyExists | StoreError::Conflict | StoreError::NotFound) => {
                continue;
            }
            Err(error) => return Err(store_error(error)),
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

fn operator_id(request: &CicsRequest, run: &Run) -> Result<String, HostProblem> {
    let value = request
        .arguments
        .get("OPERATOR.ID")
        .ok_or(HostProblem::Malformed)?;
    if value.schema() != "mainframe-env.cics.operator-id@1" {
        return Err(HostProblem::Malformed);
    }
    let text = std::str::from_utf8(value.bytes()).map_err(|_| HostProblem::Malformed)?;
    let suffix = text
        .strip_prefix(run.invocation.run_unit_id.as_str())
        .and_then(|value| value.strip_prefix(':'));
    if text.len() > 256
        || !suffix.is_some_and(|position| {
            !position.is_empty() && position.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(text.into())
}

fn response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    record: &authority::OperatorMessage,
) -> Result<CicsResponse, HostProblem> {
    match record.state {
        authority::MessageState::Complete => normal(service, run),
        authority::MessageState::Waiting => service.response(
            run,
            CicsDisposition::Suspended,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        ),
        authority::MessageState::Expired => Err(condition("EXPIRED", 31, 7)),
        authority::MessageState::Replied => {
            let reply = record
                .reply
                .as_deref()
                .ok_or(HostProblem::InfrastructureFailure)?;
            let maximum = record
                .maximum_reply
                .ok_or(HostProblem::InfrastructureFailure)?;
            let mut result = if reply.len() > maximum {
                super::super::condition(
                    service,
                    run,
                    &request.condition_policy,
                    condition("LENGERR", 22, 8),
                )?
            } else {
                normal(service, run)?
            };
            result.outputs.insert(
                "REPLY".into(),
                bounded(reply[..reply.len().min(maximum)].to_vec())?,
            );
            if request.arguments.contains_key("REPLYLENGTH") {
                result.outputs.insert(
                    "REPLYLENGTH".into(),
                    mainframe_env_execution_api::BoundedPayload::new(
                        "mainframe-env.cics.decimal@1",
                        reply.len().to_string().into_bytes(),
                        InvocationLimits::default(),
                    )
                    .map_err(|_| HostProblem::ResourceExhausted)?,
                );
            }
            Ok(result)
        }
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

fn decimal(request: &CicsRequest, name: &str) -> Result<Option<i64>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            if value.schema() != "mainframe-env.cics.decimal@1" {
                return Err(HostProblem::Malformed);
            }
            std::str::from_utf8(value.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<i64>()
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

fn console_name(request: &CicsRequest) -> Result<Option<String>, HostProblem> {
    let Some(value) = request.arguments.get("CONSNAME") else {
        return Ok(None);
    };
    let raw = std::str::from_utf8(value.bytes()).map_err(|_| condition("INVREQ", 16, 8))?;
    let name = raw.trim_end().to_ascii_uppercase();
    if !(2..=8).contains(&name.len()) {
        return Err(condition("INVREQ", 16, 7));
    }
    if !name.bytes().all(|byte| {
        byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'@' | b'#' | b'$')
    }) {
        return Err(condition("INVREQ", 16, 8));
    }
    Ok(Some(name))
}

fn route_codes(request: &CicsRequest, specific_console: bool) -> Result<Vec<u8>, HostProblem> {
    if specific_console {
        return Ok(Vec::new());
    }
    let Some(value) = request.arguments.get("ROUTECODES") else {
        return Ok(vec![2]);
    };
    let count = decimal(request, "NUMROUTES")?
        .unwrap_or(i64::try_from(value.bytes().len()).map_err(|_| HostProblem::ResourceExhausted)?);
    if !(1..=28).contains(&count) || count as usize > value.bytes().len() {
        return Err(condition("INVREQ", 16, 2));
    }
    let codes = value.bytes()[..count as usize].to_vec();
    if codes.iter().any(|code| !(1..=28).contains(code)) {
        return Err(condition("INVREQ", 16, 3));
    }
    Ok(codes)
}

fn action_code(request: &CicsRequest) -> Result<Option<u8>, HostProblem> {
    let flags = [
        ("OPTION.IMMEDIATE", 2),
        ("OPTION.EVENTUAL", 3),
        ("OPTION.CRITICAL", 11),
    ]
    .into_iter()
    .filter(|(name, _)| request.arguments.contains_key(*name))
    .map(|(_, code)| code)
    .collect::<Vec<_>>();
    if flags.len() > 1 || !flags.is_empty() && request.arguments.contains_key("ACTION") {
        return Err(HostProblem::Malformed);
    }
    let action = match decimal(request, "ACTION")? {
        Some(code) if matches!(code, 2 | 3 | 11) => Some(code as u8),
        Some(_) => return Err(condition("INVREQ", 16, 6)),
        None => flags.into_iter().next(),
    };
    Ok(action)
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "TEXT",
        "TEXTLENGTH",
        "ROUTECODES",
        "NUMROUTES",
        "CONSNAME",
        "ACTION",
        "REPLY",
        "REPLY.MAXLENGTH",
        "MAXLENGTH",
        "REPLYLENGTH",
        "TIMEOUT",
        "RESP",
        "RESP2",
        "OPTION.IMMEDIATE",
        "OPTION.EVENTUAL",
        "OPTION.CRITICAL",
        "OPTION.NOHANDLE",
        "OPERATOR.ID",
    ];
    if request.operation != CicsOperation::WriteOperator
        || !request.arguments.contains_key("TEXT")
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request.arguments.contains_key("CONSNAME")
            && request.arguments.contains_key("ROUTECODES")
        || request.arguments.contains_key("NUMROUTES")
            && !request.arguments.contains_key("ROUTECODES")
        || request.arguments.contains_key("REPLY") != request.arguments.contains_key("MAXLENGTH")
        || request.arguments.contains_key("REPLY")
            != request.arguments.contains_key("REPLY.MAXLENGTH")
        || !request.arguments.contains_key("REPLY")
            && (request.arguments.contains_key("TIMEOUT")
                || request.arguments.contains_key("REPLYLENGTH"))
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "TEXTLENGTH" | "NUMROUTES" | "ACTION" | "MAXLENGTH" | "REPLY.MAXLENGTH"
                    | "TIMEOUT" => value.schema() != "mainframe-env.cics.decimal@1",
                    "TEXT" | "ROUTECODES" | "CONSNAME" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
                    "OPTION.IMMEDIATE" | "OPTION.EVENTUAL" | "OPTION.CRITICAL"
                    | "OPTION.NOHANDLE" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    "REPLY" | "REPLYLENGTH" | "RESP" | "RESP2" => {
                        value.schema() != "mainframe-env.cics.argument@1"
                    }
                    "OPERATOR.ID" => value.schema() != "mainframe-env.cics.operator-id@1",
                    _ => true,
                }
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn enqueue_timeout(
    service: &CicsService,
    record: &authority::OperatorMessage,
    priority: u8,
) -> Result<(), HostProblem> {
    let store = service
        .work_store
        .as_ref()
        .ok_or(HostProblem::InfrastructureFailure)?;
    let deadline = record
        .deadline_tick
        .ok_or(HostProblem::InfrastructureFailure)?;
    let limits = InvocationLimits::default();
    let work = WorkRecord {
        work_id: record.key.clone(),
        execution_id: ExecutionId::new(format!("cics-operator-{}", &record.key[14..38]), limits)
            .map_err(|_| HostProblem::ResourceExhausted)?,
        required_selector: Selector::new("cics:operator-reply", limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        required_generation: CICS_OPERATOR_WORK_GENERATION.into(),
        artifact: ArtifactRef::new("artifact:none", limits)
            .map_err(|_| HostProblem::InfrastructureFailure)?,
        state: WorkState::Queued,
        priority,
        attempt: 0,
        max_attempts: 3,
        available_tick: deadline,
        deadline_tick: deadline
            .checked_add(86_400_000)
            .ok_or(HostProblem::ResourceExhausted)?,
        cancellation_requested: false,
        worker_id: None,
        lease_id: None,
        lease_epoch: 0,
        lease_expiry_tick: None,
        heartbeat_tick: None,
        terminal_tick: None,
        checkpoint_id: None,
        effect_sequence: 0,
        payload: format!("{}:0", record.run_unit).into_bytes(),
    };
    match store.enqueue(work.clone()) {
        Ok(()) => Ok(()),
        Err(StoreError::AlreadyExists | StoreError::Conflict)
            if store
                .get_work(&work.work_id)
                .map_err(store_error)?
                .as_ref()
                .is_some_and(|current| {
                    current.required_generation == work.required_generation
                        && current.required_selector == work.required_selector
                        && current.available_tick == work.available_tick
                        && current.payload == work.payload
                }) =>
        {
            Ok(())
        }
        Err(error) => Err(store_error(error)),
    }
}

impl CicsService {
    /// Promote a claimed reply timeout and report whether its online task should wake.
    pub fn promote_operator_timeout_work(
        &self,
        work: &WorkRecord,
        now_tick: u64,
    ) -> Result<bool, HostProblem> {
        if work.required_generation != CICS_OPERATOR_WORK_GENERATION
            || work.required_selector.as_str() != "cics:operator-reply"
            || work.artifact.as_str() != "artifact:none"
            || work.state != WorkState::Claimed
            || work.lease_id.is_none()
            || work.lease_epoch == 0
            || now_tick < work.available_tick
        {
            return Err(HostProblem::Malformed);
        }
        let record = authority::read(self.store.as_ref(), &work.work_id)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        if record.deadline_tick != Some(work.available_tick)
            || work.payload != format!("{}:0", record.run_unit).as_bytes()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        let expired = authority::expire(self.store.as_ref(), &record.key, now_tick)?;
        Ok(matches!(
            expired.state,
            authority::MessageState::Replied | authority::MessageState::Expired
        ) && active::pending_for_message(self.store.as_ref(), &record.key, self.limits)?)
    }
}
