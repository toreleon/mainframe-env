//! Strict JES spool row decoding and retention eligibility.

use crate::service::SpoolLimits;
use mainframe_env_host_api::SpoolResult;
use mainframe_env_store_api::ProviderStateRecord;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Current durable JES spool state contract.
pub const SPOOL_STATE_CONTRACT: &str = "mainframe-env.spool-state@2";
pub(crate) const LEGACY_SPOOL_STATE_CONTRACT: &str = "mainframe-env.spool-state@1";
pub(crate) const STATE_NAMESPACE: &str = "jes-spool";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileState {
    pub(crate) artifacts: Vec<String>,
    pub(crate) record_count: u64,
    pub(crate) byte_count: u64,
    pub(crate) sealed: bool,
    pub(crate) version: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Replay {
    pub(crate) request_digest: String,
    pub(crate) result: SpoolResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JobState {
    pub(crate) schema_version: String,
    pub(crate) files: BTreeMap<String, FileState>,
    pub(crate) replay: BTreeMap<String, Replay>,
    pub(crate) purge_pending: bool,
    pub(crate) purged: bool,
    #[serde(default)]
    pub(crate) purge_boundary_tick: Option<u64>,
    #[serde(default)]
    pub(crate) purged_tick: Option<u64>,
}

#[derive(Serialize)]
struct LegacyJobState<'a> {
    schema_version: &'a str,
    files: &'a BTreeMap<String, FileState>,
    replay: &'a BTreeMap<String, Replay>,
    purge_pending: bool,
    purged: bool,
}

impl Default for JobState {
    fn default() -> Self {
        Self {
            schema_version: SPOOL_STATE_CONTRACT.into(),
            files: BTreeMap::new(),
            replay: BTreeMap::new(),
            purge_pending: false,
            purged: false,
            purge_boundary_tick: None,
            purged_tick: None,
        }
    }
}

/// On-disk generation used by a validated spool job row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpoolRowCodecVersion {
    /// Original state without a trusted purge observation.
    LegacyV1,
    /// State with explicit purge-boundary and completion metadata.
    RetentionV2,
}

/// Exact lifecycle represented by a validated spool job row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpoolRowState {
    /// Files and replay identities are still serving a live job.
    Live,
    /// Artifact deletion started but has not durably completed.
    PurgePending,
    /// Every referenced artifact was deleted and the job is physically purged.
    Purged,
}

/// Why a spool row is protected or eligible for age-based retention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpoolRetentionState {
    /// A live job must remain available.
    LiveJob,
    /// Purge recovery and its replay identity are still required.
    PurgeRecovery,
    /// Purge completed without current, nonzero age metadata.
    LegacyProtected,
    /// Physical purge completed with a trusted observation tick.
    Terminal,
}

/// Fully validated retention description for one `jes-spool` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpoolRetentionDescriptor {
    /// Provider-state namespace.
    pub namespace: String,
    /// Durable JES job identifier.
    pub key: String,
    /// Compare-and-swap provider row version.
    pub row_version: u64,
    /// Payload codec generation.
    pub codec: SpoolRowCodecVersion,
    /// Exact spool lifecycle state.
    pub state: SpoolRowState,
    /// Retention classification.
    pub retention: SpoolRetentionState,
    /// Latest trusted purge boundary retained across recovery attempts.
    pub purge_boundary_tick: Option<u64>,
    /// Nonzero observation of completed physical purge, when current.
    pub terminal_tick: Option<u64>,
    /// Number of retained mutation replay identities.
    pub replay_count: usize,
}

/// A spool row that fails validation must remain protected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpoolRetentionValidationError {
    /// The row is not in the JES spool namespace.
    WrongNamespace,
    /// The job key or compare-and-swap version is invalid.
    InvalidIdentity,
    /// JSON or a nested durable field is malformed.
    CorruptPayload,
    /// Lifecycle flags, age metadata, and durable contents disagree.
    InconsistentState,
}

impl fmt::Display for SpoolRetentionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongNamespace => "unexpected JES spool provider-state namespace",
            Self::InvalidIdentity => "invalid JES spool provider-state identity",
            Self::CorruptPayload => "corrupt JES spool provider-state payload",
            Self::InconsistentState => "inconsistent JES spool lifecycle state",
        })
    }
}

impl std::error::Error for SpoolRetentionValidationError {}

pub(crate) fn decode_job_state(
    row: &ProviderStateRecord,
    limits: SpoolLimits,
) -> Result<(JobState, SpoolRowCodecVersion), SpoolRetentionValidationError> {
    if row.namespace != STATE_NAMESPACE {
        return Err(SpoolRetentionValidationError::WrongNamespace);
    }
    if row.version == 0 || row.version > i64::MAX as u64 || !valid_job_name(&row.key) {
        return Err(SpoolRetentionValidationError::InvalidIdentity);
    }
    let state: JobState = serde_json::from_slice(&row.payload)
        .map_err(|_| SpoolRetentionValidationError::CorruptPayload)?;
    let codec = match state.schema_version.as_str() {
        LEGACY_SPOOL_STATE_CONTRACT => SpoolRowCodecVersion::LegacyV1,
        SPOOL_STATE_CONTRACT => SpoolRowCodecVersion::RetentionV2,
        _ => return Err(SpoolRetentionValidationError::CorruptPayload),
    };
    let canonical = match codec {
        SpoolRowCodecVersion::LegacyV1 => serde_json::to_vec(&LegacyJobState {
            schema_version: &state.schema_version,
            files: &state.files,
            replay: &state.replay,
            purge_pending: state.purge_pending,
            purged: state.purged,
        }),
        SpoolRowCodecVersion::RetentionV2 => serde_json::to_vec(&state),
    }
    .map_err(|_| SpoolRetentionValidationError::CorruptPayload)?;
    if canonical != row.payload {
        return Err(SpoolRetentionValidationError::CorruptPayload);
    }
    validate_state(&row.key, row.version, &state, codec, limits)?;
    Ok((state, codec))
}

/// Strictly decode a spool row and classify it for retention.
///
/// Only a version-2 row whose artifact references are empty, whose purge replay
/// is durable, and whose completion tick is nonzero is returned as terminal.
pub fn describe_spool_retention_row(
    row: &ProviderStateRecord,
    limits: SpoolLimits,
) -> Result<SpoolRetentionDescriptor, SpoolRetentionValidationError> {
    let (state, codec) = decode_job_state(row, limits)?;
    let lifecycle = if state.purge_pending {
        SpoolRowState::PurgePending
    } else if state.purged {
        SpoolRowState::Purged
    } else {
        SpoolRowState::Live
    };
    let retention = match lifecycle {
        SpoolRowState::Live => SpoolRetentionState::LiveJob,
        SpoolRowState::PurgePending => SpoolRetentionState::PurgeRecovery,
        SpoolRowState::Purged
            if codec == SpoolRowCodecVersion::RetentionV2
                && state.purge_boundary_tick.is_some()
                && state.purged_tick.is_some() =>
        {
            SpoolRetentionState::Terminal
        }
        SpoolRowState::Purged => SpoolRetentionState::LegacyProtected,
    };
    Ok(SpoolRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        codec,
        state: lifecycle,
        retention,
        purge_boundary_tick: state.purge_boundary_tick,
        terminal_tick: (retention == SpoolRetentionState::Terminal)
            .then_some(state.purged_tick)
            .flatten(),
        replay_count: state.replay.len(),
    })
}

fn validate_state(
    job: &str,
    row_version: u64,
    state: &JobState,
    codec: SpoolRowCodecVersion,
    limits: SpoolLimits,
) -> Result<(), SpoolRetentionValidationError> {
    if state.files.len() > limits.max_files_per_job
        || state.replay.is_empty()
        || state.replay.len() > limits.max_replays_per_job
        || state.purge_pending && state.purged
        || state.purge_boundary_tick == Some(0)
        || state.purged_tick == Some(0)
        || codec == SpoolRowCodecVersion::LegacyV1
            && (state.purge_boundary_tick.is_some() || state.purged_tick.is_some())
        || !state.purge_pending
            && !state.purged
            && (state.purge_boundary_tick.is_some() || state.purged_tick.is_some())
        || state.purge_pending && state.purged_tick.is_some()
        || state.purged && !state.files.is_empty()
        || state.purged && state.purged_tick.is_some() && state.purge_boundary_tick.is_none()
        || state
            .purged_tick
            .zip(state.purge_boundary_tick)
            .is_some_and(|(purged, boundary)| purged < boundary)
    {
        return Err(SpoolRetentionValidationError::InconsistentState);
    }
    let mut total_bytes = 0u64;
    for (name, file) in &state.files {
        if !valid_file_name(name)
            || file.version == 0
            || file.version > row_version
            || file.artifacts.is_empty()
            || file.artifacts.len() > limits.max_artifacts_per_file
            || file
                .artifacts
                .iter()
                .any(|artifact| !valid_artifact(artifact))
            || usize::try_from(file.record_count)
                .map_or(true, |count| count > limits.max_records_per_file)
            || file.record_count == 0
            || usize::try_from(file.byte_count)
                .map_or(true, |bytes| bytes > limits.max_bytes_per_job)
        {
            return Err(SpoolRetentionValidationError::InconsistentState);
        }
        total_bytes = total_bytes
            .checked_add(file.byte_count)
            .ok_or(SpoolRetentionValidationError::InconsistentState)?;
    }
    if usize::try_from(total_bytes).map_or(true, |bytes| bytes > limits.max_bytes_per_job) {
        return Err(SpoolRetentionValidationError::InconsistentState);
    }
    for (key, replay) in &state.replay {
        let SpoolResult::Mutated {
            version,
            replayed: false,
        } = replay.result
        else {
            return Err(SpoolRetentionValidationError::InconsistentState);
        };
        if !valid_provider_key(key)
            || !valid_request_digest(job, &replay.request_digest)
            || version == 0
            || version > row_version
        {
            return Err(SpoolRetentionValidationError::InconsistentState);
        }
    }
    if state.purged {
        let purge_digest = format!("purge:{job}");
        if !state.replay.values().any(|replay| {
            replay.request_digest == purge_digest
                && matches!(
                    replay.result,
                    SpoolResult::Mutated {
                        version,
                        replayed: false
                    } if version == row_version
                )
        }) {
            return Err(SpoolRetentionValidationError::InconsistentState);
        }
    }
    Ok(())
}

fn valid_job_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_uppercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'@' | b'#' | b'$' | b'-' | b'_')
        })
}

fn valid_provider_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
        })
}

fn valid_file_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn valid_artifact(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn valid_request_digest(job: &str, value: &str) -> bool {
    valid_artifact(value)
        || value
            .strip_prefix(&format!("seal:{job}:"))
            .is_some_and(valid_file_name)
        || value == format!("purge:{job}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: &JobState, version: u64) -> ProviderStateRecord {
        let payload = if state.schema_version == LEGACY_SPOOL_STATE_CONTRACT {
            serde_json::to_vec(&LegacyJobState {
                schema_version: &state.schema_version,
                files: &state.files,
                replay: &state.replay,
                purge_pending: state.purge_pending,
                purged: state.purged,
            })
            .unwrap()
        } else {
            serde_json::to_vec(state).unwrap()
        };
        ProviderStateRecord {
            namespace: STATE_NAMESPACE.into(),
            key: "JOB00001".into(),
            version,
            payload,
        }
    }

    fn terminal() -> JobState {
        JobState {
            schema_version: SPOOL_STATE_CONTRACT.into(),
            files: BTreeMap::new(),
            replay: BTreeMap::from([(
                "purge-key".into(),
                Replay {
                    request_digest: "purge:JOB00001".into(),
                    result: SpoolResult::Mutated {
                        version: 2,
                        replayed: false,
                    },
                },
            )]),
            purge_pending: false,
            purged: true,
            purge_boundary_tick: Some(100),
            purged_tick: Some(120),
        }
    }

    #[test]
    fn only_current_physically_purged_rows_with_both_ticks_are_terminal() {
        let descriptor =
            describe_spool_retention_row(&row(&terminal(), 2), SpoolLimits::default()).unwrap();
        assert_eq!(descriptor.state, SpoolRowState::Purged);
        assert_eq!(descriptor.retention, SpoolRetentionState::Terminal);
        assert_eq!(descriptor.purge_boundary_tick, Some(100));
        assert_eq!(descriptor.terminal_tick, Some(120));

        let mut compatibility = terminal();
        compatibility.purge_boundary_tick = None;
        compatibility.purged_tick = None;
        assert_eq!(
            describe_spool_retention_row(&row(&compatibility, 2), SpoolLimits::default())
                .unwrap()
                .retention,
            SpoolRetentionState::LegacyProtected
        );

        compatibility.purge_boundary_tick = Some(100);
        assert_eq!(
            describe_spool_retention_row(&row(&compatibility, 2), SpoolLimits::default())
                .unwrap()
                .retention,
            SpoolRetentionState::LegacyProtected,
            "an old boundary alone cannot age a later compatibility recovery"
        );

        compatibility.purge_boundary_tick = None;
        compatibility.schema_version = LEGACY_SPOOL_STATE_CONTRACT.into();
        assert_eq!(
            describe_spool_retention_row(&row(&compatibility, 2), SpoolLimits::default())
                .unwrap()
                .retention,
            SpoolRetentionState::LegacyProtected
        );
    }

    #[test]
    fn forged_age_and_future_replay_versions_fail_closed() {
        let mut missing_boundary = terminal();
        missing_boundary.purge_boundary_tick = None;
        assert_eq!(
            describe_spool_retention_row(&row(&missing_boundary, 2), SpoolLimits::default()),
            Err(SpoolRetentionValidationError::InconsistentState)
        );

        let mut regressed = terminal();
        regressed.purged_tick = Some(99);
        assert_eq!(
            describe_spool_retention_row(&row(&regressed, 2), SpoolLimits::default()),
            Err(SpoolRetentionValidationError::InconsistentState)
        );

        let mut future = terminal();
        future.replay.insert(
            "future".into(),
            Replay {
                request_digest: "seal:JOB00001:SYSOUT".into(),
                result: SpoolResult::Mutated {
                    version: 3,
                    replayed: false,
                },
            },
        );
        assert_eq!(
            describe_spool_retention_row(&row(&future, 2), SpoolLimits::default()),
            Err(SpoolRetentionValidationError::InconsistentState)
        );
    }

    #[test]
    fn live_and_pending_rows_are_explicitly_protected() {
        let mut live = JobState::default();
        live.files.insert(
            "SYSOUT".into(),
            FileState {
                artifacts: vec![format!("sha256:{}", "a".repeat(64))],
                record_count: 1,
                byte_count: 1,
                sealed: false,
                version: 1,
            },
        );
        live.replay.insert(
            "append".into(),
            Replay {
                request_digest: format!("sha256:{}", "b".repeat(64)),
                result: SpoolResult::Mutated {
                    version: 1,
                    replayed: false,
                },
            },
        );
        assert_eq!(
            describe_spool_retention_row(&row(&live, 1), SpoolLimits::default())
                .unwrap()
                .retention,
            SpoolRetentionState::LiveJob
        );
        live.purge_pending = true;
        live.purge_boundary_tick = Some(50);
        assert_eq!(
            describe_spool_retention_row(&row(&live, 2), SpoolLimits::default())
                .unwrap()
                .retention,
            SpoolRetentionState::PurgeRecovery
        );
    }
}
