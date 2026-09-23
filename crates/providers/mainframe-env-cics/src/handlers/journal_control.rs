//! Durable task-owned journal output authority.

use super::super::{CicsLimits, CicsService, Run, store_error};
#[cfg(test)]
use mainframe_env_execution_api::RunUnitId;
use mainframe_env_host_api::{
    AccessIntent, CicsDisposition, CicsOperation, CicsRequest, CicsResponse, HostProblem,
};
use mainframe_env_store_api::{ProviderStateRecord, ProviderStateStore, ProviderStateWrite};
use std::collections::{BTreeMap, BTreeSet};

const NAMESPACE: &str = "cics-journal-v1";
const MAGIC: &[u8; 7] = b"MECJNL1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JournalAvailability {
    Open,
    Disabled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JournalOutputState {
    Pending,
    Hardened,
    IoError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JournalOutput {
    owner_run_unit: String,
    state: JournalOutputState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JournalRecord {
    availability: JournalAvailability,
    current_request: Option<i32>,
    outputs: BTreeMap<i32, JournalOutput>,
    version: u64,
}

pub(crate) fn load(
    store: &dyn ProviderStateStore,
    limits: CicsLimits,
) -> Result<BTreeMap<String, JournalRecord>, HostProblem> {
    let rows = store
        .list_provider_state(NAMESPACE, limits.max_queue_records.saturating_add(1))
        .map_err(store_error)?;
    if rows.len() > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut total_outputs = 0usize;
    let mut journals = BTreeMap::new();
    for row in rows {
        let name = normalize_name(&row.key)?;
        if name != row.key {
            return Err(HostProblem::InfrastructureFailure);
        }
        let record = decode(&row.payload, row.version, limits)?;
        total_outputs = total_outputs
            .checked_add(record.outputs.len())
            .ok_or(HostProblem::ResourceExhausted)?;
        if total_outputs > limits.max_queue_records || journals.insert(name, record).is_some() {
            return Err(HostProblem::ResourceExhausted);
        }
    }
    Ok(journals)
}

impl CicsService {
    /// Register the bounded set of journals known to this CICS region.
    pub fn register_journals(&self, names: &BTreeSet<String>) -> Result<(), HostProblem> {
        let normalized = names
            .iter()
            .map(|name| normalize_name(name))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if normalized.len() != names.len() {
            return Err(HostProblem::IdempotencyConflict);
        }
        let mut state = self.lock()?;
        let additions = normalized
            .iter()
            .filter(|name| !state.journals.contains_key(*name))
            .count();
        if state
            .journals
            .len()
            .checked_add(additions)
            .is_none_or(|total| total > self.limits.max_queue_records)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        let created = JournalRecord {
            availability: JournalAvailability::Open,
            current_request: None,
            outputs: BTreeMap::new(),
            version: 1,
        };
        let writes = normalized
            .iter()
            .filter(|name| !state.journals.contains_key(*name))
            .map(|name| {
                Ok(ProviderStateWrite {
                    record: ProviderStateRecord {
                        namespace: NAMESPACE.into(),
                        key: name.clone(),
                        version: 1,
                        payload: encode(&created, self.limits)?,
                    },
                    expected_version: None,
                })
            })
            .collect::<Result<Vec<_>, HostProblem>>()?;
        if !writes.is_empty() {
            self.store
                .put_provider_states_atomic(writes)
                .map_err(store_error)?;
        }
        for name in normalized {
            state
                .journals
                .entry(name)
                .or_insert_with(|| created.clone());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn seed_journal_output(
        &self,
        name: &str,
        owner: &RunUnitId,
        request_id: i32,
        outcome: &str,
    ) -> Result<(), HostProblem> {
        let name = normalize_name(name)?;
        let mut state = self.lock()?;
        let current = state
            .journals
            .get(&name)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let output_state = match outcome {
            "pending" => JournalOutputState::Pending,
            "hardened" => JournalOutputState::Hardened,
            "io-error" => JournalOutputState::IoError,
            _ => return Err(HostProblem::Malformed),
        };
        let mut next = current.clone();
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        next.current_request = Some(request_id);
        next.outputs.insert(
            request_id,
            JournalOutput {
                owner_run_unit: owner.as_str().into(),
                state: output_state,
            },
        );
        let existing_outputs = state
            .journals
            .values()
            .try_fold(0usize, |total, journal| {
                total.checked_add(journal.outputs.len())
            })
            .ok_or(HostProblem::ResourceExhausted)?;
        let additional = usize::from(!current.outputs.contains_key(&request_id));
        if existing_outputs
            .checked_add(additional)
            .is_none_or(|total| total > self.limits.max_queue_records)
        {
            return Err(HostProblem::ResourceExhausted);
        }
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: name.clone(),
                    version: next.version,
                    payload: encode(&next, self.limits)?,
                },
                Some(current.version),
            )
            .map_err(store_error)?;
        state.journals.insert(name, next);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_journal_availability(
        &self,
        name: &str,
        availability: &str,
    ) -> Result<(), HostProblem> {
        let name = normalize_name(name)?;
        let mut state = self.lock()?;
        let current = state
            .journals
            .get(&name)
            .cloned()
            .ok_or(HostProblem::NotFound)?;
        let mut next = current.clone();
        next.availability = match availability {
            "open" => JournalAvailability::Open,
            "disabled" => JournalAvailability::Disabled,
            "failed" => JournalAvailability::Failed,
            _ => return Err(HostProblem::Malformed),
        };
        next.version = next
            .version
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        self.store
            .put_provider_state(
                ProviderStateRecord {
                    namespace: NAMESPACE.into(),
                    key: name.clone(),
                    version: next.version,
                    payload: encode(&next, self.limits)?,
                },
                Some(current.version),
            )
            .map_err(store_error)?;
        state.journals.insert(name, next);
        Ok(())
    }
}

pub(in crate::service) fn invoke(
    service: &CicsService,
    run: &mut Run,
    request: &CicsRequest,
) -> Result<CicsResponse, HostProblem> {
    if !matches!(
        request.operation,
        CicsOperation::WaitJournalName | CicsOperation::WaitJournalNum
    ) {
        return Err(HostProblem::InfrastructureFailure);
    }
    validate_request(request)?;
    let name = match request.operation {
        CicsOperation::WaitJournalName => journal_name(request)?,
        CicsOperation::WaitJournalNum => journal_num_name(request)?,
        _ => unreachable!(),
    };
    service.authorize(
        run,
        "JOURNAL",
        &format!("CICS.JOURNAL.{name}"),
        AccessIntent::Read,
    )?;
    let request_id = request
        .arguments
        .get("REQID")
        .map(|value| {
            std::str::from_utf8(value.bytes())
                .map_err(|_| HostProblem::Malformed)?
                .parse::<i32>()
                .map_err(|_| HostProblem::Malformed)
        })
        .transpose()?;
    let state = service.lock()?;
    let journal = state.journals.get(&name).ok_or_else(jiderr)?;
    if journal.availability != JournalAvailability::Open {
        return Err(notopen());
    }
    let explicit_request = request_id.is_some();
    let request_id = request_id.or(journal.current_request).ok_or_else(notopen)?;
    let output = journal.outputs.get(&request_id).ok_or_else(jiderr)?;
    if explicit_request && output.owner_run_unit != run.invocation.run_unit_id.as_str() {
        return Err(jiderr());
    }
    match output.state {
        JournalOutputState::Pending => service.response(
            run,
            CicsDisposition::Suspended,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        ),
        JournalOutputState::Hardened => service.response(
            run,
            CicsDisposition::Complete,
            "NORMAL",
            0,
            0,
            None,
            None,
            Vec::new(),
        ),
        JournalOutputState::IoError => Err(HostProblem::Condition {
            name: "IOERR".into(),
            response: 17,
            response2: 0,
        }),
    }
}

fn validate_request(request: &CicsRequest) -> Result<(), HostProblem> {
    if request.arguments.contains_key("RESP2") && !request.arguments.contains_key("RESP")
        || request
            .arguments
            .iter()
            .any(|(name, value)| match name.as_str() {
                "JOURNALNAME" => {
                    !matches!(
                        value.schema(),
                        "mainframe-env.cics.literal@1" | "mainframe-env.cics.storage-value@1"
                    ) || request.operation != CicsOperation::WaitJournalName
                }
                "JOURNALNUM" => {
                    value.schema() != "mainframe-env.cics.decimal@1"
                        || request.operation != CicsOperation::WaitJournalNum
                }
                "REQID" => value.schema() != "mainframe-env.cics.decimal@1",
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

fn journal_name(request: &CicsRequest) -> Result<String, HostProblem> {
    let value = request
        .arguments
        .get("JOURNALNAME")
        .ok_or(HostProblem::Malformed)?;
    let name = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .trim()
        .to_ascii_uppercase();
    normalize_name(&name)
}

fn journal_num_name(request: &CicsRequest) -> Result<String, HostProblem> {
    let value = request
        .arguments
        .get("JOURNALNUM")
        .ok_or(HostProblem::Malformed)?;
    let number = std::str::from_utf8(value.bytes())
        .map_err(|_| HostProblem::Malformed)?
        .parse::<u8>()
        .map_err(|_| HostProblem::Malformed)?;
    if !(1..=99).contains(&number) {
        return Err(HostProblem::Malformed);
    }
    Ok(format!("DFHJ{number:02}"))
}

fn normalize_name(name: &str) -> Result<String, HostProblem> {
    let name = name.trim().to_ascii_uppercase();
    if !(1..=8).contains(&name.len())
        || !name.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'$' | b'@' | b'#')
        })
    {
        return Err(HostProblem::Malformed);
    }
    Ok(name)
}

fn jiderr() -> HostProblem {
    HostProblem::Condition {
        name: "JIDERR".into(),
        response: 43,
        response2: 0,
    }
}

fn notopen() -> HostProblem {
    HostProblem::Condition {
        name: "NOTOPEN".into(),
        response: 19,
        response2: 0,
    }
}

fn encode(record: &JournalRecord, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(match record.availability {
        JournalAvailability::Open => 1,
        JournalAvailability::Disabled => 2,
        JournalAvailability::Failed => 3,
    });
    out.push(u8::from(record.current_request.is_some()));
    out.extend_from_slice(&record.current_request.unwrap_or_default().to_be_bytes());
    out.extend_from_slice(
        &u32::try_from(record.outputs.len())
            .map_err(|_| HostProblem::ResourceExhausted)?
            .to_be_bytes(),
    );
    for (request_id, output) in &record.outputs {
        out.extend_from_slice(&request_id.to_be_bytes());
        let owner = output.owner_run_unit.as_bytes();
        out.extend_from_slice(
            &u32::try_from(owner.len())
                .map_err(|_| HostProblem::ResourceExhausted)?
                .to_be_bytes(),
        );
        out.extend_from_slice(owner);
        out.push(match output.state {
            JournalOutputState::Pending => 1,
            JournalOutputState::Hardened => 2,
            JournalOutputState::IoError => 3,
        });
    }
    if out.len() > limits.max_queue_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(out)
}

fn decode(bytes: &[u8], version: u64, limits: CicsLimits) -> Result<JournalRecord, HostProblem> {
    if bytes.len() > limits.max_queue_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut reader = Reader { bytes };
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(HostProblem::InfrastructureFailure);
    }
    let availability = match reader.byte()? {
        1 => JournalAvailability::Open,
        2 => JournalAvailability::Disabled,
        3 => JournalAvailability::Failed,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let current_present = match reader.byte()? {
        0 => false,
        1 => true,
        _ => return Err(HostProblem::InfrastructureFailure),
    };
    let current = reader.i32()?;
    let count = usize::try_from(reader.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
    if count > limits.max_queue_records {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut outputs = BTreeMap::new();
    for _ in 0..count {
        let request_id = reader.i32()?;
        let owner = reader.field(1024)?;
        let owner_run_unit =
            String::from_utf8(owner).map_err(|_| HostProblem::InfrastructureFailure)?;
        let state = match reader.byte()? {
            1 => JournalOutputState::Pending,
            2 => JournalOutputState::Hardened,
            3 => JournalOutputState::IoError,
            _ => return Err(HostProblem::InfrastructureFailure),
        };
        if owner_run_unit.is_empty()
            || outputs
                .insert(
                    request_id,
                    JournalOutput {
                        owner_run_unit,
                        state,
                    },
                )
                .is_some()
        {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if !reader.bytes.is_empty()
        || current_present && !outputs.contains_key(&current)
        || version == 0
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(JournalRecord {
        availability,
        current_request: current_present.then_some(current),
        outputs,
        version,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], HostProblem> {
        if self.bytes.len() < count {
            return Err(HostProblem::InfrastructureFailure);
        }
        let (head, tail) = self.bytes.split_at(count);
        self.bytes = tail;
        Ok(head)
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

    fn i32(&mut self) -> Result<i32, HostProblem> {
        Ok(i32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| HostProblem::InfrastructureFailure)?,
        ))
    }

    fn field(&mut self, maximum: usize) -> Result<Vec<u8>, HostProblem> {
        let length = usize::try_from(self.u32()?).map_err(|_| HostProblem::ResourceExhausted)?;
        if length > maximum {
            return Err(HostProblem::ResourceExhausted);
        }
        Ok(self.take(length)?.to_vec())
    }
}
