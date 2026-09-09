//! Full-codec retention validation for durable Db2 replay rows.

use crate::service::{
    Db2Limits, OBJECT_ROW_SCHEMA, REPLAY_NAMESPACE, RecordedResult, ReplayDigestFormat,
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

/// Persisted owner domain of a Db2 replay receipt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Db2ReplayOwnerKind {
    /// The replay is owned by a same-key core effect.
    CoreEffect,
    /// The replay is owned by an explicitly marked nested CICS call.
    CicsNested,
}

/// Dependency which must be verified before a Db2 receipt is archived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Db2ReplayDependency {
    /// Require the same-key canonical completed core effect.
    CoreEffect,
    /// Require exact finalized CICS provenance instead of a same-key effect.
    CicsNested {
        /// Owning CICS run unit.
        run_unit: String,
        /// Nested host-effect sequence.
        sequence: u64,
        /// Exact outer CICS effect key.
        outer_effect_key: String,
    },
}

/// Whether a validated Db2 replay row has complete retention metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ReplayRetentionState {
    /// A legacy row lacks complete owner or age metadata.
    LegacyProtected,
    /// A newly persisted row has exact provenance but no successful clock observation yet.
    PendingProtected,
    /// The row has complete owner, result, dependency, and age bindings.
    Terminal,
}

/// Exact retention description of one `db2-v1-replay` row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Db2ReplayRetentionDescriptor {
    /// Exact provider-state namespace.
    pub namespace: String,
    /// Exact replay/idempotency key.
    pub key: String,
    /// Provider-state compare-and-swap version.
    pub row_version: u64,
    /// SHA-256 of the exact persisted payload.
    pub payload_digest: [u8; 32],
    /// Retention eligibility state.
    pub retention: Db2ReplayRetentionState,
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
    pub dependency: Option<Db2ReplayDependency>,
}

/// Fail-closed Db2 replay validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Db2ReplayRetentionError {
    /// Row is outside the Db2 replay namespace.
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

impl fmt::Display for Db2ReplayRetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WrongNamespace => "unexpected Db2 replay namespace",
            Self::InvalidIdentity => "invalid Db2 replay identity",
            Self::CorruptPayload => "corrupt Db2 replay payload",
            Self::InconsistentMetadata => "inconsistent Db2 replay retention metadata",
            Self::MissingCoreEffect => "Db2 replay is missing its core effect",
            Self::CoreEffectNotCompleted => "Db2 replay core effect is not canonically completed",
            Self::CoreEffectMismatch => "Db2 replay does not match its core effect",
        })
    }
}

impl std::error::Error for Db2ReplayRetentionError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayObjectRow {
    schema_version: String,
    object_key: String,
    value: RecordedResult,
}

#[cfg(test)]
pub(crate) fn stamp_db2_replay(
    recorded: &mut RecordedResult,
    key: &str,
    invocation: &Invocation,
    sequence: u64,
    resolution_tick: u64,
    limits: Db2Limits,
) -> Result<(), HostProblem> {
    if resolution_tick == 0 {
        return Err(HostProblem::Malformed);
    }
    prepare_db2_replay(recorded, key, invocation, sequence, limits)?;
    resolve_db2_replay(recorded, key, resolution_tick, resolution_tick)?;
    Ok(())
}

pub(crate) fn prepare_db2_replay(
    recorded: &mut RecordedResult,
    key: &str,
    invocation: &Invocation,
    sequence: u64,
    limits: Db2Limits,
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

pub(crate) fn resolve_db2_replay(
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

pub(crate) fn db2_pending_replay_matches(
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

/// Fully decode a Db2 replay row and bind core-owned rows to their completed effect.
pub fn describe_db2_replay_row(
    row: &ProviderStateRecord,
    effect: Option<&EffectRecord>,
    limits: Db2Limits,
) -> Result<Db2ReplayRetentionDescriptor, Db2ReplayRetentionError> {
    let recorded = decode_row(row, limits)?;
    let result_digest = validate_db2_recorded_result(&row.key, &recorded, limits)?;
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
    if dependency == Db2ReplayDependency::CoreEffect {
        validate_core_effect(effect, row, &recorded, result_digest)?;
    }
    Ok(Db2ReplayRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest,
        retention: Db2ReplayRetentionState::Terminal,
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
) -> Result<Db2ReplayRetentionDescriptor, Db2ReplayRetentionError> {
    Ok(Db2ReplayRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest,
        retention: Db2ReplayRetentionState::PendingProtected,
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
) -> Db2ReplayRetentionDescriptor {
    Db2ReplayRetentionDescriptor {
        namespace: row.namespace.clone(),
        key: row.key.clone(),
        row_version: row.version,
        payload_digest,
        retention: Db2ReplayRetentionState::LegacyProtected,
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
    limits: Db2Limits,
) -> Result<RecordedResult, Db2ReplayRetentionError> {
    if row.namespace != REPLAY_NAMESPACE {
        return Err(Db2ReplayRetentionError::WrongNamespace);
    }
    if row.version == 0
        || row.version > i64::MAX as u64
        || IdempotencyKey::new(&row.key, InvocationLimits::default()).is_err()
    {
        return Err(Db2ReplayRetentionError::InvalidIdentity);
    }
    if row.payload.len() > limits.max_state_bytes {
        return Err(Db2ReplayRetentionError::CorruptPayload);
    }
    let envelope: ReplayObjectRow = serde_json::from_slice(&row.payload)
        .map_err(|_| Db2ReplayRetentionError::CorruptPayload)?;
    if envelope.schema_version != OBJECT_ROW_SCHEMA || envelope.object_key != row.key {
        return Err(Db2ReplayRetentionError::InconsistentMetadata);
    }
    Ok(envelope.value)
}

pub(crate) fn validate_db2_recorded_result(
    key: &str,
    recorded: &RecordedResult,
    limits: Db2Limits,
) -> Result<[u8; 32], Db2ReplayRetentionError> {
    let digest = result_digest(recorded, limits)?;
    if metadata_complete(recorded) {
        validate_current_metadata(key, recorded)?;
        if recorded.result_sha256 != digest {
            return Err(Db2ReplayRetentionError::InconsistentMetadata);
        }
    } else if metadata_pending(recorded) {
        validate_pending_metadata(key, recorded)?;
        if recorded.result_sha256 != digest {
            return Err(Db2ReplayRetentionError::InconsistentMetadata);
        }
    } else if !metadata_absent(recorded) {
        return Err(Db2ReplayRetentionError::InconsistentMetadata);
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
    limits: Db2Limits,
) -> Result<[u8; 32], Db2ReplayRetentionError> {
    let result = recorded.result();
    HostResult::Db2(result.clone())
        .validate(HostLimits {
            max_name_bytes: 128,
            max_record_bytes: limits.max_column_bytes,
            max_records: limits.max_rows_per_table,
            max_fields: limits.max_columns,
            max_audit_fields: 128,
            max_state_bytes: limits.max_state_bytes,
        })
        .map_err(|_| Db2ReplayRetentionError::CorruptPayload)?;
    mainframe_env_host_api::canonical_result_digest(&Ok(HostResult::Db2(result)))
        .map_err(|_| Db2ReplayRetentionError::CorruptPayload)
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
) -> Result<(), Db2ReplayRetentionError> {
    validate_attributed_metadata(key, recorded)?;
    if recorded.resolution_tick == 0 || recorded.resolution_tick < recorded.recorded_deadline_tick {
        return Err(Db2ReplayRetentionError::InconsistentMetadata);
    }
    Ok(())
}

fn validate_pending_metadata(
    key: &str,
    recorded: &RecordedResult,
) -> Result<(), Db2ReplayRetentionError> {
    validate_attributed_metadata(key, recorded)?;
    if recorded.resolution_tick != 0 {
        return Err(Db2ReplayRetentionError::InconsistentMetadata);
    }
    Ok(())
}

fn validate_attributed_metadata(
    key: &str,
    recorded: &RecordedResult,
) -> Result<(), Db2ReplayRetentionError> {
    let execution = recorded
        .owner_execution
        .as_deref()
        .ok_or(Db2ReplayRetentionError::InconsistentMetadata)?;
    let run = recorded
        .owner_run_unit
        .as_deref()
        .ok_or(Db2ReplayRetentionError::InconsistentMetadata)?;
    if recorded.request_digest_format != ReplayDigestFormat::CanonicalHostV1
        || ExecutionId::new(execution, InvocationLimits::default()).is_err()
        || RunUnitId::new(run, InvocationLimits::default()).is_err()
        || recorded.recorded_deadline_tick == 0
        || recorded.recorded_sequence == 0
        || recorded.retention_binding_sha256 != binding_digest(key, recorded)
    {
        return Err(Db2ReplayRetentionError::InconsistentMetadata);
    }
    if recorded.owner_kind == Some(Db2ReplayOwnerKind::CicsNested) {
        let (key_run, sequence) =
            parse_cics_key(key).ok_or(Db2ReplayRetentionError::InconsistentMetadata)?;
        if key_run != run
            || sequence != recorded.recorded_sequence
            || recorded
                .outer_effect_key
                .as_deref()
                .is_none_or(|key| IdempotencyKey::new(key, InvocationLimits::default()).is_err())
        {
            return Err(Db2ReplayRetentionError::InconsistentMetadata);
        }
    } else if recorded.outer_effect_key.is_some() {
        return Err(Db2ReplayRetentionError::InconsistentMetadata);
    }
    Ok(())
}

fn dependency(
    key: &str,
    recorded: &RecordedResult,
) -> Result<Db2ReplayDependency, Db2ReplayRetentionError> {
    match recorded.owner_kind {
        Some(Db2ReplayOwnerKind::CoreEffect) => Ok(Db2ReplayDependency::CoreEffect),
        Some(Db2ReplayOwnerKind::CicsNested) => {
            let (run_unit, sequence) =
                parse_cics_key(key).ok_or(Db2ReplayRetentionError::InconsistentMetadata)?;
            Ok(Db2ReplayDependency::CicsNested {
                run_unit,
                sequence,
                outer_effect_key: recorded
                    .outer_effect_key
                    .clone()
                    .ok_or(Db2ReplayRetentionError::InconsistentMetadata)?,
            })
        }
        None => Err(Db2ReplayRetentionError::InconsistentMetadata),
    }
}

fn validate_core_effect(
    effect: Option<&EffectRecord>,
    row: &ProviderStateRecord,
    recorded: &RecordedResult,
    result_digest: [u8; 32],
) -> Result<(), Db2ReplayRetentionError> {
    let effect = effect.ok_or(Db2ReplayRetentionError::MissingCoreEffect)?;
    if effect.state != EffectState::Completed
        || effect.digest_format != EffectDigestFormat::CanonicalHostV1
        || effect.result_digest.is_none()
    {
        return Err(Db2ReplayRetentionError::CoreEffectNotCompleted);
    }
    if effect.key.as_str() != row.key
        || recorded.owner_execution.as_deref() != Some(effect.execution_id.as_str())
        || effect.intent.owner != effect.execution_id
        || recorded.owner_run_unit.as_deref() != Some(effect.run_unit_id.as_str())
        || effect.sequence != recorded.recorded_sequence
        || effect.request_digest != recorded.request_sha256
        || effect.result_digest != Some(result_digest)
    {
        return Err(Db2ReplayRetentionError::CoreEffectMismatch);
    }
    Ok(())
}

fn origin_for(
    invocation: &Invocation,
    key: &str,
    sequence: u64,
) -> Result<(Db2ReplayOwnerKind, Option<String>), HostProblem> {
    let nested = invocation.bindings.get(CICS_NESTED_EFFECT_ORIGIN_BINDING);
    let outer = invocation.bindings.get(CICS_OUTER_EFFECT_ORIGIN_BINDING);
    let Some(binding) = nested else {
        return if outer.is_none() {
            Ok((Db2ReplayOwnerKind::CoreEffect, None))
        } else {
            Err(HostProblem::Malformed)
        };
    };
    if binding.schema() != CICS_NESTED_EFFECT_ORIGIN_SCHEMA || binding.bytes() != key.as_bytes() {
        return Err(HostProblem::Malformed);
    }
    let (run, nested_sequence) = parse_cics_key(key).ok_or(HostProblem::Malformed)?;
    if run != invocation.run_unit_id.as_str() || nested_sequence != sequence {
        return Err(HostProblem::Malformed);
    }
    let outer = outer.ok_or(HostProblem::Malformed)?;
    if outer.schema() != CICS_OUTER_EFFECT_ORIGIN_SCHEMA {
        return Err(HostProblem::Malformed);
    }
    let outer_key = std::str::from_utf8(outer.bytes()).map_err(|_| HostProblem::Malformed)?;
    IdempotencyKey::new(outer_key, InvocationLimits::default())
        .map_err(|_| HostProblem::Malformed)?;
    Ok((Db2ReplayOwnerKind::CicsNested, Some(outer_key.into())))
}

fn parse_cics_key(key: &str) -> Option<(String, u64)> {
    let (run, encoded) = key.strip_prefix("cics:")?.rsplit_once(':')?;
    let sequence = encoded.parse::<u64>().ok()?;
    if sequence == 0
        || sequence.to_string() != encoded
        || RunUnitId::new(run, InvocationLimits::default()).is_err()
    {
        return None;
    }
    Some((run.into(), sequence))
}

fn binding_digest(key: &str, recorded: &RecordedResult) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"mainframe-env.db2-replay-retention-binding@1\0");
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
        Some(Db2ReplayOwnerKind::CoreEffect) => 1,
        Some(Db2ReplayOwnerKind::CicsNested) => 2,
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
    use mainframe_env_host_api::Db2Result;
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
        let result = Db2Result {
            sqlcode: 0,
            sqlstate: "00000".into(),
            message: "row inserted".into(),
            rows: Vec::new(),
            affected_rows: 1,
        };
        let mut recorded = RecordedResult::from(&result);
        recorded.request_sha256 = [3; 32];
        recorded
    }

    fn current_row(key: &str, invocation: &Invocation, sequence: u64) -> ProviderStateRecord {
        let mut recorded = recorded();
        stamp_db2_replay(
            &mut recorded,
            key,
            invocation,
            sequence,
            140,
            Db2Limits::default(),
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
        decode_row(row, Db2Limits::default()).unwrap()
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
        let invocation = invocation("db2-retention", None);
        let row = current_row("db2-retention-key", &invocation, 7);
        let recorded = decoded(&row);
        let effect = effect(&row, &recorded);
        let descriptor =
            describe_db2_replay_row(&row, Some(&effect), Db2Limits::default()).unwrap();
        assert_eq!(descriptor.namespace, REPLAY_NAMESPACE);
        assert_eq!(descriptor.key, row.key);
        assert_eq!(descriptor.row_version, 4);
        assert_eq!(descriptor.retention, Db2ReplayRetentionState::Terminal);
        assert_eq!(descriptor.terminal_tick, Some(140));
        assert_eq!(descriptor.dependency, Some(Db2ReplayDependency::CoreEffect));

        assert_eq!(
            describe_db2_replay_row(&row, None, Db2Limits::default()),
            Err(Db2ReplayRetentionError::MissingCoreEffect)
        );
        let mut forged = effect.clone();
        forged.execution_id =
            ExecutionId::new("forged-execution", InvocationLimits::default()).unwrap();
        assert_eq!(
            describe_db2_replay_row(&row, Some(&forged), Db2Limits::default()),
            Err(Db2ReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect.clone();
        forged.run_unit_id = RunUnitId::new("forged-run", InvocationLimits::default()).unwrap();
        assert_eq!(
            describe_db2_replay_row(&row, Some(&forged), Db2Limits::default()),
            Err(Db2ReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect.clone();
        forged.key = IdempotencyKey::new("forged-key", InvocationLimits::default()).unwrap();
        assert_eq!(
            describe_db2_replay_row(&row, Some(&forged), Db2Limits::default()),
            Err(Db2ReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect.clone();
        forged.request_digest = [8; 32];
        assert_eq!(
            describe_db2_replay_row(&row, Some(&forged), Db2Limits::default()),
            Err(Db2ReplayRetentionError::CoreEffectMismatch)
        );
        let mut forged = effect;
        forged.result_digest = Some([9; 32]);
        assert_eq!(
            describe_db2_replay_row(&row, Some(&forged), Db2Limits::default()),
            Err(Db2ReplayRetentionError::CoreEffectMismatch)
        );
    }

    #[test]
    fn full_codec_rejects_partial_forged_and_oversized_payloads() {
        let invocation = invocation("db2-corrupt", None);
        let row = current_row("db2-corrupt-key", &invocation, 9);
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
                value["value"]["message"] = json!("forged result");
            }),
            mutate_payload(&row, |value| {
                value["object_key"] = json!("forged-key");
            }),
        ] {
            assert_eq!(
                describe_db2_replay_row(&corrupt, None, Db2Limits::default()),
                Err(Db2ReplayRetentionError::InconsistentMetadata)
            );
        }

        let unknown = mutate_payload(&row, |value| {
            value["value"]["forged_field"] = json!(true);
        });
        assert_eq!(
            describe_db2_replay_row(&unknown, None, Db2Limits::default()),
            Err(Db2ReplayRetentionError::CorruptPayload)
        );

        let mut oversized = decoded(&row);
        oversized.rows = vec![vec![vec![1, 2]]];
        oversized.result_sha256 = result_digest(&oversized, Db2Limits::default()).unwrap();
        oversized.retention_binding_sha256 = binding_digest(&row.key, &oversized);
        let oversized = encode_row(&row.key, &oversized);
        assert_eq!(
            describe_db2_replay_row(
                &oversized,
                None,
                Db2Limits {
                    max_column_bytes: 1,
                    ..Db2Limits::default()
                }
            ),
            Err(Db2ReplayRetentionError::CorruptPayload)
        );
    }

    #[test]
    fn legacy_is_protected_and_only_explicit_cics_origin_skips_core_effect() {
        let legacy = encode_row("legacy-key", &recorded());
        let descriptor = describe_db2_replay_row(&legacy, None, Db2Limits::default()).unwrap();
        assert_eq!(
            descriptor.retention,
            Db2ReplayRetentionState::LegacyProtected
        );
        assert_eq!(descriptor.dependency, None);

        let direct = invocation("cics-run", None);
        let key = "cics:cics-run:7";
        let direct_row = current_row(key, &direct, 7);
        assert_eq!(
            describe_db2_replay_row(&direct_row, None, Db2Limits::default()),
            Err(Db2ReplayRetentionError::MissingCoreEffect)
        );

        let nested = invocation("cics-run", Some(key));
        let nested_row = current_row(key, &nested, 7);
        let descriptor = describe_db2_replay_row(&nested_row, None, Db2Limits::default()).unwrap();
        assert_eq!(descriptor.retention, Db2ReplayRetentionState::Terminal);
        assert_eq!(
            descriptor.dependency,
            Some(Db2ReplayDependency::CicsNested {
                run_unit: "cics-run".into(),
                sequence: 7,
                outer_effect_key: "outer-cics-run".into(),
            })
        );
        let current_nested = decoded(&nested_row);
        assert!(!db2_pending_replay_matches(&current_nested, key, &nested, 7).unwrap());
        assert_eq!(
            db2_pending_replay_matches(&current_nested, key, &direct, 7),
            Err(HostProblem::IdempotencyConflict)
        );
        assert_eq!(
            db2_pending_replay_matches(
                &decoded(&direct_row),
                key,
                &invocation("forged-owner", None),
                7,
            ),
            Err(HostProblem::IdempotencyConflict)
        );

        let mut pending = recorded();
        prepare_db2_replay(&mut pending, key, &nested, 7, Db2Limits::default()).unwrap();
        let pending_row = encode_row(key, &pending);
        let descriptor = describe_db2_replay_row(&pending_row, None, Db2Limits::default()).unwrap();
        assert_eq!(
            descriptor.retention,
            Db2ReplayRetentionState::PendingProtected
        );
        assert_eq!(descriptor.resolution_tick, None);
        assert_eq!(descriptor.terminal_tick, None);
        assert!(db2_pending_replay_matches(&pending, key, &nested, 7).unwrap());
        assert_eq!(
            db2_pending_replay_matches(&pending, key, &direct, 7),
            Err(HostProblem::IdempotencyConflict)
        );

        let owner = invocation("pending-owner", None);
        let mut owner_pending = recorded();
        prepare_db2_replay(
            &mut owner_pending,
            "pending-owner-key",
            &owner,
            8,
            Db2Limits::default(),
        )
        .unwrap();
        assert_eq!(
            db2_pending_replay_matches(
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
            describe_db2_replay_row(&encode_row(key, &reversed_age), None, Db2Limits::default()),
            Err(Db2ReplayRetentionError::InconsistentMetadata)
        );
    }
}
