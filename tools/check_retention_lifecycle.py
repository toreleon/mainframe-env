"""Guard the complete, provider-owned durable retention lifecycle."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
TEST_MODULE = re.compile(r"(?m)^#\[cfg\(test\)\]\s*\nmod\s+[A-Za-z_]")

TARGETS = (
    ("Db2Replay", "db2-replay"),
    ("ImsReplay", "ims-replay"),
    ("MqReplay", "mq-replay"),
    ("DatasetReplay", "dataset-replay"),
    ("CicsUnitOfWork", "cics-unit-of-work"),
    ("CicsReplay", "cics-replay"),
    ("RacfEvidence", "racf-evidence"),
    ("CobolLifecycle", "cobol-lifecycle"),
    ("SpoolJobs", "spool-jobs"),
    ("ConsoleLog", "console-log"),
    ("ResolvedEffects", "resolved-effects"),
    ("DeliveredOutbox", "delivered-outbox"),
    ("Audit", "audit"),
    ("TerminalWork", "terminal-work"),
    ("LifecycleEvents", "lifecycle-events"),
    ("TerminalExecutions", "terminal-executions"),
)


def read(path: Path, *, production: bool = False) -> str:
    source = path.read_text()
    if production:
        for marker in reversed(tuple(TEST_MODULE.finditer(source))):
            line_end = source.find("\n", marker.end())
            line_end = len(source) if line_end < 0 else line_end
            declaration = source[marker.start() : line_end]
            if ";" in declaration and source[line_end:].strip():
                # An external test module declared near the top does not mark the end of
                # production. Only a terminal declaration or terminal inline module does.
                continue
            source = source[: marker.start()]
            break
    return source


def require(
    path: Path,
    fragments: tuple[str, ...],
    *,
    production: bool = False,
    companions: tuple[Path, ...] = (),
) -> str:
    source = read(path, production=production)
    for companion in companions:
        source += "\n" + read(companion, production=production)
    missing = [fragment for fragment in fragments if fragment not in source]
    if missing:
        relative = path.relative_to(ROOT)
        raise ValueError(f"{relative} retention contract is incomplete: {missing}")
    return source


def rust_block(source: str, signature: str) -> str:
    start = source.find(signature)
    if start < 0:
        raise ValueError(f"missing Rust item: {signature}")
    opening = source.find("{", start + len(signature))
    if opening < 0:
        raise ValueError(f"missing Rust item body: {signature}")
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start : index + 1]
    raise ValueError(f"unterminated Rust item: {signature}")


def normalized(source: str) -> str:
    return " ".join(source.split())


def check_targets(root: Path) -> None:
    path = root / "crates/contracts/mainframe-env-store-api/src/model.rs"
    source = read(path, production=True)
    target_enum = rust_block(source, "pub enum RetentionTarget")
    variants = re.findall(r"(?m)^\s{4}([A-Z][A-Za-z0-9]+),\s*$", target_enum)
    expected = [variant for variant, _ in TARGETS]
    if len(variants) != len(expected) or set(variants) != set(expected):
        raise ValueError(f"RetentionTarget is not the closed sixteen-family set: {variants}")

    match = re.search(
        r"pub const ALL:\s*\[Self;\s*16\]\s*=\s*\[(.*?)\];",
        source,
        re.DOTALL,
    )
    if match is None:
        raise ValueError("RetentionTarget::ALL is missing or no longer has sixteen entries")
    order = re.findall(r"Self::([A-Za-z0-9]+)", match.group(1))
    if order != expected:
        raise ValueError(f"RetentionTarget::ALL dependency order drifted: {order}")
    for variant, stable_name in TARGETS:
        if f'Self::{variant} => "{stable_name}"' not in source:
            raise ValueError(f"RetentionTarget::{variant} stable name drifted")

    require(
        path,
        (
            "pub const MAX_RETENTION_BATCH: usize = 4_096",
            "pub struct RetentionPolicy",
            "pub lifecycle_ticks: u64",
            "pub idempotency_ticks: u64",
            "pub audit_ticks: u64",
            "pub archive_ticks: u64",
            "pub struct RetentionWatermarks",
            "lifecycle_tick: now_tick.saturating_sub(self.lifecycle_ticks)",
            "idempotency_tick: now_tick.saturating_sub(self.idempotency_ticks)",
            "audit_tick: now_tick.saturating_sub(self.audit_ticks)",
            "archive_tick: now_tick.saturating_sub(self.archive_ticks)",
            "pub struct RetentionCapacityHealth",
            "pub struct ProviderStateArchiveReplacement",
            "pub struct ProviderStateArchiveDeletion",
            "pub struct ProviderRetentionRow",
            "pub enum ProviderRetentionDependency",
            "pub struct CoreRetentionDependencySnapshot",
            "pub blocked_executions: Vec<ExecutionId>",
            "pub blocked_effect_keys: Vec<IdempotencyKey>",
            "pub unowned: bool",
            "CoreEffect",
            "CicsNested",
            "ProviderGraph",
            "required_executions",
            "DirectProduct",
            "pub struct RetentionObservation",
            "pub struct RetentionObservationProof",
            "pub struct RetentionArchive",
            "pub struct RetentionArchivePruneRequest",
            "pub enum RetentionArchivePruneOutcome",
            "pub struct RetentionReceipt",
            "pub observations_created: usize",
            "pub observations_reused: usize",
            "pub stale_observations_removed: usize",
        ),
        production=True,
    )


def check_store_contract(root: Path) -> None:
    require(
        root / "crates/contracts/mainframe-env-store-api/src/traits.rs",
        (
            "pub trait RetentionStore: Send + Sync",
            "fn retention_capacity_health(",
            "fn retention_forecast(",
            "fn retention_forecast_with_dependencies(",
            "fn provider_validated_retention_forecast(",
            "fn archive_and_prune(",
            "fn archive_and_prune_with_dependencies(",
            "fn retention_archives(",
            "fn prune_retention_archives(",
            "fn prune_retention_archives_authorized(",
            "fn reconcile_retention_age(",
            "fn retention_legacy_rows(",
            "fn archive_provider_state_replacement(",
            "fn provider_retention_authority_usage(",
            "fn provider_state_retention_epoch(",
            "fn provider_retention_observation_page(",
            "fn delete_provider_retention_observation(",
            "fn record_provider_retention_observation(",
            "fn archive_provider_state_deletion(",
            "+ RetentionStore",
        ),
        production=True,
    )

    path = root / "crates/stores/mainframe-env-store/src/retention.rs"
    source = require(
        path,
        (
            "pub(crate) fn capacity_health(",
            "RetentionTarget::ALL",
            "pub(crate) fn validate_provider_replacement(",
            "mod provider_deletion;",
            "pub(crate) use provider_deletion::validate_provider_deletion;",
            "pub(crate) fn target_watermark(",
            "pub(crate) const fn target_window_open(",
            'b"mainframe-env.retention-archive@1\\0"',
            "pub(crate) fn archive_storage_bytes(",
            "pub(crate) fn validate_archive_row_domain(",
            "pub(crate) struct ForecastCounts",
            "observation_byte_capacity",
        ),
        production=True,
    )
    if "retention-archive-v1" in source:
        raise ValueError("retention regressed to a live provider-state archive singleton")
    check_provider_deletion(root)

    watermark = normalized(rust_block(source, "pub(crate) fn target_watermark"))
    for fragment in (
        "RetentionTarget::RacfEvidence => watermarks.audit_tick.min(watermarks.idempotency_tick)",
        "RetentionTarget::CobolLifecycle | RetentionTarget::SpoolJobs => { watermarks.lifecycle_tick.min(watermarks.idempotency_tick) }",
        "RetentionTarget::Audit => watermarks.audit_tick",
        "RetentionTarget::ResolvedEffects",
        "RetentionTarget::ConsoleLog",
    ):
        if fragment not in watermark:
            raise ValueError(f"retention watermark mapping drifted: {fragment}")
    window = normalized(rust_block(source, "pub(crate) const fn target_window_open"))
    for fragment in (
        "policy.lifecycle_ticks",
        "policy.idempotency_ticks",
        "policy.audit_ticks",
        "RetentionTarget::RacfEvidence",
        "RetentionTarget::CobolLifecycle | RetentionTarget::SpoolJobs",
    ):
        if fragment not in window:
            raise ValueError(f"retention pre-window protection drifted: {fragment}")

    observation = require(
        root / "crates/stores/mainframe-env-store/src/retention/observation.rs",
        (
            "observation.source_version == source_version",
            "observation.source_digest == source_digest(payload)",
            "observation.observed_tick == 0",
            "pub(crate) fn storage_bytes(",
            "Sha256::digest(payload).into()",
        ),
        production=True,
    )
    if "SystemTime" in observation or "UNIX_EPOCH" in observation:
        raise ValueError("retention observations regressed to process wall-clock time")


def check_provider_deletion(root: Path) -> None:
    require(
        root / "crates/stores/mainframe-env-store/src/retention/provider_deletion.rs",
        (
            "pub(crate) fn validate_provider_deletion(",
            "ProviderRetentionDependency::CoreEffect",
            "ProviderRetentionDependency::CicsNested",
            "ProviderRetentionDependency::ProviderGraph",
            "required_executions.len() > 32",
            "required_executions.len() <= 32",
            "ProviderRetentionDependency::DirectProduct",
        ),
        production=True,
    )


def check_dedicated_authorities(root: Path) -> None:
    migration_fragments = (
        "CREATE TABLE IF NOT EXISTS retention_archive (",
        "CREATE TABLE IF NOT EXISTS retention_archive_row (",
        "CREATE TABLE IF NOT EXISTS retention_observation (",
        "CREATE TABLE IF NOT EXISTS retention_lock (",
        "source_digest",
        "observed_tick",
        "accounted_bytes",
        "clock_tick",
    )
    for backend in ("sqlite", "postgres"):
        require(
            root
            / f"crates/stores/mainframe-env-store/migrations/{backend}/0002-retention-lifecycle.sql",
            migration_fragments,
        )

    durable = require(
        root / "crates/stores/mainframe-env-store/src/durable_retention.rs",
        (
            "fn durable_archive_and_prune(",
            "fn durable_prune_archives(",
            "fn durable_reconcile_retention_age(",
            "fn core_dependencies(",
            "CoreRetentionDependencySnapshot",
            "store.retention_commit_archive(&archive, after)",
            "Err(StoreError::CapacityExceeded | StoreError::PayloadTooLarge) if batch > 1",
            "batch = (batch / 2).max(1)",
            "now_tick < policy.archive_ticks",
            "RetentionArchivePruneOutcome::AuthorizationRequired",
            "authorized_oversized_archive_id",
            "durable_retention_backend!(SqliteStateStore)",
            "durable_retention_backend!(PostgresStateStore)",
        ),
        production=True,
    )
    if "SystemTime" in durable or "UNIX_EPOCH" in durable:
        raise ValueError("durable retention regressed to process wall-clock time")
    require(
        root / "crates/stores/mainframe-env-store/src/durable_retention/implementations.rs",
        (
            "impl RetentionStore for $store",
            "durable_retention!(SqliteStateStore)",
            "durable_retention!(PostgresStateStore)",
            "fn retention_capacity_health(",
            "fn retention_forecast_with_dependencies(",
            "fn provider_validated_retention_forecast(",
            "fn archive_and_prune_with_dependencies(",
        ),
        production=True,
    )

    memory = require(
        root / "crates/stores/mainframe-env-store/src/memory.rs",
        (
            "archives: BTreeMap<String, RetentionArchive>",
            "archive_bytes: u64",
            "observations: BTreeMap<",
            "observation_bytes: u64",
            "logical_tick: u64",
            "impl RetentionStore for MemoryStore",
            "let mut staged = self.snapshot(&state)",
            "insert_memory_archive(&mut staged",
            "*state = staged",
            "if next > max && !result.is_empty()",
            "if next > request.max_records && !keys.is_empty()",
            "archives.sort_by(",
            ".then_with(|| left.archive_id.cmp(&right.archive_id))",
        ),
        production=True,
    )
    if "retention-archive-v1" in memory:
        raise ValueError("Memory retention regressed to provider-state archive storage")

    for backend in ("sqlite", "postgres"):
        source = require(
            root / f"crates/stores/mainframe-env-store/src/{backend}.rs",
            (
                "pub(crate) fn retention_archive_usage(",
                "pub(crate) fn retention_observation_usage(",
                "pub(crate) fn commit_retention_observation(",
                "pub(crate) fn commit_retention_archive(",
                "fn commit_provider_retention_replacement(",
                "mod container_retention;",
                "pub(crate) fn load_retention_archives(",
                "pub(crate) fn delete_retention_archives(",
            ),
            production=True,
        )
        reader = root / f"crates/stores/mainframe-env-store/src/{backend}/retention_read.rs"
        container_archive = require(
            root / f"crates/stores/mainframe-env-store/src/{backend}/container_retention.rs",
            (
                "fn commit_provider_retention_deletion(",
                "validate_provider_deletion(",
                "outer_receipts",
                "UPDATE provider_state SET version=",
            ),
            production=True,
        )
        archive_source = source + container_archive
        if reader.is_file():
            archive_source += read(reader, production=True)
        if "if next_rows > max && selected_rows != 0" not in archive_source:
            raise ValueError(f"{backend} retention archive prefix selection is incomplete")
        if "retention-archive-v1" in source:
            raise ValueError(f"{backend} retention regressed to provider-state archive storage")


def check_provider_codecs(root: Path) -> None:
    for provider, title in (("db2", "Db2"), ("ims", "Ims"), ("mq", "Mq")):
        retention = require(
            root / f"crates/providers/mainframe-env-{provider}/src/retention.rs",
            (
                f"pub enum {title}ReplayOwnerKind",
                f"pub enum {title}ReplayDependency",
                f"pub enum {title}ReplayRetentionState",
                f"pub struct {title}ReplayRetentionDescriptor",
                f"pub fn describe_{provider}_replay_row(",
                "CicsNested",
                "outer_effect_key",
                "owner_execution",
                "owner_run_unit",
                "sequence",
                "deadline_tick",
                "resolution_tick",
                "request_digest",
                "result_digest",
                "payload_digest",
                "canonical_result_digest",
                "fn metadata_pending(",
                "fn metadata_absent(",
                "fn binding_digest(",
            ),
            production=True,
        )
        for reserved in ("cics.nested-effect-origin", "cics.outer-effect-origin"):
            if reserved not in retention:
                raise ValueError(f"{provider} full codec omits trusted {reserved} provenance")
        require(
            root / f"crates/providers/mainframe-env-{provider}/src/service.rs",
            (
                f"pub trait {title}ReplayClock",
                "open_with_replay_clock(",
                "fn refresh_replay(",
                "HostProblem::UnknownOutcome",
            ),
            production=True,
            companions=(root / "crates/providers/mainframe-env-ims/src/service/execution.rs",)
            if provider == "ims" else (),
        )

    require(
        root / "crates/providers/mainframe-env-dataset/src/retention.rs",
        (
            "pub enum DatasetReplayOwnerKind",
            "pub enum DatasetReplayDependencyState",
            "pub struct DatasetReplayRowDescriptor",
            "CicsNested",
            "outer_effect_key",
            "resolution_tick",
            "request_digest",
            "result_digest",
            'REPLAY_V3_MAGIC: &[u8; 5] = b"MEDR3"',
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-dataset/src/service.rs",
        (
            "pub trait DatasetReplayClock",
            "open_with_replay_clock(",
            "pub fn refresh_replay_index(",
            "pub fn describe_dataset_replay_row(",
            "pub fn describe_dataset_replay_row_with_limits(",
            "HostProblem::UnknownOutcome",
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-cics/src/retention.rs",
        (
            "pub const CICS_RETENTION_NAMESPACES",
            'CICS_NESTED_EFFECT_ORIGIN_BINDING: &str = "cics.nested-effect-origin"',
            'CICS_OUTER_EFFECT_ORIGIN_BINDING: &str = "cics.outer-effect-origin"',
            "pub struct CicsReplayRowDescriptor",
            "pub fn describe_cics_replay_row(",
            "pub struct CicsUowRowDescriptor",
            "pub fn describe_cics_uow_row(",
            "pub fn describe_cics_undo_row(",
            "payload_digest",
            "effect_key",
            "owner_execution",
            "owner_run_unit",
            "terminal_tick",
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-cics/src/service.rs",
        (
            "pub trait CicsReplayClock",
            "open_with_replay_clock(",
            "CICS_NESTED_EFFECT_ORIGIN_BINDING",
            "CICS_OUTER_EFFECT_ORIGIN_BINDING",
            "fn reject_reserved_nested_origin(",
            "fn invocation_with_nested_origin(",
            "invocation.bindings.len().saturating_add(2)",
            "key.as_str().as_bytes().to_vec()",
            "outer_effect_key.as_bytes().to_vec()",
            "HostProblem::UnknownOutcome",
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-spool/src/retention.rs",
        (
            'SPOOL_STATE_CONTRACT: &str = "mainframe-env.spool-state@2"',
            "pub struct SpoolRetentionDescriptor",
            "pub fn describe_spool_retention_row(",
            "purge_boundary_tick",
            "terminal_tick",
            "PurgeRecovery",
            "LegacyProtected",
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-spool/src/service.rs",
        (
            "pub trait SpoolRetentionClock",
            "open_with_retention_clock(",
            "refresh_after_external_retention(",
            "HostProblem::UnknownOutcome",
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-racf/src/retention/planner.rs",
        (
            "pub struct RacfRetentionDescriptor",
            "pub fn retention_descriptors(",
            "archive_provider_state_replacement(request.clone())",
            "provider_retention_authority_usage(RetentionTarget::RacfEvidence)",
            "RACF_RECOVERY_NAMESPACE",
            "RACF_TRANSACTION_NAMESPACE",
            "RACF_AUDIT_NAMESPACE",
        ),
        production=True,
    )
    require(
        root / "crates/apps/mainframe-env-server/src/cobol/retention.rs",
        (
            'CALL_REPLAY_NAMESPACE: &str = "cobol-call-replay@1"',
            'CALL_PROTOCOL_NAMESPACE: &str = "cobol-call-protocol@2"',
            'RUN_STATE_NAMESPACE: &str = "cobol-run-state@1"',
            'CANCEL_NAMESPACE: &str = "cobol-cancel@1"',
            'INSTANCE_NAMESPACE_PREFIX: &str = "cobol-instance@1:"',
            "pub(crate) enum CobolRetentionDependency",
            "pub(crate) fn describe_cobol_retention_row(",
            "pub(crate) fn cobol_retention_dependencies(",
        ),
        production=True,
    )
    instance = read(
        root / "crates/apps/mainframe-env-server/src/cobol/instance.rs",
        production=True,
    )
    descriptor = normalized(rust_block(instance, "pub(super) fn describe_instance_row"))
    for fragment in (
        "state: CobolRetentionState::Active",
        "owner_execution: None",
        "terminal_tick: None",
    ):
        if fragment not in descriptor:
            raise ValueError(f"COBOL live-instance protection drifted: {fragment}")
    finish = normalized(rust_block(instance, "pub(super) fn finish_run_unit"))
    for fragment in (
        "ProviderStateMutation::Delete",
        "state.ended = true",
        "state.ended_tick = Some(ended_tick)",
        "state.programs.clear()",
        "mutate_provider_states_atomic(mutations)",
    ):
        if fragment not in finish:
            raise ValueError(f"COBOL terminal instance cleanup drifted: {fragment}")
    require(
        root / "crates/apps/mainframe-env-server/src/console_retention.rs",
        (
            'CONSOLE_LOG_CONTRACT: &str = "mainframe-env.console-log@2"',
            "DirectProductRoute",
            "pub(crate) struct ConsoleLogRetentionDescriptor",
            "pub(crate) fn describe_console_log_row(",
            "pub(crate) fn decode_console_log_rows(",
            "payload_digest",
            "terminal_tick",
        ),
        production=True,
    )


def check_product_and_operator(root: Path) -> None:
    planner = require(
        root
        / "crates/apps/mainframe-env-server/src/retention_maintenance/provider.rs",
        (
            "pub(crate) struct RetentionPlanner",
            "pub(crate) fn from_existing(",
            "pub(crate) fn forecast(",
            "pub(crate) fn archive_and_prune(",
            "pub(crate) fn legacy_rows(",
            "pub(crate) fn reconcile(",
            "pub(crate) fn provider_plan(",
            "pub(crate) fn core_dependencies(",
            "archive_provider_state_deletion(request)",
            "stale_observations_removed != 0",
            "!= plan.expected_epoch",
            "fn provider_safety_index(",
            ".unresolved_effects(MAX_SCAN)",
            "fn provider_candidate_is_clear(",
            "fn provider_execution_is_clear(",
            ".get_checkpoint(owner)",
            "eligible_records",
        ),
        production=True,
    )
    if "ProductServer" in planner:
        raise ValueError("neutral retention planner depends on serving composition")

    require(
        root
        / "crates/apps/mainframe-env-server/src/retention_maintenance/provider/rows.rs",
        (
            "describe_db2_replay_row(",
            "describe_ims_replay_row(",
            "describe_mq_replay_row(",
            "describe_dataset_replay_row(",
            "describe_cics_replay_row(",
            "describe_cics_uow_row(",
            "describe_cics_undo_row(",
            "describe_cobol_retention_row(",
            "describe_spool_retention_row(",
            "describe_console_log_row(",
            "ProviderRetentionDependency::CoreEffect",
            "ProviderRetentionDependency::CicsNested",
            "ProviderRetentionDependency::ProviderGraph",
            "ProviderRetentionDependency::DirectProduct",
            "legacy_undo_present",
            "legacy_cobol_owner_is_clear_indexed(",
            "cics_uow_exists_for_run(",
            "cobol_replay_index(",
            "cics_uow_index(",
            "delete_provider_retention_observation(",
        ),
        production=True,
    )
    require(
        root
        / "crates/apps/mainframe-env-server/src/retention_maintenance/provider/dependencies.rs",
        (
            "fn core_dependencies_impl(",
            "fn cics_nested_outer_effect_keys(",
            "provider_state_retention_epoch",
            "blocked_executions: executions.into_iter().collect()",
            "blocked_effect_keys: effects.into_iter().collect()",
            'self.bounded_prefix("cobol-")?',
            "collect_cobol_execution_dependencies(",
            "cobol_attribution",
            "observed_owner(",
            "describe_cics_undo_row(",
            "legacy_cobol_owner_is_clear_indexed(",
        ),
        production=True,
    )
    require(
        root
        / "crates/apps/mainframe-env-server/src/retention_maintenance/provider/operations.rs",
        (
            "fn racf_forecast(",
            "fn require_epoch(",
            "fn racf_archive(",
            "fn provider_legacy_rows(",
            "fn reconcile_provider(",
            "fn reconcile_racf(",
            "record_provider_retention_observation(",
            "observations_created",
            "stale_observations_removed",
        ),
        production=True,
    )

    product = require(
        root / "crates/apps/mainframe-env-server/src/product.rs",
        (
            "fn retention_planner(&self)",
            "RetentionPlanner::from_existing(",
            "pub fn operator_retention_forecast(",
            ".forecast(target, self.jes_tick()?, observed_growth_per_tick)",
            "pub fn operator_archive_and_prune(",
            ".archive_and_prune(",
            "if receipt.pruned != 0",
            "self.dataset.refresh_replay_index()?",
            "self.spool.refresh_after_external_retention()?",
            "self.refresh_console_cache()?",
            "pub fn operator_prune_retention_archives_authorized(",
            "prune_retention_archives_authorized(",
            "pub fn operator_retention_legacy_rows(",
            ".legacy_rows(target, max)",
            "pub fn operator_reconcile_retention_age(",
            ".reconcile(request, self.jes_tick()?)",
        ),
        production=True,
    )
    for stale in (
        "fn provider_retention_plan(",
        "fn core_retention_dependencies(",
        "fn cics_nested_retention_dependency(",
    ):
        if stale in product:
            raise ValueError(f"Product retained a duplicate retention planner: {stale}")

    maintenance = require(
        root / "crates/apps/mainframe-env-server/src/maintenance.rs",
        (
            "if profile == StoreProfile::Memory",
            '"durable_store_required"',
            "RetentionTarget::ALL.into_iter().enumerate()",
            "operator.prune_archives(max_records, authorize_oversized_archive.clone())",
            "operator.archive_and_prune(target, max_records)",
            "RetentionArchivePruneOutcome::AuthorizationRequired",
            '"oversized_archive_authorization_required"',
            '"drained_window_required_for_guaranteed_progress": true',
            '"schema": "mainframe-env.retention-maintenance@2"',
            '"phase": phase',
            '"safe_retry": "rerun-the-full-bounded-pass"',
            '"observations_created": receipt.observations_created',
            '"observations_reused": receipt.observations_reused',
            '"stale_observations_removed": receipt.stale_observations_removed',
            "fn retry_conflicts",
        ),
        production=True,
    )
    if "ProductServer" in maintenance:
        raise ValueError("maintenance CLI regressed to opening ProductServer")

    main = require(
        root / "crates/apps/mainframe-env-server/src/main.rs",
        (
            "ServerConfig::from_sources_for_retention(",
            "let store: Arc<dyn PlatformStore>",
            "RetentionMaintenance::open(store, policy)",
            '"schema": "mainframe-env.retention-error@2"',
            'if self.partial { "partial" } else { "error" }',
            "PostgresArtifactStore::open(",
            "ProductServer::open_with_package_trust",
        ),
        production=True,
    )
    maintenance_branch = main.find("RetentionMaintenance::open(store, policy)")
    artifact_open = main.find("PostgresArtifactStore::open(")
    product_open = main.find("ProductServer::open_with_package_trust")
    if min(maintenance_branch, artifact_open, product_open) < 0:
        raise ValueError("headless maintenance startup branch is incomplete")
    if not maintenance_branch < artifact_open < product_open:
        raise ValueError("maintenance must branch before artifact and Product initialization")

    require(
        root / "crates/apps/mainframe-env-server/src/config.rs",
        (
            "pub fn from_sources_for_retention(",
            "if !retention_only",
            "pub fn validate_for_retention(",
            "self.store_profile == StoreProfile::Memory",
            "sqlite_url_is_ephemeral(&self.sqlite_url)",
            'field.trim() == "mode=memory"',
        ),
        production=True,
    )
    require(
        root / "crates/providers/mainframe-env-racf/src/database.rs",
        (
            "pub fn open_existing_for_retention(",
            "LegacySnapshot::read",
            "decode_snapshot(&record, limits)?",
            "pub fn legacy_rows_for_retention(",
            "legacy.migrated_snapshot(self.limits)?",
            "mutate_provider_states_atomic(mutations)",
            "source_rows: self.backup_rows()",
        ),
        production=True,
    )


def check_verification(root: Path) -> None:
    tests = read(root / "crates/stores/mainframe-env-store/tests/retention_contract.rs")
    for fragment in (
        "fn memory_retention_contract()",
        "fn sqlite_retention_contract_survives_restart()",
        "fn postgres_retention_contract()",
        "fn memory_concurrent_archive_has_one_atomic_winner()",
        "fn sqlite_concurrent_archive_has_one_atomic_winner()",
        "fn full_memory_archive_bytes_roll_back_source_prune()",
        "fn full_sqlite_archive_bytes_roll_back_source_prune()",
        "fn memory_zero_tick_terminal_rows_require_explicit_age_reconciliation()",
        "fn sqlite_zero_tick_terminal_rows_require_explicit_age_reconciliation()",
        "fn memory_clock_advances_at_full_live_quota_without_aging_per_request()",
        "fn sqlite_clock_is_unmetered_monotonic_and_survives_restart()",
        "fn memory_requires_exact_authorization_for_an_oversized_archive()",
        "fn sqlite_requires_exact_authorization_for_an_oversized_archive()",
    ):
        if fragment not in tests:
            raise ValueError(f"retention parity verification is missing: {fragment}")

    provider_tests = {
        "db2": (
            "external_replay_prune_refreshes_live_cache_before_replay_and_capacity",
            "post_commit_clock_and_cas_failures_are_unknown_then_retry_recovers",
        ),
        "ims": (
            "external_replay_prune_refreshes_live_cache_before_replay_and_capacity",
            "post_commit_clock_and_cas_failures_are_unknown_then_retry_recovers",
        ),
        "mq": (
            "external_replay_prune_refreshes_live_cache_before_replay_and_capacity",
            "post_commit_clock_and_cas_failures_are_unknown_then_retry_recovers",
        ),
        "dataset": (
            "refresh_replay_index_releases_capacity_after_external_prune",
            "replay_metadata_cas_failure_is_unknown_and_retry_recovers",
        ),
        "cics": (
            "every_enterprise_syncpoint_dispatch_carries_nested_and_outer_attestation",
            "outer_replay_clock_failure_recovers_once_without_sliding_and_codec_is_strict",
        ),
        "spool": (
            "two_live_services_observe_external_retention_and_reuse_capacity",
            "clock_failure_after_physical_purge_leaves_pending_and_retry_resolves_once",
        ),
    }
    for provider, cases in provider_tests.items():
        source = read(root / f"crates/providers/mainframe-env-{provider}/src/service.rs")
        missing = [case for case in cases if case not in source]
        if missing:
            raise ValueError(f"{provider} retention verification is incomplete: {missing}")


def check(root: Path = ROOT) -> None:
    check_targets(root)
    check_store_contract(root)
    check_dedicated_authorities(root)
    check_provider_codecs(root)
    check_product_and_operator(root)
    check_verification(root)


if __name__ == "__main__":
    check()
    print("durable retention lifecycle architecture guard: pass")
