use mainframe_env_cics::{
    CicsFileDefinition, CicsFileFaultPoint, CicsLimits, CicsService, cics_provider,
};
use mainframe_env_coverage::{
    CompiledSpec, ConformanceDriver, ConformanceLimits, ConformanceObservation,
    ConformanceScenarioDriver, DriverOutput, DriverRef, FixtureRef, ObservationCheck,
    ObservationRef, RuntimeRegistry, ScenarioId, ScenarioObservationBundle, ScenarioSpec,
    SpecProblem,
};
use mainframe_env_dataset::{DatasetLimits, DatasetService, dataset_providers};
use mainframe_env_encoding::CodePage;
use mainframe_env_execution_api::{
    ArtifactRef, BoundedPayload, CapabilityId, ExecutionId, IdempotencyKey, Invocation,
    InvocationLimits, Principal, PrincipalId, RequestId, ResourceLimits, RunUnitId, Selector,
    ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, CicsConditionPolicy, CicsOperation, CicsRequest, DatasetAttributes,
    DatasetName, DatasetOrganization, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    HostLimits, HostProblem, HostProvider, HostRequest, HostResult, Mutation, RecordFormat,
    RegistrySnapshot, ScopedHostService, SecurityDecision, SecurityRequest, SessionId,
};
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::{PlatformStore, ProviderStateStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

mod coordinator;

use coordinator::{PilotExecution, drive_artifact};

const FIXTURES: &str = include_str!("../../../../conformance/0.9/cics/pilot-fixtures.json");
const ENVIRONMENT: &str = include_str!("../../../../conformance/0.9/cics/pilot-environment.json");

const COMMIT_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSPILOT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'PLAIN:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'UPDATE1:' RESP-X ':' RESP2-X ':' REC-X.
MOVE 'AA22' TO REC-X. EXEC CICS REWRITE FILE('ACCTDAT') FROM(REC-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'REWRITE1:' RESP-X ':' RESP2-X.
EXEC CICS SYNCPOINT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'COMMIT:' RESP-X ':' RESP2-X.
STOP RUN.
"#;

const CONTEXT_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSCTX.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'CTXREAD:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS SYNCPOINT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'CTXSYNC:' RESP-X ':' RESP2-X.
EXEC CICS REWRITE FILE('ACCTDAT') FROM(REC-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'CTXREWRITE:' RESP-X ':' RESP2-X.
STOP RUN.
"#;

const ROLLBACK_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSROLL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'UPDATE2:' RESP-X ':' RESP2-X ':' REC-X.
MOVE 'AA33' TO REC-X. EXEC CICS REWRITE FILE('ACCTDAT') FROM(REC-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'REWRITE2:' RESP-X ':' RESP2-X.
EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'PREBACKOUT:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS SYNCPOINT ROLLBACK RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'ROLLBACK:' RESP-X ':' RESP2-X.
EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'FINAL:' RESP-X ':' RESP2-X ':' REC-X.
STOP RUN.
"#;

const INVALID_REWRITE_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSINV.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'INVPLAIN:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS REWRITE FILE('ACCTDAT') FROM(REC-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'INVALID:' RESP-X ':' RESP2-X.
STOP RUN.
"#;

const READ_SOURCE_PREFIX: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSNEG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
"#;

const RESTART_MUTATE_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSCRASH.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
MOVE 'AA33' TO REC-X. EXEC CICS REWRITE FILE('ACCTDAT') FROM(REC-X) RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'UNREACHED'.
STOP RUN.
"#;

const RESTART_ROLLBACK_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSRECOVER.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS SYNCPOINT ROLLBACK RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'RECOVERED:' RESP-X ':' RESP2-X.
STOP RUN.
"#;

static PROFILE_NONCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CicsCommandObservation {
    pub resp: u64,
    pub resp2: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_hex: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CicsDurableRestartObservation {
    pub fault_point_records: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CicsPilotProfileObservation {
    pub profile: String,
    pub commands: BTreeMap<String, CicsCommandObservation>,
    pub record_snapshots: BTreeMap<String, String>,
    pub resource_states: BTreeMap<String, String>,
    pub raw_application_outputs: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub durable_restart: Option<CicsDurableRestartObservation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CicsPilotReport {
    pub schema_version: String,
    pub environment_manifest_digest: String,
    pub fixture_digest: String,
    pub comparison_policy: String,
    pub profiles: Vec<CicsPilotProfileObservation>,
    pub differential_credit: u8,
}

type EvidenceSelection<'a> = (
    Vec<(&'a str, &'a str)>,
    Vec<(&'a str, &'a str)>,
    Vec<(&'a str, &'a str)>,
);

pub struct CicsPilotRuntime {
    case_driver: ScenarioOnlyDriver,
    readback_driver: ScenarioOnlyDriver,
    scenario_driver: CicsPilotScenarioDriver,
    observations: Vec<CicsPilotObservation>,
}

#[must_use]
pub fn cics_pilot_runtime() -> CicsPilotRuntime {
    let obligations = [
        "plain-read",
        "read-update",
        "missing-record",
        "unauthorized",
        "closed-file",
        "requires-read-update",
        "rewrite-record",
        "forbidden-mutation-on-invreq",
        "context-invalidated-at-syncpoint",
        "commit-boundary",
        "rollback-boundary",
        "durable-restart",
    ];
    CicsPilotRuntime {
        case_driver: ScenarioOnlyDriver,
        readback_driver: ScenarioOnlyDriver,
        scenario_driver: CicsPilotScenarioDriver,
        observations: obligations
            .into_iter()
            .map(|obligation| CicsPilotObservation { obligation })
            .collect(),
    }
}

impl CicsPilotRuntime {
    pub fn bind<'a>(
        &'a self,
        spec: &CompiledSpec,
        drivers: &mut Vec<(DriverRef, &'a dyn ConformanceDriver)>,
        observations: &mut Vec<(ObservationRef, &'a dyn ConformanceObservation)>,
        limits: ConformanceLimits,
    ) -> Result<Vec<(ScenarioId, &'a dyn ConformanceScenarioDriver)>, SpecProblem> {
        let id = ScenarioId::new("cics.file-uow.local", limits)?;
        if spec.scenario(&id).is_none() {
            return Ok(Vec::new());
        }
        drivers.push((
            DriverRef::new("cics.pilot.product-path", limits)?,
            &self.case_driver,
        ));
        drivers.push((
            DriverRef::new("cics.pilot.readback-path", limits)?,
            &self.readback_driver,
        ));
        for observation in &self.observations {
            observations.push((
                ObservationRef::new(format!("cics.pilot.{}", observation.obligation), limits)?,
                observation,
            ));
        }
        Ok(vec![(id, &self.scenario_driver)])
    }

    pub fn attach_scenarios<'a>(
        &'a self,
        spec: &CompiledSpec,
        runtime: RuntimeRegistry<'a>,
        limits: ConformanceLimits,
    ) -> Result<RuntimeRegistry<'a>, SpecProblem> {
        let id = ScenarioId::new("cics.file-uow.local", limits)?;
        if spec.scenario(&id).is_none() {
            return Ok(runtime);
        }
        runtime.with_scenario_drivers(spec, vec![(id, &self.scenario_driver)], limits)
    }
}

struct ScenarioOnlyDriver;

impl ConformanceDriver for ScenarioOnlyDriver {
    fn execute(&self, _fixture: &FixtureRef) -> Result<DriverOutput, String> {
        Err("CICS pilot bindings must execute through their registered ScenarioSpec".into())
    }
}

struct CicsPilotScenarioDriver;

impl ConformanceScenarioDriver for CicsPilotScenarioDriver {
    fn execute(&self, scenario: &ScenarioSpec) -> Result<ScenarioObservationBundle, String> {
        let report = run_cics_pilot_profiles()?;
        let bytes = serde_json::to_vec(&report).map_err(|error| error.to_string())?;
        let observations = scenario
            .credits()
            .iter()
            .map(|key| {
                DriverOutput::new(bytes.clone(), ConformanceLimits::default())
                    .map(|output| (key.clone(), output))
                    .map_err(|problem| problem.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        ScenarioObservationBundle::new(observations, ConformanceLimits::default())
            .map_err(|problem| problem.to_string())
    }
}

struct CicsPilotObservation {
    obligation: &'static str,
}

impl ConformanceObservation for CicsPilotObservation {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let report: CicsPilotReport =
            serde_json::from_slice(output.bytes()).map_err(|error| error.to_string())?;
        let fixtures: Value = serde_json::from_str(FIXTURES).map_err(|error| error.to_string())?;
        let expected_policy = fixtures["comparison_policy"]["version"]
            .as_str()
            .ok_or_else(|| "CICS pilot comparison policy is missing".to_string())?;
        let expected_identity = serde_json::json!({
            "schema_version": "mainframe-env.cics-pilot-observation@1",
            "environment_manifest_digest": digest(ENVIRONMENT.as_bytes()),
            "fixture_digest": digest(FIXTURES.as_bytes()),
            "comparison_policy": expected_policy,
            "differential_credit": 0,
        });
        let actual_identity = serde_json::json!({
            "schema_version": report.schema_version,
            "environment_manifest_digest": report.environment_manifest_digest,
            "fixture_digest": report.fixture_digest,
            "comparison_policy": report.comparison_policy,
            "differential_credit": report.differential_credit,
        });
        let (matched, expected, actual) = compare_obligation(self.obligation, &report)?;
        ObservationCheck::new(
            matched && expected_identity == actual_identity,
            format!("identity={expected_identity}; {expected}"),
            format!("identity={actual_identity}; {actual}"),
            ConformanceLimits::default(),
        )
        .map_err(|problem| problem.to_string())
    }
}

pub fn run_cics_pilot_profiles() -> Result<CicsPilotReport, String> {
    let fixtures: Value = serde_json::from_str(FIXTURES).map_err(|error| error.to_string())?;
    let expected_policy = fixtures["comparison_policy"]["version"]
        .as_str()
        .ok_or_else(|| "CICS pilot comparison policy is missing".to_string())?;
    let memory = Arc::new(MemoryStore::new(StoreLimits::default()));
    let memory_observation = run_profile("memory", memory)?;

    let nonce = PROFILE_NONCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-cics-pilot-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let database = directory.join("state.db");
    let url = format!("sqlite://{}?mode=rwc", database.display());
    let sqlite_observation = (|| {
        let sqlite = Arc::new(
            SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144)
                .map_err(|error| error.to_string())?,
        );
        let mut observation = run_profile("sqlite", sqlite)?;
        observation.durable_restart = Some(run_durable_restart_profile(&directory)?);
        Ok::<_, String>(observation)
    })();
    let _ = fs::remove_dir_all(&directory);

    Ok(CicsPilotReport {
        schema_version: "mainframe-env.cics-pilot-observation@1".into(),
        environment_manifest_digest: digest(ENVIRONMENT.as_bytes()),
        fixture_digest: digest(FIXTURES.as_bytes()),
        comparison_policy: expected_policy.into(),
        profiles: vec![memory_observation, sqlite_observation?],
        differential_credit: 0,
    })
}

fn run_profile<S>(profile: &str, store: Arc<S>) -> Result<CicsPilotProfileObservation, String>
where
    S: PlatformStore + 'static,
{
    let provider_store: Arc<dyn ProviderStateStore> = store.clone();
    let platform_store: Arc<dyn PlatformStore> = store;
    let dataset = DatasetService::open(provider_store.clone(), DatasetLimits::default())
        .map_err(|problem| format!("{profile} dataset open: {problem}"))?;
    seed_dataset(&dataset, profile).map_err(|problem| format!("{profile} seed: {problem}"))?;
    let inner = pilot_inner_host(dataset.clone())
        .map_err(|problem| format!("{profile} inner host: {problem}"))?;
    let cics = CicsService::open(inner, provider_store, CicsLimits::default())
        .map_err(|problem| format!("{profile} CICS open: {problem}"))?;
    cics.register_file_definitions(&BTreeMap::from([(
        "ACCTDAT".into(),
        CicsFileDefinition {
            dataset: dataset_name()?,
            ccsid: Some(37),
        },
    )]))
    .map_err(|problem| format!("{profile} file registration: {problem}"))?;
    let outer = pilot_outer_host(cics.clone())
        .map_err(|problem| format!("{profile} outer host: {problem}"))?;
    let execution = PilotExecution::new(outer, platform_store);

    let invalid_output = execute_source(
        INVALID_REWRITE_SOURCE,
        "CICSINV",
        "IBMUSER",
        &format!("{profile}-invalid"),
        &execution,
    )
    .map_err(|problem| format!("{profile} invalid rewrite path: {problem}"))?;
    let record_after_invalid_hex = read_record_hex(&dataset)?;

    let commit_output = execute_source(
        COMMIT_SOURCE,
        "CICSPILOT",
        "IBMUSER",
        &format!("{profile}-commit"),
        &execution,
    )
    .map_err(|problem| format!("{profile} commit path: {problem}"))?;
    let record_after_commit_hex = read_record_hex(&dataset)?;

    let context_output = execute_source(
        CONTEXT_SOURCE,
        "CICSCTX",
        "IBMUSER",
        &format!("{profile}-context"),
        &execution,
    )
    .map_err(|problem| format!("{profile} context path: {problem}"))?;
    let record_after_context_hex = read_record_hex(&dataset)?;

    let rollback_output = execute_source(
        ROLLBACK_SOURCE,
        "CICSROLL",
        "IBMUSER",
        &format!("{profile}-rollback"),
        &execution,
    )
    .map_err(|problem| format!("{profile} rollback path: {problem}"))?;
    let record_after_rollback_hex = read_record_hex(&dataset)?;

    let missing_output = execute_read_case(
        "MISSING",
        "ZZ",
        "IBMUSER",
        &format!("{profile}-missing"),
        &execution,
    )
    .map_err(|problem| format!("{profile} missing path: {problem}"))?;
    let unauthorized_output = execute_read_case(
        "UNAUTHORIZED",
        "AA",
        "DENIED",
        &format!("{profile}-denied"),
        &execution,
    )
    .map_err(|problem| format!("{profile} unauthorized path: {problem}"))?;
    let record_after_unauthorized_hex = read_record_hex(&dataset)?;

    let closed_source = read_source("CLOSED", "AA");
    let closed_artifact = crate::compile(&closed_source)?;
    let closed_invocation = pilot_invocation(
        &closed_artifact,
        "IBMUSER",
        &format!("{profile}-closed"),
        "CICSNEG",
    )?;
    let session = SessionId::new(
        format!("{profile}-closed-session"),
        InvocationLimits::default().max_binding_bytes,
    )
    .map_err(|problem| problem.to_string())?;
    cics.create_session(&session, 24, 80)
        .map_err(|problem| problem.to_string())?;
    cics.register_run(closed_invocation.clone(), &session, "PIL1", "ME01", "S001")
        .map_err(|problem| problem.to_string())?;
    set_file_status(
        &cics,
        &closed_invocation,
        "CLOSED-UNENABLED",
        "close-unenabled",
    )
    .map_err(|problem| format!("{profile} close setup: {problem}"))?;
    let closed_output = drive_artifact(&closed_artifact, closed_invocation, &execution)
        .map_err(|problem| format!("{profile} closed path: {problem}"))?;
    let record_after_closed_hex = read_record_hex(&dataset)?;

    let enabled_source = read_source_with_record("CLOSEDENABLED", "AA");
    let enabled_artifact = crate::compile(&enabled_source)?;
    let enabled_invocation = pilot_invocation(
        &enabled_artifact,
        "IBMUSER",
        &format!("{profile}-closed-enabled"),
        "CICSNEG",
    )?;
    let enabled_session = SessionId::new(
        format!("{profile}-closed-enabled-session"),
        InvocationLimits::default().max_binding_bytes,
    )
    .map_err(|problem| problem.to_string())?;
    cics.create_session(&enabled_session, 24, 80)
        .map_err(|problem| problem.to_string())?;
    cics.register_run(
        enabled_invocation.clone(),
        &enabled_session,
        "PIL1",
        "ME01",
        "S002",
    )
    .map_err(|problem| problem.to_string())?;
    set_file_status(
        &cics,
        &enabled_invocation,
        "CLOSED-ENABLED",
        "close-enabled",
    )
    .map_err(|problem| format!("{profile} closed-enabled setup: {problem}"))?;
    let enabled_output = drive_artifact(&enabled_artifact, enabled_invocation, &execution)
        .map_err(|problem| format!("{profile} closed-enabled path: {problem}"))?;
    let record_after_closed_enabled_hex = read_record_hex(&dataset)?;

    let commands = BTreeMap::from([
        (
            "plain_read".into(),
            parse_command(&commit_output, "PLAIN", true)?,
        ),
        (
            "invalid_plain_read".into(),
            parse_command(&invalid_output, "INVPLAIN", true)?,
        ),
        (
            "rewrite_without_update".into(),
            parse_command(&invalid_output, "INVALID", false)?,
        ),
        (
            "read_update".into(),
            parse_command(&commit_output, "UPDATE1", true)?,
        ),
        (
            "rewrite_commit".into(),
            parse_command(&commit_output, "REWRITE1", false)?,
        ),
        (
            "syncpoint_commit".into(),
            parse_command(&commit_output, "COMMIT", false)?,
        ),
        (
            "context_read_update".into(),
            parse_command(&context_output, "CTXREAD", true)?,
        ),
        (
            "context_syncpoint".into(),
            parse_command(&context_output, "CTXSYNC", false)?,
        ),
        (
            "rewrite_after_syncpoint".into(),
            parse_command(&context_output, "CTXREWRITE", false)?,
        ),
        (
            "rollback_read_update".into(),
            parse_command(&rollback_output, "UPDATE2", true)?,
        ),
        (
            "rewrite_before_rollback".into(),
            parse_command(&rollback_output, "REWRITE2", false)?,
        ),
        (
            "read_before_rollback".into(),
            parse_command(&rollback_output, "PREBACKOUT", true)?,
        ),
        (
            "syncpoint_rollback".into(),
            parse_command(&rollback_output, "ROLLBACK", false)?,
        ),
        (
            "final_read".into(),
            parse_command(&rollback_output, "FINAL", true)?,
        ),
        (
            "missing_record".into(),
            parse_command(&missing_output, "MISSING", false)?,
        ),
        (
            "unauthorized".into(),
            parse_command(&unauthorized_output, "UNAUTHORIZED", false)?,
        ),
        (
            "closed_unenabled".into(),
            parse_command(&closed_output, "CLOSED", false)?,
        ),
        (
            "closed_enabled".into(),
            parse_command(&enabled_output, "CLOSEDENABLED", true)?,
        ),
    ]);

    Ok(CicsPilotProfileObservation {
        profile: profile.into(),
        commands,
        record_snapshots: BTreeMap::from([
            ("after_invalid_rewrite".into(), record_after_invalid_hex),
            ("after_commit".into(), record_after_commit_hex),
            ("after_context_rewrite".into(), record_after_context_hex),
            ("after_rollback".into(), record_after_rollback_hex),
            ("after_unauthorized".into(), record_after_unauthorized_hex),
            ("after_closed_unenabled".into(), record_after_closed_hex),
            (
                "after_closed_enabled".into(),
                record_after_closed_enabled_hex,
            ),
        ]),
        resource_states: BTreeMap::from([(
            "after_closed_enabled".into(),
            format!(
                "{:?}",
                cics.file_status("ACCTDAT").map_err(|e| e.to_string())?
            ),
        )]),
        raw_application_outputs: BTreeMap::from([
            ("invalid_rewrite".into(), invalid_output),
            ("commit".into(), commit_output),
            ("context".into(), context_output),
            ("rollback".into(), rollback_output),
            ("missing_record".into(), missing_output),
            ("unauthorized".into(), unauthorized_output),
            ("closed_unenabled".into(), closed_output),
            ("closed_enabled".into(), enabled_output),
        ]),
        durable_restart: None,
    })
}

fn restart_mutate(url: &str, fault_point: CicsFileFaultPoint) -> Result<(), String> {
    let store = Arc::new(
        SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| error.to_string())?,
    );
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    seed_dataset(&dataset, "restart")?;
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone())?,
        store.clone(),
        CicsLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    cics.register_file_definitions(&BTreeMap::from([(
        "ACCTDAT".into(),
        CicsFileDefinition {
            dataset: dataset_name()?,
            ccsid: Some(37),
        },
    )]))
    .map_err(|problem| problem.to_string())?;
    cics.inject_file_fault_once(CicsOperation::Rewrite, "ACCTDAT", fault_point)
        .map_err(|problem| problem.to_string())?;
    let execution = PilotExecution::new(pilot_outer_host(cics)?, store);
    let result = execute_source(
        RESTART_MUTATE_SOURCE,
        "CICSCRASH",
        "IBMUSER",
        "restart",
        &execution,
    );
    if result.is_ok() {
        return Err("faulted rewrite unexpectedly completed".into());
    }
    let expected = if fault_point == CicsFileFaultPoint::AfterMutation {
        "AA33"
    } else {
        "AA11"
    };
    if read_record_hex(&dataset)? != hex(&encode(expected)?) {
        return Err(format!(
            "faulted rewrite state does not match {fault_point:?}"
        ));
    }
    Ok(())
}

fn restart_recover(url: &str) -> Result<String, String> {
    let store = Arc::new(
        SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| error.to_string())?,
    );
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone())?,
        store.clone(),
        CicsLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    cics.register_file_definitions(&BTreeMap::from([(
        "ACCTDAT".into(),
        CicsFileDefinition {
            dataset: dataset_name()?,
            ccsid: Some(37),
        },
    )]))
    .map_err(|problem| problem.to_string())?;
    let execution = PilotExecution::new(pilot_outer_host(cics)?, store);
    let artifact = crate::compile(RESTART_ROLLBACK_SOURCE)?;
    let mut invocation = pilot_invocation(&artifact, "IBMUSER", "restart-recover", "CICSRECOVER")?;
    invocation.run_unit_id = RunUnitId::new("restart-run", InvocationLimits::default())
        .map_err(|problem| problem.to_string())?;
    let output = drive_artifact(&artifact, invocation, &execution)?;
    if output != "RECOVERED:000:000\n" {
        return Err(format!("unexpected restart output: {output:?}"));
    }
    let recovered = read_record_hex(&dataset)?;
    if recovered != hex(&encode("AA11")?) {
        return Err("restart rollback did not restore the pre-UOW record".into());
    }
    Ok(recovered)
}

fn run_durable_restart_profile(
    directory: &std::path::Path,
) -> Result<CicsDurableRestartObservation, String> {
    let mut fault_point_records = BTreeMap::new();
    for (name, point) in [
        ("before-intent", CicsFileFaultPoint::BeforeIntent),
        ("after-intent", CicsFileFaultPoint::AfterIntent),
        ("after-mutation", CicsFileFaultPoint::AfterMutation),
    ] {
        let url = format!(
            "sqlite://{}?mode=rwc",
            directory.join(format!("restart-{name}.db")).display()
        );
        restart_mutate(&url, point)?;
        fault_point_records.insert(name.into(), restart_recover(&url)?);
    }
    Ok(CicsDurableRestartObservation {
        fault_point_records,
    })
}

fn seed_dataset(dataset: &DatasetService, profile: &str) -> Result<(), String> {
    dataset
        .invoke(DatasetRequest::Create {
            dataset: dataset_name()?,
            attributes: DatasetAttributes {
                organization: DatasetOrganization::KeySequenced,
                record_format: RecordFormat::Fixed,
                logical_record_length: 4,
                key_offset: Some(0),
                key_length: Some(2),
                ccsid: Some(37),
            },
            mutation: mutation(1, &format!("{profile}-create"))?,
        })
        .map_err(|problem| problem.to_string())?;
    dataset
        .invoke(DatasetRequest::Write {
            dataset: dataset_name()?,
            member: None,
            records: vec![encode("AA11")?],
            expected_version: Some(1),
            mutation: mutation(2, &format!("{profile}-seed"))?,
        })
        .map_err(|problem| problem.to_string())?;
    Ok(())
}

fn pilot_inner_host(dataset: Arc<DatasetService>) -> Result<Arc<ScopedHostService>, String> {
    let limits = InvocationLimits::default();
    let mut providers: Vec<Arc<dyn HostProvider>> = vec![Arc::new(PilotSecurityProvider {
        descriptor: CapabilityDescriptor {
            capability: CapabilityId::new("host.security.authorize", limits)
                .map_err(|problem| problem.to_string())?,
            provider_id: "cics-pilot-security-v1".into(),
            generation: "1".into(),
            request_schema: "security@1".into(),
            result_schema: "decision@1".into(),
            max_request_bytes: 65_536,
            max_result_bytes: 65_536,
            ready: true,
        },
    })];
    providers.extend(dataset_providers(dataset, limits));
    Ok(Arc::new(ScopedHostService::new(
        Arc::new(RegistrySnapshot::new(1, providers, limits).map_err(|error| error.to_string())?),
        HostLimits::default(),
    )))
}

fn pilot_outer_host(cics: Arc<CicsService>) -> Result<Arc<ScopedHostService>, String> {
    let limits = InvocationLimits::default();
    Ok(Arc::new(ScopedHostService::new(
        Arc::new(
            RegistrySnapshot::new(1, vec![cics_provider(cics, limits)], limits)
                .map_err(|error| error.to_string())?,
        ),
        HostLimits::default(),
    )))
}

struct PilotSecurityProvider {
    descriptor: CapabilityDescriptor,
}

impl HostProvider for PilotSecurityProvider {
    fn descriptor(&self) -> &CapabilityDescriptor {
        &self.descriptor
    }

    fn invoke(&self, _invocation: &Invocation, effect: EffectRequest) -> EffectResult {
        let outcome = match effect.request {
            HostRequest::Security(SecurityRequest::Authorize {
                principal, class, ..
            }) => Ok(HostResult::Security(
                if principal.as_str() == "DENIED" && class == "DATASET" {
                    SecurityDecision::Deny
                } else {
                    SecurityDecision::Allow
                },
            )),
            _ => Err(HostProblem::Malformed),
        };
        EffectResult {
            sequence: effect.sequence,
            outcome,
        }
    }
}

fn execute_source(
    source: &str,
    program: &str,
    principal: &str,
    identity: &str,
    execution: &PilotExecution,
) -> Result<String, String> {
    let artifact = crate::compile(source)?;
    let invocation = pilot_invocation(&artifact, principal, identity, program)?;
    drive_artifact(&artifact, invocation, execution)
}

fn execute_read_case(
    label: &str,
    key: &str,
    principal: &str,
    identity: &str,
    execution: &PilotExecution,
) -> Result<String, String> {
    execute_source(
        &read_source(label, key),
        "CICSNEG",
        principal,
        identity,
        execution,
    )
}

fn read_source(label: &str, key: &str) -> String {
    format!(
        "{READ_SOURCE_PREFIX}EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('{key}') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nDISPLAY '{label}:' RESP-X ':' RESP2-X.\nSTOP RUN.\n"
    )
}

fn read_source_with_record(label: &str, key: &str) -> String {
    format!(
        "{READ_SOURCE_PREFIX}EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('{key}') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nDISPLAY '{label}:' RESP-X ':' RESP2-X ':' REC-X.\nSTOP RUN.\n"
    )
}

fn pilot_invocation(
    artifact: &mainframe_env_compiler_api::PublishedArtifact,
    principal: &str,
    identity: &str,
    program: &str,
) -> Result<Invocation, String> {
    let limits = InvocationLimits::default();
    let grants = [
        "host.cics.execute",
        "host.security.authorize",
        "host.dataset.read",
        "host.dataset.write",
    ]
    .into_iter()
    .map(|value| CapabilityId::new(value, limits))
    .collect::<Result<BTreeSet<_>, _>>()
    .map_err(|problem| problem.to_string())?;
    Invocation::new(
        RequestId::new(format!("{identity}-request"), limits)
            .map_err(|problem| problem.to_string())?,
        ExecutionId::new(format!("{identity}-execution"), limits)
            .map_err(|problem| problem.to_string())?,
        RunUnitId::new(format!("{identity}-run"), limits).map_err(|problem| problem.to_string())?,
        None,
        Selector::new(format!("program:COBOL:{program}"), limits)
            .map_err(|problem| problem.to_string())?,
        ArtifactRef::new(artifact.content_id().to_reference(), limits)
            .map_err(|problem| problem.to_string())?,
        Principal::new(
            PrincipalId::new(principal, limits).map_err(|problem| problem.to_string())?,
            grants,
            limits,
        )
        .map_err(|problem| problem.to_string())?,
        ServiceClass::Interactive,
        0,
        1_000,
        TraceId::new(format!("{identity}-trace"), limits).map_err(|problem| problem.to_string())?,
        IdempotencyKey::new(format!("{identity}-invocation"), limits)
            .map_err(|problem| problem.to_string())?,
        1,
        ResourceLimits {
            max_output_bytes: 16 * 1024,
            ..ResourceLimits::default()
        },
        BTreeMap::new(),
        limits,
    )
    .map_err(|problem| problem.to_string())
}

fn set_file_status(
    cics: &CicsService,
    invocation: &Invocation,
    status: &str,
    identity: &str,
) -> Result<(), String> {
    let payload = BoundedPayload::new(
        "mainframe-env.cics.file-status@1",
        status.as_bytes().to_vec(),
        InvocationLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    let mut close_mutation = mutation(1, identity)?;
    close_mutation.transaction = Some("PIL1".into());
    let request = CicsRequest {
        operation: CicsOperation::SetFileStatus,
        arguments: BTreeMap::from([("ACCTDAT".into(), payload)]),
        condition_policy: CicsConditionPolicy::Default,
        mutation: Some(close_mutation),
    };
    let effect = EffectRequest {
        run_unit: invocation.run_unit_id.clone(),
        sequence: 1,
        deadline_tick: invocation.deadline_tick,
        idempotency_key: request
            .mutation
            .as_ref()
            .map(|mutation| mutation.idempotency_key.clone()),
        request: HostRequest::Cics(request.clone()),
    };
    cics.invoke(&effect, request)
        .map(|_| ())
        .map_err(|problem| problem.to_string())
}

fn read_record_hex(dataset: &DatasetService) -> Result<String, String> {
    match dataset
        .invoke(DatasetRequest::Read {
            dataset: dataset_name()?,
            member: None,
            key: Some(encode("AA")?),
            max_records: 1,
            control: Default::default(),
        })
        .map_err(|problem| problem.to_string())?
    {
        DatasetResult::Records { records, .. } if records.len() == 1 => Ok(hex(&records[0])),
        other => Err(format!("CICS pilot readback returned {other:?}")),
    }
}

fn parse_command(
    output: &str,
    label: &str,
    has_record: bool,
) -> Result<CicsCommandObservation, String> {
    let prefix = format!("{label}:");
    let matches = output
        .lines()
        .filter(|line| line.starts_with(&prefix))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "CICS pilot expected exactly one {label} observation, found {}",
            matches.len()
        ));
    }
    let fields = matches[0].split(':').collect::<Vec<_>>();
    let expected_fields = if has_record { 4 } else { 3 };
    if fields.len() != expected_fields || fields[0] != label {
        return Err(format!(
            "malformed CICS pilot {label} observation: {:?}",
            matches[0]
        ));
    }
    let resp = fields[1]
        .parse::<u64>()
        .map_err(|_| format!("invalid CICS pilot {label} RESP"))?;
    let resp2 = fields[2]
        .parse::<u64>()
        .map_err(|_| format!("invalid CICS pilot {label} RESP2"))?;
    let record_hex = has_record
        .then(|| encode(fields[3]).map(|bytes| hex(&bytes)))
        .transpose()?;
    Ok(CicsCommandObservation {
        resp,
        resp2,
        record_hex,
    })
}

fn fixture_u64(value: &Value, field: &str, fixture: &str) -> Result<u64, String> {
    value[field]
        .as_u64()
        .ok_or_else(|| format!("CICS fixture {fixture}.{field} is missing"))
}

fn expected_command(expected: &Value, fixture: &str) -> Result<CicsCommandObservation, String> {
    let value = expected
        .get(fixture)
        .ok_or_else(|| format!("CICS fixture {fixture} is missing"))?;
    let record_hex = value
        .get("record")
        .map(|record| {
            record
                .as_str()
                .ok_or_else(|| format!("CICS fixture {fixture}.record is invalid"))
                .and_then(|record| encode(record).map(|bytes| hex(&bytes)))
        })
        .transpose()?;
    Ok(CicsCommandObservation {
        resp: fixture_u64(value, "resp", fixture)?,
        resp2: fixture_u64(value, "resp2", fixture)?,
        record_hex,
    })
}

fn expected_record_after(expected: &Value, fixture: &str) -> Result<String, String> {
    let record = expected
        .get(fixture)
        .and_then(|value| value.get("record_after"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("CICS fixture {fixture}.record_after is missing"))?;
    encode(record).map(|bytes| hex(&bytes))
}

fn profile_evidence(
    profile: &CicsPilotProfileObservation,
    expected: &Value,
    commands: &[(&str, &str)],
    snapshots: &[(&str, &str)],
    resource_states: &[(&str, &str)],
) -> Result<(bool, String, String), String> {
    let mut expected_commands = BTreeMap::new();
    let mut actual_commands = BTreeMap::new();
    for (actual_key, fixture_key) in commands {
        expected_commands.insert(
            (*actual_key).to_string(),
            expected_command(expected, fixture_key)?,
        );
        actual_commands.insert(
            (*actual_key).to_string(),
            profile
                .commands
                .get(*actual_key)
                .cloned()
                .ok_or_else(|| format!("{} command {actual_key} is missing", profile.profile))?,
        );
    }
    let mut expected_snapshots = BTreeMap::new();
    let mut actual_snapshots = BTreeMap::new();
    for (actual_key, fixture_key) in snapshots {
        expected_snapshots.insert(
            (*actual_key).to_string(),
            expected_record_after(expected, fixture_key)?,
        );
        actual_snapshots.insert(
            (*actual_key).to_string(),
            profile
                .record_snapshots
                .get(*actual_key)
                .cloned()
                .ok_or_else(|| format!("{} snapshot {actual_key} is missing", profile.profile))?,
        );
    }
    let mut expected_states = BTreeMap::new();
    let mut actual_states = BTreeMap::new();
    for (actual_key, fixture_key) in resource_states {
        let state = expected
            .get(*fixture_key)
            .and_then(|value| value.get("resource_state_after"))
            .and_then(Value::as_str)
            .ok_or_else(|| format!("CICS fixture {fixture_key}.resource_state_after is missing"))?;
        expected_states.insert((*actual_key).to_string(), state.to_string());
        actual_states.insert(
            (*actual_key).to_string(),
            profile
                .resource_states
                .get(*actual_key)
                .cloned()
                .ok_or_else(|| {
                    format!("{} resource state {actual_key} is missing", profile.profile)
                })?,
        );
    }
    let expected_value = serde_json::json!({
        "profile": profile.profile,
        "commands": expected_commands,
        "record_snapshots": expected_snapshots,
        "resource_states": expected_states,
    });
    let actual_value = serde_json::json!({
        "profile": profile.profile,
        "commands": actual_commands,
        "record_snapshots": actual_snapshots,
        "resource_states": actual_states,
    });
    Ok((
        expected_value == actual_value,
        expected_value.to_string(),
        actual_value.to_string(),
    ))
}

fn durable_restart_evidence(
    report: &CicsPilotReport,
    expected: &Value,
) -> Result<(bool, String, String), String> {
    let fixture = expected
        .get("durable_restart")
        .ok_or_else(|| "CICS durable_restart fixture is missing".to_string())?;
    let claimed_profiles = fixture["claimed_profiles"]
        .as_array()
        .ok_or_else(|| "CICS durable_restart claimed_profiles is missing".to_string())?
        .iter()
        .map(|profile| {
            profile
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "CICS durable_restart profile is invalid".to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let fault_points = fixture["fault_points"]
        .as_array()
        .ok_or_else(|| "CICS durable_restart fault_points is missing".to_string())?
        .iter()
        .map(|point| {
            point
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "CICS durable_restart fault point is invalid".to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let expected_record = expected_record_after(expected, "durable_restart")?;
    let expected_records = claimed_profiles
        .iter()
        .map(|profile| {
            (
                profile.clone(),
                fault_points
                    .iter()
                    .map(|point| (point.clone(), expected_record.clone()))
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let actual_records = report
        .profiles
        .iter()
        .filter_map(|profile| {
            profile
                .durable_restart
                .as_ref()
                .map(|restart| (profile.profile.clone(), restart.fault_point_records.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let expected_value = serde_json::json!({"claimed_profile_records": expected_records});
    let actual_value = serde_json::json!({"claimed_profile_records": actual_records});
    Ok((
        expected_value == actual_value,
        expected_value.to_string(),
        actual_value.to_string(),
    ))
}

fn compare_obligation(
    obligation: &str,
    report: &CicsPilotReport,
) -> Result<(bool, String, String), String> {
    let fixtures: Value = serde_json::from_str(FIXTURES).map_err(|error| error.to_string())?;
    let expected = &fixtures["expected"];
    let known = [
        "plain-read",
        "read-update",
        "requires-read-update",
        "rewrite-record",
        "context-invalidated-at-syncpoint",
        "commit-boundary",
        "rollback-boundary",
        "forbidden-mutation-on-invreq",
        "missing-record",
        "unauthorized",
        "closed-file",
        "durable-restart",
    ];
    if !known.contains(&obligation) {
        return Err(format!("unknown CICS pilot obligation {obligation}"));
    }
    if obligation == "durable-restart" {
        return durable_restart_evidence(report, expected);
    }
    let (commands, snapshots, resource_states): EvidenceSelection<'_> = match obligation {
        "plain-read" => (vec![("plain_read", "plain_read")], vec![], vec![]),
        "read-update" => (vec![("read_update", "read_update")], vec![], vec![]),
        "requires-read-update" => (
            vec![
                ("invalid_plain_read", "invalid_plain_read"),
                ("rewrite_without_update", "rewrite_without_update"),
            ],
            vec![],
            vec![],
        ),
        "rewrite-record" => (
            vec![("rewrite_commit", "rewrite_commit")],
            vec![("after_commit", "rewrite_commit")],
            vec![],
        ),
        "forbidden-mutation-on-invreq" => (
            vec![],
            vec![("after_invalid_rewrite", "rewrite_without_update")],
            vec![],
        ),
        "context-invalidated-at-syncpoint" => (
            vec![
                ("context_read_update", "context_read_update"),
                ("context_syncpoint", "context_syncpoint"),
                ("rewrite_after_syncpoint", "rewrite_after_syncpoint"),
            ],
            vec![("after_context_rewrite", "rewrite_after_syncpoint")],
            vec![],
        ),
        "commit-boundary" => (
            vec![("syncpoint_commit", "syncpoint_commit")],
            vec![("after_commit", "syncpoint_commit")],
            vec![],
        ),
        "rollback-boundary" => (
            vec![
                ("rollback_read_update", "rollback_read_update"),
                ("rewrite_before_rollback", "rewrite_before_rollback"),
                ("read_before_rollback", "read_before_rollback"),
                ("syncpoint_rollback", "syncpoint_rollback"),
                ("final_read", "final_read"),
            ],
            vec![("after_rollback", "syncpoint_rollback")],
            vec![],
        ),
        "missing-record" => (vec![("missing_record", "missing_record")], vec![], vec![]),
        "unauthorized" => (
            vec![("unauthorized", "unauthorized")],
            vec![("after_unauthorized", "unauthorized")],
            vec![],
        ),
        "closed-file" => (
            vec![
                ("closed_unenabled", "closed_unenabled"),
                ("closed_enabled", "closed_enabled"),
            ],
            vec![
                ("after_closed_unenabled", "closed_unenabled"),
                ("after_closed_enabled", "closed_enabled"),
            ],
            vec![("after_closed_enabled", "closed_enabled")],
        ),
        _ => unreachable!("obligation was checked above"),
    };
    let checks = report
        .profiles
        .iter()
        .map(|profile| profile_evidence(profile, expected, &commands, &snapshots, &resource_states))
        .collect::<Result<Vec<_>, _>>()?;
    let matched = checks.iter().all(|check| check.0);
    let expected = checks
        .iter()
        .map(|check| check.1.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    let actual = checks
        .iter()
        .map(|check| check.2.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    Ok((matched, expected, actual))
}

fn dataset_name() -> Result<DatasetName, String> {
    DatasetName::new("PILOT.ACCTDAT", 128).map_err(|problem| problem.to_string())
}

fn encode(value: &str) -> Result<Vec<u8>, String> {
    CodePage::Cp037
        .encode(value, 1024)
        .map_err(|problem| problem.to_string())
}

fn mutation(sequence: u64, identity: &str) -> Result<Mutation, String> {
    Ok(Mutation {
        sequence,
        idempotency_key: IdempotencyKey::new(identity, InvocationLimits::default())
            .map_err(|problem| problem.to_string())?,
        transaction: None,
    })
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)] as char,
                DIGITS[usize::from(byte & 15)] as char,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const WRITE_LENGTH_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSWRL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 RECORD-BUFFER PIC X(8) VALUE 'ABC12345'.
01 RECORD-KEY PIC X(3) VALUE 'ABC'.
01 WRITE-LENGTH PIC S9(4) COMP VALUE 5.
01 READ-LENGTH PIC S9(4) COMP VALUE 8.
01 KEY-LENGTH PIC S9(4) COMP VALUE 3.
01 READ-BUFFER PIC X(8) VALUE ALL 'X'.
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS WRITE FILE('WRITELEN') FROM(RECORD-BUFFER) RIDFLD(RECORD-KEY)
    LENGTH(WRITE-LENGTH) KEYLENGTH(KEY-LENGTH)
    RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'WRITE:' RESP-X ':' RESP2-X.
EXEC CICS READ FILE('WRITELEN') INTO(READ-BUFFER) RIDFLD(RECORD-KEY)
    LENGTH(READ-LENGTH) KEYLENGTH(KEY-LENGTH)
    RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'READ:' RESP-X ':' RESP2-X ':' READ-BUFFER.
STOP RUN.
"#;

    const REMOTE_ROLLBACK_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSREMOTE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS SYNCPOINT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'REMOTE:' RESP-X ':' RESP2-X.
STOP RUN.
"#;

    const TASK_ENQUEUE_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSENQ.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 LOCK-NAME PIC X(4) VALUE 'LOCK'.
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS ENQ RESOURCE(LOCK-NAME) LENGTH(4) UOW RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY EIBFN.
EXEC CICS DEQ RESOURCE(LOCK-NAME) LENGTH(4) UOW RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY EIBFN.
STOP RUN.
"#;

    #[test]
    fn cics_pilot_sources_use_only_the_typed_executable_dialects() {
        for source in [
            COMMIT_SOURCE,
            CONTEXT_SOURCE,
            ROLLBACK_SOURCE,
            INVALID_REWRITE_SOURCE,
        ] {
            let artifact = crate::compile(source).unwrap();
            assert!(
                artifact
                    .manifest()
                    .dialect_contracts
                    .iter()
                    .any(|dialect| matches!(dialect.as_str(), "cics.file@1" | "cics.recovery@1"))
            );
            let module = mainframe_env_ir::decode_binary(
                artifact.payload(),
                mainframe_env_ir::CodecLimits::default(),
            )
            .unwrap();
            let operations = module
                .regions()
                .iter()
                .flat_map(|region| &region.blocks)
                .flat_map(|block| &block.operations)
                .collect::<Vec<_>>();
            assert!(!operations.iter().any(|operation| {
                operation.identity.namespace() == "mainframe.core.cobol"
                    && operation.identity.name() == "exec_cics"
            }));
            let typed = operations
                .iter()
                .filter(|operation| {
                    matches!(
                        operation.identity.namespace(),
                        "cics.file" | "cics.recovery"
                    )
                })
                .collect::<Vec<_>>();
            assert!(!typed.is_empty());
            assert!(typed.iter().all(|operation| {
                operation.attributes.contains_key("cics_plan")
                    && !operation.attributes.contains_key("arguments")
                    && !operation.attributes.contains_key("control_text")
            }));
        }
    }

    #[test]
    fn cics_pilot_runs_compiler_interpreter_coordinator_providers_and_real_stores() {
        let report = run_cics_pilot_profiles().unwrap();
        assert_eq!(report.profiles.len(), 2);
        assert_eq!(report.differential_credit, 0);
        for obligation in [
            "plain-read",
            "read-update",
            "missing-record",
            "unauthorized",
            "closed-file",
            "requires-read-update",
            "rewrite-record",
            "forbidden-mutation-on-invreq",
            "context-invalidated-at-syncpoint",
            "commit-boundary",
            "rollback-boundary",
            "durable-restart",
        ] {
            let (matched, expected, actual) = compare_obligation(obligation, &report).unwrap();
            assert!(
                matched,
                "{obligation}: expected={expected:?} actual={actual:?}"
            );
        }
    }

    /// PR #179 review remediation: prove WRITE LENGTH across compiler lowering,
    /// typed interpreter execution, CICS file control, and persisted dataset
    /// read-back rather than checking any one layer in isolation.
    #[test]
    fn write_length_compiles_executes_and_persists_only_the_selected_prefix() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let platform_store: Arc<dyn PlatformStore> = store;
        let dataset =
            DatasetService::open(provider_store.clone(), DatasetLimits::default()).unwrap();
        let dataset_name = DatasetName::new("PILOT.WRITELEN", 128).unwrap();
        dataset
            .invoke(DatasetRequest::Create {
                dataset: dataset_name.clone(),
                attributes: DatasetAttributes {
                    organization: DatasetOrganization::KeySequenced,
                    record_format: RecordFormat::Variable,
                    logical_record_length: 8,
                    key_offset: Some(0),
                    key_length: Some(3),
                    ccsid: None,
                },
                mutation: mutation(1, "write-length-create").unwrap(),
            })
            .unwrap();
        let inner = pilot_inner_host(dataset.clone()).unwrap();
        let cics = CicsService::open(inner, provider_store, CicsLimits::default()).unwrap();
        cics.register_file_definitions(&BTreeMap::from([(
            "WRITELEN".into(),
            CicsFileDefinition {
                dataset: dataset_name.clone(),
                ccsid: None,
            },
        )]))
        .unwrap();
        let execution = PilotExecution::new(pilot_outer_host(cics).unwrap(), platform_store);
        let output = execute_source(
            WRITE_LENGTH_SOURCE,
            "CICSWRL",
            "IBMUSER",
            "write-length-selected-route",
            &execution,
        )
        .unwrap();
        assert_eq!(output, "WRITE:000:000\nREAD:000:000:ABC12   \n");

        let readback = dataset
            .invoke(DatasetRequest::Read {
                dataset: dataset_name,
                member: None,
                key: Some(b"ABC".to_vec()),
                max_records: 1,
                control: Default::default(),
            })
            .unwrap();
        assert!(matches!(
            readback,
            DatasetResult::Records { records, .. } if records == [b"ABC12".to_vec()]
        ));
    }

    #[test]
    fn remote_rollback_uses_the_typed_selected_route_without_claiming_credit() {
        let store = Arc::new(MemoryStore::new(StoreLimits::default()));
        let provider_store: Arc<dyn ProviderStateStore> = store.clone();
        let platform_store: Arc<dyn PlatformStore> = store;
        let dataset =
            DatasetService::open(provider_store.clone(), DatasetLimits::default()).unwrap();
        seed_dataset(&dataset, "remote-rollback-selected-route").unwrap();
        let inner = pilot_inner_host(dataset).unwrap();
        let cics = CicsService::open(inner, provider_store, CicsLimits::default()).unwrap();
        let outer = pilot_outer_host(cics).unwrap();
        let execution = PilotExecution::new(outer, platform_store);
        let artifact = crate::compile(REMOTE_ROLLBACK_SOURCE).unwrap();
        let limits = InvocationLimits::default();
        let mut invocation = pilot_invocation(
            &artifact,
            "IBMUSER",
            "remote-rollback-selected-route",
            "CICSREMOTE",
        )
        .unwrap();
        invocation.bindings = BTreeMap::from([
            (
                "cics.execution-context".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.execution-context@1",
                    b"dpl-synconreturn".to_vec(),
                    limits,
                )
                .unwrap(),
            ),
            (
                "cics.syncpoint.remote-outcome".into(),
                BoundedPayload::new(
                    "mainframe-env.cics.syncpoint.remote-outcome@1",
                    b"unable-to-commit".to_vec(),
                    limits,
                )
                .unwrap(),
            ),
        ]);
        let output = drive_artifact(&artifact, invocation, &execution).unwrap();
        assert_eq!(parse_command(&output, "REMOTE", false).unwrap().resp, 82);
        assert_eq!(parse_command(&output, "REMOTE", false).unwrap().resp2, 0);
    }

    #[test]
    fn task_enqueue_uses_typed_selected_routes_in_local_and_dpl_contexts() {
        for (identity, context) in [
            ("task-enqueue-local", None),
            (
                "task-enqueue-dpl",
                Some(b"dpl-without-synconreturn".as_slice()),
            ),
        ] {
            let store = Arc::new(MemoryStore::new(StoreLimits::default()));
            let provider_store: Arc<dyn ProviderStateStore> = store.clone();
            let platform_store: Arc<dyn PlatformStore> = store.clone();
            let dataset =
                DatasetService::open(provider_store.clone(), DatasetLimits::default()).unwrap();
            let inner = pilot_inner_host(dataset).unwrap();
            let cics = CicsService::open(inner, provider_store, CicsLimits::default()).unwrap();
            let outer = pilot_outer_host(cics).unwrap();
            let execution = PilotExecution::new(outer, platform_store);
            let artifact = crate::compile(TASK_ENQUEUE_SOURCE).unwrap();
            assert!(
                artifact
                    .manifest()
                    .dialect_contracts
                    .contains("cics.task@1")
            );
            let mut invocation =
                pilot_invocation(&artifact, "IBMUSER", identity, "CICSENQ").unwrap();
            if let Some(context) = context {
                invocation.bindings.insert(
                    "cics.execution-context".into(),
                    BoundedPayload::new(
                        "mainframe-env.cics.execution-context@1",
                        context.to_vec(),
                        InvocationLimits::default(),
                    )
                    .unwrap(),
                );
            }
            let output = drive_artifact(&artifact, invocation, &execution).unwrap();
            assert!(
                output
                    .as_bytes()
                    .windows(2)
                    .any(|bytes| bytes == [0x12, 0x04])
            );
            assert!(
                output
                    .as_bytes()
                    .windows(2)
                    .any(|bytes| bytes == [0x12, 0x06])
            );
            assert!(
                store
                    .list_provider_state("cics-enqueue-v1", 2)
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn cics_pilot_restart_worker() {
        let Ok(phase) = std::env::var("MAINFRAME_ENV_CICS_RESTART_PHASE") else {
            return;
        };
        let url = std::env::var("MAINFRAME_ENV_CICS_RESTART_URL").unwrap();
        match phase.as_str() {
            "mutate" => {
                let point = match std::env::var("MAINFRAME_ENV_CICS_RESTART_POINT")
                    .unwrap()
                    .as_str()
                {
                    "before-intent" => CicsFileFaultPoint::BeforeIntent,
                    "after-intent" => CicsFileFaultPoint::AfterIntent,
                    "after-mutation" => CicsFileFaultPoint::AfterMutation,
                    other => panic!("unknown restart fault point {other}"),
                };
                restart_mutate(&url, point).unwrap();
                std::process::exit(86);
            }
            "recover" => {
                restart_recover(&url).unwrap();
            }
            other => panic!("unknown restart worker phase {other}"),
        }
    }

    #[test]
    fn cics_pilot_sqlite_process_restart_recovers_at_bounded_file_faults() {
        let executable = std::env::current_exe().unwrap();
        for point in ["before-intent", "after-intent", "after-mutation"] {
            let nonce = PROFILE_NONCE.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "mainframe-env-cics-restart-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&directory).unwrap();
            let url = format!("sqlite://{}?mode=rwc", directory.join("state.db").display());
            let mutate = Command::new(&executable)
                .args([
                    "--exact",
                    "cics_pilot::tests::cics_pilot_restart_worker",
                    "--nocapture",
                ])
                .env("MAINFRAME_ENV_CICS_RESTART_PHASE", "mutate")
                .env("MAINFRAME_ENV_CICS_RESTART_POINT", point)
                .env("MAINFRAME_ENV_CICS_RESTART_URL", &url)
                .status()
                .unwrap();
            assert_eq!(mutate.code(), Some(86), "{point}");
            let recover = Command::new(&executable)
                .args([
                    "--exact",
                    "cics_pilot::tests::cics_pilot_restart_worker",
                    "--nocapture",
                ])
                .env("MAINFRAME_ENV_CICS_RESTART_PHASE", "recover")
                .env("MAINFRAME_ENV_CICS_RESTART_URL", &url)
                .status()
                .unwrap();
            let _ = fs::remove_dir_all(&directory);
            assert!(recover.success(), "{point}");
        }
    }

    #[test]
    fn cics_pilot_comparator_rejects_status_and_state_perturbations_independently() {
        fn failed_obligations(report: &CicsPilotReport) -> BTreeSet<&'static str> {
            [
                "plain-read",
                "read-update",
                "missing-record",
                "unauthorized",
                "closed-file",
                "requires-read-update",
                "rewrite-record",
                "forbidden-mutation-on-invreq",
                "context-invalidated-at-syncpoint",
                "commit-boundary",
                "rollback-boundary",
                "durable-restart",
            ]
            .into_iter()
            .filter(|obligation| !compare_obligation(obligation, report).unwrap().0)
            .collect()
        }

        let report = run_cics_pilot_profiles().unwrap();
        let mut false_status = report.clone();
        false_status.profiles[0]
            .commands
            .get_mut("rewrite_without_update")
            .unwrap()
            .resp = 0;
        assert_eq!(
            failed_obligations(&false_status),
            BTreeSet::from(["requires-read-update"])
        );

        let mut forbidden_mutation = report.clone();
        forbidden_mutation.profiles[0].record_snapshots.insert(
            "after_invalid_rewrite".into(),
            hex(&encode("AA22").unwrap()),
        );
        assert_eq!(
            failed_obligations(&forbidden_mutation),
            BTreeSet::from(["forbidden-mutation-on-invreq"])
        );

        let mut rollback_status = report.clone();
        rollback_status.profiles[0]
            .commands
            .get_mut("syncpoint_rollback")
            .unwrap()
            .resp = 16;
        assert_eq!(
            failed_obligations(&rollback_status),
            BTreeSet::from(["rollback-boundary"])
        );

        let mut durable_missing = report;
        durable_missing
            .profiles
            .iter_mut()
            .find(|profile| profile.profile == "sqlite")
            .unwrap()
            .durable_restart = None;
        assert_eq!(
            failed_obligations(&durable_missing),
            BTreeSet::from(["durable-restart"])
        );

        let mut stale_identity = run_cics_pilot_profiles().unwrap();
        stale_identity.environment_manifest_digest =
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        let output = DriverOutput::new(
            serde_json::to_vec(&stale_identity).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        assert!(
            !CicsPilotObservation {
                obligation: "plain-read"
            }
            .evaluate(&output)
            .unwrap()
            .matched
        );
    }
}
