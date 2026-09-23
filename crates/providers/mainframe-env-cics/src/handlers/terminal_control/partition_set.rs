//! Durable partition-set definitions and task associations for BMS terminals.

use super::super::super::{
    CicsService, CicsTerminalSnapshot, Run, Session, bounded, decimal_payload, mutation_problem,
    terminal_snapshot,
};
use super::super::{field, store_error};
use mainframe_env_execution_api::{PrincipalId, RunUnitId};
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, SessionId, canonical_request_digest,
};
use mainframe_env_store_api::{
    ProviderStateMutation, ProviderStateRecord, ProviderStateWrite, StoreError,
};
use std::collections::BTreeSet;

const DEFINITION_NAMESPACE: &str = "cics-terminal-partition-set-v1";
const ASSOCIATION_NAMESPACE: &str = "cics-terminal-partition-association-v1";
const RECEIPT_NAMESPACE: &str = "cics-terminal-partition-receipt-v1";
const DEFINITION_MAGIC: &[u8; 8] = b"MECPS001";
const ASSOCIATION_MAGIC: &[u8; 8] = b"MECPA001";
const RECEIPT_MAGIC: &[u8; 8] = b"MECPR001";
const INPUT_MAGIC: &[u8; 8] = b"MECPI001";
const RECEIVE_RECEIPT_NAMESPACE: &str = "cics-terminal-partition-input-receipt-v1";
const RECEIVE_RECEIPT_MAGIC: &[u8; 8] = b"MECPR002";
const MAX_ATTEMPTS: usize = 8;

/// One rectangular 8775 partition in a registered application partition set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsPartitionDefinition {
    /// One- or two-character partition name.
    pub name: String,
    /// Zero-based first terminal row.
    pub top: u16,
    /// Zero-based first terminal column.
    pub left: u16,
    /// Positive partition height.
    pub rows: u16,
    /// Positive partition width.
    pub columns: u16,
}

/// Immutable BMS partition-set resource admitted for SEND PARTNSET.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsPartitionSetDefinition {
    /// One- to eight-character CICS resource name.
    pub name: String,
    /// Nonoverlapping terminal partitions.
    pub partitions: Vec<CicsPartitionDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Association {
    pub(super) name: Option<String>,
    pub(super) session_version: u64,
    pub(super) logical_message_active: bool,
    pub(super) intervening_send: bool,
    pub(super) received_once: bool,
    version: u64,
}

impl CicsService {
    /// Register immutable partition-set definitions for local BMS tasks.
    pub fn register_partition_sets(
        &self,
        definitions: &[CicsPartitionSetDefinition],
    ) -> Result<(), HostProblem> {
        if definitions.is_empty() || definitions.len() > self.limits.max_maps {
            return Err(HostProblem::Malformed);
        }
        let mut normalized = Vec::with_capacity(definitions.len());
        let mut supplied = BTreeSet::new();
        for definition in definitions {
            let definition = normalized_definition(definition, self.limits.max_fields)?;
            if !supplied.insert(definition.name.clone()) {
                return Err(HostProblem::Malformed);
            }
            normalized.push(definition);
        }
        let existing = self
            .store
            .list_provider_state(DEFINITION_NAMESPACE, self.limits.max_maps)
            .map_err(store_error)?;
        let additions = normalized
            .iter()
            .filter(|definition| !existing.iter().any(|row| row.key == definition.name))
            .count();
        if existing.len().saturating_add(additions) > self.limits.max_maps {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut writes = Vec::new();
        for definition in &normalized {
            let payload = encode_definition(definition)?;
            if let Some(row) = existing.iter().find(|row| row.key == definition.name) {
                if row.version != 1 || row.payload != payload {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DEFINITION_NAMESPACE.into(),
                    key: definition.name.clone(),
                    version: 1,
                    payload,
                },
                expected_version: None,
            });
        }
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        Ok(())
    }

    /// Read the task's associated partition set; `None` means base state.
    pub fn partition_set_for_run(
        &self,
        run_unit: &RunUnitId,
    ) -> Result<Option<String>, HostProblem> {
        Ok(read_association(self, run_unit.as_str())?.and_then(|value| value.name))
    }

    /// Submit bounded data from a named partition on the owning terminal.
    pub fn submit_partition_input(
        &self,
        session: &SessionId,
        principal: &PrincipalId,
        csrf_token: &str,
        aid: u8,
        partition: &str,
        data: &[u8],
        cursor: u16,
        now_tick: u64,
    ) -> Result<CicsTerminalSnapshot, HostProblem> {
        if !super::valid_aid(aid) || data.len() > self.limits.max_screen_bytes {
            return Err(HostProblem::Malformed);
        }
        let current = self.public_session(session, principal, Some(csrf_token), now_tick)?;
        let partition = normalized_name(partition.as_bytes(), 2)?;
        let association = read_association(self, &current.run_unit)?
            .and_then(|value| value.name)
            .ok_or_else(|| condition("INVPARTN", 65, 0))?;
        let row = self
            .store
            .get_provider_state(DEFINITION_NAMESPACE, &association)
            .map_err(store_error)?
            .ok_or(HostProblem::InfrastructureFailure)?;
        let definition = decode_definition(&row)?;
        let selected = definition
            .partitions
            .iter()
            .find(|candidate| candidate.name == partition)
            .ok_or_else(|| condition("INVPARTN", 65, 0))?;
        if cursor >= selected.rows.saturating_mul(selected.columns) {
            return Err(HostProblem::Malformed);
        }
        let mut encoded = INPUT_MAGIC.to_vec();
        field(&mut encoded, partition.as_bytes())?;
        field(&mut encoded, data)?;
        encoded.extend_from_slice(&cursor.to_be_bytes());
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.expires_at_tick = now_tick
            .checked_add(next.idle_timeout_ticks)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.aid = aid;
        next.input.replace(encoded)?;
        next.suspended = false;
        let mut state = self.lock()?;
        if state
            .sessions
            .get(session.as_str())
            .is_none_or(|value| value.version != current.version)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        self.persist_session(session.as_str(), &next, Some(current.version))?;
        state.sessions.insert(session.as_str().into(), next.clone());
        Ok(terminal_snapshot(session.as_str(), &next))
    }
}

pub(super) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_request(request)?;
    super::validate_purge_message_context(run)?;
    let name = request
        .arguments
        .get("PARTNSET")
        .map(|value| normalized_name(value.bytes(), 8))
        .transpose()?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let owner = run.invocation.run_unit_id.as_str().to_owned();
    if let Some(receipt) = service
        .store
        .get_provider_state(RECEIPT_NAMESPACE, key)
        .map_err(store_error)?
    {
        validate_receipt(&receipt, &owner, name.as_deref(), digest)?;
        return normal(service, run);
    }
    if super::bms::message_active(service, &run.session)? {
        return Err(invreq(0));
    }
    let session = service
        .lock()?
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or_else(|| invreq(0))?;
    if let Some(name) = &name {
        let row = service
            .store
            .get_provider_state(DEFINITION_NAMESPACE, name)
            .map_err(store_error)?
            .ok_or_else(|| condition("INVPARTNSET", 64, 0))?;
        let definition = decode_definition(&row)?;
        if definition.name != *name
            || definition.partitions.iter().any(|partition| {
                partition
                    .top
                    .checked_add(partition.rows)
                    .is_none_or(|end| end > session.rows)
                    || partition
                        .left
                        .checked_add(partition.columns)
                        .is_none_or(|end| end > session.columns)
            })
        {
            return Err(condition("INVPARTNSET", 64, 0));
        }
    }
    service.authorize(
        run,
        "FACILITY",
        &format!(
            "CICS.TERMINAL.PARTNSET.{}",
            name.as_deref().unwrap_or("BASE")
        ),
        AccessIntent::Update,
    )?;
    for _ in 0..MAX_ATTEMPTS {
        let current = read_association(service, &owner)?;
        if current
            .as_ref()
            .is_some_and(|value| value.logical_message_active)
        {
            return Err(invreq(0));
        }
        let next = Association {
            name: name.clone(),
            session_version: session.version,
            logical_message_active: false,
            intervening_send: false,
            received_once: false,
            version: current
                .as_ref()
                .map(|value| {
                    value
                        .version
                        .checked_add(1)
                        .ok_or(HostProblem::ResourceExhausted)
                })
                .transpose()?
                .unwrap_or(1),
        };
        let mutations = vec![
            ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: ASSOCIATION_NAMESPACE.into(),
                    key: owner.clone(),
                    version: next.version,
                    payload: encode_association(&next)?,
                },
                expected_version: current.map(|value| value.version),
            }),
            ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: RECEIPT_NAMESPACE.into(),
                    key: key.into(),
                    version: 1,
                    payload: encode_receipt(&owner, name.as_deref(), digest)?,
                },
                expected_version: None,
            }),
        ];
        match service.store.mutate_provider_states_atomic(mutations) {
            Ok(()) => return normal(service, run),
            Err(StoreError::Conflict | StoreError::AlreadyExists) => {
                if let Some(receipt) = service
                    .store
                    .get_provider_state(RECEIPT_NAMESPACE, key)
                    .map_err(store_error)?
                {
                    validate_receipt(&receipt, &owner, name.as_deref(), digest)?;
                    return normal(service, run);
                }
            }
            Err(error) => return Err(mutation_problem(store_error(error))),
        }
    }
    Err(HostProblem::UnknownOutcome)
}

struct ReceivedPartition {
    partition: String,
    data: Vec<u8>,
    cursor: u16,
    aid: u8,
}

pub(super) fn invoke_receive(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    validate_receive_request(request)?;
    super::validate_purge_message_context(run)?;
    let mutation = request
        .mutation
        .as_ref()
        .ok_or(HostProblem::MissingIdempotency)?;
    let key = mutation.idempotency_key.as_str();
    let digest = canonical_request_digest(&HostRequest::Cics(request.clone()))
        .map_err(|_| HostProblem::ResourceExhausted)?;
    let owner = run.invocation.run_unit_id.as_str().to_owned();
    if let Some(row) = service
        .store
        .get_provider_state(RECEIVE_RECEIPT_NAMESPACE, key)
        .map_err(store_error)?
    {
        let received =
            decode_receive_receipt(&row, &owner, digest, service.limits.max_screen_bytes)?;
        return receive_response(service, run, request, &received);
    }
    let current = service
        .lock()?
        .sessions
        .get(&run.session)
        .cloned()
        .ok_or_else(|| invreq(0))?;
    require_intervening_send(service, run)?;
    let association = read_association(service, &owner)?
        .filter(|value| value.name.is_some())
        .ok_or_else(|| condition("INVPARTN", 65, 0))?;
    let Some(input) = &current.input.payload else {
        service.authorize(
            run,
            "FACILITY",
            &format!(
                "CICS.TERMINAL.PARTN.{}",
                association.name.as_deref().unwrap_or("BASE")
            ),
            AccessIntent::Read,
        )?;
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
    };
    let mut received = decode_partition_input(input, current.aid, service.limits.max_screen_bytes)?;
    let name = association
        .name
        .as_deref()
        .ok_or_else(|| condition("INVPARTN", 65, 0))?;
    let row = service
        .store
        .get_provider_state(DEFINITION_NAMESPACE, name)
        .map_err(store_error)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    if !decode_definition(&row)?
        .partitions
        .iter()
        .any(|value| value.name == received.partition)
    {
        return Err(condition("INVPARTN", 65, 0));
    }
    service.authorize(
        run,
        "FACILITY",
        &format!("CICS.TERMINAL.PARTN.{}", received.partition),
        AccessIntent::Read,
    )?;
    if !association.received_once || !request.arguments.contains_key("OPTION.ASIS") {
        received.data.make_ascii_uppercase();
    }
    let response = receive_response(service, run, request, &received)?;
    let mut next = current.clone();
    next.version = next
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    next.input.payload = None;
    next.input.message_length = 0;
    next.suspended = false;
    let mut next_association = association.clone();
    next_association.version = next_association
        .version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    next_association.received_once = true;
    let writes = vec![
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: "cics-session".into(),
                key: run.session.clone(),
                version: next.version,
                payload: super::super::encode_session(&next)?,
            },
            expected_version: Some(current.version),
        }),
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: ASSOCIATION_NAMESPACE.into(),
                key: owner.clone(),
                version: next_association.version,
                payload: encode_association(&next_association)?,
            },
            expected_version: Some(association.version),
        }),
        ProviderStateMutation::Put(ProviderStateWrite {
            record: ProviderStateRecord {
                namespace: RECEIVE_RECEIPT_NAMESPACE.into(),
                key: key.into(),
                version: 1,
                payload: encode_receive_receipt(&owner, digest, &received)?,
            },
            expected_version: None,
        }),
    ];
    match service.store.mutate_provider_states_atomic(writes) {
        Ok(()) => {
            service.lock()?.sessions.insert(run.session.clone(), next);
            Ok(response)
        }
        Err(StoreError::Conflict | StoreError::AlreadyExists) => {
            if let Some(row) = service
                .store
                .get_provider_state(RECEIVE_RECEIPT_NAMESPACE, key)
                .map_err(store_error)?
            {
                let received =
                    decode_receive_receipt(&row, &owner, digest, service.limits.max_screen_bytes)?;
                receive_response(service, run, request, &received)
            } else {
                Err(HostProblem::IdempotencyConflict)
            }
        }
        Err(error) => Err(mutation_problem(store_error(error))),
    }
}

fn validate_receive_request(request: &CicsRequest) -> Result<(), HostProblem> {
    let arguments = &request.arguments;
    if request.operation != CicsOperation::ReceivePartn
        || request.mutation.is_none()
        || !arguments.contains_key("PARTN")
        || arguments.contains_key("INTO") && arguments.contains_key("SET")
        || arguments.contains_key("INTO") != arguments.contains_key("LENGTH")
        || arguments.contains_key("RESP2") && !arguments.contains_key("RESP")
        || arguments.iter().any(|(name, value)| match name.as_str() {
            "PARTN" | "INTO" | "SET" | "RESP" | "RESP2" => {
                value.schema() != "mainframe-env.cics.argument@1"
            }
            "LENGTH" | "INTO.MAXLENGTH" | "SET.MAXLENGTH" => {
                value.schema() != "mainframe-env.cics.decimal@1"
            }
            "OPTION.ASIS" | "OPTION.NOHANDLE" => {
                value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
            }
            _ => true,
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn decode_partition_input(
    bytes: &[u8],
    aid: u8,
    max_data: usize,
) -> Result<ReceivedPartition, HostProblem> {
    let mut input = bytes;
    take(&mut input, INPUT_MAGIC.len(), INPUT_MAGIC)?;
    let partition = read_name(&mut input, 2)?;
    let data = read_field(&mut input, max_data)?.to_vec();
    let cursor = u16::from_be_bytes(read_array(&mut input)?);
    if !input.is_empty() || !super::valid_aid(aid) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(ReceivedPartition {
        partition,
        data,
        cursor,
        aid,
    })
}

fn receive_response(
    service: &CicsService,
    run: &Run,
    request: &CicsRequest,
    received: &ReceivedPartition,
) -> Result<CicsResponse, HostProblem> {
    let actual = received.data.len();
    let mut payload = Vec::new();
    let mut response_code = 0;
    let mut condition_name = "NORMAL";
    let mut outputs = std::collections::BTreeMap::new();
    outputs.insert(
        "PARTN".into(),
        bounded(received.partition.as_bytes().to_vec())?,
    );
    outputs.insert(
        "EIBCPOSN".into(),
        decimal_payload(i64::from(received.cursor))?,
    );
    if request.arguments.contains_key("INTO") {
        let declared = decimal_argument(request, "LENGTH")?.ok_or(HostProblem::Malformed)?;
        let capacity = decimal_argument(request, "INTO.MAXLENGTH")?.unwrap_or(declared);
        let allowed = declared.min(capacity);
        payload.extend_from_slice(&received.data[..actual.min(allowed)]);
        outputs.insert("LENGTH".into(), decimal_payload(actual as i64)?);
        if actual > allowed {
            response_code = 22;
            condition_name = "LENGERR";
        }
    } else if request.arguments.contains_key("SET") {
        let capacity = decimal_argument(request, "SET.MAXLENGTH")?.ok_or(HostProblem::Malformed)?;
        if actual.checked_add(12).is_none_or(|size| size > capacity) {
            return Err(HostProblem::ResourceExhausted);
        }
        let mut content = vec![0; 12];
        content.extend_from_slice(&received.data);
        outputs.insert("SET".into(), bounded(content)?);
    }
    let mut response = service.response(
        run,
        CicsDisposition::Complete,
        condition_name,
        response_code,
        0,
        None,
        None,
        payload,
    )?;
    response.aid = received.aid;
    response.outputs = outputs;
    Ok(response)
}

fn decimal_argument(request: &CicsRequest, name: &str) -> Result<Option<usize>, HostProblem> {
    request
        .arguments
        .get(name)
        .map(|value| {
            std::str::from_utf8(value.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<usize>()
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()
}

fn encode_receive_receipt(
    owner: &str,
    digest: [u8; 32],
    received: &ReceivedPartition,
) -> Result<Vec<u8>, HostProblem> {
    let mut bytes = RECEIVE_RECEIPT_MAGIC.to_vec();
    field(&mut bytes, owner.as_bytes())?;
    bytes.extend_from_slice(&digest);
    field(&mut bytes, received.partition.as_bytes())?;
    field(&mut bytes, &received.data)?;
    bytes.extend_from_slice(&received.cursor.to_be_bytes());
    bytes.push(received.aid);
    Ok(bytes)
}

fn decode_receive_receipt(
    row: &ProviderStateRecord,
    owner: &str,
    digest: [u8; 32],
    max_data: usize,
) -> Result<ReceivedPartition, HostProblem> {
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut input = row.payload.as_slice();
    take(
        &mut input,
        RECEIVE_RECEIPT_MAGIC.len(),
        RECEIVE_RECEIPT_MAGIC,
    )?;
    let recorded_owner = read_field(&mut input, 256)?;
    let recorded_digest = read_array::<32>(&mut input)?;
    if recorded_owner != owner.as_bytes() || recorded_digest != digest {
        return Err(HostProblem::IdempotencyConflict);
    }
    let partition = read_name(&mut input, 2)?;
    let data = read_field(&mut input, max_data)?.to_vec();
    let cursor = u16::from_be_bytes(read_array(&mut input)?);
    let aid = take_bytes(&mut input, 1)?[0];
    if !input.is_empty() || !super::valid_aid(aid) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(ReceivedPartition {
        partition,
        data,
        cursor,
        aid,
    })
}

pub(in crate::service) fn release_task(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    let key = run.invocation.run_unit_id.as_str();
    if let Some(row) = service
        .store
        .get_provider_state(ASSOCIATION_NAMESPACE, key)
        .map_err(store_error)?
    {
        service
            .store
            .mutate_provider_states_atomic(vec![ProviderStateMutation::Delete {
                namespace: ASSOCIATION_NAMESPACE.into(),
                key: key.into(),
                expected_version: row.version,
            }])
            .map_err(store_error)
            .map_err(mutation_problem)?;
    }
    Ok(())
}

pub(super) fn read_association(
    service: &CicsService,
    owner: &str,
) -> Result<Option<Association>, HostProblem> {
    service
        .store
        .get_provider_state(ASSOCIATION_NAMESPACE, owner)
        .map_err(store_error)?
        .map(|row| decode_association(&row))
        .transpose()
}

pub(super) fn partition_exists(
    service: &CicsService,
    run: &Run,
    name: &str,
) -> Result<bool, HostProblem> {
    let Some(set_name) = read_association(service, run.invocation.run_unit_id.as_str())?
        .and_then(|association| association.name)
    else {
        return Ok(false);
    };
    let row = service
        .store
        .get_provider_state(DEFINITION_NAMESPACE, &set_name)
        .map_err(store_error)?
        .ok_or(HostProblem::InfrastructureFailure)?;
    Ok(decode_definition(&row)?
        .partitions
        .iter()
        .any(|partition| partition.name == name))
}

pub(super) fn require_intervening_send(
    service: &CicsService,
    run: &Run,
) -> Result<(), HostProblem> {
    if read_association(service, run.invocation.run_unit_id.as_str())?
        .is_some_and(|association| !association.intervening_send)
    {
        return Err(invreq(0));
    }
    Ok(())
}

pub(super) fn persist_send(
    service: &CicsService,
    run: &Run,
    current: &Session,
    next: &Session,
) -> Result<(), HostProblem> {
    let owner = run.invocation.run_unit_id.as_str();
    let Some(mut association) = read_association(service, owner)? else {
        return service.persist_session(&run.session, next, Some(current.version));
    };
    let old_version = association.version;
    association.version = old_version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    association.intervening_send = true;
    service
        .store
        .mutate_provider_states_atomic(vec![
            ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: "cics-session".into(),
                    key: run.session.clone(),
                    version: next.version,
                    payload: super::super::encode_session(next)?,
                },
                expected_version: Some(current.version),
            }),
            ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: ASSOCIATION_NAMESPACE.into(),
                    key: owner.into(),
                    version: association.version,
                    payload: encode_association(&association)?,
                },
                expected_version: Some(old_version),
            }),
        ])
        .map_err(store_error)
}

pub(super) fn mark_control_send(
    service: &CicsService,
    run: &Run,
) -> Result<Option<ProviderStateMutation>, HostProblem> {
    let owner = run.invocation.run_unit_id.as_str();
    let Some(mut association) = read_association(service, owner)? else {
        return Ok(None);
    };
    let old_version = association.version;
    association.version = old_version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    association.intervening_send = true;
    Ok(Some(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: ASSOCIATION_NAMESPACE.into(),
            key: owner.into(),
            version: association.version,
            payload: encode_association(&association)?,
        },
        expected_version: Some(old_version),
    })))
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

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.operation != CicsOperation::SendPartnset
        || request.mutation.is_none()
        || request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "PARTNSET" => !matches!(
                    value.schema(),
                    "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                ),
                "RESP" | "RESP2" => value.schema() != "mainframe-env.cics.argument@1",
                "OPTION.NOHANDLE" => {
                    value.schema() != "mainframe-env.cics.option@1" || !value.bytes().is_empty()
                }
                _ => true,
            })
    {
        Err(HostProblem::Malformed)
    } else {
        Ok(())
    }
}

fn normalized_definition(
    definition: &CicsPartitionSetDefinition,
    limit: usize,
) -> Result<CicsPartitionSetDefinition, HostProblem> {
    let name = normalized_name(definition.name.as_bytes(), 8)?;
    if definition.partitions.is_empty() || definition.partitions.len() > limit {
        return Err(HostProblem::Malformed);
    }
    let mut partitions = Vec::with_capacity(definition.partitions.len());
    let mut seen = BTreeSet::new();
    for partition in &definition.partitions {
        let name = normalized_name(partition.name.as_bytes(), 2)?;
        if !seen.insert(name.clone()) || partition.rows == 0 || partition.columns == 0 {
            return Err(HostProblem::Malformed);
        }
        partition
            .top
            .checked_add(partition.rows)
            .ok_or(HostProblem::Malformed)?;
        partition
            .left
            .checked_add(partition.columns)
            .ok_or(HostProblem::Malformed)?;
        let normalized = CicsPartitionDefinition {
            name,
            ..partition.clone()
        };
        if partitions.iter().any(|other: &CicsPartitionDefinition| {
            normalized.top < other.top + other.rows
                && other.top < normalized.top + normalized.rows
                && normalized.left < other.left + other.columns
                && other.left < normalized.left + normalized.columns
        }) {
            return Err(HostProblem::Malformed);
        }
        partitions.push(normalized);
    }
    Ok(CicsPartitionSetDefinition { name, partitions })
}

pub(super) fn normalized_name(bytes: &[u8], max: usize) -> Result<String, HostProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostProblem::Malformed)?;
    let text = text.trim_end_matches(' ');
    if text.is_empty() || text.len() > max || !text.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(HostProblem::Malformed);
    }
    Ok(text.to_ascii_uppercase())
}

fn encode_definition(definition: &CicsPartitionSetDefinition) -> Result<Vec<u8>, HostProblem> {
    let mut bytes = DEFINITION_MAGIC.to_vec();
    field(&mut bytes, definition.name.as_bytes())?;
    let count =
        u16::try_from(definition.partitions.len()).map_err(|_| HostProblem::ResourceExhausted)?;
    bytes.extend_from_slice(&count.to_be_bytes());
    for partition in &definition.partitions {
        field(&mut bytes, partition.name.as_bytes())?;
        for number in [
            partition.top,
            partition.left,
            partition.rows,
            partition.columns,
        ] {
            bytes.extend_from_slice(&number.to_be_bytes());
        }
    }
    Ok(bytes)
}

pub(super) fn decode_definition(
    row: &ProviderStateRecord,
) -> Result<CicsPartitionSetDefinition, HostProblem> {
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut input = row.payload.as_slice();
    take(&mut input, DEFINITION_MAGIC.len(), DEFINITION_MAGIC)?;
    let name = read_name(&mut input, 8)?;
    let count = u16::from_be_bytes(read_array(&mut input)?);
    if usize::from(count) > 255 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut partitions = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        partitions.push(CicsPartitionDefinition {
            name: read_name(&mut input, 2)?,
            top: u16::from_be_bytes(read_array(&mut input)?),
            left: u16::from_be_bytes(read_array(&mut input)?),
            rows: u16::from_be_bytes(read_array(&mut input)?),
            columns: u16::from_be_bytes(read_array(&mut input)?),
        });
    }
    let definition = normalized_definition(&CicsPartitionSetDefinition { name, partitions }, 255)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if !input.is_empty() || row.key != definition.name {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(definition)
}

fn encode_association(value: &Association) -> Result<Vec<u8>, HostProblem> {
    let mut bytes = ASSOCIATION_MAGIC.to_vec();
    field(&mut bytes, value.name.as_deref().unwrap_or("").as_bytes())?;
    bytes.extend_from_slice(&value.session_version.to_be_bytes());
    bytes.push(u8::from(value.logical_message_active));
    bytes.push(u8::from(value.intervening_send));
    bytes.push(u8::from(value.received_once));
    Ok(bytes)
}

fn decode_association(row: &ProviderStateRecord) -> Result<Association, HostProblem> {
    let mut input = row.payload.as_slice();
    take(&mut input, ASSOCIATION_MAGIC.len(), ASSOCIATION_MAGIC)?;
    let raw = read_field(&mut input, 8)?;
    let name = if raw.is_empty() {
        None
    } else {
        Some(normalized_name(raw, 8).map_err(|_| HostProblem::InfrastructureFailure)?)
    };
    let session_version = u64::from_be_bytes(read_array(&mut input)?);
    let logical_message_active = match take_bytes(&mut input, 1)?[0] {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let intervening_send = match input.first().copied() {
        None | Some(0) => false,
        Some(1) => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !input.is_empty() {
        input = &input[1..];
    }
    let received_once = match input.first().copied() {
        None | Some(0) => false,
        Some(1) => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    if !input.is_empty() {
        input = &input[1..];
    }
    if !input.is_empty() || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Association {
        name,
        session_version,
        logical_message_active,
        intervening_send,
        received_once,
        version: row.version,
    })
}

fn encode_receipt(
    owner: &str,
    name: Option<&str>,
    digest: [u8; 32],
) -> Result<Vec<u8>, HostProblem> {
    let mut bytes = RECEIPT_MAGIC.to_vec();
    field(&mut bytes, owner.as_bytes())?;
    field(&mut bytes, name.unwrap_or("").as_bytes())?;
    bytes.extend_from_slice(&digest);
    Ok(bytes)
}

fn validate_receipt(
    row: &ProviderStateRecord,
    owner: &str,
    name: Option<&str>,
    digest: [u8; 32],
) -> Result<(), HostProblem> {
    if row.version != 1 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut input = row.payload.as_slice();
    take(&mut input, RECEIPT_MAGIC.len(), RECEIPT_MAGIC)?;
    let recorded_owner = read_field(&mut input, 256)?;
    let recorded_name = read_field(&mut input, 8)?;
    let recorded_digest = read_array::<32>(&mut input)?;
    if !input.is_empty()
        || recorded_owner != owner.as_bytes()
        || recorded_name != name.unwrap_or("").as_bytes()
        || recorded_digest != digest
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(())
}

fn read_name(input: &mut &[u8], max: usize) -> Result<String, HostProblem> {
    let raw = read_field(input, max)?;
    normalized_name(raw, max).map_err(|_| HostProblem::InfrastructureFailure)
}

pub(super) fn read_field<'a>(input: &mut &'a [u8], max: usize) -> Result<&'a [u8], HostProblem> {
    let length = u32::from_be_bytes(read_array(input)?) as usize;
    if length > max {
        return Err(HostProblem::InfrastructureFailure);
    }
    take_bytes(input, length)
}

pub(super) fn read_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], HostProblem> {
    take_bytes(input, N)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)
}

pub(super) fn take(input: &mut &[u8], length: usize, expected: &[u8]) -> Result<(), HostProblem> {
    if take_bytes(input, length)? != expected {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

pub(super) fn take_bytes<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], HostProblem> {
    let (head, tail) = input
        .split_at_checked(length)
        .ok_or(HostProblem::InfrastructureFailure)?;
    *input = tail;
    Ok(head)
}

fn condition(name: &str, response: i32, response2: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2,
    }
}

fn invreq(response2: i32) -> HostProblem {
    condition("INVREQ", 16, response2)
}
