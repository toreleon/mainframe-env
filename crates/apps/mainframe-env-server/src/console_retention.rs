//! Strict console-log codecs and retention integration seams.

use mainframe_env_store_api::ProviderStateRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

pub(crate) const CONSOLE_LOG_NAMESPACE: &str = "console-log";
pub(crate) const CONSOLE_LOG_CONTRACT: &str = "mainframe-env.console-log@2";
const MAX_CONSOLE_BYTES: usize = 238;
const MAX_TEXT_BYTES: usize = 1024 * 1024;

/// Durable payload generation used by a console-log row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConsoleLogCodecVersion {
    LegacyV1,
    RetentionV2,
}

/// Why a validated console-log row is protected or terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConsoleLogRetentionState {
    LegacyProtected,
    Terminal,
}

/// Durable ownership domain for a console response.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConsoleLogOwnerKind {
    /// A direct product route which does not create an `ExecutionRecord`.
    DirectProductRoute,
    /// A response owned by an admitted execution and run unit.
    Execution,
}

/// Durable owner which must outlive a console response.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ConsoleLogDependency {
    Execution(String),
    RunUnit(String),
}

/// Decoded console message used to rebuild an in-process read cache.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConsoleLogEntry {
    pub(crate) key: String,
    pub(crate) console: String,
    pub(crate) text: Vec<u8>,
}

/// Fully validated retention view of a console-log provider row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConsoleLogRetentionDescriptor {
    pub(crate) namespace: String,
    pub(crate) key: String,
    pub(crate) row_version: u64,
    pub(crate) codec: ConsoleLogCodecVersion,
    pub(crate) retention: ConsoleLogRetentionState,
    pub(crate) owner_kind: Option<ConsoleLogOwnerKind>,
    pub(crate) owner_execution: Option<String>,
    pub(crate) owner_run_unit: Option<String>,
    pub(crate) terminal_tick: Option<u64>,
    pub(crate) dependencies: Vec<ConsoleLogDependency>,
    pub(crate) payload_digest: [u8; 32],
}

/// Invalid console rows are never eligible for deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConsoleLogValidationError {
    WrongNamespace,
    InvalidIdentity,
    CorruptPayload,
    InconsistentState,
}

impl fmt::Display for ConsoleLogValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongNamespace => "unexpected console-log namespace",
            Self::InvalidIdentity => "invalid console-log identity",
            Self::CorruptPayload => "corrupt console-log payload",
            Self::InconsistentState => "inconsistent console-log owner or age metadata",
        })
    }
}

impl std::error::Error for ConsoleLogValidationError {}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConsoleLogV2 {
    contract: String,
    key: String,
    console: String,
    text: Vec<u8>,
    owner_execution: String,
    owner_run_unit: String,
    owner_kind: ConsoleLogOwnerKind,
    observed_tick: u64,
    metadata_digest: String,
}

enum DecodedConsoleLog {
    Legacy(ConsoleLogEntry),
    Current(ConsoleLogV2),
}

/// Encode a new owner- and age-attributed console-log payload.
pub(crate) fn encode_console_log(
    key: &str,
    console: &str,
    text: &[u8],
    owner_execution: &str,
    owner_run_unit: &str,
    observed_tick: u64,
) -> Result<Vec<u8>, ConsoleLogValidationError> {
    let mut value = ConsoleLogV2 {
        contract: CONSOLE_LOG_CONTRACT.into(),
        key: key.into(),
        console: console.into(),
        text: text.into(),
        owner_execution: owner_execution.into(),
        owner_run_unit: owner_run_unit.into(),
        owner_kind: ConsoleLogOwnerKind::DirectProductRoute,
        observed_tick,
        metadata_digest: String::new(),
    };
    value.metadata_digest = console_metadata_digest(&value);
    validate_current(&value)?;
    serde_json::to_vec(&value).map_err(|_| ConsoleLogValidationError::CorruptPayload)
}

/// Describe a console-log row without treating legacy or corrupt data as aged.
pub(crate) fn describe_console_log_row(
    row: &ProviderStateRecord,
) -> Result<ConsoleLogRetentionDescriptor, ConsoleLogValidationError> {
    let payload_digest = Sha256::digest(&row.payload).into();
    match decode_console_log(row)? {
        DecodedConsoleLog::Legacy(_) => Ok(ConsoleLogRetentionDescriptor {
            namespace: row.namespace.clone(),
            key: row.key.clone(),
            row_version: row.version,
            codec: ConsoleLogCodecVersion::LegacyV1,
            retention: ConsoleLogRetentionState::LegacyProtected,
            owner_kind: None,
            owner_execution: None,
            owner_run_unit: None,
            terminal_tick: None,
            dependencies: Vec::new(),
            payload_digest,
        }),
        DecodedConsoleLog::Current(value) => Ok(ConsoleLogRetentionDescriptor {
            namespace: row.namespace.clone(),
            key: row.key.clone(),
            row_version: row.version,
            codec: ConsoleLogCodecVersion::RetentionV2,
            retention: ConsoleLogRetentionState::Terminal,
            owner_kind: Some(value.owner_kind),
            owner_execution: Some(value.owner_execution.clone()),
            owner_run_unit: Some(value.owner_run_unit.clone()),
            terminal_tick: Some(value.observed_tick),
            dependencies: match value.owner_kind {
                ConsoleLogOwnerKind::DirectProductRoute => Vec::new(),
                ConsoleLogOwnerKind::Execution => vec![
                    ConsoleLogDependency::Execution(value.owner_execution),
                    ConsoleLogDependency::RunUnit(value.owner_run_unit),
                ],
            },
            payload_digest,
        }),
    }
}

/// Strictly rebuild console cache entries from the rows which remain durable.
pub(crate) fn decode_console_log_rows(
    rows: &[ProviderStateRecord],
) -> Result<Vec<ConsoleLogEntry>, ConsoleLogValidationError> {
    let mut entries = rows
        .iter()
        .map(|row| match decode_console_log(row)? {
            DecodedConsoleLog::Legacy(entry) => Ok(entry),
            DecodedConsoleLog::Current(value) => Ok(ConsoleLogEntry {
                key: value.key,
                console: value.console,
                text: value.text,
            }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    if entries.windows(2).any(|pair| pair[0].key == pair[1].key) {
        return Err(ConsoleLogValidationError::InconsistentState);
    }
    Ok(entries)
}

fn decode_console_log(
    row: &ProviderStateRecord,
) -> Result<DecodedConsoleLog, ConsoleLogValidationError> {
    validate_row(row)?;
    if let Ok(value) = serde_json::from_slice::<ConsoleLogV2>(&row.payload) {
        validate_current(&value)?;
        if value.key != row.key {
            return Err(ConsoleLogValidationError::InconsistentState);
        }
        return Ok(DecodedConsoleLog::Current(value));
    }
    let separator = row
        .payload
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(ConsoleLogValidationError::CorruptPayload)?;
    if row.payload[separator + 1..].contains(&0) {
        return Err(ConsoleLogValidationError::CorruptPayload);
    }
    let console = std::str::from_utf8(&row.payload[..separator])
        .map_err(|_| ConsoleLogValidationError::CorruptPayload)?;
    let text = &row.payload[separator + 1..];
    validate_content(console, text)?;
    Ok(DecodedConsoleLog::Legacy(ConsoleLogEntry {
        key: row.key.clone(),
        console: console.into(),
        text: text.into(),
    }))
}

fn validate_row(row: &ProviderStateRecord) -> Result<(), ConsoleLogValidationError> {
    if row.namespace != CONSOLE_LOG_NAMESPACE {
        return Err(ConsoleLogValidationError::WrongNamespace);
    }
    let sequence = row
        .key
        .parse::<u64>()
        .map_err(|_| ConsoleLogValidationError::InvalidIdentity)?;
    if row.version != 1 || sequence == 0 || format!("{sequence:016}") != row.key {
        return Err(ConsoleLogValidationError::InvalidIdentity);
    }
    Ok(())
}

fn validate_current(value: &ConsoleLogV2) -> Result<(), ConsoleLogValidationError> {
    if value.contract != CONSOLE_LOG_CONTRACT
        || value.observed_tick == 0
        || !valid_owner(&value.owner_execution)
        || !valid_owner(&value.owner_run_unit)
        || value.metadata_digest != console_metadata_digest(value)
        || value.key.parse::<u64>().is_err()
        || value
            .key
            .parse::<u64>()
            .is_ok_and(|sequence| sequence == 0 || format!("{sequence:016}") != value.key)
    {
        return Err(ConsoleLogValidationError::InconsistentState);
    }
    validate_content(&value.console, &value.text)
}

fn console_metadata_digest(value: &ConsoleLogV2) -> String {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.console-log-metadata@2\0");
    for field in [
        value.key.as_bytes(),
        value.console.as_bytes(),
        value.text.as_slice(),
        value.owner_execution.as_bytes(),
        value.owner_run_unit.as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update([match value.owner_kind {
        ConsoleLogOwnerKind::DirectProductRoute => 1,
        ConsoleLogOwnerKind::Execution => 2,
    }]);
    hash.update(value.observed_tick.to_be_bytes());
    format!("{:x}", hash.finalize())
}

fn validate_content(console: &str, text: &[u8]) -> Result<(), ConsoleLogValidationError> {
    if console.is_empty()
        || console.len() > MAX_CONSOLE_BYTES
        || !console.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(
                    byte,
                    b'@' | b'#' | b'$' | b'.' | b'*' | b'%' | b'-' | b'_' | b'/'
                )
        })
        || text.is_empty()
        || text.len() > MAX_TEXT_BYTES
        || text.contains(&0)
    {
        return Err(ConsoleLogValidationError::CorruptPayload);
    }
    Ok(())
}

fn valid_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy() -> ProviderStateRecord {
        ProviderStateRecord {
            namespace: CONSOLE_LOG_NAMESPACE.into(),
            key: "0000000000000001".into(),
            version: 1,
            payload: b"OPER\0IEE254I IPLINFO".to_vec(),
        }
    }

    fn current(key: &str, execution: &str, run: &str, tick: u64) -> ProviderStateRecord {
        ProviderStateRecord {
            namespace: CONSOLE_LOG_NAMESPACE.into(),
            key: key.into(),
            version: 1,
            payload: encode_console_log(key, "OPER", b"current", execution, run, tick).unwrap(),
        }
    }

    #[test]
    fn legacy_is_strictly_decoded_but_never_aged() {
        let row = legacy();
        let descriptor = describe_console_log_row(&row).unwrap();
        assert_eq!(descriptor.codec, ConsoleLogCodecVersion::LegacyV1);
        assert_eq!(
            descriptor.retention,
            ConsoleLogRetentionState::LegacyProtected
        );
        assert_eq!(descriptor.terminal_tick, None);
        let entries = decode_console_log_rows(&[row]).unwrap();
        assert_eq!(entries[0].console, "OPER");
    }

    #[test]
    fn replacement_codec_is_versioned_owned_and_ageable() {
        let replacement = current("0000000000000001", "execution-1", "run-1", 400);
        let descriptor = describe_console_log_row(&replacement).unwrap();
        assert_eq!(descriptor.codec, ConsoleLogCodecVersion::RetentionV2);
        assert_eq!(descriptor.retention, ConsoleLogRetentionState::Terminal);
        assert_eq!(
            descriptor.owner_kind,
            Some(ConsoleLogOwnerKind::DirectProductRoute)
        );
        assert_eq!(descriptor.terminal_tick, Some(400));
        assert!(descriptor.dependencies.is_empty());
    }

    #[test]
    fn direct_product_owner_is_not_an_execution_record_dependency() {
        let row = ProviderStateRecord {
            namespace: CONSOLE_LOG_NAMESPACE.into(),
            key: "0000000000000009".into(),
            version: 1,
            payload: encode_console_log(
                "0000000000000009",
                "OPER",
                b"direct response",
                "route-provenance-9",
                "route-run-9",
                900,
            )
            .unwrap(),
        };
        let descriptor = describe_console_log_row(&row).unwrap();
        assert_eq!(
            descriptor.owner_kind,
            Some(ConsoleLogOwnerKind::DirectProductRoute)
        );
        assert_eq!(
            descriptor.owner_execution.as_deref(),
            Some("route-provenance-9")
        );
        assert!(descriptor.dependencies.is_empty());
    }

    #[test]
    fn zero_age_unknown_fields_and_corruption_fail_closed() {
        assert_eq!(
            encode_console_log(
                "0000000000000001",
                "OPER",
                b"message",
                "execution-1",
                "run-1",
                0
            ),
            Err(ConsoleLogValidationError::InconsistentState)
        );
        let mut corrupt = legacy();
        corrupt.payload = b"OPER-without-separator".to_vec();
        assert_eq!(
            describe_console_log_row(&corrupt),
            Err(ConsoleLogValidationError::CorruptPayload)
        );
        let mut current = current("0000000000000001", "execution-1", "run-1", 5);
        let mut json: serde_json::Value = serde_json::from_slice(&current.payload).unwrap();
        json["unexpected"] = serde_json::json!(true);
        current.payload = serde_json::to_vec(&json).unwrap();
        assert_eq!(
            describe_console_log_row(&current),
            Err(ConsoleLogValidationError::CorruptPayload)
        );
    }

    #[test]
    fn cache_rebuild_uses_only_rows_remaining_after_external_removal() {
        let second = current("0000000000000002", "execution-2", "run-2", 6);
        let entries = decode_console_log_rows(&[second]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "0000000000000002");
    }
}
