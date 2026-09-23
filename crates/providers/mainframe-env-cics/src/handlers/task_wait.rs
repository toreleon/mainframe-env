use super::super::{CicsService, Run, field, store_error};
use crate::{CicsEventPostMode, CicsEventPurgeMode};
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const WAIT_NAMESPACE: &str = "cics-task-wait-v1";
const MAX_CAS_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitKind {
    Event,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitState {
    Pending,
    Posted,
    Consumed,
    Purged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WaitEvent {
    index: u32,
    address: [u8; 4],
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WaitRecord {
    kind: WaitKind,
    execution: String,
    run_unit: String,
    session: String,
    events: Vec<WaitEvent>,
    name: Option<String>,
    purgeable: bool,
    state: WaitState,
    selected_index: Option<u32>,
    consumer_effect_key: Option<String>,
    consumer_request_digest: Option<[u8; 32]>,
    version: u64,
}

pub(super) fn invoke(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    if let Some(response2) = invalid_response2(request)? {
        return Err(invreq(response2));
    }
    let (kind, events, selected_index) = request_events(request)?;
    let name = request
        .arguments
        .get("NAME")
        .map(|value| normalize_name(value.bytes()))
        .transpose()?;
    let purgeable = purgeability(request)?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    if let Some(selected_index) = selected_index {
        return completed(service, run, selected_index);
    }
    let key = wait_key(run.invocation.run_unit_id.as_str());
    for _ in 0..MAX_CAS_ATTEMPTS {
        let current = service
            .store
            .get_provider_state(WAIT_NAMESPACE, &key)
            .map_err(store_error)?;
        let Some(current) = current else {
            let record = WaitRecord {
                kind,
                execution: run.invocation.execution_id.as_str().into(),
                run_unit: run.invocation.run_unit_id.as_str().into(),
                session: run.session.clone(),
                events: events.clone(),
                name: name.clone(),
                purgeable,
                state: WaitState::Pending,
                selected_index: None,
                consumer_effect_key: None,
                consumer_request_digest: None,
                version: 1,
            };
            match service.store.put_provider_state(
                ProviderStateRecord {
                    namespace: WAIT_NAMESPACE.into(),
                    key: key.clone(),
                    version: 1,
                    payload: encode(&record)?,
                },
                None,
            ) {
                Ok(()) => return suspended(service, run),
                Err(StoreError::AlreadyExists | StoreError::Conflict) => continue,
                Err(error) => return Err(store_error(error)),
            }
        };
        let mut record = decode(&current.payload, current.version)?;
        validate_identity(&record, run, kind, &events, name.as_deref(), purgeable)?;
        match record.state {
            WaitState::Pending => return suspended(service, run),
            WaitState::Purged => return abended(service, run),
            WaitState::Posted => {
                record.state = WaitState::Consumed;
                record.consumer_effect_key = Some(mutation.idempotency_key.as_str().into());
                record.consumer_request_digest = Some(request_digest);
                record.version = record
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                let selected_index = record
                    .selected_index
                    .ok_or(HostProblem::InfrastructureFailure)?;
                match service.store.put_provider_state(
                    ProviderStateRecord {
                        namespace: WAIT_NAMESPACE.into(),
                        key: key.clone(),
                        version: record.version,
                        payload: encode(&record)?,
                    },
                    Some(current.version),
                ) {
                    Ok(()) => return completed(service, run, selected_index),
                    Err(StoreError::Conflict | StoreError::NotFound) => continue,
                    Err(error) => return Err(store_error(error)),
                }
            }
            WaitState::Consumed => {
                if record.consumer_effect_key.as_deref() != Some(mutation.idempotency_key.as_str())
                    || record.consumer_request_digest != Some(request_digest)
                {
                    return Err(HostProblem::IdempotencyConflict);
                }
                return completed(
                    service,
                    run,
                    record
                        .selected_index
                        .ok_or(HostProblem::InfrastructureFailure)?,
                );
            }
        }
    }
    Err(HostProblem::IdempotencyConflict)
}

impl CicsService {
    /// Post one event owned by the currently suspended task in `session`.
    pub fn post_task_event(
        &self,
        session: &mainframe_env_host_api::SessionId,
        principal: &PrincipalId,
        now_tick: u64,
        event_index: usize,
        mode: CicsEventPostMode,
    ) -> Result<(), HostProblem> {
        let run_unit = self.authorized_wait_run(session, principal, now_tick)?;
        let key = wait_key(&run_unit);
        for _ in 0..MAX_CAS_ATTEMPTS {
            let row = self
                .store
                .get_provider_state(WAIT_NAMESPACE, &key)
                .map_err(store_error)?
                .ok_or(HostProblem::NotFound)?;
            let mut record = decode(&row.payload, row.version)?;
            if record.session != session.as_str() || record.run_unit != run_unit {
                return Err(HostProblem::IdempotencyConflict);
            }
            if mode == CicsEventPostMode::Hand || record.kind == WaitKind::Event && event_index != 0
            {
                return Err(HostProblem::Malformed);
            }
            let selected = u32::try_from(event_index).map_err(|_| HostProblem::Malformed)?;
            if !record.events.iter().any(|event| event.index == selected) {
                return Err(HostProblem::Malformed);
            }
            match record.state {
                WaitState::Posted | WaitState::Consumed => return Ok(()),
                WaitState::Purged => return Err(HostProblem::NotFound),
                WaitState::Pending => {
                    record.state = WaitState::Posted;
                    record.selected_index = Some(selected);
                }
            }
            record.version = record
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: WAIT_NAMESPACE.into(),
                    key: key.clone(),
                    version: record.version,
                    payload: encode(&record)?,
                },
                Some(row.version),
            ) {
                Ok(()) => return Ok(()),
                Err(StoreError::Conflict | StoreError::NotFound) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    /// Apply one timeout or task-purge request to a durable event wait.
    pub fn purge_task_event(
        &self,
        session: &mainframe_env_host_api::SessionId,
        principal: &PrincipalId,
        now_tick: u64,
        mode: CicsEventPurgeMode,
    ) -> Result<bool, HostProblem> {
        let run_unit = self.authorized_wait_run(session, principal, now_tick)?;
        let key = wait_key(&run_unit);
        for _ in 0..MAX_CAS_ATTEMPTS {
            let row = self
                .store
                .get_provider_state(WAIT_NAMESPACE, &key)
                .map_err(store_error)?
                .ok_or(HostProblem::NotFound)?;
            let mut record = decode(&row.payload, row.version)?;
            if record.session != session.as_str() || record.run_unit != run_unit {
                return Err(HostProblem::IdempotencyConflict);
            }
            if record.state != WaitState::Pending {
                return Ok(false);
            }
            if !record.purgeable && mode != CicsEventPurgeMode::ForcePurge {
                return Ok(false);
            }
            record.state = WaitState::Purged;
            record.version = record
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?;
            match self.store.put_provider_state(
                ProviderStateRecord {
                    namespace: WAIT_NAMESPACE.into(),
                    key: key.clone(),
                    version: record.version,
                    payload: encode(&record)?,
                },
                Some(row.version),
            ) {
                Ok(()) => return Ok(true),
                Err(StoreError::Conflict | StoreError::NotFound) => continue,
                Err(error) => return Err(store_error(error)),
            }
        }
        Err(HostProblem::IdempotencyConflict)
    }

    fn authorized_wait_run(
        &self,
        session: &mainframe_env_host_api::SessionId,
        principal: &PrincipalId,
        now_tick: u64,
    ) -> Result<String, HostProblem> {
        let state = self.lock()?;
        let terminal = state
            .sessions
            .get(session.as_str())
            .ok_or(HostProblem::NotFound)?;
        if !terminal.connected || now_tick >= terminal.expires_at_tick {
            return Err(HostProblem::TimedOut);
        }
        let mut matches = state.runs.values().filter(|run| {
            run.session == session.as_str() && run.invocation.principal.id() == principal
        });
        let run_unit = matches
            .next()
            .map(|run| run.invocation.run_unit_id.as_str().to_string())
            .ok_or(HostProblem::Unauthorized)?;
        if matches.next().is_some() {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(run_unit)
    }
}

pub(super) fn release_task(service: &CicsService, run: &Run) -> Result<(), HostProblem> {
    let key = wait_key(run.invocation.run_unit_id.as_str());
    let Some(row) = service
        .store
        .get_provider_state(WAIT_NAMESPACE, &key)
        .map_err(store_error)?
    else {
        return Ok(());
    };
    let record = decode(&row.payload, row.version)?;
    if record.execution != run.invocation.execution_id.as_str()
        || record.run_unit != run.invocation.run_unit_id.as_str()
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    match service
        .store
        .delete_provider_state(WAIT_NAMESPACE, &key, row.version)
    {
        Ok(()) | Err(StoreError::NotFound) => Ok(()),
        Err(error) => Err(store_error(error)),
    }
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    const ALLOWED: &[&str] = &[
        "ECADDR",
        "ECBLIST",
        "EVENT.POSTED",
        "NAME",
        "NUMEVENTS",
        "OPTION.NOHANDLE",
        "OPTION.NOTPURGEABLE",
        "OPTION.PURGEABLE",
        "PURGEABILITY",
        "RESP",
        "RESP2",
    ];
    if !matches!(
        request.operation,
        CicsOperation::WaitEvent | CicsOperation::WaitExternal
    ) || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .keys()
            .any(|name| !ALLOWED.contains(&name.as_str()))
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "ECADDR" | "ECBLIST" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.event-list@1"
                        | "mainframe-env.cics.external-event-list@1"
                        | "mainframe-env.cics.invalid-event-list@1"
                ),
                "EVENT.POSTED" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.event-posted@1" | "mainframe-env.cics.event-posted-index@1"
                ),
                "NAME" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "NUMEVENTS" => value.schema() != "mainframe-env.cics.decimal@1",
                "PURGEABILITY" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1"
                        | "mainframe-env.cics.storage-value@1"
                        | "mainframe-env.cics.decimal@1"
                ),
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                name if name.starts_with("OPTION.") => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        return Err(HostProblem::Malformed);
    }
    let valid_shape = match request.operation {
        CicsOperation::WaitEvent => {
            request.arguments.contains_key("ECADDR")
                && !request.arguments.contains_key("ECBLIST")
                && !request.arguments.contains_key("NUMEVENTS")
                && !request.arguments.contains_key("PURGEABILITY")
                && !request.arguments.contains_key("OPTION.PURGEABLE")
                && !request.arguments.contains_key("OPTION.NOTPURGEABLE")
                && (request.arguments["ECADDR"].schema()
                    == "mainframe-env.cics.invalid-event-list@1"
                    || request.arguments.get("EVENT.POSTED").is_some_and(|value| {
                        value.schema() == "mainframe-env.cics.event-posted@1"
                            && matches!(value.bytes(), [0] | [1])
                    }))
        }
        CicsOperation::WaitExternal => {
            request.arguments.contains_key("ECBLIST")
                && request.arguments.contains_key("NUMEVENTS")
                && !request.arguments.contains_key("ECADDR")
                && (request.arguments["ECBLIST"].schema()
                    == "mainframe-env.cics.invalid-event-list@1"
                    || request.arguments.get("EVENT.POSTED").is_some_and(|value| {
                        value.schema() == "mainframe-env.cics.event-posted-index@1"
                            && matches!(value.bytes().len(), 0 | 4)
                    }))
                && usize::from(request.arguments.contains_key("PURGEABILITY"))
                    + usize::from(request.arguments.contains_key("OPTION.PURGEABLE"))
                    + usize::from(request.arguments.contains_key("OPTION.NOTPURGEABLE"))
                    <= 1
        }
        _ => false,
    };
    valid_shape.then_some(()).ok_or(HostProblem::Malformed)
}

fn invalid_response2(request: &CicsRequest) -> Result<Option<u8>, HostProblem> {
    request
        .arguments
        .get(match request.operation {
            CicsOperation::WaitEvent => "ECADDR",
            CicsOperation::WaitExternal => "ECBLIST",
            _ => return Err(HostProblem::InfrastructureFailure),
        })
        .filter(|value| value.schema() == "mainframe-env.cics.invalid-event-list@1")
        .map(|value| value.bytes().first().copied().ok_or(HostProblem::Malformed))
        .transpose()
}

fn request_events(
    request: &CicsRequest,
) -> Result<(WaitKind, Vec<WaitEvent>, Option<u32>), HostProblem> {
    if request.operation == CicsOperation::WaitEvent {
        let address = request.arguments["ECADDR"]
            .bytes()
            .try_into()
            .map_err(|_| HostProblem::Malformed)?;
        let posted = request.arguments["EVENT.POSTED"].bytes() == [1];
        return Ok((
            WaitKind::Event,
            vec![WaitEvent { index: 0, address }],
            posted.then_some(0),
        ));
    }
    let count = decimal_i64(request.arguments["NUMEVENTS"].bytes())?;
    if count <= 0 {
        return Err(invreq(3));
    }
    let count = u32::try_from(count).map_err(|_| invreq(3))?;
    let bytes = request.arguments["ECBLIST"].bytes();
    if bytes.is_empty() || bytes.len() % 8 != 0 {
        return Err(HostProblem::Malformed);
    }
    let mut indices = BTreeSet::new();
    let mut events = Vec::with_capacity(bytes.len() / 8);
    for entry in bytes.chunks_exact(8) {
        let index = u32::from_be_bytes(entry[..4].try_into().expect("four-byte chunk"));
        let address = entry[4..].try_into().expect("four-byte chunk");
        if index >= count || !indices.insert(index) {
            return Err(HostProblem::Malformed);
        }
        events.push(WaitEvent { index, address });
    }
    let selected_index = match request.arguments["EVENT.POSTED"].bytes() {
        [] => None,
        bytes => {
            let index = u32::from_be_bytes(bytes.try_into().map_err(|_| HostProblem::Malformed)?);
            Some(
                events
                    .iter()
                    .any(|event| event.index == index)
                    .then_some(index)
                    .ok_or(HostProblem::Malformed)?,
            )
        }
    };
    Ok((WaitKind::External, events, selected_index))
}

fn purgeability(request: &CicsRequest) -> Result<bool, HostProblem> {
    if request.arguments.contains_key("OPTION.NOTPURGEABLE") {
        return Ok(false);
    }
    if request.arguments.contains_key("OPTION.PURGEABLE") {
        return Ok(true);
    }
    let Some(value) = request.arguments.get("PURGEABILITY") else {
        return Ok(true);
    };
    let value = std::str::from_utf8(value.bytes())
        .map_err(|_| invreq(4))?
        .trim()
        .to_ascii_uppercase();
    match value.as_str() {
        "PURGEABLE" => Ok(true),
        "NOTPURGEABLE" => Ok(false),
        _ => Err(invreq(4)),
    }
}

fn validate_identity(
    record: &WaitRecord,
    run: &Run,
    kind: WaitKind,
    events: &[WaitEvent],
    name: Option<&str>,
    purgeable: bool,
) -> Result<(), HostProblem> {
    if record.execution != run.invocation.execution_id.as_str()
        || record.run_unit != run.invocation.run_unit_id.as_str()
        || record.session != run.session
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    if record.kind != kind
        || record.events != events
        || record.name.as_deref() != name
        || record.purgeable != purgeable
    {
        return if kind == WaitKind::External {
            Err(invreq(1))
        } else {
            Err(HostProblem::IdempotencyConflict)
        };
    }
    Ok(())
}

fn normalize_name(bytes: &[u8]) -> Result<String, HostProblem> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| HostProblem::Malformed)?
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if !matches!(value.len(), 1..=8) || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(HostProblem::Malformed);
    }
    Ok(value)
}

fn decimal_i64(bytes: &[u8]) -> Result<i64, HostProblem> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or(HostProblem::Malformed)
}

fn invreq(response2: u8) -> HostProblem {
    HostProblem::Condition {
        name: "INVREQ".into(),
        response: 16,
        response2: i32::from(response2),
    }
}

fn wait_key(run_unit: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            [
                b"mainframe-env.cics-task-wait@1\0".as_slice(),
                run_unit.as_bytes()
            ]
            .concat()
        )
    )
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

fn completed(
    service: &CicsService,
    run: &Run,
    selected_index: u32,
) -> Result<CicsResponse, HostProblem> {
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
    response.outputs.insert(
        "EVENT.POSTED".into(),
        BoundedPayload::new(
            "mainframe-env.cics.event-index@1",
            selected_index.to_string().into_bytes(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(response)
}

fn abended(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
    service.response(
        run,
        CicsDisposition::Abended,
        "AEXY",
        0,
        0,
        None,
        None,
        Vec::new(),
    )
}

fn encode(record: &WaitRecord) -> Result<Vec<u8>, HostProblem> {
    let selected_required = matches!(record.state, WaitState::Posted | WaitState::Consumed);
    let consumer_required = record.state == WaitState::Consumed;
    let indices = record
        .events
        .iter()
        .map(|event| event.index)
        .collect::<BTreeSet<_>>();
    if record.version == 0
        || record.execution.is_empty()
        || record.run_unit.is_empty()
        || record.session.is_empty()
        || record.events.is_empty()
        || indices.len() != record.events.len()
        || record.selected_index.is_some() != selected_required
        || record
            .selected_index
            .is_some_and(|index| !indices.contains(&index))
        || record.consumer_effect_key.is_some() != record.consumer_request_digest.is_some()
        || record.consumer_effect_key.is_some() != consumer_required
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let count = u32::try_from(record.events.len()).map_err(|_| HostProblem::ResourceExhausted)?;
    let mut out = b"MECW2".to_vec();
    out.push(match record.kind {
        WaitKind::Event => 0,
        WaitKind::External => 1,
    });
    field(&mut out, record.execution.as_bytes())?;
    field(&mut out, record.run_unit.as_bytes())?;
    field(&mut out, record.session.as_bytes())?;
    out.extend_from_slice(&count.to_be_bytes());
    for event in &record.events {
        out.extend_from_slice(&event.index.to_be_bytes());
        out.extend_from_slice(&event.address);
    }
    field(&mut out, record.name.as_deref().unwrap_or("").as_bytes())?;
    out.push(u8::from(record.purgeable));
    out.push(match record.state {
        WaitState::Pending => 0,
        WaitState::Posted => 1,
        WaitState::Consumed => 2,
        WaitState::Purged => 3,
    });
    match record.selected_index {
        Some(index) => {
            out.push(1);
            out.extend_from_slice(&index.to_be_bytes());
        }
        None => out.push(0),
    }
    field(
        &mut out,
        record
            .consumer_effect_key
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    )?;
    match record.consumer_request_digest {
        Some(digest) => {
            out.push(1);
            out.extend_from_slice(&digest);
        }
        None => out.push(0),
    }
    Ok(out)
}

fn decode(bytes: &[u8], version: u64) -> Result<WaitRecord, HostProblem> {
    if bytes.starts_with(b"MECW1") {
        return decode_v1(bytes, version);
    }
    let mut reader = WaitReader::new(bytes);
    if reader.take(5)? != b"MECW2" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let kind = match reader.byte()? {
        0 => WaitKind::Event,
        1 => WaitKind::External,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let execution = reader.text(256)?;
    let run_unit = reader.text(256)?;
    let session = reader.text(256)?;
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
    if count == 0 || count > 1_024 {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        events.push(WaitEvent {
            index: reader.u32()?,
            address: reader
                .take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        });
    }
    let name = match reader.text(8)? {
        value if value.is_empty() => None,
        value => Some(value),
    };
    let purgeable = match reader.byte()? {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let state = decode_state(reader.byte()?)?;
    let selected_index = match reader.byte()? {
        0 => None,
        1 => Some(reader.u32()?),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let consumer_effect_key = match reader.text(256)? {
        value if value.is_empty() => None,
        value => Some(value),
    };
    let consumer_request_digest = reader.optional_digest()?;
    if !reader.done() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record = WaitRecord {
        kind,
        execution,
        run_unit,
        session,
        events,
        name,
        purgeable,
        state,
        selected_index,
        consumer_effect_key,
        consumer_request_digest,
        version,
    };
    encode(&record)?;
    Ok(record)
}

fn decode_v1(bytes: &[u8], version: u64) -> Result<WaitRecord, HostProblem> {
    let mut reader = WaitReader::new(bytes);
    if reader.take(5)? != b"MECW1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let execution = reader.text(256)?;
    let run_unit = reader.text(256)?;
    let session = reader.text(256)?;
    let address = reader
        .take(4)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let name = match reader.text(8)? {
        value if value.is_empty() => None,
        value => Some(value),
    };
    let state = decode_state(reader.byte()?)?;
    let consumer_effect_key = match reader.text(256)? {
        value if value.is_empty() => None,
        value => Some(value),
    };
    let consumer_request_digest = reader.optional_digest()?;
    if !reader.done() {
        return Err(HostProblem::InfrastructureFailure);
    }
    let record = WaitRecord {
        kind: WaitKind::Event,
        execution,
        run_unit,
        session,
        events: vec![WaitEvent { index: 0, address }],
        name,
        purgeable: true,
        state,
        selected_index: matches!(state, WaitState::Posted | WaitState::Consumed).then_some(0),
        consumer_effect_key,
        consumer_request_digest,
        version,
    };
    encode(&record)?;
    Ok(record)
}

fn decode_state(value: u8) -> Result<WaitState, HostProblem> {
    match value {
        0 => Ok(WaitState::Pending),
        1 => Ok(WaitState::Posted),
        2 => Ok(WaitState::Consumed),
        3 => Ok(WaitState::Purged),
        _ => Err(HostProblem::InfrastructureFailure),
    }
}

struct WaitReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> WaitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, amount: usize) -> Result<&'a [u8], HostProblem> {
        let end = self
            .at
            .checked_add(amount)
            .ok_or(HostProblem::InfrastructureFailure)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(HostProblem::InfrastructureFailure)?;
        self.at = end;
        Ok(value)
    }

    fn field(&mut self, max: usize) -> Result<Vec<u8>, HostProblem> {
        let amount =
            usize::try_from(self.u32()?).map_err(|_| HostProblem::InfrastructureFailure)?;
        if amount > max {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(amount)?.to_vec())
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, HostProblem> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn text(&mut self, max: usize) -> Result<String, HostProblem> {
        String::from_utf8(self.field(max)?).map_err(|_| HostProblem::InfrastructureFailure)
    }

    fn optional_digest(&mut self) -> Result<Option<[u8; 32]>, HostProblem> {
        match self.byte()? {
            0 => Ok(None),
            1 => self
                .take(32)?
                .try_into()
                .map(Some)
                .map_err(|_| HostProblem::InfrastructureFailure),
            _ => Err(HostProblem::InfrastructureFailure),
        }
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}
