use super::super::{CicsService, Run, field, store_error};
use crate::CicsEventPostMode;
use mainframe_env_execution_api::{BoundedPayload, InvocationLimits, PrincipalId};
use mainframe_env_host_api::{
    CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem, HostRequest,
    canonical_request_digest,
};
use mainframe_env_store_api::{ProviderStateRecord, StoreError};
use sha2::{Digest, Sha256};

const WAIT_NAMESPACE: &str = "cics-task-wait-v1";
const MAX_CAS_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitState {
    Pending,
    Posted,
    Consumed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WaitRecord {
    execution: String,
    run_unit: String,
    session: String,
    event: [u8; 4],
    name: Option<String>,
    state: WaitState,
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
    let event = request
        .arguments
        .get("ECADDR")
        .ok_or(HostProblem::Malformed)?;
    if event.schema() == "mainframe-env.cics.invalid-event-list@1" {
        let response2 = event
            .bytes()
            .first()
            .copied()
            .ok_or(HostProblem::Malformed)?;
        return Err(HostProblem::Condition {
            name: "INVREQ".into(),
            response: 16,
            response2: i32::from(response2),
        });
    }
    let event: [u8; 4] = event
        .bytes()
        .try_into()
        .map_err(|_| HostProblem::Malformed)?;
    let posted = request
        .arguments
        .get("EVENT.POSTED")
        .and_then(|value| value.bytes().first())
        .copied()
        .ok_or(HostProblem::Malformed)?
        == 1;
    let name = request
        .arguments
        .get("NAME")
        .map(|value| normalize_name(value.bytes()))
        .transpose()?;
    let request_digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    if posted {
        return completed(service, run);
    }
    let key = wait_key(run.invocation.run_unit_id.as_str());
    for _ in 0..MAX_CAS_ATTEMPTS {
        let current = service
            .store
            .get_provider_state(WAIT_NAMESPACE, &key)
            .map_err(store_error)?;
        let Some(current) = current else {
            let record = WaitRecord {
                execution: run.invocation.execution_id.as_str().into(),
                run_unit: run.invocation.run_unit_id.as_str().into(),
                session: run.session.clone(),
                event,
                name: name.clone(),
                state: WaitState::Pending,
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
        validate_identity(&record, run, event, name.as_deref())?;
        match record.state {
            WaitState::Pending => return suspended(service, run),
            WaitState::Posted => {
                record.state = WaitState::Consumed;
                record.consumer_effect_key = Some(mutation.idempotency_key.as_str().into());
                record.consumer_request_digest = Some(request_digest);
                record.version = record
                    .version
                    .checked_add(1)
                    .ok_or(HostProblem::ResourceExhausted)?;
                match service.store.put_provider_state(
                    ProviderStateRecord {
                        namespace: WAIT_NAMESPACE.into(),
                        key: key.clone(),
                        version: record.version,
                        payload: encode(&record)?,
                    },
                    Some(current.version),
                ) {
                    Ok(()) => return completed(service, run),
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
                return completed(service, run);
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
        if event_index != 0 || mode != CicsEventPostMode::Standard {
            return Err(HostProblem::Malformed);
        }
        let run_unit = {
            let state = self.lock()?;
            let terminal = state
                .sessions
                .get(session.as_str())
                .ok_or(HostProblem::NotFound)?;
            if !terminal.connected || now_tick >= terminal.expires_at_tick {
                return Err(HostProblem::TimedOut);
            }
            state
                .runs
                .values()
                .find(|run| {
                    run.session == session.as_str() && run.invocation.principal.id() == principal
                })
                .map(|run| run.invocation.run_unit_id.as_str().to_string())
                .ok_or(HostProblem::Unauthorized)?
        };
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
            match record.state {
                WaitState::Posted | WaitState::Consumed => return Ok(()),
                WaitState::Pending => record.state = WaitState::Posted,
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
        "EVENT.POSTED",
        "NAME",
        "OPTION.NOHANDLE",
        "RESP",
        "RESP2",
    ];
    let invalid_event = request
        .arguments
        .get("ECADDR")
        .is_some_and(|value| value.schema() == "mainframe-env.cics.invalid-event-list@1");
    if request.operation != CicsOperation::WaitEvent
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || !request.arguments.contains_key("ECADDR")
        || !invalid_event && !request.arguments.contains_key("EVENT.POSTED")
        || request.arguments.iter().any(|(name, value)| {
            !ALLOWED.contains(&name.as_str())
                || match name.as_str() {
                    "ECADDR" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.event-list@1"
                            | "mainframe-env.cics.invalid-event-list@1"
                    ),
                    "EVENT.POSTED" => {
                        value.schema() != "mainframe-env.cics.event-posted@1"
                            || !matches!(value.bytes(), [0] | [1])
                    }
                    "NAME" => !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ),
                    "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                    "OPTION.NOHANDLE" => {
                        value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                    }
                    _ => true,
                }
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn validate_identity(
    record: &WaitRecord,
    run: &Run,
    event: [u8; 4],
    name: Option<&str>,
) -> Result<(), HostProblem> {
    if record.execution != run.invocation.execution_id.as_str()
        || record.run_unit != run.invocation.run_unit_id.as_str()
        || record.session != run.session
        || record.event != event
        || record.name.as_deref() != name
    {
        return Err(HostProblem::IdempotencyConflict);
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

fn completed(service: &CicsService, run: &Run) -> Result<CicsResponse, HostProblem> {
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
            b"0".to_vec(),
            InvocationLimits::default(),
        )
        .map_err(|_| HostProblem::ResourceExhausted)?,
    );
    Ok(response)
}

fn encode(record: &WaitRecord) -> Result<Vec<u8>, HostProblem> {
    if record.version == 0
        || record.execution.is_empty()
        || record.run_unit.is_empty()
        || record.session.is_empty()
        || record.consumer_effect_key.is_some() != record.consumer_request_digest.is_some()
        || (record.state == WaitState::Consumed && record.consumer_effect_key.is_none())
        || (record.state != WaitState::Consumed && record.consumer_effect_key.is_some())
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut out = b"MECW1".to_vec();
    field(&mut out, record.execution.as_bytes())?;
    field(&mut out, record.run_unit.as_bytes())?;
    field(&mut out, record.session.as_bytes())?;
    out.extend_from_slice(&record.event);
    field(&mut out, record.name.as_deref().unwrap_or("").as_bytes())?;
    out.push(match record.state {
        WaitState::Pending => 0,
        WaitState::Posted => 1,
        WaitState::Consumed => 2,
    });
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
    let mut reader = WaitReader::new(bytes);
    if reader.take(5)? != b"MECW1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let execution = reader.text(256)?;
    let run_unit = reader.text(256)?;
    let session = reader.text(256)?;
    let event = reader
        .take(4)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let name = match reader.text(8)? {
        value if value.is_empty() => None,
        value => Some(value),
    };
    let state = match reader.byte()? {
        0 => WaitState::Pending,
        1 => WaitState::Posted,
        2 => WaitState::Consumed,
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
        execution,
        run_unit,
        session,
        event,
        name,
        state,
        consumer_effect_key,
        consumer_request_digest,
        version,
    };
    encode(&record)?;
    Ok(record)
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
        let amount = usize::try_from(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
        .map_err(|_| HostProblem::InfrastructureFailure)?;
        if amount > max {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(amount)?.to_vec())
    }

    fn byte(&mut self) -> Result<u8, HostProblem> {
        Ok(self.take(1)?[0])
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
