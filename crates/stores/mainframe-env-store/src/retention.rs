use mainframe_env_execution_api::{ExecutionId, InvocationLimits};
use mainframe_env_store_api::{
    ArchivedRetentionRow, MAX_PROVIDER_KEY_BYTES, MAX_PROVIDER_NAMESPACE_BYTES,
    MAX_RETENTION_BATCH, ProviderRetentionDependency, ProviderStateArchiveDeletion,
    ProviderStateArchiveReplacement, RetentionArchive, RetentionCapacityHealth, RetentionForecast,
    RetentionPolicy, RetentionRequest, RetentionTarget, RetentionTargetCapacity,
    RetentionWatermarks, SaturationLevel, StoreError,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) mod observation;
pub(crate) use observation::{matches_source as observation_matches_source, source_digest};

pub(crate) const fn dependency_sensitive_core_target(target: RetentionTarget) -> bool {
    matches!(
        target,
        RetentionTarget::ResolvedEffects
            | RetentionTarget::TerminalWork
            | RetentionTarget::LifecycleEvents
            | RetentionTarget::TerminalExecutions
    )
}

pub(crate) const fn provider_owned_target(target: RetentionTarget) -> bool {
    matches!(
        target,
        RetentionTarget::Db2Replay
            | RetentionTarget::ImsReplay
            | RetentionTarget::MqReplay
            | RetentionTarget::DatasetReplay
            | RetentionTarget::CicsUnitOfWork
            | RetentionTarget::CicsReplay
            | RetentionTarget::RacfEvidence
            | RetentionTarget::CobolLifecycle
            | RetentionTarget::SpoolJobs
            | RetentionTarget::ConsoleLog
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn capacity_health(
    policy: RetentionPolicy,
    source_usage: Vec<(RetentionTarget, usize, usize)>,
    archive_rows: usize,
    archive_row_capacity: usize,
    archive_bytes: u64,
    archive_byte_capacity: u64,
    observation_rows: usize,
    observation_row_capacity: usize,
    observation_bytes: u64,
    observation_byte_capacity: u64,
) -> Result<RetentionCapacityHealth, StoreError> {
    policy.validate()?;
    if source_usage.len() != RetentionTarget::ALL.len()
        || source_usage
            .iter()
            .map(|(target, _, _)| *target)
            .ne(RetentionTarget::ALL)
        || source_usage
            .iter()
            .any(|(_, used, capacity)| *capacity == 0 || used > capacity)
        || archive_row_capacity == 0
        || archive_rows > archive_row_capacity
        || archive_byte_capacity == 0
        || archive_bytes > archive_byte_capacity
        || observation_row_capacity == 0
        || observation_rows > observation_row_capacity
        || observation_byte_capacity == 0
        || observation_bytes > observation_byte_capacity
    {
        return Err(StoreError::IncompatibleVersion);
    }
    let targets = source_usage
        .into_iter()
        .map(|(target, used, capacity)| RetentionTargetCapacity {
            target,
            used,
            capacity,
            saturation: saturation(
                capacity,
                used,
                policy.low_watermark_percent,
                policy.high_watermark_percent,
            ),
        })
        .collect::<Vec<_>>();
    let saturation = targets
        .iter()
        .map(|entry| entry.saturation)
        .chain([
            saturation(
                archive_row_capacity,
                archive_rows,
                policy.low_watermark_percent,
                policy.high_watermark_percent,
            ),
            saturation_u64(
                archive_byte_capacity,
                archive_bytes,
                policy.low_watermark_percent,
                policy.high_watermark_percent,
            ),
            saturation(
                observation_row_capacity,
                observation_rows,
                policy.low_watermark_percent,
                policy.high_watermark_percent,
            ),
            saturation_u64(
                observation_byte_capacity,
                observation_bytes,
                policy.low_watermark_percent,
                policy.high_watermark_percent,
            ),
        ])
        .max()
        .unwrap_or(SaturationLevel::Full);
    Ok(RetentionCapacityHealth {
        targets,
        archive_rows,
        archive_row_capacity,
        archive_bytes,
        archive_byte_capacity,
        observation_rows,
        observation_row_capacity,
        observation_bytes,
        observation_byte_capacity,
        saturation,
    })
}

pub(crate) fn validate_request(
    policy: RetentionPolicy,
    request: RetentionRequest,
) -> Result<RetentionWatermarks, StoreError> {
    let watermarks = policy.watermarks(request.now_tick)?;
    if request.now_tick == 0
        || request.max_records == 0
        || request.max_records > policy.max_batch
        || request.max_records > MAX_RETENTION_BATCH
    {
        Err(StoreError::InvalidTransition)
    } else {
        Ok(watermarks)
    }
}

pub(crate) fn validate_provider_replacement(
    request: &ProviderStateArchiveReplacement,
    max_payload_bytes: usize,
) -> Result<(), StoreError> {
    let source = &request.source;
    let replacement = &request.replacement;
    if request.archived_tick == 0
        || request.watermark_tick > request.archived_tick
        || request.rows.is_empty()
        || request.rows.len() > MAX_RETENTION_BATCH
        || source.namespace != replacement.record.namespace
        || source.key != replacement.record.key
        || source.version == 0
        || replacement.expected_version != Some(source.version)
        || replacement.record.version
            != source.version.checked_add(1).ok_or(StoreError::Conflict)?
        || replacement.record.payload.len() > max_payload_bytes
    {
        return Err(StoreError::InvalidTransition);
    }
    let allowed = match request.target {
        RetentionTarget::RacfEvidence
            if source.namespace == "racf-database-v2" && source.key == "authority" =>
        {
            ["racf-audit", "racf-transaction", "racf-recovery"].as_slice()
        }
        _ => return Err(StoreError::InvalidTransition),
    };
    let mut identities = std::collections::BTreeSet::new();
    for candidate in &request.rows {
        let row = &candidate.row;
        if !allowed.contains(&row.namespace.as_str())
            || row.key.is_empty()
            || row.key.len() > 1024
            || row.version == 0
            || candidate.retention_tick == 0
            || candidate.retention_tick > request.watermark_tick
            || candidate.owner_execution.is_some()
            || candidate.owner_run_unit.is_some()
            || candidate.dependency != ProviderRetentionDependency::None
            || !identities.insert((row.namespace.as_str(), row.key.as_str()))
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if let Some(proof) = &candidate.observation {
            let observation = &proof.observation;
            observation::validate(observation)?;
            if proof.version == 0
                || observation.target != request.target
                || observation.namespace != row.namespace
                || observation.key != row.key
                || observation.source_version != row.version
                || observation.source_digest != source_digest(&row.payload)
                || observation.owner_execution.is_some()
                || observation.observed_tick > candidate.retention_tick
            {
                return Err(StoreError::IncompatibleVersion);
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_provider_deletion(
    request: &ProviderStateArchiveDeletion,
    max_payload_bytes: usize,
) -> Result<(), StoreError> {
    if request.archived_tick == 0
        || request.watermark_tick > request.archived_tick
        || request.rows.is_empty()
        || request.rows.len() > MAX_RETENTION_BATCH
    {
        return Err(StoreError::InvalidTransition);
    }
    let allowed_namespaces: &[&str] = match request.target {
        RetentionTarget::Db2Replay => &["db2-v1-replay"],
        RetentionTarget::ImsReplay => &["ims-v1-replay"],
        RetentionTarget::MqReplay => &["mq-v1-replay"],
        RetentionTarget::CicsReplay => &["cics-effect-replay-v1"],
        RetentionTarget::DatasetReplay => &["dataset-replay"],
        RetentionTarget::CicsUnitOfWork => &["cics-uow", "cics-uow-undo"],
        RetentionTarget::CobolLifecycle => &[
            "cobol-call-replay@1",
            "cobol-call-protocol@1",
            "cobol-call-protocol@2",
            "cobol-run-state@1",
            "cobol-cancel@1",
        ],
        RetentionTarget::SpoolJobs => &["jes-spool"],
        RetentionTarget::ConsoleLog => &["console-log"],
        _ => return Err(StoreError::InvalidTransition),
    };
    let mut identities = std::collections::BTreeSet::new();
    let candidate_identities = request
        .rows
        .iter()
        .map(|candidate| (candidate.row.namespace.as_str(), candidate.row.key.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    for candidate in &request.rows {
        let row = &candidate.row;
        let namespace_allowed = allowed_namespaces.contains(&row.namespace.as_str())
            || request.target == RetentionTarget::CobolLifecycle
                && row.namespace.starts_with("cobol-instance@1:");
        if !namespace_allowed
            || row.key.is_empty()
            || row.key.len() > 1024
            || row.version == 0
            || row.payload.len() > max_payload_bytes
            || candidate.retention_tick == 0
            || candidate.retention_tick > request.watermark_tick
            || !identities.insert((row.namespace.as_str(), row.key.as_str()))
        {
            return Err(StoreError::IncompatibleVersion);
        }
        if let Some(proof) = &candidate.observation {
            let observation = &proof.observation;
            observation::validate(observation)?;
            if proof.version == 0
                || observation.target != request.target
                || observation.namespace != row.namespace
                || observation.key != row.key
                || observation.source_version != row.version
                || observation.source_digest != source_digest(&row.payload)
                || observation.owner_execution != candidate.owner_execution
                || observation.observed_tick > candidate.retention_tick
            {
                return Err(StoreError::IncompatibleVersion);
            }
        }
        match &candidate.dependency {
            ProviderRetentionDependency::CoreEffect { key, .. }
                if key.as_str() == row.key
                    && candidate.owner_execution.is_some()
                    && candidate.owner_run_unit.is_some() => {}
            ProviderRetentionDependency::CicsNested { provenance, absent } => {
                let owner = candidate
                    .owner_execution
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                let run = candidate
                    .owner_run_unit
                    .as_ref()
                    .ok_or(StoreError::IncompatibleVersion)?;
                let prefix = format!("cics:{}:", run.as_str());
                if !row
                    .key
                    .strip_prefix(&prefix)
                    .is_some_and(|sequence| sequence.parse::<u64>().is_ok_and(|value| value != 0))
                    || owner.as_str().is_empty()
                {
                    return Err(StoreError::IncompatibleVersion);
                }
                provenance.validate_write(max_payload_bytes)?;
                if provenance.namespace != "cics-uow"
                    || absent.is_empty()
                    || absent.len() > 8
                    || absent.iter().any(|identity| {
                        identity.namespace.is_empty()
                            || identity.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
                            || identity.key.is_empty()
                            || identity.key.len() > MAX_PROVIDER_KEY_BYTES
                    })
                {
                    return Err(StoreError::IncompatibleVersion);
                }
            }
            ProviderRetentionDependency::ProviderGraph {
                required_rows,
                required_executions,
            } if candidate.owner_execution.is_some()
                && candidate.owner_run_unit.is_some()
                && required_rows.len() <= 32
                && required_executions.len() <= 32
                && required_rows.iter().all(|required| {
                    required.validate_write(max_payload_bytes).is_ok()
                        && !candidate_identities
                            .contains(&(required.namespace.as_str(), required.key.as_str()))
                }) => {}
            ProviderRetentionDependency::DirectProduct
                if request.target == RetentionTarget::ConsoleLog
                    && candidate.owner_execution.is_none()
                    && candidate.owner_run_unit.is_none() => {}
            ProviderRetentionDependency::None
                if request.target == RetentionTarget::SpoolJobs
                    && candidate.owner_execution.is_none()
                    && candidate.owner_run_unit.is_none() => {}
            _ => return Err(StoreError::IncompatibleVersion),
        }
    }
    Ok(())
}

pub(crate) fn target_watermark(target: RetentionTarget, watermarks: RetentionWatermarks) -> u64 {
    match target {
        RetentionTarget::TerminalExecutions
        | RetentionTarget::TerminalWork
        | RetentionTarget::LifecycleEvents
        | RetentionTarget::DeliveredOutbox
        | RetentionTarget::ConsoleLog => watermarks.lifecycle_tick,
        RetentionTarget::ResolvedEffects
        | RetentionTarget::Db2Replay
        | RetentionTarget::ImsReplay
        | RetentionTarget::MqReplay
        | RetentionTarget::CicsReplay
        | RetentionTarget::DatasetReplay
        | RetentionTarget::CicsUnitOfWork => watermarks.idempotency_tick,
        RetentionTarget::Audit => watermarks.audit_tick,
        RetentionTarget::RacfEvidence => watermarks.audit_tick.min(watermarks.idempotency_tick),
        RetentionTarget::CobolLifecycle | RetentionTarget::SpoolJobs => {
            watermarks.lifecycle_tick.min(watermarks.idempotency_tick)
        }
    }
}

pub(crate) const fn target_window_open(
    target: RetentionTarget,
    policy: RetentionPolicy,
    now_tick: u64,
) -> bool {
    let lifetime = match target {
        RetentionTarget::TerminalExecutions
        | RetentionTarget::TerminalWork
        | RetentionTarget::LifecycleEvents
        | RetentionTarget::DeliveredOutbox
        | RetentionTarget::ConsoleLog => policy.lifecycle_ticks,
        RetentionTarget::ResolvedEffects
        | RetentionTarget::Db2Replay
        | RetentionTarget::ImsReplay
        | RetentionTarget::MqReplay
        | RetentionTarget::CicsReplay
        | RetentionTarget::DatasetReplay
        | RetentionTarget::CicsUnitOfWork => policy.idempotency_ticks,
        RetentionTarget::Audit => policy.audit_ticks,
        RetentionTarget::RacfEvidence => {
            if policy.audit_ticks > policy.idempotency_ticks {
                policy.audit_ticks
            } else {
                policy.idempotency_ticks
            }
        }
        RetentionTarget::CobolLifecycle | RetentionTarget::SpoolJobs => {
            if policy.lifecycle_ticks > policy.idempotency_ticks {
                policy.lifecycle_ticks
            } else {
                policy.idempotency_ticks
            }
        }
    };
    now_tick >= lifetime
}

pub(crate) const fn target_namespace(target: RetentionTarget) -> Option<&'static str> {
    match target {
        RetentionTarget::TerminalExecutions => Some("durable-execution"),
        RetentionTarget::TerminalWork => Some("durable-work"),
        RetentionTarget::DeliveredOutbox => Some("durable-outbox"),
        RetentionTarget::ResolvedEffects => Some("durable-effect"),
        RetentionTarget::Db2Replay => Some("db2-v1-replay"),
        RetentionTarget::ImsReplay => Some("ims-v1-replay"),
        RetentionTarget::MqReplay => Some("mq-v1-replay"),
        RetentionTarget::CicsReplay => Some("cics-effect-replay-v1"),
        RetentionTarget::DatasetReplay => Some("dataset-replay"),
        RetentionTarget::CicsUnitOfWork => Some("cics-uow"),
        RetentionTarget::SpoolJobs => Some("jes-spool"),
        RetentionTarget::ConsoleLog => Some("console-log"),
        RetentionTarget::LifecycleEvents
        | RetentionTarget::Audit
        | RetentionTarget::RacfEvidence
        | RetentionTarget::CobolLifecycle => None,
    }
}

pub(crate) const fn replay_schema(target: RetentionTarget) -> Option<&'static str> {
    match target {
        RetentionTarget::Db2Replay => Some("mainframe-env.db2-object-row@1"),
        RetentionTarget::ImsReplay => Some("mainframe-env.ims-object-row@1"),
        RetentionTarget::MqReplay => Some("mainframe-env.mq-object-row@1"),
        _ => None,
    }
}

pub(crate) const fn target_name(target: RetentionTarget) -> &'static str {
    target.as_str()
}

pub(crate) fn target_back(value: &str) -> Result<RetentionTarget, StoreError> {
    Ok(match value {
        "terminal-executions" => RetentionTarget::TerminalExecutions,
        "terminal-work" => RetentionTarget::TerminalWork,
        "lifecycle-events" => RetentionTarget::LifecycleEvents,
        "delivered-outbox" => RetentionTarget::DeliveredOutbox,
        "resolved-effects" => RetentionTarget::ResolvedEffects,
        "db2-replay" => RetentionTarget::Db2Replay,
        "ims-replay" => RetentionTarget::ImsReplay,
        "mq-replay" => RetentionTarget::MqReplay,
        "cics-replay" => RetentionTarget::CicsReplay,
        "dataset-replay" => RetentionTarget::DatasetReplay,
        "cics-unit-of-work" => RetentionTarget::CicsUnitOfWork,
        "audit" => RetentionTarget::Audit,
        "racf-evidence" => RetentionTarget::RacfEvidence,
        "cobol-lifecycle" => RetentionTarget::CobolLifecycle,
        "spool-jobs" => RetentionTarget::SpoolJobs,
        "console-log" => RetentionTarget::ConsoleLog,
        _ => return Err(StoreError::IncompatibleVersion),
    })
}

pub(crate) struct ReplayRetentionMetadata {
    pub owner_execution: ExecutionId,
    pub deadline_tick: u64,
}

pub(crate) fn replay_metadata(
    payload: &[u8],
    expected_key: &str,
    expected_schema: &str,
) -> Result<Option<ReplayRetentionMetadata>, StoreError> {
    let value: Value =
        serde_json::from_slice(payload).map_err(|_| StoreError::IncompatibleVersion)?;
    let row = value.as_object().ok_or(StoreError::IncompatibleVersion)?;
    if row.len() != 3
        || row.get("schema_version").and_then(Value::as_str) != Some(expected_schema)
        || row.get("object_key").and_then(Value::as_str) != Some(expected_key)
    {
        return Err(StoreError::IncompatibleVersion);
    }
    let replay = row
        .get("value")
        .and_then(Value::as_object)
        .ok_or(StoreError::IncompatibleVersion)?;
    validate_provider_replay(expected_schema, replay)?;
    let deadline_tick = match replay.get("recorded_deadline_tick") {
        None => None,
        Some(value) => {
            Some(value.as_u64().ok_or(StoreError::IncompatibleVersion)?).filter(|tick| *tick != 0)
        }
    };
    let owner_execution = match replay.get("owner_execution") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            ExecutionId::new(
                value.as_str().ok_or(StoreError::IncompatibleVersion)?,
                InvocationLimits::default(),
            )
            .map_err(|_| StoreError::IncompatibleVersion)?,
        ),
    };
    match (owner_execution, deadline_tick) {
        (Some(owner_execution), Some(deadline_tick)) => Ok(Some(ReplayRetentionMetadata {
            owner_execution,
            deadline_tick,
        })),
        // Rows written before owner/age metadata existed remain readable but are never
        // guessed old. A CAS-fenced operator reconciliation can age them explicitly.
        _ => Ok(None),
    }
}

pub(crate) fn cics_replay_metadata(
    payload: &[u8],
) -> Result<Option<ReplayRetentionMetadata>, StoreError> {
    let mut reader = ReplayReader {
        bytes: payload,
        at: 0,
    };
    let version = match reader.take(8)? {
        b"MECER001" => 1,
        b"MECER002" => 2,
        _ => return Err(StoreError::IncompatibleVersion),
    };
    let metadata = if version == 2 {
        let owner = reader.text(InvocationLimits::default().max_identity_bytes)?;
        let deadline_tick = reader.u64()?;
        if deadline_tick == 0 {
            return Err(StoreError::IncompatibleVersion);
        }
        Some(ReplayRetentionMetadata {
            owner_execution: ExecutionId::new(owner, InvocationLimits::default())
                .map_err(|_| StoreError::IncompatibleVersion)?,
            deadline_tick,
        })
    } else {
        None
    };
    reader.take(32)?;
    if !matches!(reader.byte()?, 1..=6) {
        return Err(StoreError::IncompatibleVersion);
    }
    reader.text(128)?;
    reader.take(4)?;
    reader.take(4)?;
    reader.text(16)?;
    reader.text(16)?;
    reader.text(16)?;
    reader.byte()?;
    reader.optional_text(128)?;
    reader.optional_text(128)?;
    if reader.text(128)?.is_empty() {
        return Err(StoreError::IncompatibleVersion);
    }
    reader.field(4 * 1024 * 1024)?;
    let outputs = usize::try_from(reader.u32()?).map_err(|_| StoreError::CapacityExceeded)?;
    if outputs > 512 {
        return Err(StoreError::IncompatibleVersion);
    }
    for _ in 0..outputs {
        reader.text(128)?;
        if reader.text(128)?.is_empty() {
            return Err(StoreError::IncompatibleVersion);
        }
        reader.field(4 * 1024 * 1024)?;
    }
    if !matches!(reader.byte()?, 0..=2) || reader.at != payload.len() {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(metadata)
}

fn validate_provider_replay(
    schema: &str,
    replay: &serde_json::Map<String, Value>,
) -> Result<(), StoreError> {
    let common = [
        "request_digest_format",
        "request_sha256",
        "recorded_deadline_tick",
        "owner_execution",
    ];
    if let Some(format) = replay.get("request_digest_format") {
        match format.as_str() {
            Some("legacy-debug@0" | "mainframe-env.provider-replay-canonical@1") => {}
            _ => return Err(StoreError::IncompatibleVersion),
        }
    }
    byte_array(
        replay
            .get("request_sha256")
            .ok_or(StoreError::IncompatibleVersion)?,
        Some(32),
    )?;
    if let Some(tick) = replay.get("recorded_deadline_tick") {
        tick.as_u64().ok_or(StoreError::IncompatibleVersion)?;
    }
    if let Some(owner) = replay.get("owner_execution") {
        match owner {
            Value::Null | Value::String(_) => {}
            _ => return Err(StoreError::IncompatibleVersion),
        }
    }
    let allowed = match schema {
        "mainframe-env.db2-object-row@1" => {
            replay_i64(replay, "sqlcode", i64::from(i32::MIN), i64::from(i32::MAX))?;
            replay_text(replay, "sqlstate")?;
            replay_text(replay, "message")?;
            let rows = replay
                .get("rows")
                .and_then(Value::as_array)
                .ok_or(StoreError::IncompatibleVersion)?;
            for row in rows {
                for column in row.as_array().ok_or(StoreError::IncompatibleVersion)? {
                    byte_array(column, None)?;
                }
            }
            replay_u64(replay, "affected_rows")?;
            [
                "request_digest_format",
                "request_sha256",
                "recorded_deadline_tick",
                "owner_execution",
                "sqlcode",
                "sqlstate",
                "message",
                "rows",
                "affected_rows",
            ]
            .as_slice()
        }
        "mainframe-env.ims-object-row@1" => {
            replay_text(replay, "status")?;
            let segments = replay
                .get("segments")
                .and_then(Value::as_array)
                .ok_or(StoreError::IncompatibleVersion)?;
            for segment in segments {
                let fields = segment
                    .as_array()
                    .filter(|row| row.len() == 3)
                    .ok_or(StoreError::IncompatibleVersion)?;
                fields[0].as_str().ok_or(StoreError::IncompatibleVersion)?;
                if !fields[1].is_null() {
                    byte_array(&fields[1], None)?;
                }
                byte_array(&fields[2], None)?;
            }
            replay_optional_text(replay, "checkpoint_id")?;
            replay_u64(replay, "affected_segments")?;
            [
                "request_digest_format",
                "request_sha256",
                "recorded_deadline_tick",
                "owner_execution",
                "status",
                "segments",
                "checkpoint_id",
                "affected_segments",
            ]
            .as_slice()
        }
        "mainframe-env.mq-object-row@1" => {
            replay_i64(
                replay,
                "completion_code",
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            )?;
            replay_i64(
                replay,
                "reason_code",
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            )?;
            replay_optional_u64(replay, "handle", u64::from(u32::MAX))?;
            byte_array(
                replay
                    .get("message")
                    .ok_or(StoreError::IncompatibleVersion)?,
                None,
            )?;
            replay_optional_bytes(replay, "message_id")?;
            replay_optional_bytes(replay, "correlation_id")?;
            replay_optional_text(replay, "trigger_program")?;
            [
                "request_digest_format",
                "request_sha256",
                "recorded_deadline_tick",
                "owner_execution",
                "completion_code",
                "reason_code",
                "handle",
                "message",
                "message_id",
                "correlation_id",
                "trigger_program",
            ]
            .as_slice()
        }
        _ => return Err(StoreError::IncompatibleVersion),
    };
    if replay
        .keys()
        .any(|key| !common.contains(&key.as_str()) && !allowed.contains(&key.as_str()))
    {
        return Err(StoreError::IncompatibleVersion);
    }
    Ok(())
}

fn replay_text<'a>(
    replay: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, StoreError> {
    replay
        .get(key)
        .and_then(Value::as_str)
        .ok_or(StoreError::IncompatibleVersion)
}

fn replay_optional_text(
    replay: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<(), StoreError> {
    match replay.get(key).ok_or(StoreError::IncompatibleVersion)? {
        Value::Null | Value::String(_) => Ok(()),
        _ => Err(StoreError::IncompatibleVersion),
    }
}

fn replay_u64(replay: &serde_json::Map<String, Value>, key: &str) -> Result<u64, StoreError> {
    replay
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(StoreError::IncompatibleVersion)
}

fn replay_optional_u64(
    replay: &serde_json::Map<String, Value>,
    key: &str,
    max: u64,
) -> Result<(), StoreError> {
    match replay.get(key).ok_or(StoreError::IncompatibleVersion)? {
        Value::Null => Ok(()),
        value if value.as_u64().is_some_and(|value| value <= max) => Ok(()),
        _ => Err(StoreError::IncompatibleVersion),
    }
}

fn replay_i64(
    replay: &serde_json::Map<String, Value>,
    key: &str,
    minimum: i64,
    maximum: i64,
) -> Result<(), StoreError> {
    if replay
        .get(key)
        .and_then(Value::as_i64)
        .is_some_and(|value| (minimum..=maximum).contains(&value))
    {
        Ok(())
    } else {
        Err(StoreError::IncompatibleVersion)
    }
}

fn replay_optional_bytes(
    replay: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<(), StoreError> {
    match replay.get(key).ok_or(StoreError::IncompatibleVersion)? {
        Value::Null => Ok(()),
        value => byte_array(value, None),
    }
}

fn byte_array(value: &Value, exact: Option<usize>) -> Result<(), StoreError> {
    let values = value.as_array().ok_or(StoreError::IncompatibleVersion)?;
    if exact.is_some_and(|exact| values.len() != exact)
        || values.iter().any(|value| {
            value
                .as_u64()
                .is_none_or(|value| value > u64::from(u8::MAX))
        })
    {
        Err(StoreError::IncompatibleVersion)
    } else {
        Ok(())
    }
}

struct ReplayReader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> ReplayReader<'a> {
    fn take(&mut self, amount: usize) -> Result<&'a [u8], StoreError> {
        let end = self
            .at
            .checked_add(amount)
            .ok_or(StoreError::CapacityExceeded)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(StoreError::IncompatibleVersion)?;
        self.at = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, StoreError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, StoreError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| StoreError::IncompatibleVersion)?,
        ))
    }

    fn field(&mut self, max: usize) -> Result<&'a [u8], StoreError> {
        let amount = usize::try_from(self.u32()?).map_err(|_| StoreError::CapacityExceeded)?;
        if amount > max {
            return Err(StoreError::IncompatibleVersion);
        }
        self.take(amount)
    }

    fn text(&mut self, max: usize) -> Result<&'a str, StoreError> {
        std::str::from_utf8(self.field(max)?).map_err(|_| StoreError::IncompatibleVersion)
    }

    fn optional_text(&mut self, max: usize) -> Result<(), StoreError> {
        match self.byte()? {
            0 => Ok(()),
            1 => self.text(max).map(|_| ()),
            _ => Err(StoreError::IncompatibleVersion),
        }
    }
}

pub(crate) fn build_archive(
    target: RetentionTarget,
    archived_tick: u64,
    watermark_tick: u64,
    rows: Vec<ArchivedRetentionRow>,
) -> Result<RetentionArchive, StoreError> {
    if archived_tick == 0
        || watermark_tick > archived_tick
        || rows.is_empty()
        || rows.len() > MAX_RETENTION_BATCH
    {
        return Err(StoreError::InvalidTransition);
    }
    let mut digest = Sha256::new();
    digest.update(b"mainframe-env.retention-archive@1\0");
    digest.update(target_name(target).as_bytes());
    digest.update(archived_tick.to_be_bytes());
    digest.update(watermark_tick.to_be_bytes());
    for row in &rows {
        validate_archive_row_domain(target, row)?;
        if row.retention_tick == 0 || row.retention_tick > watermark_tick {
            return Err(StoreError::IncompatibleVersion);
        }
        digest.update(
            u64::try_from(row.namespace.len())
                .map_err(|_| StoreError::CapacityExceeded)?
                .to_be_bytes(),
        );
        digest.update(row.namespace.as_bytes());
        digest.update(
            u64::try_from(row.key.len())
                .map_err(|_| StoreError::CapacityExceeded)?
                .to_be_bytes(),
        );
        digest.update(row.key.as_bytes());
        digest.update(row.version.to_be_bytes());
        digest.update(Sha256::digest(&row.payload));
        digest.update(row.retention_tick.to_be_bytes());
        if let Some(owner) = &row.owner_execution {
            digest.update([1]);
            digest.update(
                u64::try_from(owner.as_str().len())
                    .map_err(|_| StoreError::CapacityExceeded)?
                    .to_be_bytes(),
            );
            digest.update(owner.as_str().as_bytes());
        } else {
            digest.update([0]);
        }
    }
    Ok(RetentionArchive {
        archive_id: format!("sha256:{:x}", digest.finalize()),
        target,
        archived_tick,
        watermark_tick,
        rows,
    })
}

/// Bytes charged to each archived source row for its ordinal, source version,
/// archive identifier, and length fields in addition to variable content.
pub(crate) const RETENTION_ROW_FIXED_BYTES: u64 = 96;
pub(crate) const RETENTION_ARCHIVE_FIXED_BYTES: u64 = 256;

pub(crate) fn archived_row_storage_bytes(row: &ArchivedRetentionRow) -> Result<u64, StoreError> {
    let variable = row
        .namespace
        .len()
        .checked_add(row.key.len())
        .and_then(|bytes| bytes.checked_add(row.payload.len()))
        .and_then(|bytes| {
            bytes.checked_add(
                row.owner_execution
                    .as_ref()
                    .map_or(0, |owner| owner.as_str().len()),
            )
        })
        .ok_or(StoreError::CapacityExceeded)?;
    RETENTION_ROW_FIXED_BYTES
        .checked_add(u64::try_from(variable).map_err(|_| StoreError::CapacityExceeded)?)
        .ok_or(StoreError::CapacityExceeded)
}

pub(crate) fn archive_storage_bytes(rows: &[ArchivedRetentionRow]) -> Result<u64, StoreError> {
    rows.iter()
        .try_fold(RETENTION_ARCHIVE_FIXED_BYTES, |total, row| {
            total
                .checked_add(archived_row_storage_bytes(row)?)
                .ok_or(StoreError::CapacityExceeded)
        })
}

pub(crate) fn worst_case_archived_row_storage_bytes(max_payload_bytes: usize) -> u64 {
    u64::try_from(max_payload_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(RETENTION_ROW_FIXED_BYTES)
        .saturating_add(RETENTION_ARCHIVE_FIXED_BYTES)
        .saturating_add(
            u64::try_from(
                MAX_PROVIDER_NAMESPACE_BYTES
                    + MAX_PROVIDER_KEY_BYTES
                    + InvocationLimits::default().max_identity_bytes,
            )
            .unwrap_or(u64::MAX),
        )
}

pub(crate) fn validate_archive_row_domain(
    target: RetentionTarget,
    row: &ArchivedRetentionRow,
) -> Result<(), StoreError> {
    if row.namespace.is_empty()
        || row.namespace.len() > MAX_PROVIDER_NAMESPACE_BYTES
        || row.key.is_empty()
        || row.key.len() > MAX_PROVIDER_KEY_BYTES
        || row.version == 0
        || row.version > i64::MAX as u64
    {
        return Err(StoreError::IncompatibleVersion);
    }
    let valid = match target {
        RetentionTarget::TerminalExecutions => {
            row.namespace == "durable-execution"
                && row.owner_execution.as_ref().map(|owner| owner.as_str())
                    == Some(row.key.as_str())
        }
        RetentionTarget::TerminalWork => {
            row.namespace == "durable-work" && row.owner_execution.is_some()
        }
        RetentionTarget::DeliveredOutbox => {
            row.namespace == "durable-outbox" && row.owner_execution.is_some()
        }
        RetentionTarget::ResolvedEffects => {
            row.namespace == "durable-effect" && row.owner_execution.is_some()
        }
        RetentionTarget::Db2Replay => {
            row.namespace == "db2-v1-replay" && row.owner_execution.is_some()
        }
        RetentionTarget::ImsReplay => {
            row.namespace == "ims-v1-replay" && row.owner_execution.is_some()
        }
        RetentionTarget::MqReplay => {
            row.namespace == "mq-v1-replay" && row.owner_execution.is_some()
        }
        RetentionTarget::CicsReplay => {
            row.namespace == "cics-effect-replay-v1" && row.owner_execution.is_some()
        }
        RetentionTarget::DatasetReplay => {
            row.namespace == "dataset-replay" && row.owner_execution.is_some()
        }
        RetentionTarget::CicsUnitOfWork => {
            matches!(row.namespace.as_str(), "cics-uow" | "cics-uow-undo")
                && (row.namespace == "cics-uow-undo" || row.owner_execution.is_some())
        }
        RetentionTarget::Audit => {
            row.namespace == "durable-audit-v1" && row.owner_execution.is_some()
        }
        RetentionTarget::RacfEvidence => {
            matches!(
                row.namespace.as_str(),
                "racf-audit" | "racf-transaction" | "racf-recovery"
            )
        }
        RetentionTarget::CobolLifecycle => {
            matches!(
                row.namespace.as_str(),
                "cobol-call-replay@1"
                    | "cobol-call-protocol@1"
                    | "cobol-call-protocol@2"
                    | "cobol-run-state@1"
                    | "cobol-cancel@1"
            ) || row.namespace.starts_with("cobol-instance@1:")
        }
        RetentionTarget::SpoolJobs => row.namespace == "jes-spool",
        RetentionTarget::ConsoleLog => row.namespace == "console-log",
        RetentionTarget::LifecycleEvents => {
            let Some(owner) = row.namespace.strip_prefix("durable-event:") else {
                return Err(StoreError::IncompatibleVersion);
            };
            ExecutionId::new(owner, InvocationLimits::default()).is_ok()
                && row.owner_execution.as_ref().map(|value| value.as_str()) == Some(owner)
                && row.key.len() == 20
                && row.key.parse::<u64>().is_ok_and(|sequence| sequence != 0)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(StoreError::IncompatibleVersion)
    }
}

pub(crate) struct ForecastCounts {
    pub active: usize,
    pub eligible: usize,
    pub capacity: usize,
    pub total_used: usize,
    pub archive_records: usize,
    pub archive_capacity: usize,
    pub archive_total_used: usize,
    pub archive_bytes: u64,
    pub archive_byte_capacity: u64,
    pub observation_records: usize,
    pub observation_capacity: usize,
    pub observation_bytes: u64,
    pub observation_byte_capacity: u64,
    pub max_source_storage_bytes: u64,
}

pub(crate) fn forecast(
    target: RetentionTarget,
    policy: RetentionPolicy,
    now_tick: u64,
    observed_growth_per_tick: u64,
    counts: ForecastCounts,
) -> Result<RetentionForecast, StoreError> {
    if now_tick == 0 {
        return Err(StoreError::InvalidTransition);
    }
    let watermarks = policy.watermarks(now_tick)?;
    let headroom = counts.capacity.saturating_sub(counts.total_used);
    let archive_headroom = counts
        .archive_capacity
        .saturating_sub(counts.archive_total_used);
    let archive_byte_headroom = counts
        .archive_byte_capacity
        .saturating_sub(counts.archive_bytes);
    let observation_headroom = counts
        .observation_capacity
        .saturating_sub(counts.observation_records);
    let observation_byte_headroom = counts
        .observation_byte_capacity
        .saturating_sub(counts.observation_bytes);
    let saturation = saturation(
        counts.capacity,
        counts.total_used,
        policy.low_watermark_percent,
        policy.high_watermark_percent,
    )
    .max(saturation(
        counts.archive_capacity,
        counts.archive_total_used,
        policy.low_watermark_percent,
        policy.high_watermark_percent,
    ))
    .max(saturation_u64(
        counts.archive_byte_capacity,
        counts.archive_bytes,
        policy.low_watermark_percent,
        policy.high_watermark_percent,
    ))
    .max(saturation(
        counts.observation_capacity,
        counts.observation_records,
        policy.low_watermark_percent,
        policy.high_watermark_percent,
    ))
    .max(saturation_u64(
        counts.observation_byte_capacity,
        counts.observation_bytes,
        policy.low_watermark_percent,
        policy.high_watermark_percent,
    ));
    let ticks_to_capacity = (observed_growth_per_tick != 0).then(|| {
        u64::try_from(headroom.min(archive_headroom).min(observation_headroom))
            .unwrap_or(u64::MAX)
            .div_ceil(observed_growth_per_tick)
    });
    let worst_case_byte_growth =
        observed_growth_per_tick.saturating_mul(counts.max_source_storage_bytes);
    let ticks_to_archive_byte_capacity = (worst_case_byte_growth != 0)
        .then(|| archive_byte_headroom.div_ceil(worst_case_byte_growth));
    let ticks_to_observation_byte_capacity = (worst_case_byte_growth != 0)
        .then(|| observation_byte_headroom.div_ceil(worst_case_byte_growth));
    Ok(RetentionForecast {
        target,
        active_records: counts.active,
        eligible_records: counts.eligible,
        protected_records: counts.active.saturating_sub(counts.eligible),
        capacity: counts.capacity,
        headroom,
        archive_records: counts.archive_records,
        archive_capacity: counts.archive_capacity,
        archive_headroom,
        archive_bytes: counts.archive_bytes,
        archive_byte_capacity: counts.archive_byte_capacity,
        archive_byte_headroom,
        observation_records: counts.observation_records,
        observation_capacity: counts.observation_capacity,
        observation_headroom,
        observation_bytes: counts.observation_bytes,
        observation_byte_capacity: counts.observation_byte_capacity,
        observation_byte_headroom,
        observed_growth_per_tick,
        ticks_to_capacity,
        ticks_to_archive_byte_capacity,
        ticks_to_observation_byte_capacity,
        saturation,
        watermarks,
    })
}

pub(crate) fn saturation_u64(
    capacity: u64,
    total_used: u64,
    low_watermark_percent: u8,
    high_watermark_percent: u8,
) -> SaturationLevel {
    let headroom = capacity.saturating_sub(total_used);
    let used_percent = if capacity == 0 {
        100
    } else {
        total_used.saturating_mul(100).div_ceil(capacity).min(100)
    };
    if headroom == 0 {
        SaturationLevel::Full
    } else if used_percent >= u64::from(high_watermark_percent) {
        SaturationLevel::HighWatermark
    } else if used_percent >= u64::from(low_watermark_percent) {
        SaturationLevel::LowWatermark
    } else {
        SaturationLevel::Healthy
    }
}

pub(crate) fn saturation(
    capacity: usize,
    total_used: usize,
    low_watermark_percent: u8,
    high_watermark_percent: u8,
) -> SaturationLevel {
    let headroom = capacity.saturating_sub(total_used);
    let used_percent = if capacity == 0 {
        100
    } else {
        total_used.saturating_mul(100).div_ceil(capacity).min(100)
    };
    if headroom == 0 {
        SaturationLevel::Full
    } else if used_percent >= usize::from(high_watermark_percent) {
        SaturationLevel::HighWatermark
    } else if used_percent >= usize::from(low_watermark_percent) {
        SaturationLevel::LowWatermark
    } else {
        SaturationLevel::Healthy
    }
}
