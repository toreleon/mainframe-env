#[cfg(test)]
use mainframe_env_cics::CicsFileFaultPoint;
use mainframe_env_cics::{CicsFileDefinition, CicsLimits, CicsService, cics_provider};
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
    InvocationLimits, Machine, MachineDrive, MachineResume, Principal, PrincipalId, Quantum,
    RequestId, ResourceLimits, RunUnitId, Selector, ServiceClass, TraceId,
};
use mainframe_env_host_api::{
    CapabilityDescriptor, CicsConditionPolicy, CicsOperation, CicsRequest, DatasetAttributes,
    DatasetName, DatasetOrganization, DatasetRequest, DatasetResult, EffectRequest, EffectResult,
    HostLimits, HostProblem, HostProvider, HostRequest, HostResult, Mutation, RecordFormat,
    RegistrySnapshot, ScopedHostService, SecurityDecision, SecurityRequest, SessionId,
};
use mainframe_env_interpreter::ReferenceMachine;
use mainframe_env_ir::CodecLimits;
use mainframe_env_store::{MemoryStore, SqliteStateStore, StoreLimits};
use mainframe_env_store_api::ProviderStateStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const FIXTURES: &str = include_str!("../../../../conformance/0.9/cics/pilot-fixtures.json");
const ENVIRONMENT: &str = include_str!("../../../../conformance/0.9/cics/pilot-environment.json");

const HAPPY_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSPILOT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'PLAIN:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS REWRITE FILE('ACCTDAT') FROM('AA22') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'INVALID:' RESP-X ':' RESP2-X.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'UPDATE1:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS REWRITE FILE('ACCTDAT') FROM('AA22') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'REWRITE1:' RESP-X ':' RESP2-X.
EXEC CICS SYNCPOINT RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'COMMIT:' RESP-X ':' RESP2-X.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'UPDATE2:' RESP-X ':' RESP2-X ':' REC-X.
EXEC CICS REWRITE FILE('ACCTDAT') FROM('AA33') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'REWRITE2:' RESP-X ':' RESP2-X.
EXEC CICS SYNCPOINT ROLLBACK RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'ROLLBACK:' RESP-X ':' RESP2-X.
EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'FINAL:' RESP-X ':' RESP2-X ':' REC-X.
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

#[cfg(test)]
const RESTART_MUTATE_SOURCE: &str = r#"IDENTIFICATION DIVISION.
PROGRAM-ID. CICSCRASH.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC-X PIC X(4).
01 RESP-X PIC 9(3) VALUE 0.
01 RESP2-X PIC 9(3) VALUE 0.
PROCEDURE DIVISION.
EXEC CICS READ FILE('ACCTDAT') UPDATE INTO(REC-X) RIDFLD('AA') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
EXEC CICS REWRITE FILE('ACCTDAT') FROM('AA33') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.
DISPLAY 'UNREACHED'.
STOP RUN.
"#;

#[cfg(test)]
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
pub struct CicsPilotProfileObservation {
    pub profile: String,
    pub happy_output: String,
    pub record_after_invalid_hex: String,
    pub final_record_hex: String,
    pub missing_output: String,
    pub unauthorized_output: String,
    pub record_after_unauthorized_hex: String,
    pub closed_output: String,
    pub record_after_closed_hex: String,
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

pub struct CicsPilotRuntime {
    case_driver: ScenarioOnlyDriver,
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
        drivers.push((
            DriverRef::new("cics.pilot.product-path", limits)?,
            &self.case_driver,
        ));
        for observation in &self.observations {
            observations.push((
                ObservationRef::new(format!("cics.pilot.{}", observation.obligation), limits)?,
                observation,
            ));
        }
        let id = ScenarioId::new("cics.file-uow.local", limits)?;
        if spec.scenario(&id).is_none() {
            return Ok(Vec::new());
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
        let (matched, expected, actual) = compare_obligation(self.obligation, &report)?;
        ObservationCheck::new(matched, expected, actual, ConformanceLimits::default())
            .map_err(|problem| problem.to_string())
    }
}

pub fn run_cics_pilot_profiles() -> Result<CicsPilotReport, String> {
    let fixtures: Value = serde_json::from_str(FIXTURES).map_err(|error| error.to_string())?;
    let expected_policy = fixtures["comparison_policy"]["version"]
        .as_str()
        .ok_or_else(|| "CICS pilot comparison policy is missing".to_string())?;
    let memory: Arc<dyn ProviderStateStore> = Arc::new(MemoryStore::new(StoreLimits::default()));
    let memory_observation = run_profile("memory", memory)?;

    let nonce = PROFILE_NONCE.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-cics-pilot-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let database = directory.join("state.db");
    let url = format!("sqlite://{}?mode=rwc", database.display());
    let sqlite: Arc<dyn ProviderStateStore> = Arc::new(
        SqliteStateStore::open(&url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| error.to_string())?,
    );
    let sqlite_observation = run_profile("sqlite", sqlite);
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

fn run_profile(
    profile: &str,
    store: Arc<dyn ProviderStateStore>,
) -> Result<CicsPilotProfileObservation, String> {
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default())
        .map_err(|problem| format!("{profile} dataset open: {problem}"))?;
    seed_dataset(&dataset, profile).map_err(|problem| format!("{profile} seed: {problem}"))?;
    let inner = pilot_inner_host(dataset.clone())
        .map_err(|problem| format!("{profile} inner host: {problem}"))?;
    let cics = CicsService::open(inner, store, CicsLimits::default())
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

    let happy_output = execute_source(
        HAPPY_SOURCE,
        "CICSPILOT",
        "IBMUSER",
        &format!("{profile}-happy"),
        outer.clone(),
    )
    .map_err(|problem| format!("{profile} happy path: {problem}"))?;
    let record_after_invalid_hex = extract_record_after_invalid(&happy_output)?;
    let final_record_hex = read_record_hex(&dataset)?;

    let missing_output = execute_read_case(
        "MISSING",
        "ZZ",
        "IBMUSER",
        &format!("{profile}-missing"),
        outer.clone(),
    )
    .map_err(|problem| format!("{profile} missing path: {problem}"))?;
    let unauthorized_output = execute_read_case(
        "UNAUTHORIZED",
        "AA",
        "DENIED",
        &format!("{profile}-denied"),
        outer.clone(),
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
    close_file(&cics, &closed_invocation)
        .map_err(|problem| format!("{profile} close setup: {problem}"))?;
    let closed_output = drive_artifact(&closed_artifact, closed_invocation, outer)
        .map_err(|problem| format!("{profile} closed path: {problem}"))?;
    let record_after_closed_hex = read_record_hex(&dataset)?;

    Ok(CicsPilotProfileObservation {
        profile: profile.into(),
        happy_output,
        record_after_invalid_hex,
        final_record_hex,
        missing_output,
        unauthorized_output,
        record_after_unauthorized_hex,
        closed_output,
        record_after_closed_hex,
    })
}

#[cfg(test)]
fn restart_mutate(url: &str, fault_point: CicsFileFaultPoint) -> Result<(), String> {
    let store: Arc<dyn ProviderStateStore> = Arc::new(
        SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| error.to_string())?,
    );
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    seed_dataset(&dataset, "restart")?;
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone())?,
        store,
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
    let outer = pilot_outer_host(cics)?;
    let result = execute_source(
        RESTART_MUTATE_SOURCE,
        "CICSCRASH",
        "IBMUSER",
        "restart",
        outer,
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

#[cfg(test)]
fn restart_recover(url: &str) -> Result<(), String> {
    let store: Arc<dyn ProviderStateStore> = Arc::new(
        SqliteStateStore::open(url, 64 * 1024 * 1024, 262_144)
            .map_err(|error| error.to_string())?,
    );
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default())
        .map_err(|problem| problem.to_string())?;
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone())?,
        store,
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
    let output = execute_source(
        RESTART_ROLLBACK_SOURCE,
        "CICSRECOVER",
        "IBMUSER",
        "restart",
        pilot_outer_host(cics)?,
    )?;
    if output != "RECOVERED:000:000\n" {
        return Err(format!("unexpected restart output: {output:?}"));
    }
    if read_record_hex(&dataset)? != hex(&encode("AA11")?) {
        return Err("restart rollback did not restore the pre-UOW record".into());
    }
    Ok(())
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
    host: Arc<ScopedHostService>,
) -> Result<String, String> {
    let artifact = crate::compile(source)?;
    let invocation = pilot_invocation(&artifact, principal, identity, program)?;
    drive_artifact(&artifact, invocation, host)
}

fn execute_read_case(
    label: &str,
    key: &str,
    principal: &str,
    identity: &str,
    host: Arc<ScopedHostService>,
) -> Result<String, String> {
    execute_source(
        &read_source(label, key),
        "CICSNEG",
        principal,
        identity,
        host,
    )
}

fn read_source(label: &str, key: &str) -> String {
    format!(
        "{READ_SOURCE_PREFIX}EXEC CICS READ FILE('ACCTDAT') INTO(REC-X) RIDFLD('{key}') RESP(RESP-X) RESP2(RESP2-X) END-EXEC.\nDISPLAY '{label}:' RESP-X ':' RESP2-X.\nSTOP RUN.\n"
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
        ArtifactRef::new(format!("sha256:{}", artifact.id().to_hex()), limits)
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

fn drive_artifact(
    artifact: &mainframe_env_compiler_api::PublishedArtifact,
    invocation: Invocation,
    host: Arc<ScopedHostService>,
) -> Result<String, String> {
    let mut machine = ReferenceMachine::from_binary(
        artifact.payload(),
        invocation.clone(),
        CodecLimits::default(),
    )
    .map_err(|problem| format!("{problem:?}"))?;
    let mut resume = MachineResume::Start;
    loop {
        let quantum =
            Quantum::new(128, 4096).ok_or_else(|| "CICS pilot quantum is invalid".to_string())?;
        match machine.drive(resume, quantum) {
            MachineDrive::Continue => resume = MachineResume::Start,
            MachineDrive::HostCall(effect) => {
                let result = host.invoke(
                    &invocation,
                    invocation.deadline_tick.saturating_sub(1),
                    false,
                    effect,
                );
                resume = MachineResume::HostResult(result.effect);
            }
            MachineDrive::Completed(done) => {
                return String::from_utf8(done.output.bytes().to_vec())
                    .map_err(|_| "CICS pilot application output is not UTF-8".into());
            }
            other => return Err(format!("CICS pilot machine did not complete: {other:?}")),
        }
    }
}

fn close_file(cics: &CicsService, invocation: &Invocation) -> Result<(), String> {
    let payload = BoundedPayload::new(
        "mainframe-env.cics.file-status@1",
        b"CLOSED".to_vec(),
        InvocationLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    let mut close_mutation = mutation(1, "close-file")?;
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

fn extract_record_after_invalid(output: &str) -> Result<String, String> {
    let line = output
        .lines()
        .find(|line| line.starts_with("UPDATE1:"))
        .ok_or_else(|| "CICS pilot UPDATE1 observation is missing".to_string())?;
    let record = line
        .rsplit(':')
        .next()
        .ok_or_else(|| "CICS pilot UPDATE1 record is missing".to_string())?;
    Ok(hex(&encode(record)?))
}

fn compare_obligation(
    obligation: &str,
    report: &CicsPilotReport,
) -> Result<(bool, String, String), String> {
    let fixtures: Value = serde_json::from_str(FIXTURES).map_err(|error| error.to_string())?;
    let expected = &fixtures["expected"];
    let expected_happy = format!(
        "PLAIN:{:03}:{:03}:{}\nINVALID:{:03}:{:03}\nUPDATE1:{:03}:{:03}:{}\nREWRITE1:{:03}:{:03}\nCOMMIT:000:000\nUPDATE2:000:000:{}\nREWRITE2:000:000\nROLLBACK:{:03}:{:03}\nFINAL:000:000:{}\n",
        expected["plain_read"]["resp"].as_u64().unwrap_or(u64::MAX),
        expected["plain_read"]["resp2"].as_u64().unwrap_or(u64::MAX),
        expected["plain_read"]["record"]
            .as_str()
            .unwrap_or("missing"),
        expected["rewrite_without_update"]["resp"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["rewrite_without_update"]["resp2"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["read_update"]["resp"].as_u64().unwrap_or(u64::MAX),
        expected["read_update"]["resp2"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["read_update"]["record"]
            .as_str()
            .unwrap_or("missing"),
        expected["rewrite_commit"]["resp"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["rewrite_commit"]["resp2"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["rewrite_commit"]["record_after"]
            .as_str()
            .unwrap_or("missing"),
        expected["rollback"]["resp"].as_u64().unwrap_or(u64::MAX),
        expected["rollback"]["resp2"].as_u64().unwrap_or(u64::MAX),
        expected["rollback"]["record_after"]
            .as_str()
            .unwrap_or("missing"),
    );
    let expected_missing = format!(
        "MISSING:{:03}:{:03}\n",
        expected["missing_record"]["resp"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["missing_record"]["resp2"]
            .as_u64()
            .unwrap_or(u64::MAX)
    );
    let expected_unauthorized = format!(
        "UNAUTHORIZED:{:03}:{:03}\n",
        expected["unauthorized"]["resp"]
            .as_u64()
            .unwrap_or(u64::MAX),
        expected["unauthorized"]["resp2"]
            .as_u64()
            .unwrap_or(u64::MAX)
    );
    let expected_closed = format!(
        "CLOSED:{:03}:{:03}\n",
        expected["closed_file"]["resp"].as_u64().unwrap_or(u64::MAX),
        expected["closed_file"]["resp2"]
            .as_u64()
            .unwrap_or(u64::MAX)
    );
    let expected_record = hex(&encode(
        expected["rollback"]["record_after"]
            .as_str()
            .ok_or_else(|| "rollback expected record is missing".to_string())?,
    )?);
    let expected_initial = hex(&encode(
        expected["rewrite_without_update"]["record_after"]
            .as_str()
            .ok_or_else(|| "invalid rewrite expected record is missing".to_string())?,
    )?);
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
    let checks = report
        .profiles
        .iter()
        .map(|profile| match obligation {
            "plain-read"
            | "read-update"
            | "requires-read-update"
            | "rewrite-record"
            | "context-invalidated-at-syncpoint"
            | "commit-boundary"
            | "rollback-boundary" => (
                profile.happy_output == expected_happy,
                expected_happy.clone(),
                profile.happy_output.clone(),
            ),
            "forbidden-mutation-on-invreq" => (
                profile.record_after_invalid_hex == expected_initial,
                expected_initial.clone(),
                profile.record_after_invalid_hex.clone(),
            ),
            "missing-record" => (
                profile.missing_output == expected_missing,
                expected_missing.clone(),
                profile.missing_output.clone(),
            ),
            "unauthorized" => (
                profile.unauthorized_output == expected_unauthorized
                    && profile.record_after_unauthorized_hex == expected_record,
                format!("{expected_unauthorized}record={expected_record}"),
                format!(
                    "{}record={}",
                    profile.unauthorized_output, profile.record_after_unauthorized_hex
                ),
            ),
            "closed-file" => (
                profile.closed_output == expected_closed
                    && profile.record_after_closed_hex == expected_record,
                format!("{expected_closed}record={expected_record}"),
                format!(
                    "{}record={}",
                    profile.closed_output, profile.record_after_closed_hex
                ),
            ),
            "durable-restart" => (
                profile.profile != "sqlite" || profile.final_record_hex == expected_record,
                format!("sqlite-record={expected_record}"),
                format!("{}-record={}", profile.profile, profile.final_record_hex),
            ),
            _ => unreachable!("obligation was checked above"),
        })
        .collect::<Vec<_>>();
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

    #[test]
    fn cics_pilot_runs_compiler_interpreter_providers_and_real_stores() {
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
            "recover" => restart_recover(&url).unwrap(),
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
        let report = run_cics_pilot_profiles().unwrap();
        let mut false_status = report.clone();
        false_status.profiles[0].happy_output = false_status.profiles[0]
            .happy_output
            .replace("INVALID:016:030", "INVALID:000:000");
        assert!(
            !compare_obligation("requires-read-update", &false_status)
                .unwrap()
                .0
        );

        let mut forbidden_mutation = report;
        forbidden_mutation.profiles[0].record_after_invalid_hex = hex(&encode("AA22").unwrap());
        assert!(
            !compare_obligation("forbidden-mutation-on-invreq", &forbidden_mutation)
                .unwrap()
                .0
        );
    }
}
