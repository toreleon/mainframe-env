//! Durable partition-set definitions and task associations for BMS terminals.

use super::super::super::{CicsService, Run, mutation_problem};
use super::super::{field, store_error};
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
    HostRequest, canonical_request_digest,
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

pub(super) fn require_intervening_send(
    service: &CicsService,
    run: &Run,
    session_version: u64,
) -> Result<(), HostProblem> {
    if read_association(service, run.invocation.run_unit_id.as_str())?
        .is_some_and(|association| session_version <= association.session_version)
    {
        return Err(invreq(0));
    }
    Ok(())
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

fn normalized_name(bytes: &[u8], max: usize) -> Result<String, HostProblem> {
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

fn decode_definition(row: &ProviderStateRecord) -> Result<CicsPartitionSetDefinition, HostProblem> {
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
    if !input.is_empty() || row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(Association {
        name,
        session_version,
        logical_message_active,
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

fn read_field<'a>(input: &mut &'a [u8], max: usize) -> Result<&'a [u8], HostProblem> {
    let length = u32::from_be_bytes(read_array(input)?) as usize;
    if length > max {
        return Err(HostProblem::InfrastructureFailure);
    }
    take_bytes(input, length)
}

fn read_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], HostProblem> {
    take_bytes(input, N)?
        .try_into()
        .map_err(|_| HostProblem::InfrastructureFailure)
}

fn take(input: &mut &[u8], length: usize, expected: &[u8]) -> Result<(), HostProblem> {
    if take_bytes(input, length)? != expected {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

fn take_bytes<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], HostProblem> {
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
