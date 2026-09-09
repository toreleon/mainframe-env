//! Full-codec retention validation for durable MQ replay rows.

use crate::service::{
    MqLimits, OBJECT_ROW_SCHEMA, REPLAY_NAMESPACE, RecordedResult, ReplayDigestFormat,
};
use mainframe_env_execution_api::{
    ExecutionId, IdempotencyKey, Invocation, InvocationLimits, RunUnitId,
};
use mainframe_env_host_api::{HostLimits, HostProblem, HostResult};
use mainframe_env_store_api::{EffectDigestFormat, EffectRecord, EffectState, ProviderStateRecord};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Reserved invocation binding which attests an internally dispatched CICS nested effect.
pub const CICS_NESTED_EFFECT_ORIGIN_BINDING: &str = "cics.nested-effect-origin";
/// Payload schema for [`CICS_NESTED_EFFECT_ORIGIN_BINDING`].
pub const CICS_NESTED_EFFECT_ORIGIN_SCHEMA: &str = "mainframe-env.cics.nested-effect-origin@1";
/// Reserved CICS binding carrying the exact outer effect key.
pub const CICS_OUTER_EFFECT_ORIGIN_BINDING: &str = "cics.outer-effect-origin";
/// Schema for [`CICS_OUTER_EFFECT_ORIGIN_BINDING`].
pub const CICS_OUTER_EFFECT_ORIGIN_SCHEMA: &str = "mainframe-env.cics.outer-effect-origin@1";

/// Persisted owner domain of an MQ replay receipt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MqReplayOwnerKind {
    /// The replay key is owned by a canonical host-journal effect.
    CoreEffect,
    /// An explicitly marked internal CICS nested call owns the replay key.
    CicsNested,
}

/// Dependency which must be verified before an MQ receipt is archived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MqReplayDependency {
    /// Same-key canonical completed effect in the core journal.
    CoreEffect,
    /// Exact nested CICS provenance; no same-key core effect is required.
    CicsNested {
        /// Owning CICS run unit parsed from the replay key.
        run_unit: String,
        /// Nested host sequence parsed from the replay key.
        sequence: u64,
        /// Exact completed outer CICS effect which authorized the call.
        outer_effect_key: String,
    },
}

/// Whether a validated MQ replay row has complete retention metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqReplayRetentionState {
    /// A pre-retention or partially attributed row remains protected.
    LegacyProtected,
    /// A newly persisted row has exact provenance but no successful clock observation yet.
    PendingProtected,
    /// Owner, age, request, result, and dependency bindings are complete.
    Terminal,
}

/// Exact retention description of one `mq-v1-replay` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MqReplayRetentionDescriptor {
    /// Exact provider-state namespace.
    pub namespace: String,
    /// Exact replay/idempotency key.
    pub key: String,
    /// Provider-state compare-and-swap version.
    pub row_version: u64,
    /// SHA-256 of the exact persisted payload.
    pub payload_digest: [u8; 32],
    /// Retention eligibility state.
    pub retention: MqReplayRetentionState,
    /// Owning execution, when attributed.
    pub owner_execution: Option<String>,
    /// Owning run unit, when attributed.
    pub owner_run_unit: Option<String>,
    /// Exact outer CICS effect for nested rows.
    pub outer_effect_key: Option<String>,
    /// Persisted host-effect sequence.
    pub sequence: Option<u64>,
    /// Original durable deadline lower bound.
    pub recorded_deadline_tick: Option<u64>,
    /// Post-persistence resolution observation.
    pub resolution_tick: Option<u64>,
    /// Conservative age origin.
    pub terminal_tick: Option<u64>,
    /// Canonical request digest.
    pub request_digest: [u8; 32],
    /// Canonical result digest.
    pub result_digest: [u8; 32],
    /// Exact recovery dependency classification.
    pub dependency: Option<MqReplayDependency>,
}

/// Fail-closed MQ replay validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MqReplayRetentionError {
    /// Row is outside the MQ replay namespace.
    WrongNamespace,
    /// Row identity or version is invalid.
    InvalidIdentity,
    /// Durable payload cannot be decoded canonically.
    CorruptPayload,
    /// Owner, age, or origin metadata is contradictory.
    InconsistentMetadata,
    /// A core-owned replay lacks its matching effect.
    MissingCoreEffect,
    /// The matching effect is not canonically completed.
    CoreEffectNotCompleted,
    /// The matching effect has different owner or digests.
    CoreEffectMismatch,
}

impl fmt::Display for MqReplayRetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongNamespace => "unexpected MQ replay namespace",
            Self::InvalidIdentity => "invalid MQ replay identity",
            Self::CorruptPayload => "corrupt MQ replay payload",
            Self::InconsistentMetadata => "inconsistent MQ replay retention metadata",
            Self::MissingCoreEffect => "MQ replay is missing its core effect",
            Self::CoreEffectNotCompleted => "MQ replay core effect is not canonically completed",
            Self::CoreEffectMismatch => "MQ replay does not match its core effect",
        })
    }
}

impl std::error::Error for MqReplayRetentionError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayObjectRow {
    schema_version: String,
    object_key: String,
    value: RecordedResult,
}

#[cfg(test)]
pub(crate) fn stamp_mq_replay(
    recorded: &mut RecordedResult,
    key: &str,
    invocation: &Invocation,
    sequence: u64,
    resolution_tick: u64,
    limits: MqLimits,
) -> Result<(), HostProblem> {
    if resolution_tick == 0 {
        return Err(HostProblem::Malformed);
    }
    prepare_mq_replay(recorded, key, invocation, sequence, limits)?;
    resolve_mq_replay(recorded, key, resolution_tick, resolution_tick)?;
    Ok(())
}

pub(crate) fn prepare_mq_replay(
    recorded: &mut RecordedResult,
    key: &str,
    invocation: &Invocation,
    sequence: u64,
    limits: MqLimits,
) -> Result<(), HostProblem> {
    if sequence == 0 {
        return Err(HostProblem::Malformed);
    }
    recorded.recorded_deadline_tick = invocation.deadline_tick;
    recorded.owner_execution = Some(invocation.execution_id.as_str().into());
    recorded.owner_run_unit = Some(invocation.run_unit_id.as_str().into());
    recorded.recorded_sequence = sequence;
    recorded.resolution_tick = 0;
    let (owner_kind, outer_effect_key) = origin_for(invocation, key, sequence)?;
    recorded.owner_kind = Some(owner_kind);
    recorded.outer_effect_key = outer_effect_key;
    recorded.result_sha256 =
        result_digest(recorded, limits).map_err(|_| HostProblem::InfrastructureFailure)?;
    recorded.retention_binding_sha256 = binding_digest(key, recorded);
    Ok(())
}

pub(crate) fn resolve_mq_replay(
    recorded: &mut RecordedResult,
    key: &str,
    observed_tick: u64,
    resolution_lower_bound: u64,
) -> Result<(), HostProblem> {
    validate_pending_metadata(key, recorded).map_err(|_| HostProblem::InfrastructureFailure)?;
    if observed_tick == 0 || resolution_lower_bound == 0 {
        return Err(HostProblem::InfrastructureFailure);
    }
    recorded.resolution_tick = observed_tick
        .max(resolution_lower_bound)
        .max(recorded.recorded_deadline_tick);
    recorded.retention_binding_sha256 = binding_digest(key, recorded);
    Ok(())
}

pub(crate) fn mq_pending_replay_matches(
    recorded: &RecordedResult,
    key: &str,
    invocation: &Invocation,
    sequence: u64,
) -> Result<bool, HostProblem> {
    if metadata_absent(recorded) {
        return Ok(false);
    }
    let pending = metadata_pending(recorded);
    if pending {
        validate_pending_metadata(key, recorded).map_err(|_| HostProblem::InfrastructureFailure)?;
    } else {
        validate_current_metadata(key, recorded).map_err(|_| HostProblem::InfrastructureFailure)?;
    }
    if recorded.owner_execution.as_deref() != Some(invocation.execution_id.as_str())
        || recorded.owner_run_unit.as_deref() != Some(invocation.run_unit_id.as_str())
        || recorded.recorded_sequence != sequence
        || {
            let (owner_kind, outer_effect_key) = origin_for(invocation, key, sequence)?;
            recorded.owner_kind != Some(owner_kind) || recorded.outer_effect_key != outer_effect_key
        }
    {
        return Err(HostProblem::IdempotencyConflict);
    }
    Ok(pending)
}

/// Fully decode an MQ replay row and bind core-owned rows to their completed effect.
///
/// CICS-nested rows require the persisted explicit origin discriminator and return
/// exact run/sequence provenance for verification against the outer CICS owner.
pub fn describe_mq_replay_row(
    row: &ProviderStateRecord,
    effect: Option<&EffectRecord>,
    limits: MqLimits,
) -> Result<MqReplayRetentionDescriptor, MqReplayRetentionError> {
    let recorded = decode_row(row, limits)?;
    let result_digest = validate_mq_recorded_result(&row.key, &recorded, limits)?;
    let payload_digest = Sha256::digest(&row.payload).into();
    if metadata_absent(&recorded) {
        return Ok(legacy_descriptor(
            row,
            &recorded,
            payload_digest,
            result_digest,
        ));
    }
    if metadata_pending(&recorded) {
        return pending_descriptor(row, &recorded, payload_digest, result_digest);
    }
    validate_current_metadata(&row.key, &recorded)?;
    let dependency = dependency(&row.key, &recorded)?;
    if dependency == MqReplayDependency::CoreEffect {
        validate_core_effect(effect, row, &recorded, result_digest)?;
    }
    Ok(MqReplayRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest,
        retention: MqReplayRetentionState::Terminal,
        owner_execution: recorded.owner_execution.clone(),
        owner_run_unit: recorded.owner_run_unit.clone(),
        outer_effect_key: recorded.outer_effect_key.clone(),
        sequence: Some(recorded.recorded_sequence),
        recorded_deadline_tick: Some(recorded.recorded_deadline_tick),
        resolution_tick: Some(recorded.resolution_tick),
        terminal_tick: Some(
            recorded
                .recorded_deadline_tick
                .max(recorded.resolution_tick),
        ),
        request_digest: recorded.request_sha256,
        result_digest,
        dependency: Some(dependency),
    })
}

fn pending_descriptor(
    row: &ProviderStateRecord,
    recorded: &RecordedResult,
    payload_digest: [u8; 32],
    result_digest: [u8; 32],
) -> Result<MqReplayRetentionDescriptor, MqReplayRetentionError> {
    Ok(MqReplayRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest,
        retention: MqReplayRetentionState::PendingProtected,
        owner_execution: recorded.owner_execution.clone(),
        owner_run_unit: recorded.owner_run_unit.clone(),
        outer_effect_key: recorded.outer_effect_key.clone(),
        sequence: Some(recorded.recorded_sequence),
        recorded_deadline_tick: Some(recorded.recorded_deadline_tick),
        resolution_tick: None,
        terminal_tick: None,
        request_digest: recorded.request_sha256,
        result_digest,
        dependency: Some(dependency(&row.key, recorded)?),
    })
}

fn legacy_descriptor(
    row: &ProviderStateRecord,
    recorded: &RecordedResult,
    payload_digest: [u8; 32],
    result_digest: [u8; 32],
) -> MqReplayRetentionDescriptor {
    MqReplayRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest,
        retention: MqReplayRetentionState::LegacyProtected,
        owner_execution: None,
        owner_run_unit: None,
        outer_effect_key: None,
        sequence: None,
        recorded_deadline_tick: None,
        resolution_tick: None,
        terminal_tick: None,
        request_digest: recorded.request_sha256,
        result_digest,
        dependency: None,
    }
}

fn decode_row(
    row: &ProviderStateRecord,
    limits: MqLimits,
) -> Result<RecordedResult, MqReplayRetentionError> {
    if row.namespace != REPLAY_NAMESPACE {
        return Err(MqReplayRetentionError::WrongNamespace);
    }
    if row.version == 0
        || row.version > i64::MAX as u64
        || IdempotencyKey::new(&row.key, InvocationLimits::default()).is_err()
    {
        return Err(MqReplayRetentionError::InvalidIdentity);
    }
    if row.payload.len() > limits.max_state_bytes {
        return Err(MqReplayRetentionError::CorruptPayload);
    }
    let envelope: ReplayObjectRow =
        serde_json::from_slice(&row.payload).map_err(|_| MqReplayRetentionError::CorruptPayload)?;
    if envelope.schema_version != OBJECT_ROW_SCHEMA || envelope.object_key != row.key {
        return Err(MqReplayRetentionError::InconsistentMetadata);
    }
    Ok(envelope.value)
}

pub(crate) fn validate_mq_recorded_result(
    key: &str,
    recorded: &RecordedResult,
    limits: MqLimits,
) -> Result<[u8; 32], MqReplayRetentionError> {
    let digest = result_digest(recorded, limits)?;
    if metadata_complete(recorded) {
        validate_current_metadata(key, recorded)?;
        if recorded.result_sha256 != digest {
            return Err(MqReplayRetentionError::InconsistentMetadata);
        }
    } else if metadata_pending(recorded) {
        validate_pending_metadata(key, recorded)?;
        if recorded.result_sha256 != digest {
            return Err(MqReplayRetentionError::InconsistentMetadata);
        }
    } else if !metadata_absent(recorded) {
        return Err(MqReplayRetentionError::InconsistentMetadata);
    }
    Ok(digest)
}

fn metadata_pending(recorded: &RecordedResult) -> bool {
    recorded.request_digest_format == ReplayDigestFormat::CanonicalHostV1
        && recorded.recorded_deadline_tick != 0
        && recorded.owner_execution.is_some()
        && recorded.owner_run_unit.is_some()
        && recorded.recorded_sequence != 0
        && recorded.resolution_tick == 0
        && recorded.owner_kind.is_some()
}

fn result_digest(
    recorded: &RecordedResult,
    limits: MqLimits,
) -> Result<[u8; 32], MqReplayRetentionError> {
    let result = recorded.result();
    HostResult::Mq(result.clone())
        .validate(HostLimits {
            max_name_bytes: 128,
            max_record_bytes: limits.max_message_bytes,
            max_records: limits.max_messages_per_queue,
            max_fields: 128,
            max_audit_fields: 128,
            max_state_bytes: limits.max_state_bytes,
        })
        .map_err(|_| MqReplayRetentionError::CorruptPayload)?;
    mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::Mq(result)))
        .map_err(|_| MqReplayRetentionError::CorruptPayload)
}

fn metadata_complete(recorded: &RecordedResult) -> bool {
    recorded.request_digest_format == ReplayDigestFormat::CanonicalHostV1
        && recorded.recorded_deadline_tick != 0
        && recorded.owner_execution.is_some()
        && recorded.owner_run_unit.is_some()
        && recorded.recorded_sequence != 0
        && recorded.resolution_tick != 0
        && recorded.owner_kind.is_some()
}

fn metadata_absent(recorded: &RecordedResult) -> bool {
    recorded.recorded_deadline_tick == 0
        && recorded.owner_execution.is_none()
        && recorded.owner_run_unit.is_none()
        && recorded.recorded_sequence == 0
        && recorded.resolution_tick == 0
        && recorded.owner_kind.is_none()
        && recorded.outer_effect_key.is_none()
        && recorded.result_sha256 == [0; 32]
        && recorded.retention_binding_sha256 == [0; 32]
}

fn validate_current_metadata(
    key: &str,
    recorded: &RecordedResult,
) -> Result<(), MqReplayRetentionError> {
    validate_attributed_metadata(key, recorded)?;
    if recorded.resolution_tick == 0 || recorded.resolution_tick < recorded.recorded_deadline_tick {
        return Err(MqReplayRetentionError::InconsistentMetadata);
    }
    Ok(())
}

fn validate_pending_metadata(
    key: &str,
    recorded: &RecordedResult,
) -> Result<(), MqReplayRetentionError> {
    validate_attributed_metadata(key, recorded)?;
    if recorded.resolution_tick != 0 {
        return Err(MqReplayRetentionError::InconsistentMetadata);
    }
    Ok(())
}

fn validate_attributed_metadata(
    key: &str,
    recorded: &RecordedResult,
) -> Result<(), MqReplayRetentionError> {
    let owner_execution = recorded
        .owner_execution
        .as_deref()
        .ok_or(MqReplayRetentionError::InconsistentMetadata)?;
    let owner_run_unit = recorded
        .owner_run_unit
        .as_deref()
        .ok_or(MqReplayRetentionError::InconsistentMetadata)?;
    if recorded.request_digest_format != ReplayDigestFormat::CanonicalHostV1
        || ExecutionId::new(owner_execution, InvocationLimits::default()).is_err()
        || RunUnitId::new(owner_run_unit, InvocationLimits::default()).is_err()
        || recorded.recorded_deadline_tick == 0
        || recorded.recorded_sequence == 0
        || recorded.retention_binding_sha256 != binding_digest(key, recorded)
    {
        return Err(MqReplayRetentionError::InconsistentMetadata);
    }
    if recorded.owner_kind == Some(MqReplayOwnerKind::CicsNested) {
        let (run_unit, sequence) =
            parse_cics_key(key).ok_or(MqReplayRetentionError::InconsistentMetadata)?;
        if run_unit != owner_run_unit
            || sequence != recorded.recorded_sequence
            || recorded
                .outer_effect_key
                .as_deref()
                .is_none_or(|key| IdempotencyKey::new(key, InvocationLimits::default()).is_err())
        {
            return Err(MqReplayRetentionError::InconsistentMetadata);
        }
    } else if recorded.outer_effect_key.is_some() {
        return Err(MqReplayRetentionError::InconsistentMetadata);
    }
    Ok(())
}

fn dependency(
    key: &str,
    recorded: &RecordedResult,
) -> Result<MqReplayDependency, MqReplayRetentionError> {
    match recorded.owner_kind {
        Some(MqReplayOwnerKind::CoreEffect) => Ok(MqReplayDependency::CoreEffect),
        Some(MqReplayOwnerKind::CicsNested) => {
            let (run_unit, sequence) =
                parse_cics_key(key).ok_or(MqReplayRetentionError::InconsistentMetadata)?;
            Ok(MqReplayDependency::CicsNested {
                run_unit,
                sequence,
                outer_effect_key: recorded
                    .outer_effect_key
                    .clone()
                    .ok_or(MqReplayRetentionError::InconsistentMetadata)?,
            })
        }
        None => Err(MqReplayRetentionError::InconsistentMetadata),
    }
}

fn validate_core_effect(
    effect: Option<&EffectRecord>,
    row: &ProviderStateRecord,
    recorded: &RecordedResult,
    result_digest: [u8; 32],
) -> Result<(), MqReplayRetentionError> {
    let effect = effect.ok_or(MqReplayRetentionError::MissingCoreEffect)?;
    if effect.state != EffectState::Completed
        || effect.digest_format != EffectDigestFormat::CanonicalHostV1
        || effect.result_digest.is_none()
    {
        return Err(MqReplayRetentionError::CoreEffectNotCompleted);
    }
    if effect.key.as_str() != row.key
        || recorded.owner_execution.as_deref() != Some(effect.execution_id.as_str())
        || effect.intent.owner != effect.execution_id
        || recorded.owner_run_unit.as_deref() != Some(effect.run_unit_id.as_str())
        || effect.sequence != recorded.recorded_sequence
        || effect.request_digest != recorded.request_sha256
        || effect.result_digest != Some(result_digest)
    {
        return Err(MqReplayRetentionError::CoreEffectMismatch);
    }
    Ok(())
}

fn origin_for(
    invocation: &Invocation,
    key: &str,
    sequence: u64,
) -> Result<(MqReplayOwnerKind, Option<String>), HostProblem> {
    let nested = invocation.bindings.get(CICS_NESTED_EFFECT_ORIGIN_BINDING);
    let outer = invocation.bindings.get(CICS_OUTER_EFFECT_ORIGIN_BINDING);
    let Some(binding) = nested else {
        return if outer.is_none() {
            Ok((MqReplayOwnerKind::CoreEffect, None))
        } else {
            Err(HostProblem::Malformed)
        };
    };
    if binding.schema() != CICS_NESTED_EFFECT_ORIGIN_SCHEMA || binding.bytes() != key.as_bytes() {
        return Err(HostProblem::Malformed);
    }
    let (run_unit, nested_sequence) = parse_cics_key(key).ok_or(HostProblem::Malformed)?;
    if run_unit != invocation.run_unit_id.as_str() || nested_sequence != sequence {
        return Err(HostProblem::Malformed);
    }
    let outer = outer.ok_or(HostProblem::Malformed)?;
    if outer.schema() != CICS_OUTER_EFFECT_ORIGIN_SCHEMA {
        return Err(HostProblem::Malformed);
    }
    let outer_key = std::str::from_utf8(outer.bytes()).map_err(|_| HostProblem::Malformed)?;
    IdempotencyKey::new(outer_key, InvocationLimits::default())
        .map_err(|_| HostProblem::Malformed)?;
    Ok((MqReplayOwnerKind::CicsNested, Some(outer_key.into())))
}

fn parse_cics_key(key: &str) -> Option<(String, u64)> {
    let (run_unit, encoded_sequence) = key.strip_prefix("cics:")?.rsplit_once(':')?;
    let sequence = encoded_sequence.parse::<u64>().ok()?;
    if sequence == 0
        || sequence.to_string() != encoded_sequence
        || RunUnitId::new(run_unit, InvocationLimits::default()).is_err()
    {
        return None;
    }
    Some((run_unit.into(), sequence))
}

fn binding_digest(key: &str, recorded: &RecordedResult) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.mq-replay-retention-binding@1\0");
    for field in [
        key.as_bytes(),
        recorded
            .owner_execution
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        recorded
            .owner_run_unit
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
        recorded
            .outer_effect_key
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    ] {
        hash.update((field.len() as u64).to_be_bytes());
        hash.update(field);
    }
    hash.update([match recorded.owner_kind {
        Some(MqReplayOwnerKind::CoreEffect) => 1,
        Some(MqReplayOwnerKind::CicsNested) => 2,
        None => 0,
    }]);
    hash.update(recorded.recorded_sequence.to_be_bytes());
    hash.update(recorded.recorded_deadline_tick.to_be_bytes());
    hash.update(recorded.resolution_tick.to_be_bytes());
    hash.update(recorded.request_sha256);
    hash.update(recorded.result_sha256);
    hash.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_execution_api::{
        ArtifactRef, BoundedPayload, Principal, PrincipalId, RequestId, ResourceLimits, Selector,
        ServiceClass, TraceId,
    };
    use mainframe_env_store_api::EffectIntentMetadata;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    fn invocation(run: &str, nested_key: Option<&str>) -> Invocation {
        let limits = InvocationLimits::default();
        let bindings = nested_key.map_or_else(BTreeMap::new, |key| {
            BTreeMap::from([
                (
                    CICS_NESTED_EFFECT_ORIGIN_BINDING.into(),
                    BoundedPayload::new(
                        CICS_NESTED_EFFECT_ORIGIN_SCHEMA,
                        key.as_bytes().to_vec(),
                        limits,
                    )
                    .unwrap(),
                ),
                (
                    CICS_OUTER_EFFECT_ORIGIN_BINDING.into(),
                    BoundedPayload::new(
                        CICS_OUTER_EFFECT_ORIGIN_SCHEMA,
                        format!("outer-{run}").into_bytes(),
                        limits,
                    )
                    .unwrap(),
                ),
            ])
        });
        Invocation::new(
            RequestId::new(format!("request-{run}"), limits).unwrap(),
            ExecutionId::new(format!("execution-{run}"), limits).unwrap(),
            RunUnitId::new(run, limits).unwrap(),
            None,
            Selector::new("program:retention-test", limits).unwrap(),
            ArtifactRef::new("artifact:retention-test", limits).unwrap(),
            Principal::new(
                PrincipalId::new("IBMUSER", limits).unwrap(),
                BTreeSet::new(),
                limits,
            )
            .unwrap(),
            ServiceClass::System,
            0,
            100,
            TraceId::new(format!("trace-{run}"), limits).unwrap(),
            IdempotencyKey::new(format!("invocation-{run}"), limits).unwrap(),
            1,
            ResourceLimits::default(),
            bindings,
            limits,
        )
        .unwrap()
    }

    fn recorded() -> RecordedResult {
        RecordedResult {
            request_digest_format: ReplayDigestFormat::CanonicalHostV1,
            request_sha256: [3; 32],
            recorded_deadline_tick: 0,
            owner_execution: None,
            owner_run_unit: None,
            recorded_sequence: 0,
            resolution_tick: 0,
            owner_kind: None,
            outer_effect_key: None,
            result_sha256: [0; 32],
            retention_binding_sha256: [0; 32],
            completion_code: 0,
            reason_code: 0,
            handle: None,
            message: Vec::new(),
            message_id: None,
            correlation_id: None,
            trigger_program: None,
        }
    }

    fn current_row(key: &str, invocation: &Invocation, sequence: u64) -> ProviderStateRecord {
        let mut recorded = recorded();
        stamp_mq_replay(
            &mut recorded,
            key,
            invocation,
            sequence,
            140,
            MqLimits::default(),
        )
        .unwrap();
        encode_row(key, &recorded)
    }

    fn encode_row(key: &str, recorded: &RecordedResult) -> ProviderStateRecord {
        ProviderStateRecord {
            namespace: REPLAY_NAMESPACE.into(),
            key: key.into(),
            version: 4,
            payload: serde_json::to_vec(&json!({
                "schema_version": OBJECT_ROW_SCHEMA,
                "object_key": key,
                "value": recorded,
            }))
            .unwrap(),
        }
    }

    fn decoded(row: &ProviderStateRecord) -> RecordedResult {
        decode_row(row, MqLimits::default()).unwrap()
    }

    fn effect(row: &ProviderStateRecord, recorded: &RecordedResult) -> EffectRecord {
        let limits = InvocationLimits::default();
        let execution = ExecutionId::new(
            recorded.owner_execution.as_deref().unwrap(),
            InvocationLimits::default(),
        )
        .unwrap();
        EffectRecord {
            execution_id: execution.clone(),
            run_unit_id: RunUnitId::new(
                recorded.owner_run_unit.as_deref().unwrap(),
                InvocationLimits::default(),
            )
            .unwrap(),
            sequence: recorded.recorded_sequence,
            key: IdempotencyKey::new(&row.key, limits).unwrap(),
            digest_format: EffectDigestFormat::CanonicalHostV1,
            request_digest: recorded.request_sha256,
            intent: EffectIntentMetadata {
                owner: execution,
                attempt: 1,
                capability: None,
                audit_resource: None,
                audit_invocation_key: None,
                created_tick: 1,
                recovery_after_tick: 2,
                epoch: 1,
                recovery_lease: None,
            },
            state: EffectState::Completed,
            result_digest: Some(recorded.result_sha256),
            resolved_tick: Some(recorded.resolution_tick),
        }
    }

    fn mutate_payload(
        row: &ProviderStateRecord,
        mutate: impl FnOnce(&mut serde_json::Value),
    ) -> ProviderStateRecord {
        let mut value: serde_json::Value = serde_json::from_slice(&row.payload).unwrap();
        mutate(&mut value);
        ProviderStateRecord {
            payload: serde_json::to_vec(&value).unwrap(),
            ..row.clone()
        }
    }

    #[test]
    fn full_codec_binds_identity_owner_request_result_and_completed_effect() {
        let invocation = invocation("mq-retention", None);
        let row = current_row("mq-retention-key", &invocation, 7);
        let recorded = decoded(&row);
        let effect = effect(&row, &recorded);
        let descriptor = describe_mq_replay_row(&row, Some(&effect), MqLimits::default()).unwrap();
        assert_eq!(descriptor.namespace, REPLAY_NAMESPACE);
        assert_eq!(descriptor.key, row.key);
        assert_eq!(descriptor.row_version, 4);
        assert_eq!(descriptor.retention, MqReplayRetentionState::Terminal);
        assert_eq!(descriptor.terminal_tick, Some(140));
        assert_eq!(descriptor.dependency, Some(MqReplayDependency::CoreEffect));

        assert_eq!(
            describe_mq_replay_row(&row, None, MqLimits::default()),
            Err(MqReplayRetentionError::MissingCoreEffect)
        );
        let mut forged = effect.clone();
        forged.execution_id =
            ExecutionId::new("forged-execution", InvocationLimits::default()).unwrap();
        assert_eq!(
            describe_mq_replay_row(&row, Some(&forged), MqLimits::default()),
            Err(MqReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect.clone();
        forged.run_unit_id = RunUnitId::new("forged-run", InvocationLimits::default()).unwrap();
        assert_eq!(
            describe_mq_replay_row(&row, Some(&forged), MqLimits::default()),
            Err(MqReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect.clone();
        forged.key = IdempotencyKey::new("forged-key", InvocationLimits::default()).unwrap();
        assert_eq!(
            describe_mq_replay_row(&row, Some(&forged), MqLimits::default()),
            Err(MqReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect.clone();
        forged.request_digest = [8; 32];
        assert_eq!(
            describe_mq_replay_row(&row, Some(&forged), MqLimits::default()),
            Err(MqReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect;
        forged.result_digest = Some([9; 32]);
        assert_eq!(
            describe_mq_replay_row(&row, Some(&forged), MqLimits::default()),
            Err(MqReplayRetentionError::CoreEffectMismatch)
        );
    }

    #[test]
    fn full_codec_rejects_partial_forged_and_oversized_payloads() {
        let invocation = invocation("mq-corrupt", None);
        let row = current_row("mq-corrupt-key", &invocation, 9);
        for corrupt in [
            mutate_payload(&row, |value| {
                value["value"]
                    .as_object_mut()
                    .unwrap()
                    .remove("owner_run_unit");
            }),
            mutate_payload(&row, |value| {
                value["value"]["owner_execution"] = json!("forged-execution");
            }),
            mutate_payload(&row, |value| {
                value["value"]["request_sha256"] = json!(vec![8_u8; 32]);
            }),
            mutate_payload(&row, |value| {
                value["value"]["reason_code"] = json!(2033);
            }),
            mutate_payload(&row, |value| {
                value["object_key"] = json!("forged-key");
            }),
        ] {
            assert_eq!(
                describe_mq_replay_row(&corrupt, None, MqLimits::default()),
                Err(MqReplayRetentionError::InconsistentMetadata)
            );
        }

        let unknown = mutate_payload(&row, |value| {
            value["value"]["forged_field"] = json!(true);
        });
        assert_eq!(
            describe_mq_replay_row(&unknown, None, MqLimits::default()),
            Err(MqReplayRetentionError::CorruptPayload)
        );

        let mut oversized = decoded(&row);
        oversized.message = vec![1, 2];
        oversized.result_sha256 = result_digest(&oversized, MqLimits::default()).unwrap();
        oversized.retention_binding_sha256 = binding_digest(&row.key, &oversized);
        let oversized = encode_row(&row.key, &oversized);
        assert_eq!(
            describe_mq_replay_row(
                &oversized,
                None,
                MqLimits {
                    max_message_bytes: 1,
                    ..MqLimits::default()
                }
            ),
            Err(MqReplayRetentionError::CorruptPayload)
        );
    }

    #[test]
    fn legacy_is_protected_and_only_explicit_cics_origin_skips_core_effect() {
        let legacy = encode_row("legacy-key", &recorded());
        let descriptor = describe_mq_replay_row(&legacy, None, MqLimits::default()).unwrap();
        assert_eq!(
            descriptor.retention,
            MqReplayRetentionState::LegacyProtected
        );
        assert_eq!(descriptor.dependency, None);

        let direct = invocation("cics-run", None);
        let key = "cics:cics-run:7";
        let direct_row = current_row(key, &direct, 7);
        assert_eq!(
            describe_mq_replay_row(&direct_row, None, MqLimits::default()),
            Err(MqReplayRetentionError::MissingCoreEffect)
        );

        let nested = invocation("cics-run", Some(key));
        let nested_row = current_row(key, &nested, 7);
        let descriptor = describe_mq_replay_row(&nested_row, None, MqLimits::default()).unwrap();
        assert_eq!(descriptor.retention, MqReplayRetentionState::Terminal);
        assert_eq!(
            descriptor.dependency,
            Some(MqReplayDependency::CicsNested {
                run_unit: "cics-run".into(),
                sequence: 7,
                outer_effect_key: "outer-cics-run".into(),
            })
        );
        let current_nested = decoded(&nested_row);
        assert!(!mq_pending_replay_matches(&current_nested, key, &nested, 7).unwrap());
        assert_eq!(
            mq_pending_replay_matches(&current_nested, key, &direct, 7),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            mq_pending_replay_matches(
                &decoded(&direct_row),
                key,
                &invocation("forged-owner", None),
                7,
            ),
            Err(HostProblem::IdempotencyConflict)
        );

        let mut pending = recorded();
        prepare_mq_replay(&mut pending, key, &nested, 7, MqLimits::default()).unwrap();
        let pending_row = encode_row(key, &pending);
        let descriptor = describe_mq_replay_row(&pending_row, None, MqLimits::default()).unwrap();
        assert_eq!(
            descriptor.retention,
            MqReplayRetentionState::PendingProtected
        );
        assert_eq!(descriptor.resolution_tick, None);
        assert_eq!(descriptor.terminal_tick, None);
        assert!(mq_pending_replay_matches(&pending, key, &nested, 7).unwrap());
        assert_eq!(
            mq_pending_replay_matches(&pending, key, &direct, 7),
            Err(HostProblem::IdempotencyConflict)
        );

        let owner = invocation("pending-owner", None);
        let mut owner_pending = recorded();
        prepare_mq_replay(
            &mut owner_pending,
            "pending-owner-key",
            &owner,
            8,
            MqLimits::default(),
        )
        .unwrap();
        assert_eq!(
            mq_pending_replay_matches(
                &owner_pending,
                "pending-owner-key",
                &invocation("forged-owner", None),
                8,
            ),
            Err(HostProblem::IdempotencyConflict)
        );

        let mut reversed_age = decoded(&nested_row);
        reversed_age.resolution_tick = 99;
        reversed_age.retention_binding_sha256 = binding_digest(key, &reversed_age);
        assert_eq!(
            describe_mq_replay_row(&encode_row(key, &reversed_age), None, MqLimits::default()),
            Err(MqReplayRetentionError::InconsistentMetadata)
        );
    }
}
