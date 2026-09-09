use clap::Subcommand;
use mainframe_env_execution_api::{ExecutionId, InvocationLimits};
use mainframe_env_host_api::HostProblem;
use mainframe_env_server::{RetentionMaintenance, RetentionMaintenancePass, StoreProfile};
use mainframe_env_store_api::{
    MAX_RETENTION_BATCH, RetentionAgeReconciliation, RetentionArchivePruneOutcome,
    RetentionForecast, RetentionLegacyRow, RetentionReceipt, RetentionReconciliationReceipt,
    RetentionTarget, SaturationLevel,
};
use serde_json::{Value, json};
use std::thread;
use std::time::Duration;

const MAX_CONFLICT_RETRIES: u8 = 8;
const MAX_RETRY_DELAY_MILLIS: u64 = 16;

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum ServerCommand {
    /// Inspect or compact durable retention state without starting the service.
    Retention {
        #[command(subcommand)]
        action: RetentionAction,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum RetentionAction {
    /// Forecast saturation for every durable retention target.
    Forecast {
        /// Recent source-row growth observed per logical clock tick.
        #[arg(long, default_value_t = 0)]
        observed_growth_per_tick: u64,
        /// Additional attempts after a concurrent state mutation is observed.
        #[arg(
            long,
            default_value_t = 3,
            value_parser = clap::value_parser!(u8).range(0..=i64::from(MAX_CONFLICT_RETRIES))
        )]
        conflict_retries: u8,
    },
    /// Run one dependency-ordered, bounded archive/prune pass.
    Maintain {
        /// Maximum source or archived rows moved by any one operation.
        #[arg(long, value_parser = parse_batch_bound)]
        max_records: Option<usize>,
        /// Exact reviewed archive identity authorizing one indivisible oversized deletion.
        #[arg(long, value_parser = parse_archive_id)]
        authorize_oversized_archive: Option<String>,
        /// Additional attempts after a concurrent state mutation is observed.
        #[arg(
            long,
            default_value_t = 3,
            value_parser = clap::value_parser!(u8).range(0..=i64::from(MAX_CONFLICT_RETRIES))
        )]
        conflict_retries: u8,
    },
    /// List exact CAS tokens for legacy rows that lack trustworthy age.
    Legacy {
        /// Maximum legacy rows returned across all targets.
        #[arg(long, value_parser = parse_batch_bound)]
        max_records: Option<usize>,
        /// Additional attempts after a concurrent state mutation is observed.
        #[arg(
            long,
            default_value_t = 3,
            value_parser = clap::value_parser!(u8).range(0..=i64::from(MAX_CONFLICT_RETRIES))
        )]
        conflict_retries: u8,
    },
    /// Record a conservative age for one operator-inspected legacy row.
    Reconcile {
        /// Retention target containing the exact source row.
        #[arg(long, value_parser = parse_retention_target)]
        target: RetentionTarget,
        /// Exact logical namespace reported by retention legacy.
        #[arg(long)]
        namespace: String,
        /// Exact logical key reported by retention legacy.
        #[arg(long)]
        key: String,
        /// Exact source version reported by retention legacy.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        expected_version: u64,
        /// Verified owner for a legacy provider replay; never inferred.
        #[arg(long)]
        owner_execution: Option<String>,
        /// Additional attempts after a concurrent state mutation is observed.
        #[arg(
            long,
            default_value_t = 3,
            value_parser = clap::value_parser!(u8).range(0..=i64::from(MAX_CONFLICT_RETRIES))
        )]
        conflict_retries: u8,
    },
}

#[derive(Debug)]
pub(crate) struct MaintenanceError {
    pub(crate) code: &'static str,
    pub(crate) detail: Box<str>,
    pub(crate) target: Option<&'static str>,
    pub(crate) attempts: Option<usize>,
    pub(crate) phase: Option<&'static str>,
    pub(crate) partial: bool,
    pub(crate) progress: Option<Box<Value>>,
    pub(crate) authorization_required: Option<Box<Value>>,
}

impl MaintenanceError {
    fn operation(problem: HostProblem, target: Option<RetentionTarget>, attempts: usize) -> Self {
        Self {
            code: if problem == HostProblem::IdempotencyConflict {
                "conflict_retries_exhausted"
            } else {
                host_problem_code(&problem)
            },
            detail: problem.to_string().into_boxed_str(),
            target: target.map(RetentionTarget::as_str),
            attempts: Some(attempts),
            phase: None,
            partial: false,
            progress: None,
            authorization_required: None,
        }
    }

    fn simple(code: &'static str, detail: impl Into<Box<str>>) -> Self {
        Self {
            code,
            detail: detail.into(),
            target: None,
            attempts: None,
            phase: None,
            partial: false,
            progress: None,
            authorization_required: None,
        }
    }

    fn with_progress(
        mut self,
        phase: &'static str,
        expired_archives: Option<Value>,
        targets: &[Value],
    ) -> Self {
        self.phase = Some(phase);
        self.partial = expired_archives.is_some() || !targets.is_empty();
        self.progress = Some(Box::new(json!({
            "phase": phase,
            "expired_archives": expired_archives,
            "completed_targets": targets,
            "safe_retry": "rerun-the-full-bounded-pass",
        })));
        self
    }
}

trait LegacyOperator {
    fn legacy_rows(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem>;
}

trait RetentionOperator: LegacyOperator {
    fn forecast(
        &self,
        target: RetentionTarget,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, HostProblem>;

    fn archive_and_prune(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<RetentionReceipt, HostProblem>;

    fn prune_archives(
        &self,
        max_records: usize,
        authorized_oversized_archive_id: Option<String>,
    ) -> Result<RetentionArchivePruneOutcome, HostProblem>;

    fn reconcile(
        &self,
        request: RetentionAgeReconciliation,
    ) -> Result<RetentionReconciliationReceipt, HostProblem>;
}

impl LegacyOperator for RetentionMaintenance {
    fn legacy_rows(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        self.legacy_rows(target, max_records)
    }
}

impl LegacyOperator for RetentionMaintenancePass<'_> {
    fn legacy_rows(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
        self.legacy_rows(target, max_records)
    }
}

impl RetentionOperator for RetentionMaintenancePass<'_> {
    fn forecast(
        &self,
        target: RetentionTarget,
        observed_growth_per_tick: u64,
    ) -> Result<RetentionForecast, HostProblem> {
        self.forecast(target, observed_growth_per_tick)
    }

    fn archive_and_prune(
        &self,
        target: RetentionTarget,
        max_records: usize,
    ) -> Result<RetentionReceipt, HostProblem> {
        self.archive_and_prune(target, max_records)
    }

    fn prune_archives(
        &self,
        max_records: usize,
        authorized_oversized_archive_id: Option<String>,
    ) -> Result<RetentionArchivePruneOutcome, HostProblem> {
        self.prune_archives(max_records, authorized_oversized_archive_id)
    }

    fn reconcile(
        &self,
        request: RetentionAgeReconciliation,
    ) -> Result<RetentionReconciliationReceipt, HostProblem> {
        self.reconcile(request)
    }
}

pub(crate) fn execute(
    maintenance: &RetentionMaintenance,
    profile: StoreProfile,
    configured_max_batch: usize,
    action: RetentionAction,
) -> Result<Value, MaintenanceError> {
    validate_common(profile, &action)?;
    if matches!(action, RetentionAction::Legacy { .. }) {
        return execute_legacy_only(maintenance, profile, configured_max_batch, action);
    }
    let pass = maintenance.begin_pass().map_err(|problem| {
        let mut error = MaintenanceError::operation(problem, None, 1);
        error.phase = Some("clock");
        error
    })?;
    execute_with_operator(&pass, profile, configured_max_batch, action)
}

fn validate_common(
    profile: StoreProfile,
    action: &RetentionAction,
) -> Result<(), MaintenanceError> {
    if profile == StoreProfile::Memory {
        return Err(MaintenanceError::simple(
            "durable_store_required",
            "retention maintenance requires the sqlite or postgres store profile",
        ));
    }
    let conflict_retries = action_conflict_retries(action);
    if conflict_retries > MAX_CONFLICT_RETRIES {
        return Err(MaintenanceError::simple(
            "invalid_conflict_retry_bound",
            format!("conflict_retries cannot exceed {MAX_CONFLICT_RETRIES}"),
        ));
    }
    Ok(())
}

fn execute_legacy_only(
    operator: &dyn LegacyOperator,
    profile: StoreProfile,
    configured_max_batch: usize,
    action: RetentionAction,
) -> Result<Value, MaintenanceError> {
    let RetentionAction::Legacy {
        max_records,
        conflict_retries,
    } = action
    else {
        return Err(MaintenanceError::simple(
            "invalid_action",
            "a clock-free maintenance route accepted a mutating action",
        ));
    };
    legacy(
        operator,
        profile,
        configured_max_batch,
        max_records,
        conflict_retries,
    )
}

fn execute_with_operator(
    operator: &dyn RetentionOperator,
    profile: StoreProfile,
    configured_max_batch: usize,
    action: RetentionAction,
) -> Result<Value, MaintenanceError> {
    validate_common(profile, &action)?;
    match action {
        RetentionAction::Forecast {
            observed_growth_per_tick,
            conflict_retries,
        } => forecast(
            operator,
            profile,
            observed_growth_per_tick,
            conflict_retries,
        ),
        RetentionAction::Maintain {
            max_records,
            authorize_oversized_archive,
            conflict_retries,
        } => maintain(
            operator,
            profile,
            configured_max_batch,
            max_records,
            authorize_oversized_archive,
            conflict_retries,
        ),
        RetentionAction::Legacy {
            max_records,
            conflict_retries,
        } => legacy(
            operator,
            profile,
            configured_max_batch,
            max_records,
            conflict_retries,
        ),
        RetentionAction::Reconcile {
            target,
            namespace,
            key,
            expected_version,
            owner_execution,
            conflict_retries,
        } => reconcile(
            operator,
            profile,
            target,
            namespace,
            key,
            expected_version,
            owner_execution,
            conflict_retries,
        ),
    }
}

fn forecast(
    operator: &dyn RetentionOperator,
    profile: StoreProfile,
    observed_growth_per_tick: u64,
    conflict_retries: u8,
) -> Result<Value, MaintenanceError> {
    let mut targets = Vec::with_capacity(RetentionTarget::ALL.len());
    for (ordinal, target) in RetentionTarget::ALL.into_iter().enumerate() {
        let attempted = retry_conflicts(conflict_retries, ordinal, || {
            operator.forecast(target, observed_growth_per_tick)
        })
        .map_err(|failure| {
            MaintenanceError::operation(failure.problem, Some(target), failure.attempts)
                .with_progress("forecast", None, &targets)
        })?;
        targets.push(forecast_json(&attempted.value, attempted.attempts));
    }
    Ok(json!({
        "schema": "mainframe-env.retention-forecast@2",
        "status": "ok",
        "mode": "forecast",
        "store_profile": profile_name(profile),
        "observed_growth_per_tick": observed_growth_per_tick,
        "conflict_retries": conflict_retries,
        "targets": targets,
    }))
}

fn maintain(
    operator: &dyn RetentionOperator,
    profile: StoreProfile,
    configured_max_batch: usize,
    requested_max_records: Option<usize>,
    authorize_oversized_archive: Option<String>,
    conflict_retries: u8,
) -> Result<Value, MaintenanceError> {
    let max_records = resolve_max_records(configured_max_batch, requested_max_records)?;
    let expired = retry_conflicts(conflict_retries, RetentionTarget::ALL.len(), || {
        operator.prune_archives(max_records, authorize_oversized_archive.clone())
    })
    .map_err(|failure| {
        MaintenanceError::operation(failure.problem, None, failure.attempts).with_progress(
            "archive_prune",
            None,
            &[],
        )
    })?;
    let expired_json = match expired.value {
        RetentionArchivePruneOutcome::Pruned(receipt) => {
            if receipt.pruned_source_rows > max_records && !receipt.oversized_authorization_used {
                return Err(MaintenanceError::simple(
                    "invalid_archive_prune_receipt",
                    "store exceeded the bound without consuming exact authorization",
                ));
            }
            json!({
                "pruned_source_rows": receipt.pruned_source_rows,
                "archive_ids": receipt.archive_ids,
                "oversized_authorization_used": receipt.oversized_authorization_used,
                "attempts": expired.attempts,
            })
        }
        RetentionArchivePruneOutcome::AuthorizationRequired {
            archive_id,
            source_rows,
            requested_max_records,
        } => {
            let authorization = json!({
                "archive_id": archive_id,
                "source_rows": source_rows,
                "requested_max_records": requested_max_records,
                "retry_flag": format!("--authorize-oversized-archive {archive_id}"),
            });
            let mut problem = MaintenanceError::simple(
                "oversized_archive_authorization_required",
                "the oldest expired archive is indivisible and exceeds --max-records",
            );
            problem.attempts = Some(expired.attempts);
            problem.phase = Some("archive_prune");
            problem.authorization_required = Some(Box::new(authorization));
            return Err(problem);
        }
    };

    let mut targets = Vec::with_capacity(RetentionTarget::ALL.len());
    for (ordinal, target) in RetentionTarget::ALL.into_iter().enumerate() {
        let attempted = retry_conflicts(conflict_retries, ordinal, || {
            operator.archive_and_prune(target, max_records)
        })
        .map_err(|failure| {
            MaintenanceError::operation(failure.problem, Some(target), failure.attempts)
                .with_progress("target", Some(expired_json.clone()), &targets)
        })?;
        validate_mutation_receipt(&attempted.value, max_records).map_err(|problem| {
            problem.with_progress("target", Some(expired_json.clone()), &targets)
        })?;
        targets.push(receipt_json(&attempted.value, attempted.attempts));
    }

    let legacy = list_legacy_rows(operator, max_records, conflict_retries).map_err(|problem| {
        problem.with_progress("legacy_inventory", Some(expired_json.clone()), &targets)
    })?;
    Ok(json!({
        "schema": "mainframe-env.retention-maintenance@2",
        "status": "ok",
        "mode": "maintain",
        "store_profile": profile_name(profile),
        "drained_window_required_for_guaranteed_progress": true,
        "max_records_per_operation": max_records,
        "conflict_retries": conflict_retries,
        "expired_archives": expired_json,
        "targets": targets,
        "legacy": legacy,
    }))
}

fn validate_mutation_receipt(
    receipt: &RetentionReceipt,
    max_records: usize,
) -> Result<(), MaintenanceError> {
    let mutations = receipt
        .pruned
        .checked_add(receipt.observations_created)
        .and_then(|value| value.checked_add(receipt.stale_observations_removed))
        .ok_or_else(|| {
            MaintenanceError::simple(
                "invalid_retention_receipt",
                "retention mutation accounting overflowed",
            )
        })?;
    if receipt.archived != receipt.pruned || mutations > max_records {
        return Err(MaintenanceError::simple(
            "invalid_retention_receipt",
            format!(
                "{} reported {mutations} mutations for a {max_records}-record operation",
                receipt.target.as_str()
            ),
        ));
    }
    Ok(())
}

fn legacy(
    operator: &dyn LegacyOperator,
    profile: StoreProfile,
    configured_max_batch: usize,
    requested_max_records: Option<usize>,
    conflict_retries: u8,
) -> Result<Value, MaintenanceError> {
    let max_records = resolve_max_records(configured_max_batch, requested_max_records)?;
    Ok(json!({
        "schema": "mainframe-env.retention-legacy@2",
        "status": "ok",
        "mode": "legacy",
        "store_profile": profile_name(profile),
        "max_records": max_records,
        "conflict_retries": conflict_retries,
        "legacy": list_legacy_rows(operator, max_records, conflict_retries).map_err(|problem| {
            problem.with_progress("legacy_inventory", None, &[])
        })?,
    }))
}

#[allow(clippy::too_many_arguments)]
fn reconcile(
    operator: &dyn RetentionOperator,
    profile: StoreProfile,
    target: RetentionTarget,
    namespace: String,
    key: String,
    expected_version: u64,
    owner_execution: Option<String>,
    conflict_retries: u8,
) -> Result<Value, MaintenanceError> {
    let owner_execution = owner_execution
        .map(|owner| {
            ExecutionId::new(owner, InvocationLimits::default()).map_err(|problem| {
                let mut error =
                    MaintenanceError::simple("invalid_owner_execution", problem.to_string());
                error.target = Some(target.as_str());
                error
            })
        })
        .transpose()?;
    let attempted = retry_conflicts(conflict_retries, 0, || {
        operator.reconcile(RetentionAgeReconciliation {
            target,
            namespace: namespace.clone(),
            key: key.clone(),
            expected_version,
            owner_execution: owner_execution.clone(),
        })
    })
    .map_err(|failure| {
        let mut error =
            MaintenanceError::operation(failure.problem, Some(target), failure.attempts);
        error.phase = Some("reconciliation");
        error
    })?;
    Ok(json!({
        "schema": "mainframe-env.retention-reconciliation@2",
        "status": "ok",
        "mode": "reconcile",
        "store_profile": profile_name(profile),
        "attempts": attempted.attempts,
        "receipt": reconciliation_json(&attempted.value),
    }))
}

fn list_legacy_rows(
    operator: &dyn LegacyOperator,
    max_records: usize,
    conflict_retries: u8,
) -> Result<Value, MaintenanceError> {
    let mut remaining = max_records;
    let mut rows = Vec::new();
    let mut attempts = 0;
    for (ordinal, target) in RetentionTarget::ALL.into_iter().enumerate() {
        if remaining == 0 {
            break;
        }
        let attempted = retry_conflicts(conflict_retries, ordinal, || {
            operator.legacy_rows(target, remaining)
        })
        .map_err(|failure| {
            MaintenanceError::operation(failure.problem, Some(target), failure.attempts)
        })?;
        attempts += attempted.attempts;
        remaining = remaining.saturating_sub(attempted.value.len());
        rows.extend(attempted.value.iter().map(legacy_row_json));
    }
    Ok(json!({
        "returned_records": rows.len(),
        "attempts": attempts,
        "limit_reached": rows.len() == max_records,
        "rows": rows,
    }))
}

fn resolve_max_records(
    configured_max_batch: usize,
    requested_max_records: Option<usize>,
) -> Result<usize, MaintenanceError> {
    let max_records = requested_max_records.unwrap_or(configured_max_batch);
    if max_records == 0 || max_records > configured_max_batch || max_records > MAX_RETENTION_BATCH {
        Err(MaintenanceError::simple(
            "invalid_batch_bound",
            format!(
                "max_records must be between 1 and the configured maximum of {configured_max_batch}"
            ),
        ))
    } else {
        Ok(max_records)
    }
}

struct Attempted<T> {
    value: T,
    attempts: usize,
}

struct RetryFailure {
    problem: HostProblem,
    attempts: usize,
}

fn retry_conflicts<T>(
    conflict_retries: u8,
    jitter_seed: usize,
    mut operation: impl FnMut() -> Result<T, HostProblem>,
) -> Result<Attempted<T>, RetryFailure> {
    let total_attempts = usize::from(conflict_retries) + 1;
    for attempt in 1..=total_attempts {
        match operation() {
            Ok(value) => {
                return Ok(Attempted {
                    value,
                    attempts: attempt,
                });
            }
            Err(HostProblem::IdempotencyConflict) if attempt < total_attempts => {
                thread::sleep(retry_delay(attempt, jitter_seed));
            }
            Err(problem) => {
                return Err(RetryFailure {
                    problem,
                    attempts: attempt,
                });
            }
        }
    }
    unreachable!("the positive retry bound always returns")
}

fn retry_delay(attempt: usize, jitter_seed: usize) -> Duration {
    let entropy = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            usize::try_from(elapsed.subsec_nanos()).unwrap_or(0)
        })
        ^ usize::try_from(std::process::id()).unwrap_or(0);
    let mixed = attempt
        .wrapping_mul(11)
        .wrapping_add(jitter_seed.wrapping_mul(7))
        .wrapping_add(entropy);
    Duration::from_millis(1 + (mixed as u64 % MAX_RETRY_DELAY_MILLIS))
}

fn forecast_json(forecast: &RetentionForecast, attempts: usize) -> Value {
    json!({
        "target": forecast.target.as_str(),
        "attempts": attempts,
        "active_records": forecast.active_records,
        "eligible_records": forecast.eligible_records,
        "protected_records": forecast.protected_records,
        "capacity": forecast.capacity,
        "headroom": forecast.headroom,
        "archive_records": forecast.archive_records,
        "archive_capacity": forecast.archive_capacity,
        "archive_headroom": forecast.archive_headroom,
        "archive_bytes": forecast.archive_bytes,
        "archive_byte_capacity": forecast.archive_byte_capacity,
        "archive_byte_headroom": forecast.archive_byte_headroom,
        "observation_records": forecast.observation_records,
        "observation_capacity": forecast.observation_capacity,
        "observation_headroom": forecast.observation_headroom,
        "observation_bytes": forecast.observation_bytes,
        "observation_byte_capacity": forecast.observation_byte_capacity,
        "observation_byte_headroom": forecast.observation_byte_headroom,
        "observed_growth_per_tick": forecast.observed_growth_per_tick,
        "ticks_to_capacity": forecast.ticks_to_capacity,
        "ticks_to_archive_byte_capacity": forecast.ticks_to_archive_byte_capacity,
        "ticks_to_observation_byte_capacity": forecast.ticks_to_observation_byte_capacity,
        "saturation": saturation_name(forecast.saturation),
        "watermarks": {
            "lifecycle_tick": forecast.watermarks.lifecycle_tick,
            "idempotency_tick": forecast.watermarks.idempotency_tick,
            "audit_tick": forecast.watermarks.audit_tick,
            "archive_tick": forecast.watermarks.archive_tick,
        },
    })
}

fn receipt_json(receipt: &RetentionReceipt, attempts: usize) -> Value {
    json!({
        "target": receipt.target.as_str(),
        "attempts": attempts,
        "watermark_tick": receipt.watermark_tick,
        "examined": receipt.examined,
        "archived": receipt.archived,
        "pruned": receipt.pruned,
        "protected": receipt.protected,
        "archive_id": receipt.archive_id,
        "observations_created": receipt.observations_created,
        "observations_reused": receipt.observations_reused,
        "stale_observations_removed": receipt.stale_observations_removed,
    })
}

fn legacy_row_json(row: &RetentionLegacyRow) -> Value {
    json!({
        "target": row.target.as_str(),
        "namespace": row.namespace,
        "key": row.key,
        "source_version": row.source_version,
    })
}

fn reconciliation_json(receipt: &RetentionReconciliationReceipt) -> Value {
    json!({
        "target": receipt.target.as_str(),
        "namespace": receipt.namespace,
        "key": receipt.key,
        "source_version": receipt.source_version,
        "observation_version": receipt.observation_version,
        "reconciled_tick": receipt.reconciled_tick,
    })
}

fn parse_retention_target(value: &str) -> Result<RetentionTarget, String> {
    RetentionTarget::ALL
        .into_iter()
        .find(|target| target.as_str() == value)
        .ok_or_else(|| {
            let accepted = RetentionTarget::ALL
                .into_iter()
                .map(RetentionTarget::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            format!("unknown retention target {value:?}; expected one of: {accepted}")
        })
}

fn parse_batch_bound(value: &str) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| "batch bound must be a positive integer".to_string())?;
    if value == 0 || value > MAX_RETENTION_BATCH {
        Err(format!(
            "batch bound must be between 1 and {MAX_RETENTION_BATCH}"
        ))
    } else {
        Ok(value)
    }
}

fn parse_archive_id(value: &str) -> Result<String, String> {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return Err("archive authorization must use sha256:<64 lowercase hex>".into());
    };
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("archive authorization must use sha256:<64 lowercase hex>".into());
    }
    Ok(value.into())
}

const fn action_conflict_retries(action: &RetentionAction) -> u8 {
    match action {
        RetentionAction::Forecast {
            conflict_retries, ..
        }
        | RetentionAction::Maintain {
            conflict_retries, ..
        }
        | RetentionAction::Legacy {
            conflict_retries, ..
        }
        | RetentionAction::Reconcile {
            conflict_retries, ..
        } => *conflict_retries,
    }
}

const fn saturation_name(level: SaturationLevel) -> &'static str {
    match level {
        SaturationLevel::Healthy => "healthy",
        SaturationLevel::LowWatermark => "low-watermark",
        SaturationLevel::HighWatermark => "high-watermark",
        SaturationLevel::Full => "full",
    }
}

const fn profile_name(profile: StoreProfile) -> &'static str {
    match profile {
        StoreProfile::Memory => "memory",
        StoreProfile::Sqlite => "sqlite",
        StoreProfile::Postgres => "postgres",
    }
}

const fn host_problem_code(problem: &HostProblem) -> &'static str {
    match problem {
        HostProblem::Malformed => "malformed",
        HostProblem::Unsupported => "unsupported",
        HostProblem::UnsupportedCapability { .. } => "unsupported_capability",
        HostProblem::NotFound => "not_found",
        HostProblem::Condition { .. } => "condition",
        HostProblem::Unauthorized => "unauthorized",
        HostProblem::Cancelled => "cancelled",
        HostProblem::TimedOut => "timed_out",
        HostProblem::ResourceExhausted => "resource_exhausted",
        HostProblem::ProviderFailure => "provider_failure",
        HostProblem::InfrastructureFailure => "infrastructure_failure",
        HostProblem::MissingIdempotency => "missing_idempotency",
        HostProblem::IdempotencyConflict => "conflict",
        HostProblem::UnknownOutcome => "unknown_outcome",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_store_api::{
        RetentionArchivePruneReceipt, RetentionPolicy, RetentionWatermarks,
    };
    use std::cell::{Cell, RefCell};

    struct FakeOperator {
        calls: RefCell<Vec<RetentionTarget>>,
        fail_at: Option<RetentionTarget>,
        prune: RetentionArchivePruneOutcome,
        legacy_calls: Cell<usize>,
    }

    impl FakeOperator {
        fn successful() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                fail_at: None,
                prune: RetentionArchivePruneOutcome::Pruned(RetentionArchivePruneReceipt {
                    pruned_source_rows: 0,
                    archive_ids: Vec::new(),
                    oversized_authorization_used: false,
                }),
                legacy_calls: Cell::new(0),
            }
        }
    }

    impl LegacyOperator for FakeOperator {
        fn legacy_rows(
            &self,
            _: RetentionTarget,
            _: usize,
        ) -> Result<Vec<RetentionLegacyRow>, HostProblem> {
            self.legacy_calls.set(self.legacy_calls.get() + 1);
            Ok(Vec::new())
        }
    }

    impl RetentionOperator for FakeOperator {
        fn forecast(
            &self,
            target: RetentionTarget,
            observed_growth_per_tick: u64,
        ) -> Result<RetentionForecast, HostProblem> {
            if self.fail_at == Some(target) {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(RetentionForecast {
                target,
                active_records: 0,
                eligible_records: 0,
                protected_records: 0,
                capacity: 8,
                headroom: 8,
                archive_records: 0,
                archive_capacity: 8,
                archive_headroom: 8,
                archive_bytes: 0,
                archive_byte_capacity: 1024,
                archive_byte_headroom: 1024,
                observation_records: 0,
                observation_capacity: 8,
                observation_headroom: 8,
                observation_bytes: 0,
                observation_byte_capacity: 1024,
                observation_byte_headroom: 1024,
                observed_growth_per_tick,
                ticks_to_capacity: None,
                ticks_to_archive_byte_capacity: None,
                ticks_to_observation_byte_capacity: None,
                saturation: SaturationLevel::Healthy,
                watermarks: RetentionWatermarks {
                    lifecycle_tick: 1,
                    idempotency_tick: 1,
                    audit_tick: 1,
                    archive_tick: 1,
                },
            })
        }

        fn archive_and_prune(
            &self,
            target: RetentionTarget,
            _: usize,
        ) -> Result<RetentionReceipt, HostProblem> {
            self.calls.borrow_mut().push(target);
            if self.fail_at == Some(target) {
                return Err(HostProblem::InfrastructureFailure);
            }
            Ok(RetentionReceipt {
                target,
                watermark_tick: 1,
                examined: 0,
                archived: 0,
                pruned: 0,
                protected: 0,
                archive_id: None,
                observations_created: 0,
                observations_reused: 0,
                stale_observations_removed: 0,
            })
        }

        fn prune_archives(
            &self,
            _: usize,
            _: Option<String>,
        ) -> Result<RetentionArchivePruneOutcome, HostProblem> {
            Ok(self.prune.clone())
        }

        fn reconcile(
            &self,
            _: RetentionAgeReconciliation,
        ) -> Result<RetentionReconciliationReceipt, HostProblem> {
            Err(HostProblem::Unsupported)
        }
    }

    #[test]
    fn conflict_retry_is_bounded_and_reports_attempts() {
        let calls = Cell::new(0);
        let outcome = retry_conflicts(2, 0, || {
            calls.set(calls.get() + 1);
            Err::<(), _>(HostProblem::IdempotencyConflict)
        })
        .err()
        .unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(outcome.attempts, 3);
    }

    #[test]
    fn maintenance_error_preserves_prior_receipts() {
        let mut operator = FakeOperator::successful();
        operator.fail_at = Some(RetentionTarget::CicsReplay);
        let error = execute_with_operator(
            &operator,
            StoreProfile::Sqlite,
            8,
            RetentionAction::Maintain {
                max_records: Some(8),
                authorize_oversized_archive: None,
                conflict_retries: 0,
            },
        )
        .unwrap_err();
        let progress = error.progress.unwrap();
        assert!(error.partial);
        assert_eq!(error.phase, Some("target"));
        assert_eq!(progress["phase"], "target");
        assert_eq!(
            progress["completed_targets"].as_array().unwrap().len(),
            RetentionTarget::ALL
                .into_iter()
                .position(|target| target == RetentionTarget::CicsReplay)
                .unwrap()
        );
        assert!(progress["expired_archives"].is_object());
    }

    #[test]
    fn oversized_archive_requires_exact_machine_readable_authorization() {
        let archive_id = format!("sha256:{}", "a".repeat(64));
        let mut operator = FakeOperator::successful();
        operator.prune = RetentionArchivePruneOutcome::AuthorizationRequired {
            archive_id: archive_id.clone(),
            source_rows: 17,
            requested_max_records: 8,
        };
        let error = execute_with_operator(
            &operator,
            StoreProfile::Sqlite,
            8,
            RetentionAction::Maintain {
                max_records: Some(8),
                authorize_oversized_archive: None,
                conflict_retries: 0,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "oversized_archive_authorization_required");
        assert_eq!(
            error.authorization_required.unwrap()["archive_id"],
            archive_id
        );
        assert!(operator.calls.borrow().is_empty());
    }

    #[test]
    fn receipt_json_includes_observation_accounting() {
        let receipt = RetentionReceipt {
            target: RetentionTarget::Db2Replay,
            watermark_tick: 1,
            examined: 3,
            archived: 1,
            pruned: 1,
            protected: 2,
            archive_id: Some("sha256:test".into()),
            observations_created: 4,
            observations_reused: 5,
            stale_observations_removed: 6,
        };
        let value = receipt_json(&receipt, 1);
        assert_eq!(value["observations_created"], 4);
        assert_eq!(value["observations_reused"], 5);
        assert_eq!(value["stale_observations_removed"], 6);
    }

    #[test]
    fn parsers_reject_unbounded_or_noncanonical_authority() {
        assert!(parse_batch_bound("0").is_err());
        assert!(parse_batch_bound("4097").is_err());
        assert!(parse_archive_id("sha256:ABC").is_err());
        assert!(parse_archive_id(&format!("sha256:{}", "a".repeat(64))).is_ok());
    }

    #[test]
    fn successful_maintenance_reports_contract_order() {
        let operator = FakeOperator::successful();
        let report = execute_with_operator(
            &operator,
            StoreProfile::Sqlite,
            8,
            RetentionAction::Maintain {
                max_records: Some(8),
                authorize_oversized_archive: None,
                conflict_retries: 0,
            },
        )
        .unwrap();
        let order = report["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["target"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            RetentionTarget::ALL
                .into_iter()
                .map(RetentionTarget::as_str)
                .collect::<Vec<_>>()
        );
        assert_eq!(report["schema"], "mainframe-env.retention-maintenance@2");
    }

    #[test]
    fn legacy_action_uses_only_the_read_operator() {
        let operator = FakeOperator::successful();
        let report = execute_legacy_only(
            &operator,
            StoreProfile::Sqlite,
            8,
            RetentionAction::Legacy {
                max_records: Some(8),
                conflict_retries: 0,
            },
        )
        .unwrap();
        assert_eq!(report["schema"], "mainframe-env.retention-legacy@2");
        assert_eq!(operator.legacy_calls.get(), RetentionTarget::ALL.len());
        assert!(operator.calls.borrow().is_empty());
    }

    #[allow(dead_code)]
    fn policy() -> RetentionPolicy {
        RetentionPolicy {
            lifecycle_ticks: 1,
            idempotency_ticks: 1,
            audit_ticks: 1,
            archive_ticks: 1,
            low_watermark_percent: 70,
            high_watermark_percent: 85,
            max_batch: 8,
        }
    }
}
