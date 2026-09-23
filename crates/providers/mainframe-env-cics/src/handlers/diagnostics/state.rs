use crate::service::{CicsLimits, CicsService, handlers::store_error};
use mainframe_env_host_api::HostProblem;
use mainframe_env_store_api::ProviderStateRecord;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const NAMESPACE: &str = "cics-diagnostics-v1";
const KEY: &str = "state";
const MAGIC: &[u8; 8] = b"MECDIA01";

/// Local trace destinations and the main user-trace flag.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsTraceConfiguration {
    pub user_trace: bool,
    pub internal: bool,
    pub auxiliary: bool,
    pub system: bool,
}

impl Default for CicsTraceConfiguration {
    fn default() -> Self {
        Self {
            user_trace: false,
            internal: false,
            auxiliary: false,
            system: false,
        }
    }
}

/// One retained trace entry, with the captured bytes and issuing identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsDiagnosticTraceRecord {
    pub sequence: u64,
    pub kind: String,
    pub identifier: String,
    pub resource: String,
    pub data: Vec<u8>,
    pub exception: bool,
    pub run_unit: String,
    pub principal: String,
}

/// One retained local dump request and the bytes selected by its command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsDiagnosticDumpRecord {
    /// Strictly increasing sequence within the durable diagnostic row.
    pub sequence: u64,
    /// Local run/count identifier, formatted as `xxxx/yyyy`.
    pub dump_id: String,
    /// `TRANSACTION` or `DUMP`, identifying the captured local command form.
    pub scope: String,
    /// Requested dump code, including invalid source text when INVREQ/13 followed capture.
    pub code: String,
    /// Ordered section names present in `data`.
    pub sections: Vec<String>,
    /// `MECDMP01`, a big-endian section count, then length-prefixed names and bytes.
    pub data: Vec<u8>,
    /// Issuing run unit identity at capture time.
    pub run_unit: String,
    /// Issuing principal identity at capture time.
    pub principal: String,
}

/// Installed local dump-table policy for one four-character dump code.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsDumpCodeDefinition {
    pub code: String,
    pub suppress: bool,
    pub maximum: u32,
    pub system_dump: bool,
}

/// Supported local MCT user event action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CicsMonitorAction {
    AddCounter { slot: u16 },
    SubtractCounter { slot: u16 },
    OrCounter { slot: u16 },
    StartClock { slot: u16 },
    StopClock { slot: u16 },
    Move { offset: u16, maximum_length: u16 },
}

/// Installed local user event monitoring point.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CicsMonitorPointDefinition {
    pub entry_name: String,
    pub point: u8,
    pub action: CicsMonitorAction,
}

/// Read-only durable diagnostic view for region adapters and tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsDiagnosticSnapshot {
    pub configuration: CicsTraceConfiguration,
    pub traces: Vec<CicsDiagnosticTraceRecord>,
    pub dumps: Vec<CicsDiagnosticDumpRecord>,
    pub dump_definitions: BTreeMap<String, CicsDumpCodeDefinition>,
    pub monitor_definitions: BTreeMap<String, CicsMonitorPointDefinition>,
    pub monitor_counters: BTreeMap<String, i64>,
    pub monitor_clocks: BTreeMap<String, u64>,
    pub monitor_text: BTreeMap<String, Vec<u8>>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiagnosticReply {
    pub condition: String,
    pub response: i32,
    pub response2: i32,
    pub outputs: BTreeMap<String, Vec<u8>>,
}

impl DiagnosticReply {
    #[allow(dead_code)] // Activated by the first diagnostic command slice.
    pub fn normal() -> Self {
        Self {
            condition: "NORMAL".into(),
            response: 0,
            response2: 0,
            outputs: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiagnosticReplay {
    pub digest: [u8; 32],
    pub reply: DiagnosticReply,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiagnosticState {
    pub version: u64,
    pub next_sequence: u64,
    pub next_dump_count: u32,
    pub configuration: CicsTraceConfiguration,
    pub single_trace: bool,
    pub traces: Vec<CicsDiagnosticTraceRecord>,
    pub dumps: Vec<CicsDiagnosticDumpRecord>,
    pub dump_definitions: BTreeMap<String, CicsDumpCodeDefinition>,
    pub dump_counts: BTreeMap<String, u32>,
    pub monitor_definitions: BTreeMap<String, CicsMonitorPointDefinition>,
    pub monitor_counters: BTreeMap<String, i64>,
    pub monitor_clocks: BTreeMap<String, u64>,
    pub monitor_text: BTreeMap<String, Vec<u8>>,
    pub replays: BTreeMap<String, DiagnosticReplay>,
}

impl Default for DiagnosticState {
    fn default() -> Self {
        Self {
            version: 0,
            next_sequence: 1,
            next_dump_count: 1,
            configuration: CicsTraceConfiguration::default(),
            single_trace: false,
            traces: Vec::new(),
            dumps: Vec::new(),
            dump_definitions: BTreeMap::new(),
            dump_counts: BTreeMap::new(),
            monitor_definitions: BTreeMap::new(),
            monitor_counters: BTreeMap::new(),
            monitor_clocks: BTreeMap::new(),
            monitor_text: BTreeMap::new(),
            replays: BTreeMap::new(),
        }
    }
}

impl DiagnosticState {
    pub fn snapshot(&self) -> CicsDiagnosticSnapshot {
        CicsDiagnosticSnapshot {
            configuration: self.configuration,
            traces: self.traces.clone(),
            dumps: self.dumps.clone(),
            dump_definitions: self.dump_definitions.clone(),
            monitor_definitions: self.monitor_definitions.clone(),
            monitor_counters: self.monitor_counters.clone(),
            monitor_clocks: self.monitor_clocks.clone(),
            monitor_text: self.monitor_text.clone(),
            version: self.version,
        }
    }

    #[allow(dead_code)] // Activated by the first diagnostic command slice.
    pub fn replay(
        &self,
        key: &str,
        digest: [u8; 32],
    ) -> Result<Option<DiagnosticReply>, HostProblem> {
        let Some(record) = self.replays.get(key) else {
            return Ok(None);
        };
        if record.digest != digest {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(Some(record.reply.clone()))
    }

    #[allow(dead_code)] // Activated by the first diagnostic command slice.
    pub fn record_replay(
        &mut self,
        key: &str,
        digest: [u8; 32],
        reply: DiagnosticReply,
        limits: CicsLimits,
    ) -> Result<(), HostProblem> {
        if self.replays.len() >= limits.max_diagnostic_replays {
            return Err(HostProblem::ResourceExhausted);
        }
        if self
            .replays
            .insert(key.into(), DiagnosticReplay { digest, reply })
            .is_some()
        {
            return Err(HostProblem::IdempotencyConflict);
        }
        Ok(())
    }

    #[allow(dead_code)] // Activated by the first diagnostic command slice.
    pub fn allocate_sequence(&mut self) -> Result<u64, HostProblem> {
        let sequence = self.next_sequence;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or(HostProblem::ResourceExhausted)?;
        Ok(sequence)
    }
}

pub(super) fn load(service: &CicsService) -> Result<DiagnosticState, HostProblem> {
    match service
        .store
        .get_provider_state(NAMESPACE, KEY)
        .map_err(store_error)?
    {
        Some(row) => decode(&row.payload, row.version, service.limits),
        None => Ok(DiagnosticState::default()),
    }
}

pub(super) fn persist(
    service: &CicsService,
    current_version: u64,
    next: &mut DiagnosticState,
) -> Result<(), HostProblem> {
    next.version = current_version
        .checked_add(1)
        .ok_or(HostProblem::ResourceExhausted)?;
    let payload = encode(next, service.limits)?;
    service
        .store
        .put_provider_state(
            ProviderStateRecord {
                namespace: NAMESPACE.into(),
                key: KEY.into(),
                version: next.version,
                payload,
            },
            (current_version != 0).then_some(current_version),
        )
        .map_err(store_error)
}

fn encode(state: &DiagnosticState, limits: CicsLimits) -> Result<Vec<u8>, HostProblem> {
    validate(state, limits)?;
    let mut payload = MAGIC.to_vec();
    payload.extend(serde_json::to_vec(state).map_err(|_| HostProblem::InfrastructureFailure)?);
    if payload.len() > limits.max_diagnostic_bytes {
        return Err(HostProblem::ResourceExhausted);
    }
    Ok(payload)
}

fn decode(bytes: &[u8], version: u64, limits: CicsLimits) -> Result<DiagnosticState, HostProblem> {
    if bytes.len() > limits.max_diagnostic_bytes || !bytes.starts_with(MAGIC) {
        return Err(HostProblem::InfrastructureFailure);
    }
    let state: DiagnosticState = serde_json::from_slice(&bytes[MAGIC.len()..])
        .map_err(|_| HostProblem::InfrastructureFailure)?;
    if state.version != version
        || encode(&state, limits).map_err(|_| HostProblem::InfrastructureFailure)? != bytes
    {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(state)
}

fn validate(state: &DiagnosticState, limits: CicsLimits) -> Result<(), HostProblem> {
    if state.next_sequence == 0
        || state.next_dump_count == 0
        || state
            .traces
            .len()
            .checked_add(state.dumps.len())
            .is_none_or(|count| count > limits.max_diagnostic_entries)
        || state.replays.len() > limits.max_diagnostic_replays
        || state.monitor_counters.len() > limits.max_diagnostic_entries
        || state.monitor_clocks.len() > limits.max_diagnostic_entries
        || state.monitor_text.len() > limits.max_diagnostic_entries
        || state.monitor_definitions.len() > limits.max_diagnostic_entries
        || state.dump_definitions.len() > limits.max_diagnostic_entries
        || state.dump_counts.len() > limits.max_diagnostic_entries
        || state.monitor_text.values().any(|text| text.len() > 8192)
        || state
            .traces
            .iter()
            .any(|entry| entry.data.len() > limits.max_diagnostic_payload_bytes)
        || state
            .dumps
            .iter()
            .any(|entry| entry.data.len() > limits.max_diagnostic_payload_bytes)
    {
        return Err(HostProblem::ResourceExhausted);
    }
    let mut seen = BTreeSet::new();
    for sequence in state
        .traces
        .iter()
        .map(|entry| entry.sequence)
        .chain(state.dumps.iter().map(|entry| entry.sequence))
    {
        if sequence == 0 || sequence >= state.next_sequence || !seen.insert(sequence) {
            return Err(HostProblem::InfrastructureFailure);
        }
    }
    if state.replays.iter().any(|(key, replay)| {
        key.is_empty()
            || key.len() > 256
            || replay.reply.condition.len() > 32
            || replay.reply.outputs.iter().any(|(name, value)| {
                name.is_empty()
                    || name.len() > 32
                    || value.len() > limits.max_diagnostic_payload_bytes
            })
    }) {
        return Err(HostProblem::InfrastructureFailure);
    }
    if state.dump_definitions.iter().any(|(key, definition)| {
        key != &definition.code
            || key.is_empty()
            || key.len() > 4
            || !key.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'@' | b'#' | b'$')
            })
    }) || state.monitor_definitions.iter().any(|(key, definition)| {
        key != &format!("{}:{:03}", definition.entry_name, definition.point)
            || definition.entry_name.is_empty()
            || definition.entry_name.len() > 8
            || !(1..=199).contains(&definition.point)
            || !definition.entry_name.bytes().all(|byte| {
                byte.is_ascii_uppercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'@' | b'#' | b'$')
            })
            || match definition.action {
                CicsMonitorAction::AddCounter { slot }
                | CicsMonitorAction::SubtractCounter { slot }
                | CicsMonitorAction::OrCounter { slot }
                | CicsMonitorAction::StartClock { slot }
                | CicsMonitorAction::StopClock { slot } => slot > 255,
                CicsMonitorAction::Move {
                    offset,
                    maximum_length,
                } => {
                    maximum_length == 0 || usize::from(offset) + usize::from(maximum_length) > 8192
                }
            }
    }) {
        return Err(HostProblem::InfrastructureFailure);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_state_codec_rejects_corruption_and_capacity_excess() {
        let limits = CicsLimits::default();
        let mut state = DiagnosticState::default();
        state.version = 1;
        state.next_sequence = 2;
        state.traces.push(CicsDiagnosticTraceRecord {
            sequence: 1,
            kind: "TRACENUM".into(),
            identifier: "123".into(),
            resource: "PROG1".into(),
            data: b"payload".to_vec(),
            exception: false,
            run_unit: "run-1".into(),
            principal: "IBMUSER".into(),
        });
        let bytes = encode(&state, limits).unwrap();
        assert_eq!(decode(&bytes, 1, limits).unwrap(), state);
        assert_eq!(
            decode(&bytes, 2, limits),
            Err(HostProblem::InfrastructureFailure)
        );
        let mut corrupt = bytes.clone();
        corrupt.push(b' ');
        assert_eq!(
            decode(&corrupt, 1, limits),
            Err(HostProblem::InfrastructureFailure)
        );
        let smaller = CicsLimits {
            max_diagnostic_entries: 0,
            ..limits
        };
        assert_eq!(encode(&state, smaller), Err(HostProblem::ResourceExhausted));
        state.dumps.push(CicsDiagnosticDumpRecord {
            sequence: 1,
            dump_id: "1/0001".into(),
            scope: "transaction".into(),
            code: "ABCD".into(),
            sections: vec!["TASK".into()],
            data: Vec::new(),
            run_unit: "run-1".into(),
            principal: "IBMUSER".into(),
        });
        assert_eq!(
            encode(&state, limits),
            Err(HostProblem::InfrastructureFailure)
        );
    }
}
