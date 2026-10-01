use crate::service::{CicsLimits, cics_effect_replay_binding_digest, decode_cics_effect_replay};
use mainframe_env_execution_api::{IdempotencyKey, InvocationLimits};
use mainframe_env_host_api::{
    CicsUnitOfWorkOutcome, DatasetName, HostLimits, HostResult, canonical_result_digest,
};
use mainframe_env_store_api::{EffectDigestFormat, EffectRecord, EffectState, ProviderStateRecord};
use sha2::{Digest, Sha256};
use std::fmt;

mod container_replay {
    //! Shared private container replay codec and retention ownership validation.

    use super::{CicsReplayRetentionState, CicsReplayValidationError, describe_cics_replay_row};
    use crate::service::CicsLimits;
    use mainframe_env_execution_api::{
        ExecutionId, IdempotencyKey, InvocationLimits, PrincipalId, RunUnitId,
    };
    use mainframe_env_store_api::{EffectRecord, ExecutionRecord, ProviderStateRecord};
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct ContainerReplay {
        pub(crate) schema_version: u8,
        pub(crate) owner_execution: String,
        pub(crate) owner_principal: String,
        pub(crate) owner_run_unit: String,
        pub(crate) digest: String,
    }

    impl ContainerReplay {
        pub(crate) fn decode(row: &ProviderStateRecord) -> Result<Self, CicsReplayValidationError> {
            if row.namespace != "cics-container-replay-v1" {
                return Err(CicsReplayValidationError::WrongNamespace);
            }
            let limits = InvocationLimits::default();
            let key_limits = InvocationLimits {
                max_identity_bytes: 256,
                ..limits
            };
            if row.version != 1 || IdempotencyKey::new(&row.key, key_limits).is_err() {
                return Err(CicsReplayValidationError::InvalidIdentity);
            }
            if row.payload.len() > 4096 {
                return Err(CicsReplayValidationError::CorruptPayload);
            }
            let replay: Self = serde_json::from_slice(&row.payload)
                .map_err(|_| CicsReplayValidationError::CorruptPayload)?;
            if replay.schema_version != 1
                || ExecutionId::new(&replay.owner_execution, limits).is_err()
                || RunUnitId::new(&replay.owner_run_unit, limits).is_err()
                || PrincipalId::new(&replay.owner_principal, limits).is_err()
                || replay.digest.len() != 64
                || !replay
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(CicsReplayValidationError::CorruptPayload);
            }
            Ok(replay)
        }
    }

    /// Validate a private container receipt against its fully decoded outer CICS
    /// replay, completed core effect and terminal execution owner.
    ///
    /// This performs no mutation or age decision. Maintenance must also preserve
    /// recovery dependencies and fence the exact rows during the archive transaction.
    pub fn validate_cics_container_replay_row(
        row: &ProviderStateRecord,
        outer: &ProviderStateRecord,
        effect: &EffectRecord,
        execution: &ExecutionRecord,
        limits: CicsLimits,
    ) -> Result<(), CicsReplayValidationError> {
        let replay = ContainerReplay::decode(row)?;
        let descriptor = describe_cics_replay_row(outer, Some(effect), limits)?;
        if descriptor.retention != CicsReplayRetentionState::Terminal || !execution.state.terminal()
        {
            return Err(CicsReplayValidationError::CoreEffectNotCompleted);
        }
        if row.key != outer.key
            || descriptor.owner_execution.as_deref() != Some(replay.owner_execution.as_str())
            || descriptor.owner_run_unit.as_deref() != Some(replay.owner_run_unit.as_str())
            || execution.execution_id.as_str() != replay.owner_execution
            || execution.run_unit_id.as_str() != replay.owner_run_unit
            || execution.principal.as_str() != replay.owner_principal
        {
            return Err(CicsReplayValidationError::CoreEffectMismatch);
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn container_replay_codec_rejects_invalid_identity_bounds_and_trailing_bytes() {
            let replay = ContainerReplay {
                schema_version: 1,
                owner_execution: "execution".into(),
                owner_principal: "IBMUSER".into(),
                owner_run_unit: "run".into(),
                digest: "a".repeat(64),
            };
            let row = ProviderStateRecord {
                namespace: "cics-container-replay-v1".into(),
                key: "receipt".into(),
                version: 1,
                payload: serde_json::to_vec(&replay).unwrap(),
            };
            assert!(ContainerReplay::decode(&row).is_ok());
            // The private writers admit keys up to 256 bytes. Maintenance still
            // requires the matching outer/core identities under their own limits.
            assert!(
                ContainerReplay::decode(&ProviderStateRecord {
                    key: "k".repeat(256),
                    ..row.clone()
                })
                .is_ok()
            );
            for invalid in [
                ProviderStateRecord {
                    namespace: "other".into(),
                    ..row.clone()
                },
                ProviderStateRecord {
                    key: String::new(),
                    ..row.clone()
                },
                ProviderStateRecord {
                    key: "k".repeat(257),
                    ..row.clone()
                },
                ProviderStateRecord {
                    version: 0,
                    ..row.clone()
                },
                ProviderStateRecord {
                    payload: vec![b' '; 4097],
                    ..row.clone()
                },
                ProviderStateRecord {
                    payload: [row.payload.as_slice(), b"{}"].concat(),
                    ..row.clone()
                },
            ] {
                assert!(ContainerReplay::decode(&invalid).is_err());
            }
            for field in [
                "owner_execution",
                "owner_principal",
                "owner_run_unit",
                "digest",
            ] {
                let mut value = serde_json::to_value(&replay).unwrap();
                value[field] = "".into();
                assert!(
                    ContainerReplay::decode(&ProviderStateRecord {
                        payload: serde_json::to_vec(&value).unwrap(),
                        ..row.clone()
                    })
                    .is_err()
                );
            }
        }
    }
}
pub(crate) use container_replay::ContainerReplay;
pub use container_replay::validate_cics_container_replay_row;

pub(crate) const UOW_V1_MAGIC: &[u8; 5] = b"MECU1";
pub(crate) const UOW_V2_MAGIC: &[u8; 5] = b"MECU2";
const UNDO_V1_MAGIC: &[u8; 8] = b"MECUNDO1";
const MAX_IDENTITY_BYTES: usize = 128;
const MAX_TRANSACTION_BYTES: usize = 16;

/// Reserved binding added only by CICS to an internally dispatched nested effect.
pub const CICS_NESTED_EFFECT_ORIGIN_BINDING: &str = "cics.nested-effect-origin";
/// Schema of the exact replay-key payload carried by the nested-origin binding.
pub const CICS_NESTED_EFFECT_ORIGIN_SCHEMA: &str = "mainframe-env.cics.nested-effect-origin@1";
/// Reserved binding carrying the exact outer CICS effect key for a nested dispatch.
pub const CICS_OUTER_EFFECT_ORIGIN_BINDING: &str = "cics.outer-effect-origin";
/// Schema of the exact outer-effect-key binding payload.
pub const CICS_OUTER_EFFECT_ORIGIN_SCHEMA: &str = "mainframe-env.cics.outer-effect-origin@1";
/// Provider namespaces which Product must scan for CICS retention dependencies.
pub const CICS_RETENTION_NAMESPACES: [&str; 3] =
    ["cics-effect-replay-v1", "cics-uow", "cics-uow-undo"];

/// Durable codec generation of an outer CICS effect replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsReplayCodecVersion {
    /// Original replay without ownership metadata.
    LegacyV1,
    /// Owner/deadline-only transitional replay; always protected.
    AttributionV2,
    /// Full owner, run, sequence, result, and resolution bindings.
    RetentionV3,
}

/// Fail-closed age classification of an outer CICS replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsReplayRetentionState {
    /// The row predates full attribution and remains protected.
    LegacyProtected,
    /// The row is fully bound but has no successful clock observation.
    PendingProtected,
    /// The row has a trusted post-persistence resolution observation.
    Terminal,
}

/// Full retention description of one `cics-effect-replay-v1` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsReplayRowDescriptor {
    /// Exact provider-state namespace.
    pub namespace: String,
    /// Exact idempotency key.
    pub key: String,
    /// Exact compare-and-swap row version.
    pub row_version: u64,
    /// SHA-256 of the exact durable bytes.
    pub payload_digest: [u8; 32],
    /// Decoded codec generation.
    pub codec: CicsReplayCodecVersion,
    /// Retention classification.
    pub retention: CicsReplayRetentionState,
    /// Owning execution for current rows.
    pub owner_execution: Option<String>,
    /// Owning run unit for current rows.
    pub owner_run_unit: Option<String>,
    /// Exact outer effect sequence.
    pub sequence: Option<u64>,
    /// Conservative deadline lower bound.
    pub deadline_tick: Option<u64>,
    /// Trusted post-persistence resolution observation.
    pub resolution_tick: Option<u64>,
    /// Safe age origin for terminal rows.
    pub terminal_tick: Option<u64>,
    /// Canonical request digest.
    pub request_digest: [u8; 32],
    /// Canonical result digest.
    pub result_digest: [u8; 32],
}

/// Fail-closed outer CICS replay validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsReplayValidationError {
    /// Namespace mismatch.
    WrongNamespace,
    /// Invalid key or row version.
    InvalidIdentity,
    /// Malformed, oversized, duplicate, trailing, or inconsistent payload.
    CorruptPayload,
    /// A terminal current row has no matching completed core effect.
    MissingCoreEffect,
    /// The supplied effect is not canonically completed.
    CoreEffectNotCompleted,
    /// The supplied effect does not exactly own the replay.
    CoreEffectMismatch,
}

impl fmt::Display for CicsReplayValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongNamespace => "unexpected CICS replay namespace",
            Self::InvalidIdentity => "invalid CICS replay identity",
            Self::CorruptPayload => "corrupt CICS replay payload",
            Self::MissingCoreEffect => "CICS replay is missing its core effect",
            Self::CoreEffectNotCompleted => "CICS replay core effect is not completed",
            Self::CoreEffectMismatch => "CICS replay does not match its core effect",
        })
    }
}

impl std::error::Error for CicsReplayValidationError {}

/// Fully decode and validate one outer CICS effect replay row.
pub fn describe_cics_replay_row(
    row: &ProviderStateRecord,
    effect: Option<&EffectRecord>,
    limits: CicsLimits,
) -> Result<CicsReplayRowDescriptor, CicsReplayValidationError> {
    if row.namespace != "cics-effect-replay-v1" {
        return Err(CicsReplayValidationError::WrongNamespace);
    }
    if row.version == 0
        || row.version > i64::MAX as u64
        || IdempotencyKey::new(&row.key, InvocationLimits::default()).is_err()
    {
        return Err(CicsReplayValidationError::InvalidIdentity);
    }
    if row.payload.len() > limits.max_queue_bytes {
        return Err(CicsReplayValidationError::CorruptPayload);
    }
    let replay = decode_cics_effect_replay(&row.payload, limits)
        .map_err(|_| CicsReplayValidationError::CorruptPayload)?;
    HostResult::Cics(replay.response.clone())
        .validate(HostLimits {
            max_name_bytes: 128,
            max_record_bytes: limits.max_screen_bytes,
            max_records: limits.max_queue_records,
            max_fields: limits.max_fields,
            max_state_bytes: limits.max_screen_bytes,
            ..HostLimits::default()
        })
        .map_err(|_| CicsReplayValidationError::CorruptPayload)?;
    let result_digest = canonical_result_digest(&Ok(HostResult::Cics(replay.response.clone())))
        .map_err(|_| CicsReplayValidationError::CorruptPayload)?;
    let codec = match row.payload.get(..8) {
        Some(b"MECER001") => CicsReplayCodecVersion::LegacyV1,
        Some(b"MECER002") => CicsReplayCodecVersion::AttributionV2,
        Some(b"MECER003") => CicsReplayCodecVersion::RetentionV3,
        _ => return Err(CicsReplayValidationError::CorruptPayload),
    };
    let current = codec == CicsReplayCodecVersion::RetentionV3;
    if current {
        if replay.effect_key.as_deref() != Some(row.key.as_str())
            || replay.result_digest != Some(result_digest)
            || replay.binding_digest != Some(cics_effect_replay_binding_digest(&replay))
        {
            return Err(CicsReplayValidationError::CorruptPayload);
        }
        match (row.version, replay.resolution_tick) {
            (1, None) | (2, Some(_)) => {}
            _ => return Err(CicsReplayValidationError::CorruptPayload),
        }
    } else if row.version != 1 {
        return Err(CicsReplayValidationError::CorruptPayload);
    }
    let retention = if !current {
        CicsReplayRetentionState::LegacyProtected
    } else if replay.resolution_tick.is_none() {
        CicsReplayRetentionState::PendingProtected
    } else {
        CicsReplayRetentionState::Terminal
    };
    if retention == CicsReplayRetentionState::Terminal {
        validate_cics_replay_effect(effect, row, &replay, result_digest)?;
    }
    Ok(CicsReplayRowDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest: Sha256::digest(&row.payload).into(),
        codec,
        retention,
        owner_execution: replay.owner_execution,
        owner_run_unit: replay.owner_run_unit,
        sequence: replay.sequence,
        deadline_tick: replay.deadline_tick,
        resolution_tick: replay.resolution_tick,
        terminal_tick: replay.resolution_tick,
        request_digest: replay.request_digest,
        result_digest,
    })
}

fn validate_cics_replay_effect(
    effect: Option<&EffectRecord>,
    row: &ProviderStateRecord,
    replay: &crate::service::CicsEffectReplay,
    result_digest: [u8; 32],
) -> Result<(), CicsReplayValidationError> {
    let effect = effect.ok_or(CicsReplayValidationError::MissingCoreEffect)?;
    if effect.state != EffectState::Completed
        || effect.digest_format != EffectDigestFormat::CanonicalHostV1
        || effect.result_digest.is_none()
    {
        return Err(CicsReplayValidationError::CoreEffectNotCompleted);
    }
    if effect.key.as_str() != row.key
        || replay.owner_execution.as_deref() != Some(effect.execution_id.as_str())
        || replay.owner_run_unit.as_deref() != Some(effect.run_unit_id.as_str())
        || replay.sequence != Some(effect.sequence)
        || effect.intent.owner != effect.execution_id
        || effect.request_digest != replay.request_digest
        || effect.result_digest != Some(result_digest)
    {
        return Err(CicsReplayValidationError::CoreEffectMismatch);
    }
    Ok(())
}

/// On-disk codec generation used by a CICS unit-of-work row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsUowCodecVersion {
    /// Original row without durable ownership or age metadata.
    LegacyV1,
    /// Ownership- and age-aware row.
    RetentionV2,
}

/// Exact durable state represented by a CICS unit-of-work row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsUowState {
    /// Commit was requested but its durable outcome is still in doubt.
    CommitPending,
    /// Rollback was requested but its durable outcome is still in doubt.
    RollbackPending,
    /// Commit completed.
    Committed,
    /// Rollback completed.
    RolledBack,
}

impl CicsUowState {
    pub(crate) fn from_parts(finalized: bool, outcome: CicsUnitOfWorkOutcome) -> Self {
        match (finalized, outcome) {
            (false, CicsUnitOfWorkOutcome::Committed) => Self::CommitPending,
            (false, CicsUnitOfWorkOutcome::RolledBack) => Self::RollbackPending,
            (true, CicsUnitOfWorkOutcome::Committed) => Self::Committed,
            (true, CicsUnitOfWorkOutcome::RolledBack) => Self::RolledBack,
        }
    }

    pub(crate) fn parts(self) -> (bool, CicsUnitOfWorkOutcome) {
        match self {
            Self::CommitPending => (false, CicsUnitOfWorkOutcome::Committed),
            Self::RollbackPending => (false, CicsUnitOfWorkOutcome::RolledBack),
            Self::Committed => (true, CicsUnitOfWorkOutcome::Committed),
            Self::RolledBack => (true, CicsUnitOfWorkOutcome::RolledBack),
        }
    }

    fn byte(self) -> u8 {
        match self {
            Self::CommitPending => b'C',
            Self::RollbackPending => b'R',
            Self::Committed => b'c',
            Self::RolledBack => b'r',
        }
    }

    fn from_byte(value: u8) -> Result<Self, CicsUowValidationError> {
        match value {
            b'C' => Ok(Self::CommitPending),
            b'R' => Ok(Self::RollbackPending),
            b'c' => Ok(Self::Committed),
            b'r' => Ok(Self::RolledBack),
            _ => Err(CicsUowValidationError::CorruptPayload),
        }
    }

    fn terminal(self) -> bool {
        matches!(self, Self::Committed | Self::RolledBack)
    }
}

/// Durable dependencies that decide whether a CICS UOW row can be retained externally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsUowDependencyState {
    /// The UOW is still active or in doubt.
    Active,
    /// The terminal row predates ownership metadata and requires explicit protection.
    LegacyTerminal,
    /// The row is attributed but no nonzero terminal observation has been recorded.
    TerminalObservationRequired,
    /// A valid undo log still exists for the owning run unit.
    UndoLogPresent,
    /// UOW-local dependencies are clear; the caller must still verify the owning execution.
    Clear,
}

/// Validated retention metadata for one `cics-uow` provider-state row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsUowRowDescriptor {
    /// Exact provider-state namespace.
    pub namespace: String,
    /// Provider-state key, normally the outer effect idempotency key.
    pub key: String,
    /// Compare-and-swap version of the provider-state row.
    pub row_version: u64,
    /// SHA-256 of the exact durable payload.
    pub payload_digest: [u8; 32],
    /// Payload codec generation.
    pub codec: CicsUowCodecVersion,
    /// Exact commit/rollback state.
    pub state: CicsUowState,
    /// CICS transaction identifier.
    pub transaction: String,
    /// Exact outer effect key bound into current metadata.
    pub effect_key: Option<String>,
    /// Owning execution for a version-2 row.
    pub owner_execution: Option<String>,
    /// Owning run unit for a version-2 row.
    pub owner_run_unit: Option<String>,
    /// Original conservative deadline lower bound.
    pub deadline_tick: Option<u64>,
    /// Conservative nonzero terminal observation tick for an eligible row.
    pub terminal_tick: Option<u64>,
    /// Durable UOW-local dependency status; owner liveness is checked separately.
    pub dependency: CicsUowDependencyState,
}

/// Validated identity and content summary for one `cics-uow-undo` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CicsUndoRowDescriptor {
    /// Owning run-unit key.
    pub owner_run_unit: String,
    /// Compare-and-swap row version, equal to the number of undo operations.
    pub row_version: u64,
    /// Number of fully decoded undo operations.
    pub operation_count: usize,
    /// SHA-256 of the exact durable payload for archive evidence and race checks.
    pub payload_digest: [u8; 32],
}

/// Fail-closed validation failure for a CICS UOW or undo row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CicsUowValidationError {
    /// The provider-state namespace is not the expected CICS namespace.
    WrongNamespace,
    /// A key or row version is empty/zero.
    InvalidIdentity,
    /// The payload is truncated, malformed, internally inconsistent, or unsupported.
    CorruptPayload,
    /// The supplied undo row does not belong to the UOW's owning run unit.
    MismatchedUndo,
}

impl fmt::Display for CicsUowValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WrongNamespace => "unexpected CICS provider-state namespace",
            Self::InvalidIdentity => "invalid CICS provider-state identity",
            Self::CorruptPayload => "corrupt CICS unit-of-work payload",
            Self::MismatchedUndo => "CICS undo row does not match its unit of work",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CicsUowValidationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UowRetentionMetadata {
    pub(crate) effect_key: String,
    pub(crate) owner_execution: String,
    pub(crate) owner_run_unit: String,
    pub(crate) deadline_tick: u64,
    pub(crate) terminal_tick: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedUow {
    pub(crate) state: CicsUowState,
    pub(crate) transaction: String,
    pub(crate) metadata: Option<UowRetentionMetadata>,
}

/// Decode and validate a `cics-uow` row together with its optional undo dependency.
///
/// Legacy terminal rows, active rows, and terminal rows with an undo log are returned
/// as explicitly protected dependency states. Corrupt or mismatched input is an error;
/// callers must never interpret an error as retention eligibility.
pub fn describe_cics_uow_row(
    row: &ProviderStateRecord,
    undo: Option<&ProviderStateRecord>,
) -> Result<CicsUowRowDescriptor, CicsUowValidationError> {
    if row.namespace != "cics-uow" {
        return Err(CicsUowValidationError::WrongNamespace);
    }
    if row.key.is_empty() || row.version == 0 || row.version > i64::MAX as u64 {
        return Err(CicsUowValidationError::InvalidIdentity);
    }
    let decoded = decode_uow(&row.payload)?;
    if decoded
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.effect_key != row.key)
    {
        return Err(CicsUowValidationError::InvalidIdentity);
    }
    if (decoded.state.terminal() && row.version != 2)
        || (!decoded.state.terminal() && row.version != 1)
    {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    if let Some(undo) = undo {
        validate_undo_row(undo, decoded.metadata.as_ref())?;
    }
    let dependency = if !decoded.state.terminal() {
        CicsUowDependencyState::Active
    } else if decoded.metadata.is_none() {
        CicsUowDependencyState::LegacyTerminal
    } else if decoded
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.terminal_tick.is_none())
    {
        CicsUowDependencyState::TerminalObservationRequired
    } else if undo.is_some() {
        CicsUowDependencyState::UndoLogPresent
    } else {
        CicsUowDependencyState::Clear
    };
    let codec = if decoded.metadata.is_some() {
        CicsUowCodecVersion::RetentionV2
    } else {
        CicsUowCodecVersion::LegacyV1
    };
    Ok(CicsUowRowDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest: Sha256::digest(&row.payload).into(),
        codec,
        state: decoded.state,
        transaction: decoded.transaction,
        effect_key: decoded
            .metadata
            .as_ref()
            .map(|metadata| metadata.effect_key.clone()),
        owner_execution: decoded
            .metadata
            .as_ref()
            .map(|metadata| metadata.owner_execution.clone()),
        owner_run_unit: decoded
            .metadata
            .as_ref()
            .map(|metadata| metadata.owner_run_unit.clone()),
        deadline_tick: decoded
            .metadata
            .as_ref()
            .map(|metadata| metadata.deadline_tick),
        terminal_tick: decoded.metadata.as_ref().and_then(|metadata| {
            metadata
                .terminal_tick
                .map(|tick| tick.max(metadata.deadline_tick))
        }),
        dependency,
    })
}

pub(crate) fn encode_uow(decoded: &DecodedUow) -> Result<Vec<u8>, CicsUowValidationError> {
    validate_transaction(&decoded.transaction)?;
    let Some(metadata) = &decoded.metadata else {
        let mut payload = UOW_V1_MAGIC.to_vec();
        payload.push(decoded.state.byte());
        put_field(&mut payload, decoded.transaction.as_bytes())?;
        return Ok(payload);
    };
    validate_metadata(decoded.state, metadata)?;
    let mut payload = UOW_V2_MAGIC.to_vec();
    payload.push(decoded.state.byte());
    put_field(&mut payload, decoded.transaction.as_bytes())?;
    put_field(&mut payload, metadata.effect_key.as_bytes())?;
    put_field(&mut payload, metadata.owner_execution.as_bytes())?;
    put_field(&mut payload, metadata.owner_run_unit.as_bytes())?;
    payload.extend_from_slice(&metadata.deadline_tick.to_be_bytes());
    payload.extend_from_slice(&metadata.terminal_tick.unwrap_or(0).to_be_bytes());
    Ok(payload)
}

pub(crate) fn decode_uow(payload: &[u8]) -> Result<DecodedUow, CicsUowValidationError> {
    let mut reader = RowReader::new(payload);
    let magic = reader.take(5)?;
    let codec = if magic == UOW_V1_MAGIC {
        CicsUowCodecVersion::LegacyV1
    } else if magic == UOW_V2_MAGIC {
        CicsUowCodecVersion::RetentionV2
    } else {
        return Err(CicsUowValidationError::CorruptPayload);
    };
    let state = CicsUowState::from_byte(reader.byte()?)?;
    let transaction = reader.text_field(MAX_TRANSACTION_BYTES)?;
    validate_transaction(&transaction)?;
    let metadata = match codec {
        CicsUowCodecVersion::LegacyV1 => None,
        CicsUowCodecVersion::RetentionV2 => {
            let metadata = UowRetentionMetadata {
                effect_key: reader.text_field(MAX_IDENTITY_BYTES)?,
                owner_execution: reader.text_field(MAX_IDENTITY_BYTES)?,
                owner_run_unit: reader.text_field(MAX_IDENTITY_BYTES)?,
                deadline_tick: reader.u64()?,
                terminal_tick: match reader.u64()? {
                    0 => None,
                    value => Some(value),
                },
            };
            validate_metadata(state, &metadata)?;
            Some(metadata)
        }
    };
    if !reader.finished() {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    Ok(DecodedUow {
        state,
        transaction,
        metadata,
    })
}

fn validate_metadata(
    state: CicsUowState,
    metadata: &UowRetentionMetadata,
) -> Result<(), CicsUowValidationError> {
    if !valid_identity(&metadata.effect_key)
        || !valid_identity(&metadata.owner_execution)
        || !valid_identity(&metadata.owner_run_unit)
        || metadata.deadline_tick == 0
        || metadata.terminal_tick == Some(0)
        || metadata
            .terminal_tick
            .is_some_and(|tick| tick < metadata.deadline_tick)
        || (!state.terminal() && metadata.terminal_tick.is_some())
    {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    Ok(())
}

fn validate_transaction(value: &str) -> Result<(), CicsUowValidationError> {
    if value.is_empty()
        || value.len() > MAX_TRANSACTION_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    Ok(())
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
        })
}

fn validate_undo_row(
    row: &ProviderStateRecord,
    metadata: Option<&UowRetentionMetadata>,
) -> Result<(), CicsUowValidationError> {
    let descriptor = describe_cics_undo_row(row)?;
    let Some(metadata) = metadata else {
        return Err(CicsUowValidationError::MismatchedUndo);
    };
    if descriptor.owner_run_unit != metadata.owner_run_unit {
        return Err(CicsUowValidationError::MismatchedUndo);
    }
    Ok(())
}

/// Fully decode one `cics-uow-undo` row for global orphan/dependency scans.
///
/// The durable append invariant requires the row version to equal the decoded
/// operation count. Any mismatch or trailing byte is corruption and must block
/// retention rather than being treated as an absent dependency.
pub fn describe_cics_undo_row(
    row: &ProviderStateRecord,
) -> Result<CicsUndoRowDescriptor, CicsUowValidationError> {
    if row.namespace != "cics-uow-undo" {
        return Err(CicsUowValidationError::WrongNamespace);
    }
    if row.key.is_empty() || row.version == 0 || !valid_identity(&row.key) {
        return Err(CicsUowValidationError::InvalidIdentity);
    }
    let mut reader = RowReader::new(&row.payload);
    if reader.take(UNDO_V1_MAGIC.len())? != UNDO_V1_MAGIC {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    let count =
        usize::try_from(reader.u32()?).map_err(|_| CicsUowValidationError::CorruptPayload)?;
    if count == 0 || count > 65_536 {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    if usize::try_from(row.version) != Ok(count) {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    for _ in 0..count {
        match reader.byte()? {
            1 => {
                validate_dataset_name(&reader.text_field(MAX_IDENTITY_BYTES)?)?;
                let _ = reader.field(64 * 1024 * 1024)?;
                let _ = reader.field(64 * 1024 * 1024)?;
            }
            2 => {
                validate_dataset_name(&reader.text_field(MAX_IDENTITY_BYTES)?)?;
                let _ = reader.field(64 * 1024 * 1024)?;
            }
            _ => return Err(CicsUowValidationError::CorruptPayload),
        }
    }
    if !reader.finished() {
        return Err(CicsUowValidationError::CorruptPayload);
    }
    Ok(CicsUndoRowDescriptor {
        owner_run_unit: row.key.clone(),
        row_version: row.version,
        operation_count: count,
        payload_digest: Sha256::digest(&row.payload).into(),
    })
}

fn validate_dataset_name(value: &str) -> Result<(), CicsUowValidationError> {
    DatasetName::new(value, MAX_IDENTITY_BYTES)
        .map(|_| ())
        .map_err(|_| CicsUowValidationError::CorruptPayload)
}

fn put_field(out: &mut Vec<u8>, value: &[u8]) -> Result<(), CicsUowValidationError> {
    let length = u32::try_from(value.len()).map_err(|_| CicsUowValidationError::CorruptPayload)?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(value);
    Ok(())
}

struct RowReader<'a> {
    payload: &'a [u8],
    at: usize,
}

impl<'a> RowReader<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, at: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], CicsUowValidationError> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(CicsUowValidationError::CorruptPayload)?;
        let value = self
            .payload
            .get(self.at..end)
            .ok_or(CicsUowValidationError::CorruptPayload)?;
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, CicsUowValidationError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CicsUowValidationError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| CicsUowValidationError::CorruptPayload)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, CicsUowValidationError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| CicsUowValidationError::CorruptPayload)?,
        ))
    }

    fn field(&mut self, max: usize) -> Result<&'a [u8], CicsUowValidationError> {
        let length =
            usize::try_from(self.u32()?).map_err(|_| CicsUowValidationError::CorruptPayload)?;
        if length > max {
            return Err(CicsUowValidationError::CorruptPayload);
        }
        self.take(length)
    }

    fn text_field(&mut self, max: usize) -> Result<String, CicsUowValidationError> {
        String::from_utf8(self.field(max)?.to_vec())
            .map_err(|_| CicsUowValidationError::CorruptPayload)
    }

    fn finished(&self) -> bool {
        self.at == self.payload.len()
    }
}
