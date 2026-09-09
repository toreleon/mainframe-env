use sha2::{Digest, Sha256};
use std::fmt;

pub(crate) const REPLAY_V1_MAGIC: &[u8; 5] = b"MEDR1";
pub(crate) const REPLAY_V2_MAGIC: &[u8; 5] = b"MEDR2";
pub(crate) const REPLAY_V3_MAGIC: &[u8; 5] = b"MEDR3";
const MAX_IDENTITY_BYTES: usize = 128;

/// Reserved CICS-only invocation binding that attests a nested replay owner.
pub const CICS_NESTED_EFFECT_ORIGIN_BINDING: &str = "cics.nested-effect-origin";
/// Schema for the exact nested replay-key binding payload.
pub const CICS_NESTED_EFFECT_ORIGIN_SCHEMA: &str = "mainframe-env.cics.nested-effect-origin@1";
/// Reserved CICS-only binding carrying the exact outer effect key.
pub const CICS_OUTER_EFFECT_ORIGIN_BINDING: &str = "cics.outer-effect-origin";
/// Schema for the exact outer-effect-key binding payload.
pub const CICS_OUTER_EFFECT_ORIGIN_SCHEMA: &str = "mainframe-env.cics.outer-effect-origin@1";
/// Provider namespace Product must scan for dataset replay dependencies.
pub const DATASET_REPLAY_NAMESPACE: &str = "dataset-replay";

/// On-disk codec generation used by a dataset replay row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetReplayCodecVersion {
    /// Original replay payload without durable owner or age metadata.
    LegacyV1,
    /// Replay envelope with owner, deadline, and resolution metadata.
    RetentionV2,
    /// Full replay envelope with explicit dependency origin and digest bindings.
    RetentionV3,
}

/// Persisted authority which owns a current dataset replay row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetReplayOwnerKind {
    /// A same-key completed core effect owns the row.
    CoreEffect,
    /// An explicitly attested CICS nested operation owns the row.
    CicsNested,
}

/// Whether a replay row is eligible for age-based retention.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetReplayRetentionState {
    /// A pre-full-codec row remains protected.
    LegacyProtected,
    /// Provenance is complete but the result or resolution observation is pending.
    PendingProtected,
    /// The resolved row has a trusted nonzero resolution observation.
    Terminal,
}

/// Whether the mutation represented by a replay row has a durable result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetReplayResultState {
    /// The replay row is an in-flight mutation intent.
    Pending,
    /// The replay row contains a durable mutation result.
    Resolved,
}

/// External dependencies that govern dataset replay retention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DatasetReplayDependencyState {
    /// The mutation is still pending and must remain live.
    PendingResult,
    /// The row is terminal but lacks attributable terminal-effect evidence.
    TerminalEffectRequired,
    /// A same-key completed core effect and its execution must be checked.
    CoreEffect,
    /// Exact CICS nested provenance; no same-key core effect may be required.
    CicsNested {
        /// Owning CICS run unit parsed from the exact replay key.
        run_unit: String,
        /// Nested sequence parsed from the exact replay key.
        sequence: u64,
        /// Exact completed outer CICS effect which authorized the nested call.
        outer_effect_key: String,
    },
}

/// Validated retention metadata for one `dataset-replay` provider-state row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatasetReplayRowDescriptor {
    /// Exact provider-state namespace.
    pub namespace: String,
    /// Provider-state key and effect idempotency key.
    pub key: String,
    /// Compare-and-swap version of the provider-state row.
    pub row_version: u64,
    /// SHA-256 of the exact durable payload.
    pub payload_digest: [u8; 32],
    /// Payload codec generation.
    pub codec: DatasetReplayCodecVersion,
    /// Whether the replay contains a durable result.
    pub result_state: DatasetReplayResultState,
    /// Fail-closed retention classification.
    pub retention: DatasetReplayRetentionState,
    /// Explicit current-row owner domain.
    pub owner_kind: Option<DatasetReplayOwnerKind>,
    /// Exact outer CICS effect key for a nested row.
    pub outer_effect_key: Option<String>,
    /// Owning execution for an attributed row.
    pub owner_execution: Option<String>,
    /// Owning run unit for an attributed row.
    pub owner_run_unit: Option<String>,
    /// Exact host-effect sequence for an attributed row.
    pub sequence: Option<u64>,
    /// Conservative effect deadline for an attributed row.
    pub deadline_tick: Option<u64>,
    /// Nonzero tick at which resolution was conservatively observed.
    pub resolution_tick: Option<u64>,
    /// Safe age origin, equal to the later of deadline and resolution.
    pub terminal_tick: Option<u64>,
    /// Canonical request digest decoded from the replay core.
    pub request_digest: [u8; 32],
    /// Canonical result digest, or zero while no result exists.
    pub result_digest: [u8; 32],
    /// Durable dependency status.
    pub dependency: DatasetReplayDependencyState,
}

/// Fail-closed validation failure for dataset replay retention metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetReplayValidationError {
    /// The provider-state namespace is not `dataset-replay`.
    WrongNamespace,
    /// A provider-state identity or row version is invalid.
    InvalidIdentity,
    /// The payload is truncated, malformed, inconsistent, or unsupported.
    CorruptPayload,
    /// The supplied effect does not own this replay row.
    EffectMismatch,
    /// The supplied effect has not completed successfully.
    EffectNotTerminal,
    /// A compare-and-swap version cannot advance safely.
    VersionExhausted,
}

impl fmt::Display for DatasetReplayValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::WrongNamespace => "unexpected dataset replay namespace",
            Self::InvalidIdentity => "invalid dataset replay identity",
            Self::CorruptPayload => "corrupt dataset replay payload",
            Self::EffectMismatch => "terminal effect does not own the dataset replay",
            Self::EffectNotTerminal => "dataset replay effect is not terminal",
            Self::VersionExhausted => "dataset replay row version is exhausted",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for DatasetReplayValidationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReplayRetentionMetadata {
    pub(crate) effect_key: String,
    pub(crate) owner_execution: String,
    pub(crate) owner_run_unit: String,
    pub(crate) owner_kind: Option<DatasetReplayOwnerKind>,
    pub(crate) outer_effect_key: Option<String>,
    pub(crate) sequence: u64,
    pub(crate) deadline_tick: u64,
    pub(crate) resolution_tick: Option<u64>,
    pub(crate) result_sha256: [u8; 32],
    pub(crate) binding_sha256: [u8; 32],
}

pub(crate) struct ReplayEnvelope<'a> {
    pub(crate) codec: DatasetReplayCodecVersion,
    pub(crate) metadata: Option<ReplayRetentionMetadata>,
    pub(crate) core: &'a [u8],
}

pub(crate) fn encode_replay_envelope(
    core: &[u8],
    metadata: &ReplayRetentionMetadata,
) -> Result<Vec<u8>, DatasetReplayValidationError> {
    if core.get(..REPLAY_V1_MAGIC.len()) != Some(REPLAY_V1_MAGIC)
        || !valid_identity(&metadata.effect_key)
        || !valid_identity(&metadata.owner_execution)
        || !valid_identity(&metadata.owner_run_unit)
        || metadata.owner_kind.is_none()
        || metadata.sequence == 0
        || metadata.deadline_tick == 0
        || metadata.resolution_tick == Some(0)
        || metadata
            .resolution_tick
            .is_some_and(|tick| tick < metadata.deadline_tick)
        || match metadata.owner_kind {
            Some(DatasetReplayOwnerKind::CoreEffect) => metadata.outer_effect_key.is_some(),
            Some(DatasetReplayOwnerKind::CicsNested) => metadata
                .outer_effect_key
                .as_deref()
                .is_none_or(|key| !valid_identity(key)),
            None => true,
        }
    {
        return Err(DatasetReplayValidationError::CorruptPayload);
    }
    let mut payload = REPLAY_V3_MAGIC.to_vec();
    put_field(&mut payload, metadata.effect_key.as_bytes())?;
    put_field(&mut payload, metadata.owner_execution.as_bytes())?;
    put_field(&mut payload, metadata.owner_run_unit.as_bytes())?;
    payload.push(match metadata.owner_kind {
        Some(DatasetReplayOwnerKind::CoreEffect) => 1,
        Some(DatasetReplayOwnerKind::CicsNested) => 2,
        None => return Err(DatasetReplayValidationError::CorruptPayload),
    });
    put_field(
        &mut payload,
        metadata
            .outer_effect_key
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    )?;
    payload.extend_from_slice(&metadata.sequence.to_be_bytes());
    payload.extend_from_slice(&metadata.deadline_tick.to_be_bytes());
    payload.extend_from_slice(&metadata.resolution_tick.unwrap_or(0).to_be_bytes());
    payload.extend_from_slice(&metadata.result_sha256);
    payload.extend_from_slice(&metadata.binding_sha256);
    put_field(&mut payload, core)?;
    Ok(payload)
}

pub(crate) fn decode_replay_envelope(
    payload: &[u8],
) -> Result<ReplayEnvelope<'_>, DatasetReplayValidationError> {
    if payload.get(..REPLAY_V1_MAGIC.len()) == Some(REPLAY_V1_MAGIC) {
        return Ok(ReplayEnvelope {
            codec: DatasetReplayCodecVersion::LegacyV1,
            metadata: None,
            core: payload,
        });
    }
    let mut reader = RowReader::new(payload);
    let magic = reader.take(REPLAY_V2_MAGIC.len())?;
    if magic != REPLAY_V2_MAGIC && magic != REPLAY_V3_MAGIC {
        return Err(DatasetReplayValidationError::CorruptPayload);
    }
    let effect_key = reader.text_field(MAX_IDENTITY_BYTES)?;
    let owner_execution = reader.text_field(MAX_IDENTITY_BYTES)?;
    let owner_run_unit = reader.text_field(MAX_IDENTITY_BYTES)?;
    let (owner_kind, outer_effect_key, sequence) = if magic == REPLAY_V3_MAGIC {
        let owner_kind = match reader.byte()? {
            1 => DatasetReplayOwnerKind::CoreEffect,
            2 => DatasetReplayOwnerKind::CicsNested,
            _ => return Err(DatasetReplayValidationError::CorruptPayload),
        };
        let outer_effect_key = reader.text_field(MAX_IDENTITY_BYTES)?;
        (
            Some(owner_kind),
            (!outer_effect_key.is_empty()).then_some(outer_effect_key),
            reader.u64()?,
        )
    } else {
        (None, None, 0)
    };
    let metadata = ReplayRetentionMetadata {
        effect_key,
        owner_execution,
        owner_run_unit,
        owner_kind,
        outer_effect_key,
        sequence,
        deadline_tick: reader.u64()?,
        resolution_tick: match reader.u64()? {
            0 => None,
            tick => Some(tick),
        },
        result_sha256: if magic == REPLAY_V3_MAGIC {
            reader
                .take(32)?
                .try_into()
                .map_err(|_| DatasetReplayValidationError::CorruptPayload)?
        } else {
            [0; 32]
        },
        binding_sha256: if magic == REPLAY_V3_MAGIC {
            reader
                .take(32)?
                .try_into()
                .map_err(|_| DatasetReplayValidationError::CorruptPayload)?
        } else {
            [0; 32]
        },
    };
    if !valid_identity(&metadata.effect_key)
        || !valid_identity(&metadata.owner_execution)
        || !valid_identity(&metadata.owner_run_unit)
        || metadata.deadline_tick == 0
        || metadata.resolution_tick == Some(0)
        || metadata
            .resolution_tick
            .is_some_and(|tick| tick < metadata.deadline_tick)
        || (magic == REPLAY_V3_MAGIC && (metadata.owner_kind.is_none() || metadata.sequence == 0))
        || (magic == REPLAY_V3_MAGIC
            && match metadata.owner_kind {
                Some(DatasetReplayOwnerKind::CoreEffect) => metadata.outer_effect_key.is_some(),
                Some(DatasetReplayOwnerKind::CicsNested) => metadata
                    .outer_effect_key
                    .as_deref()
                    .is_none_or(|key| !valid_identity(key)),
                None => true,
            })
    {
        return Err(DatasetReplayValidationError::CorruptPayload);
    }
    let core = reader.field(reader.remaining())?;
    if !reader.finished() || core.get(..REPLAY_V1_MAGIC.len()) != Some(REPLAY_V1_MAGIC) {
        return Err(DatasetReplayValidationError::CorruptPayload);
    }
    Ok(ReplayEnvelope {
        codec: if magic == REPLAY_V3_MAGIC {
            DatasetReplayCodecVersion::RetentionV3
        } else {
            DatasetReplayCodecVersion::RetentionV2
        },
        metadata: Some(metadata),
        core,
    })
}

pub(crate) fn dataset_replay_binding_digest(
    metadata: &ReplayRetentionMetadata,
    request_digest: [u8; 32],
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.dataset-replay-retention-binding@1\0");
    for field in [
        metadata.effect_key.as_bytes(),
        metadata.owner_execution.as_bytes(),
        metadata.owner_run_unit.as_bytes(),
        metadata
            .outer_effect_key
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update([match metadata.owner_kind {
        Some(DatasetReplayOwnerKind::CoreEffect) => 1,
        Some(DatasetReplayOwnerKind::CicsNested) => 2,
        None => 0,
    }]);
    hash.update(metadata.sequence.to_be_bytes());
    hash.update(metadata.deadline_tick.to_be_bytes());
    hash.update(metadata.resolution_tick.unwrap_or(0).to_be_bytes());
    hash.update(request_digest);
    hash.update(metadata.result_sha256);
    hash.finalize().into()
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTITY_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@')
        })
}

fn put_field(payload: &mut Vec<u8>, value: &[u8]) -> Result<(), DatasetReplayValidationError> {
    let length =
        u32::try_from(value.len()).map_err(|_| DatasetReplayValidationError::CorruptPayload)?;
    payload.extend_from_slice(&length.to_be_bytes());
    payload.extend_from_slice(value);
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

    fn take(&mut self, length: usize) -> Result<&'a [u8], DatasetReplayValidationError> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(DatasetReplayValidationError::CorruptPayload)?;
        let value = self
            .payload
            .get(self.at..end)
            .ok_or(DatasetReplayValidationError::CorruptPayload)?;
        self.at = end;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, DatasetReplayValidationError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().map_err(
            |_| DatasetReplayValidationError::CorruptPayload,
        )?))
    }

    fn byte(&mut self) -> Result<u8, DatasetReplayValidationError> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, DatasetReplayValidationError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().map_err(
            |_| DatasetReplayValidationError::CorruptPayload,
        )?))
    }

    fn field(&mut self, max: usize) -> Result<&'a [u8], DatasetReplayValidationError> {
        let length = usize::try_from(self.u32()?)
            .map_err(|_| DatasetReplayValidationError::CorruptPayload)?;
        if length > max {
            return Err(DatasetReplayValidationError::CorruptPayload);
        }
        self.take(length)
    }

    fn text_field(&mut self, max: usize) -> Result<String, DatasetReplayValidationError> {
        String::from_utf8(self.field(max)?.to_vec())
            .map_err(|_| DatasetReplayValidationError::CorruptPayload)
    }

    fn remaining(&self) -> usize {
        self.payload.len().saturating_sub(self.at)
    }

    fn finished(&self) -> bool {
        self.at == self.payload.len()
    }
}
