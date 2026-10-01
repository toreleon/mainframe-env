//! Bounded, durable outboard data-set and task state.

use super::*;
use serde::{Deserialize, Serialize};

pub(super) const DEFINITION_NAMESPACE: &str = "cics-outboard-definition-v1";
pub(super) const DATA_NAMESPACE: &str = "cics-outboard-data-v1";
pub(super) const TASK_NAMESPACE: &str = "cics-outboard-task-v1";
pub(super) const RECEIPT_NAMESPACE: &str = "cics-outboard-receipt-v1";

/// Storage organization of a local outboard destination.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CicsOutboardKind {
    /// Append-only records, available to ISSUE QUERY and ISSUE RECEIVE.
    Sequential,
    /// Records selected by up to eight configured embedded indexes.
    Keyed,
    /// Records selected by zero-based fullword relative record number.
    Relative,
    /// Virtual console, printer, card, or word-processing medium.
    Medium,
}

/// Immutable local outboard data-set definition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsOutboardDestinationDefinition {
    /// One- to eight-character destination identity.
    pub name: String,
    /// Optional one- to six-character diskette volume identity.
    pub volume: Option<String>,
    /// Outboard data-set organization.
    pub kind: CicsOutboardKind,
    /// Fixed logical record size, bounded by the CICS screen limit.
    pub record_length: u16,
    /// Key offsets and lengths for indexed data sets, in index-number order.
    pub indexes: Vec<(u16, u16)>,
}

/// One durable outboard record returned by the local inspection API.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CicsOutboardRecord {
    /// Zero-based relative number, or sequential position.
    pub number: u32,
    /// One key per configured index, empty for other organizations.
    pub keys: Vec<Vec<u8>>,
    /// Exact logical record bytes.
    pub data: Vec<u8>,
}

/// Current durable contents of one registered outboard data set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsOutboardSnapshot {
    /// Destination name.
    pub name: String,
    /// Current record organization.
    pub kind: CicsOutboardKind,
    /// Durable records in stable number order.
    pub records: Vec<CicsOutboardRecord>,
    /// True after ISSUE END or ISSUE ABORT deselects the destination.
    pub closed: bool,
    /// Monotone data-row version.
    pub version: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DataState {
    pub records: Vec<CicsOutboardRecord>,
    pub closed: bool,
    #[serde(skip)]
    pub version: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskState {
    pub selected: Option<String>,
    pub last_inbound_destination: Option<String>,
    pub pending_destination: Option<String>,
    pub query_destination: Option<String>,
    pub query_records: Vec<Vec<u8>>,
    pub query_index: usize,
    pub query_aborted: bool,
    #[serde(skip)]
    pub version: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub owner: String,
    pub digest: [u8; 32],
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub payload: Vec<u8>,
    pub outputs: BTreeMap<String, ReceiptOutput>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptOutput {
    pub schema: String,
    pub bytes: Vec<u8>,
}

impl CicsService {
    /// Register immutable, bounded local outboard data-set definitions.
    pub fn register_outboard_destinations(
        &self,
        definitions: &[CicsOutboardDestinationDefinition],
    ) -> Result<(), HostProblem> {
        if definitions.is_empty() || definitions.len() > self.limits.max_maps {
            return Err(HostProblem::Malformed);
        }
        let mut seen = BTreeSet::new();
        let mut writes = Vec::new();
        for definition in definitions {
            let definition = normalized_definition(definition, self.limits.max_screen_bytes)?;
            if !seen.insert(definition.name.clone()) {
                return Err(HostProblem::Malformed);
            }
            let bytes =
                serde_json::to_vec(&definition).map_err(|_| HostProblem::InfrastructureFailure)?;
            if let Some(existing) = self
                .store
                .get_provider_state(DEFINITION_NAMESPACE, &definition.name)
                .map_err(store_error)?
            {
                if existing.version != 1 || existing.payload != bytes {
                    return Err(HostProblem::IdempotencyConflict);
                }
                continue;
            }
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DEFINITION_NAMESPACE.into(),
                    key: definition.name.clone(),
                    version: 1,
                    payload: bytes,
                },
                expected_version: None,
            }));
            writes.push(ProviderStateMutation::Put(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DATA_NAMESPACE.into(),
                    key: definition.name,
                    version: 1,
                    payload: serde_json::to_vec(&DataState::default())
                        .map_err(|_| HostProblem::InfrastructureFailure)?,
                },
                expected_version: None,
            }));
        }
        if !writes.is_empty() {
            self.store
                .mutate_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        Ok(())
    }

    /// Read exact durable records for one registered local outboard destination.
    pub fn outboard_snapshot(&self, name: &str) -> Result<CicsOutboardSnapshot, HostProblem> {
        let name = normalized_name(name.as_bytes(), 8)?;
        let definition = read_definition(self, &name)?.ok_or(HostProblem::NotFound)?;
        let data = read_data(self, &name, &definition)?;
        Ok(CicsOutboardSnapshot {
            name,
            kind: definition.kind,
            records: data.records,
            closed: data.closed,
            version: data.version,
        })
    }

    /// Last inbound destination selected by ISSUE QUERY/RECEIVE in this task.
    pub fn outboard_last_inbound_destination(
        &self,
        run_unit: &mainframe_env_execution_api::RunUnitId,
    ) -> Result<Option<String>, HostProblem> {
        Ok(read_task(self, run_unit.as_str())?.last_inbound_destination)
    }
}

pub(super) fn normalized_definition(
    value: &CicsOutboardDestinationDefinition,
    max_record: usize,
) -> Result<CicsOutboardDestinationDefinition, HostProblem> {
    let name = normalized_name(value.name.as_bytes(), 8)?;
    let volume = value
        .volume
        .as_ref()
        .map(|name| normalized_name(name.as_bytes(), 6))
        .transpose()?;
    if value.record_length == 0 || usize::from(value.record_length) > max_record {
        return Err(HostProblem::Malformed);
    }
    if (value.kind == CicsOutboardKind::Keyed) != !value.indexes.is_empty()
        || value.indexes.len() > 8
        || value.indexes.iter().any(|(start, length)| {
            *length == 0
                || start
                    .checked_add(*length)
                    .is_none_or(|end| end > value.record_length)
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(CicsOutboardDestinationDefinition {
        name,
        volume,
        ..value.clone()
    })
}

pub(super) fn read_definition(
    service: &CicsService,
    name: &str,
) -> Result<Option<CicsOutboardDestinationDefinition>, HostProblem> {
    if let Some(definition) = media_definition(name, service) {
        return Ok(Some(definition));
    }
    service
        .store
        .get_provider_state(DEFINITION_NAMESPACE, name)
        .map_err(store_error)?
        .map(|row| {
            if row.version != 1 {
                return Err(HostProblem::InfrastructureFailure);
            }
            let value: CicsOutboardDestinationDefinition = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            let normalized = normalized_definition(&value, service.limits.max_screen_bytes)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if normalized != value || row.key != name {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(value)
        })
        .transpose()
}

pub(super) fn read_data(
    service: &CicsService,
    name: &str,
    definition: &CicsOutboardDestinationDefinition,
) -> Result<DataState, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(DATA_NAMESPACE, name)
        .map_err(store_error)?
    else {
        return if definition.kind == CicsOutboardKind::Medium {
            Ok(DataState::default())
        } else {
            Err(HostProblem::InfrastructureFailure)
        };
    };
    if row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut value: DataState =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    validate_data(&value, definition, service)?;
    value.version = row.version;
    Ok(value)
}

fn validate_data(
    data: &DataState,
    definition: &CicsOutboardDestinationDefinition,
    service: &CicsService,
) -> Result<(), HostProblem> {
    if data.records.len() > service.limits.max_queue_records {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut bytes = 0usize;
    let mut last_number = None;
    let mut keys = BTreeSet::new();
    for record in &data.records {
        bytes = bytes
            .checked_add(record.data.len())
            .ok_or(HostProblem::InfrastructureFailure)?;
        if (definition.kind == CicsOutboardKind::Medium
            && (record.data.is_empty()
                || record.data.len() > usize::from(definition.record_length))
            || definition.kind != CicsOutboardKind::Medium
                && record.data.len() != usize::from(definition.record_length))
            || record.keys.len() != definition.indexes.len()
            || last_number.is_some_and(|last| record.number <= last)
            || record.keys.iter().enumerate().any(|(index, key)| {
                let (offset, length) = definition.indexes[index];
                key.len() != usize::from(length)
                    || record
                        .data
                        .get(usize::from(offset)..usize::from(offset + length))
                        != Some(key.as_slice())
                    || !keys.insert((index, key.clone()))
            })
        {
            return Err(HostProblem::InfrastructureFailure);
        }
        last_number = Some(record.number);
    }
    if bytes > service.limits.max_queue_bytes {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

pub(super) fn validate_data_for_mutation(
    data: &DataState,
    definition: &CicsOutboardDestinationDefinition,
    service: &CicsService,
) -> Result<(), HostProblem> {
    validate_data(data, definition, service).map_err(|_| condition("FUNCERR", 48, 0))
}

pub(super) fn read_task(service: &CicsService, owner: &str) -> Result<TaskState, HostProblem> {
    let Some(row) = service
        .store
        .get_provider_state(TASK_NAMESPACE, owner)
        .map_err(store_error)?
    else {
        return Ok(TaskState::default());
    };
    if row.version == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut task: TaskState =
        serde_json::from_slice(&row.payload).map_err(|_| HostProblem::InfrastructureFailure)?;
    if task.query_index > task.query_records.len()
        || task.query_records.len() > service.limits.max_queue_records
        || task
            .query_records
            .iter()
            .try_fold(0usize, |n, r| n.checked_add(r.len()))
            .is_none_or(|n| n > service.limits.max_queue_bytes)
        || [
            task.selected.as_deref(),
            task.last_inbound_destination.as_deref(),
            task.pending_destination.as_deref(),
            task.query_destination.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|name| normalized_name(name.as_bytes(), 32).ok().as_deref() != Some(name))
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    task.version = row.version;
    Ok(task)
}

pub(super) fn data_write(
    name: &str,
    data: &DataState,
    definition: &CicsOutboardDestinationDefinition,
    service: &CicsService,
) -> Result<ProviderStateMutation, HostProblem> {
    validate_data(data, definition, service).map_err(|_| HostProblem::ResourceExhausted)?;
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: DATA_NAMESPACE.into(),
            key: name.into(),
            version: data
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload: serde_json::to_vec(data).map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: (data.version != 0).then_some(data.version),
    }))
}

pub(super) fn task_write(
    owner: &str,
    task: &TaskState,
    service: &CicsService,
) -> Result<ProviderStateMutation, HostProblem> {
    if task.query_records.len() > service.limits.max_queue_records
        || task
            .query_records
            .iter()
            .try_fold(0usize, |n, r| n.checked_add(r.len()))
            .is_none_or(|n| n > service.limits.max_queue_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: TASK_NAMESPACE.into(),
            key: owner.into(),
            version: task
                .version
                .checked_add(1)
                .ok_or(HostProblem::ResourceExhausted)?,
            payload: serde_json::to_vec(task).map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: (task.version != 0).then_some(task.version),
    }))
}

pub(super) fn receipt_write(
    key: &str,
    receipt: &Receipt,
) -> Result<ProviderStateMutation, HostProblem> {
    Ok(ProviderStateMutation::Put(ProviderStateWrite {
        record: ProviderStateRecord {
            namespace: RECEIPT_NAMESPACE.into(),
            key: key.into(),
            version: 1,
            payload: serde_json::to_vec(receipt).map_err(|_| HostProblem::InfrastructureFailure)?,
        },
        expected_version: None,
    }))
}

pub(super) fn read_receipt(
    service: &CicsService,
    key: &str,
) -> Result<Option<Receipt>, HostProblem> {
    service
        .store
        .get_provider_state(RECEIPT_NAMESPACE, key)
        .map_err(store_error)?
        .map(|row| {
            if row.version != 1 {
                return Err(HostProblem::InfrastructureFailure);
            }
            let receipt: Receipt = serde_json::from_slice(&row.payload)
                .map_err(|_| HostProblem::InfrastructureFailure)?;
            if receipt.condition.len() > 32
                || receipt.payload.len() > service.limits.max_screen_bytes
                || receipt.outputs.len() > service.limits.max_fields
                || receipt
                    .outputs
                    .values()
                    .any(|value| value.bytes.len() > service.limits.max_screen_bytes)
            {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(receipt)
        })
        .transpose()
}

pub(super) fn normalized_name(bytes: &[u8], max: usize) -> Result<String, HostProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostProblem::Malformed)?;
    let text = text.trim_end_matches(' ');
    if text.is_empty() || text.len() > max || !text.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(HostProblem::Malformed);
    }
    Ok(text.to_ascii_uppercase())
}

pub(super) fn media_definition(
    name: &str,
    service: &CicsService,
) -> Option<CicsOutboardDestinationDefinition> {
    let known = ["MCON", "MPRT", "MCRD", "MWP1", "MWP2", "MWP3", "MWP4"];
    if name.len() != 6
        || !known.iter().any(|prefix| name.starts_with(prefix))
        || !name[4..].bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(CicsOutboardDestinationDefinition {
        name: name.into(),
        volume: None,
        kind: CicsOutboardKind::Medium,
        record_length: u16::try_from(service.limits.max_screen_bytes.min(32_767)).ok()?,
        indexes: Vec::new(),
    })
}
