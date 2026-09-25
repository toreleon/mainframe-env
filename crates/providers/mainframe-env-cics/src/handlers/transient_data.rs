use super::super::{CicsLimits, TransientQueue, decode_transient, encode_transient, store_error};
#[cfg(test)]
use mainframe_env_host_api::CicsOperation;
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, ProviderStateWrite};
use std::collections::{BTreeMap, BTreeSet};

const DEFINITION_NAMESPACE: &str = "cics-tdq-definition";

/// Local transient-data queue placement supported by the owned CICS provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsTransientDataQueueKind {
    Intrapartition,
    Extrapartition,
}

/// Local access direction for an extrapartition queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsTransientDataQueueOpen {
    Input,
    Output,
    Closed,
}

/// Durable local TDQUEUE definition used by WRITEQ, READQ, and DELETEQ TD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsTransientDataQueueDefinition {
    pub name: String,
    pub kind: CicsTransientDataQueueKind,
    pub enabled: bool,
    pub open: Option<CicsTransientDataQueueOpen>,
    pub record_size: Option<usize>,
    pub max_records: usize,
    pub max_bytes: usize,
}

#[derive(Clone, Debug)]
pub(in crate::service) struct TransientDataState {
    pub definitions: BTreeMap<String, CicsTransientDataQueueDefinition>,
    pub queues: BTreeMap<String, TransientQueue>,
    pub bytes: usize,
    #[cfg(test)]
    pub io_failure: Option<(CicsOperation, String)>,
}

impl TransientDataState {
    pub fn counts(&self) -> (usize, usize, usize) {
        (self.definitions.len(), self.queues.len(), self.bytes)
    }

    pub fn definition(
        &self,
        queue: &str,
        limits: CicsLimits,
    ) -> Result<CicsTransientDataQueueDefinition, HostProblem> {
        if let Some(definition) = self.definitions.get(queue) {
            return Ok(definition.clone());
        }
        if self.definitions.is_empty() {
            return Ok(CicsTransientDataQueueDefinition {
                name: queue.into(),
                kind: CicsTransientDataQueueKind::Intrapartition,
                enabled: true,
                open: None,
                record_size: None,
                max_records: limits.max_queue_records,
                max_bytes: limits.max_queue_bytes,
            });
        }
        Err(condition("QIDERR", 44))
    }
}

pub(in crate::service) fn load(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<TransientDataState, HostProblem> {
    let mut definitions = BTreeMap::new();
    for row in store
        .list_provider_state(DEFINITION_NAMESPACE, limits.max_queue_records)
        .map_err(store_error)?
    {
        let definition = decode_definition(&row.payload, limits)?;
        if row.version != 1
            || row.key != definition.name
            || definitions.insert(row.key, definition).is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    let mut queues = BTreeMap::new();
    let mut bytes = 0usize;
    for row in store
        .list_provider_state("cics-tdq", limits.max_queue_records)
        .map_err(store_error)?
    {
        let queue = decode_transient(&row.payload, row.version, limits)?;
        bytes = queue.records.iter().try_fold(bytes, |total, (_, value)| {
            total
                .checked_add(value.len())
                .ok_or(HostProblem::ResourceExhausted)
        })?;
        if bytes > limits.max_queue_bytes || queues.insert(row.key, queue).is_some() {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    Ok(TransientDataState {
        definitions,
        queues,
        bytes,
        #[cfg(test)]
        io_failure: None,
    })
}

pub(in crate::service) fn register(
    store: &dyn ProviderStateStore,
    state: &mut TransientDataState,
    definitions: &[CicsTransientDataQueueDefinition],
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    if definitions.is_empty() || definitions.len() > limits.max_queue_records {
        return Err(HostProblem::Malformed);
    }
    let mut normalized = Vec::with_capacity(definitions.len());
    let mut names = BTreeSet::new();
    for definition in definitions {
        let mut definition = definition.clone();
        definition.name = definition.name.trim().to_ascii_uppercase();
        validate_definition(&definition, limits)?;
        if !names.insert(definition.name.clone()) {
            return Err(HostProblem::Malformed);
        }
        if state
            .definitions
            .get(&definition.name)
            .is_some_and(|existing| existing != &definition)
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        normalized.push(definition);
    }
    let mut proposed = state.definitions.clone();
    for definition in &normalized {
        proposed
            .entry(definition.name.clone())
            .or_insert_with(|| definition.clone());
    }
    if proposed.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let writes = normalized
        .iter()
        .filter(|definition| !state.definitions.contains_key(&definition.name))
        .map(|definition| {
            Ok(ProviderStateWrite {
                record: ProviderStateRecord {
                    namespace: DEFINITION_NAMESPACE.into(),
                    key: definition.name.clone(),
                    version: 1,
                    payload: encode_definition(definition)?,
                },
                expected_version: None,
            })
        })
        .collect::<Result<Vec<_>, HostProblem>>()?;
    if !writes.is_empty() {
        validate_materialized_queues(&state.queues, &proposed)?;
        store
            .put_provider_states_atomic(writes)
            .map_err(store_error)?;
    }
    for definition in normalized {
        state
            .definitions
            .entry(definition.name.clone())
            .or_insert(definition);
    }
    Ok(())
}

fn validate_materialized_queues(
    queues: &BTreeMap<String, TransientQueue>,
    definitions: &BTreeMap<String, CicsTransientDataQueueDefinition>,
) -> Result<(), HostProblem> {
    for (name, queue) in queues {
        let Some(definition) = definitions.get(name) else {
            return Err(HostProblem::IdempotencyConflict);
        };
        if queue.records.len() > definition.max_records {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut retained_bytes = 0usize;
        for (_, record) in &queue.records {
            retained_bytes = retained_bytes
                .checked_add(record.len())
                .ok_or(HostProblem::IdempotencyConflict)?;
            let compatible = match (definition.kind, definition.open) {
                (CicsTransientDataQueueKind::Intrapartition, None) => definition
                    .record_size
                    .is_none_or(|maximum| record.len() <= maximum),
                (
                    CicsTransientDataQueueKind::Extrapartition,
                    Some(CicsTransientDataQueueOpen::Output),
                ) => definition.record_size == Some(record.len()),
                (
                    CicsTransientDataQueueKind::Extrapartition,
                    Some(CicsTransientDataQueueOpen::Input | CicsTransientDataQueueOpen::Closed),
                ) => false,
                _ => false,
            };
            if !compatible {
                return Err(HostProblem::IdempotencyConflict);
            }
        }
        if retained_bytes > definition.max_bytes {
            return Err(HostProblem::IdempotencyConflict);
        }
    }
    Ok(())
}

pub(super) fn persist_queue(
    store: &dyn ProviderStateStore,
    key: String,
    queue: &TransientQueue,
    expected_version: Option<u64>,
) -> Result<(), HostProblem> {
    store
        .put_provider_state(
            ProviderStateRecord {
                namespace: "cics-tdq".into(),
                key,
                version: queue.version,
                payload: encode_transient(queue)?,
            },
            expected_version,
        )
        .map_err(store_error)
}

fn validate_definition(
    definition: &CicsTransientDataQueueDefinition,
    limits: CicsLimits,
) -> Result<(), HostProblem> {
    if !(1..=4).contains(&definition.name.len())
        || !definition
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
        || definition.max_records == 0
        || definition.max_records > limits.max_queue_records
        || definition.max_bytes == 0
        || definition.max_bytes > limits.max_queue_bytes
        || definition
            .record_size
            .is_some_and(|size| size == 0 || size > definition.max_bytes)
        || match definition.kind {
            CicsTransientDataQueueKind::Intrapartition => definition.open.is_some(),
            CicsTransientDataQueueKind::Extrapartition => {
                definition.open.is_none() || definition.record_size.is_none()
            }
        }
    {
        return Err(HostProblem::Malformed);
    }
    Ok(())
}

fn encode_definition(
    definition: &CicsTransientDataQueueDefinition,
) -> Result<Vec<u8>, HostProblem> {
    let mut out = b"METD1".to_vec();
    push_field(&mut out, definition.name.as_bytes())?;
    out.push(match definition.kind {
        CicsTransientDataQueueKind::Intrapartition => 0,
        CicsTransientDataQueueKind::Extrapartition => 1,
    });
    out.push(u8::from(definition.enabled));
    out.push(match definition.open {
        None => 0,
        Some(CicsTransientDataQueueOpen::Input) => 1,
        Some(CicsTransientDataQueueOpen::Output) => 2,
        Some(CicsTransientDataQueueOpen::Closed) => 3,
    });
    for value in [
        definition.record_size.unwrap_or(0),
        definition.max_records,
        definition.max_bytes,
    ] {
        out.extend_from_slice(
            &u64::try_from(value)
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
    }
    Ok(out)
}

fn decode_definition(
    bytes: &[u8],
    limits: CicsLimits,
) -> Result<CicsTransientDataQueueDefinition, HostProblem> {
    if bytes.len() < 5 || &bytes[..5] != b"METD1" {
        return Err(HostProblem::InfrastructureFailure);
    }
    let mut at = 5usize;
    let name = String::from_utf8(take_field(bytes, &mut at, 4)?)
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    let kind = match take_byte(bytes, &mut at)? {
        0 => CicsTransientDataQueueKind::Intrapartition,
        1 => CicsTransientDataQueueKind::Extrapartition,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let enabled = match take_byte(bytes, &mut at)? {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let open = match take_byte(bytes, &mut at)? {
        0 => None,
        1 => Some(CicsTransientDataQueueOpen::Input),
        2 => Some(CicsTransientDataQueueOpen::Output),
        3 => Some(CicsTransientDataQueueOpen::Closed),
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let record_size = take_usize(bytes, &mut at)?;
    let definition = CicsTransientDataQueueDefinition {
        name,
        kind,
        enabled,
        open,
        record_size: (record_size != 0).then_some(record_size),
        max_records: take_usize(bytes, &mut at)?,
        max_bytes: take_usize(bytes, &mut at)?,
    };
    if at != bytes.len() {
        return Err(HostProblem::InfrastructureFailure);
    }
    validate_definition(&definition, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    Ok(definition)
}

fn push_field(out: &mut Vec<u8>, value: &[u8]) -> Result<(), HostProblem> {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    out.extend_from_slice(value);
    Ok(())
}

fn take_field(bytes: &[u8], at: &mut usize, maximum: usize) -> Result<Vec<u8>, HostProblem> {
    let length = usize::try_from(u32::from_be_bytes(
        take(bytes, at, 4)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)?;
    if length > maximum {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(take(bytes, at, length)?.to_vec())
}

fn take_usize(bytes: &[u8], at: &mut usize) -> Result<usize, HostProblem> {
    usize::try_from(u64::from_be_bytes(
        take(bytes, at, 8)?
            .try_into()
            .map_err(|_| HostProblem::InfrastructureFailure)?,
    ))
    .map_err(|_| HostProblem::InfrastructureFailure)
}

fn take_byte(bytes: &[u8], at: &mut usize) -> Result<u8, HostProblem> {
    Ok(take(bytes, at, 1)?[0])
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8], HostProblem> {
    let end = at
        .checked_add(count)
        .filter(|end| *end <= bytes.len())
        .ok_or(HostProblem::InfrastructureFailure)?;
    let value = &bytes[*at..end];
    *at = end;
    Ok(value)
}

pub(super) fn condition(name: &str, response: i32) -> HostProblem {
    HostProblem::Condition {
        name: name.into(),
        response,
        response2: 0,
    }
}
