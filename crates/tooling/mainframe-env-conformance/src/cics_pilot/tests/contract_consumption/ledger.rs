//! Local test-side consumption of the manager's same-builder export.
//!
//! Setup (outside the test process, never nested Cargo):
//! `cargo run --locked --offline -p xtask -- conformance-spec-export > "$EXPORT"`
//! then set `MAINFRAME_ENV_CONFORMANCE_SPEC_EXPORT=$EXPORT` for the focused tests.
//! Regenerate after any candidate edit. Missing or stale input is an error, not a skip.
//! Source expectations remain the frozen parent fixtures; these verdicts grant no license credit.

use super::*;
use mainframe_env_coverage::{
    ConformancePredicate, ConformanceRunReport, ConformanceRunner, CoverageGate, GateState,
    OfficialCatalogRow, OfficialRowId, RunnerContext, RunnerSelection, Verdict,
};
use std::path::{Path, PathBuf};
use std::process::Command;

const PARTICIPANT_CONTRACT: &str = include_str!(
    "../../../../../../../conformance/subsystems/integration/contracts/transaction-participant.json"
);

const SCENARIO: &str = "cics.file-uow.local";
// Frozen current effective runner document includes the 18 upstream MQ selected cases.
// The CICS scenario, all prior cases and normalized catalog metadata remain unchanged.
const SPEC_DIGEST: &str = "sha256:4d6d838c9f2a2b848e6b9d6e3a32b3f519c4cf807d0071cae3f394db18950233";
// Frozen same-builder normalized catalog metadata, not an execution authority.
const CATALOG_ROWS_DIGEST: &str =
    "sha256:c5d0d72ded0d030bc05c0e89ae70390e90606419fb21e698bac16fe4864145aa";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    schema_version: String,
    candidate_digest: String,
    catalog_digest: String,
    spec_digest: String,
    spec_document: Value,
    catalog_rows: Vec<CatalogRow>,
    execution_credit: u8,
    licensed_credit: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRow {
    row_id: String,
    subsystem: String,
    family: String,
    source_locator: String,
    applicable_gates: Vec<String>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

// The exact existing xtask candidate algorithm, used only to validate input provenance.
// This is not a spec builder, runtime dispatch path, or seal.
fn current_candidate() -> Result<String, String> {
    let root = root();
    let output = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(&root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("candidate file inventory failed".into());
    }
    let mut paths = output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            std::str::from_utf8(p)
                .map(str::to_owned)
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    let mut sha = Sha256::new();
    for path in paths {
        let bytes = fs::read(root.join(&path)).map_err(|e| e.to_string())?;
        sha.update((path.len() as u64).to_be_bytes());
        sha.update(path.as_bytes());
        sha.update((bytes.len() as u64).to_be_bytes());
        sha.update(bytes);
    }
    Ok(format!("sha256:{:x}", sha.finalize()))
}

fn decode_export(bytes: &[u8], candidate: &str) -> Result<(Export, CompiledSpec), String> {
    let export: Export = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if export.schema_version != "mainframe-env.conformance-spec-export@1"
        || export.execution_credit != 0
        || export.licensed_credit != 0
        || export.candidate_digest != candidate
        || export.spec_digest != SPEC_DIGEST
        || export.catalog_digest
            != digest(
                &fs::read(root().join("conformance/subsystems/coverage/catalogs/index.json"))
                    .map_err(|e| e.to_string())?,
            )
    {
        return Err("wrong export schema, credits, candidate, catalog or spec identity".into());
    }
    let catalog_facts = export
        .catalog_rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "row_id": row.row_id,
                "subsystem": row.subsystem,
                "family": row.family,
                "source_locator": row.source_locator,
                "applicable_gates": row.applicable_gates,
            })
        })
        .collect::<Vec<_>>();
    if digest(&serde_json::to_vec(&catalog_facts).map_err(|e| e.to_string())?)
        != CATALOG_ROWS_DIGEST
    {
        return Err("exported catalog row metadata differs from frozen owner projection".into());
    }
    let limits = ConformanceLimits::default();
    let rows = export
        .catalog_rows
        .iter()
        .map(|row| {
            let gates = row
                .applicable_gates
                .iter()
                .map(|slug| {
                    CoverageGate::ALL
                        .into_iter()
                        .find(|g| g.slug() == slug)
                        .ok_or_else(|| format!("unknown catalog gate {slug}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            if gates.len() != CoverageGate::ALL.len()
                || gates.iter().copied().collect::<BTreeSet<_>>()
                    != CoverageGate::ALL.into_iter().collect()
            {
                return Err("missing or duplicate official catalog gates".into());
            }
            OfficialCatalogRow::new(
                &row.row_id,
                &row.subsystem,
                &row.family,
                &row.source_locator,
                gates,
                limits,
            )
            .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() != 1506 {
        return Err("incomplete official catalog closure".into());
    }
    let spec = CompiledSpec::compile_json(
        &export.catalog_digest,
        rows,
        &serde_json::to_vec(&export.spec_document).map_err(|e| e.to_string())?,
        limits,
    )
    .map_err(|e| e.to_string())?;
    if spec.spec_digest() != export.spec_digest {
        return Err("export document does not reproduce its spec digest".into());
    }
    let id = ScenarioId::new(SCENARIO, limits).unwrap();
    let scenario = spec.scenario(&id).ok_or("missing existing CICS scenario")?;
    if scenario.credits().len() != 30
        || scenario
            .credits()
            .iter()
            .filter(|key| key.row_id.as_str() == ROW)
            .count()
            != 7
    {
        return Err("existing CICS credits changed".into());
    }
    Ok((export, spec))
}

fn export_input() -> (Vec<u8>, Export, CompiledSpec) {
    let path = std::env::var_os("MAINFRAME_ENV_CONFORMANCE_SPEC_EXPORT").expect(
        "first generate the current same-builder export; set MAINFRAME_ENV_CONFORMANCE_SPEC_EXPORT",
    );
    let bytes = fs::read(path).expect("read external current-candidate export");
    let (export, spec) = decode_export(&bytes, &current_candidate().unwrap()).unwrap();
    (bytes, export, spec)
}

fn payload_binding(invocation: &Invocation, name: &str) -> Value {
    invocation.bindings.get(name).map_or(Value::Null, |p| {
        serde_json::json!([p.schema(), hex(p.bytes())])
    })
}

fn resource_snapshot<S: PlatformStore>(store: &S) -> String {
    let rows = store.list_provider_state_prefix("cics-", 1024).unwrap();
    digest(
        format!(
            "{:?}",
            rows.into_iter()
                .filter(|r| r.namespace != "cics-effect-replay-v1")
                .collect::<Vec<_>>()
        )
        .as_bytes(),
    )
}

fn durable_snapshot<S: PlatformStore>(store: &S, invocation: &Invocation) -> String {
    digest(
        format!(
            "{:?}",
            (
                store.effect(&effect_key(invocation)).unwrap(),
                store.list_provider_state_prefix("cics-", 1024).unwrap(),
                store.events(&invocation.execution_id, 1, 32).unwrap(),
                store
                    .audit_records(&invocation.execution_id, 1, 32)
                    .unwrap(),
                store.get_execution(&invocation.execution_id).unwrap(),
            )
        )
        .as_bytes(),
    )
}

fn audit_projection<S: PlatformStore>(store: &S, invocation: &Invocation) -> Value {
    let mut rows = store
        .audit_records(&invocation.execution_id, 1, 32)
        .unwrap()
        .into_iter()
        .filter_map(|a| match a.capability.as_str() {
            "host.cics.execute" => Some(serde_json::json!([
                a.capability.as_str(),
                format!("{:?}", a.decision),
                format!("{:?}", a.resource),
                a.principal.as_str(),
                a.execution_id.as_str(),
                a.run_unit_id.as_str(),
                a.invocation_key.as_str(),
                a.effect_sequence,
                a.attempt,
            ])),
            "host.security.authorize" => Some(serde_json::json!([
                a.capability.as_str(),
                format!("{:?}", a.decision),
                format!("{:?}", a.resource),
                a.principal.as_str(),
            ])),
            _ => None,
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|v| v[0].as_str().unwrap().to_string());
    Value::Array(rows)
}

fn observe<S: PlatformStore>(store: &S, invocation: &Invocation, output: &str) -> Value {
    let key = effect_key(invocation);
    let effect = store.effect(&key).unwrap().unwrap();
    let response = parse_command(output, "CONSUMED", false).unwrap();
    let uow = store
        .get_provider_state("cics-uow", key.as_str())
        .unwrap()
        .map(|row| {
            let d = describe_cics_uow_row(&row, None).unwrap();
            serde_json::json!([
                d.row_version,
                format!("{:?}", d.codec),
                format!("{:?}", d.state),
                d.effect_key,
                d.owner_execution,
                d.owner_run_unit,
                d.task_owner_execution,
                d.transaction,
                d.terminal_tick,
                format!("{:?}", d.dependency),
            ])
        });
    let events = store.events(&invocation.execution_id, 1, 32).unwrap();
    let execution = store
        .get_execution(&invocation.execution_id)
        .unwrap()
        .unwrap();
    let replay = store
        .get_provider_state("cics-effect-replay-v1", key.as_str())
        .unwrap()
        .unwrap();
    let execution_identity = serde_json::json!([
        execution.execution_id.as_str(),
        execution.run_unit_id.as_str(),
        format!("{:?}", execution.artifact),
        execution.principal.as_str(),
        format!("{:?}", execution.state)
    ]);
    let execution_digest = digest(&serde_json::to_vec(&execution_identity).unwrap());
    println!(
        "SYNCPOINT_EXECUTION {}",
        serde_json::json!({
            "effect_key": key.as_str(), "preimage": execution_identity, "digest": execution_digest
        })
    );
    serde_json::json!({
        "response": [response.resp, response.resp2],
        "output": digest(output.as_bytes()),
        "context": payload_binding(invocation, "cics.execution-context"),
        "remote": payload_binding(invocation, "cics.syncpoint.remote-outcome"),
        "effect": [key.as_str(), effect.execution_id.as_str(), effect.run_unit_id.as_str(),
            effect.sequence, format!("{:?}", effect.state), format!("{:?}", effect.digest_format),
            hex(&effect.request_digest), effect.result_digest.map(|d| hex(&d)),
            effect.intent.capability.as_ref().map(|c| c.as_str())],
        "resolved_tick": effect.resolved_tick,
        "deadline_tick": store.get_provider_state("cics-uow", key.as_str()).unwrap()
            .and_then(|r| describe_cics_uow_row(&r, None).unwrap().deadline_tick),
        "uow": uow,
        "undo_count": store.list_provider_state("cics-uow-undo", 16).unwrap().len(),
        "replay_codec": String::from_utf8(replay.payload[..8].to_vec()).unwrap(),
        "lifecycle": digest(format!("{:?}", events.iter().map(|e| &e.kind).collect::<Vec<_>>()).as_bytes()),
        "audit": digest(&serde_json::to_vec(&audit_projection(store, invocation)).unwrap()),
        "execution": execution_digest,
    })
}

struct ExpectedSample {
    case: Value,
    invocation: Invocation,
    profile: &'static str,
    omitted: bool,
    artifact_payload_digest: String,
    observed_output: String,
}

// All semantic expected values come from fixtures/contract and the explicit submitted identity.
// No expected condition, UOW state, effect digest or audit resource is read from the handler.
fn expected_observation(sample: &ExpectedSample) -> Value {
    let case = &sample.case;
    let inv = &sample.invocation;
    let condition = &case["expected"];
    let key = effect_key(inv);
    let mut audits = vec![
        serde_json::json!([
            "host.security.authorize",
            "Success",
            format!(
                "{:?}",
                canonical_audit_resource_digest(&HostRequest::Security(
                    SecurityRequest::Authorize {
                        principal: inv.principal.id().clone(),
                        class: "TCICSTRN".into(),
                        resource: ResourceName::new("CICS.DEFAULT", 246).unwrap(),
                        intent: AccessIntent::Execute,
                    }
                ))
            ),
            inv.principal.id().as_str(),
        ]),
        serde_json::json!([
            "host.cics.execute",
            "Success",
            format!(
                "{:?}",
                canonical_audit_resource_digest(&expected_request(case, inv))
            ),
            inv.principal.id().as_str(),
            inv.execution_id.as_str(),
            inv.run_unit_id.as_str(),
            inv.idempotency_key.as_str(),
            1,
            1,
        ]),
    ];
    let events = [
        LifecycleEventKind::Admitted,
        LifecycleEventKind::Queued,
        LifecycleEventKind::Started,
        LifecycleEventKind::EffectIntent { sequence: 1 },
        LifecycleEventKind::EffectResult { sequence: 1 },
        LifecycleEventKind::Completing,
        LifecycleEventKind::Completed { return_code: 0 },
    ];
    let context = if sample.omitted {
        Value::Null
    } else {
        serde_json::json!([
            "mainframe-env.cics.execution-context@1",
            hex(case["execution_context"].as_str().unwrap().as_bytes())
        ])
    };
    let remote = case["remote_outcome"].as_str().map_or(Value::Null, |v| {
        serde_json::json!([
            "mainframe-env.cics.syncpoint.remote-outcome@1",
            hex(v.as_bytes())
        ])
    });
    let output = format!(
        "CONSUMED:{:03}:{:03}\nEIBFN:\u{16}\u{2}\n",
        condition["response"].as_u64().unwrap(),
        condition["response2"].as_u64().unwrap()
    );
    // Audit projections are sorted by capability to avoid claiming an unstated response precedence.
    audits.sort_by_key(|v| v[0].as_str().unwrap().to_string());
    serde_json::json!({
        "response": [condition["response"], condition["response2"]], "output": digest(output.as_bytes()),
        "context": context, "remote": remote,
        "effect": [key.as_str(), inv.execution_id.as_str(), inv.run_unit_id.as_str(), 1,
            "Completed", "CanonicalHostV1", hex(&canonical_request_digest(&expected_request(case, inv)).unwrap()),
            hex(&canonical_result_digest(&expected_result(case)).unwrap()), "host.cics.execute"],
        "uow": if condition["uow_state"] == "absent" { Value::Null } else { serde_json::json!([
            2, "RetentionV2", if condition["uow_state"] == "committed" { "Committed" } else { "RolledBack" },
            key.as_str(), inv.execution_id.as_str(), inv.run_unit_id.as_str(), Value::Null,
            "DEFAULT", Value::Null, "TerminalObservationRequired",
        ]) },
        "undo_count": 0, "replay_codec": "MECER003",
        "lifecycle": digest(format!("{:?}", events.iter().collect::<Vec<_>>()).as_bytes()),
        "audit": digest(&serde_json::to_vec(&audits).unwrap()),
        "execution": digest(&serde_json::to_vec(&serde_json::json!([
            inv.execution_id.as_str(), inv.run_unit_id.as_str(), format!("{:?}", inv.artifact),
            inv.principal.id().as_str(), "Completed"
        ])).unwrap()),
    })
}

fn capture_sample<S: PlatformStore + 'static>(
    store: Arc<S>,
    case: &Value,
    omitted: bool,
    profile: &'static str,
) -> (ExpectedSample, Value) {
    let rollback = case["requested"] == "rollback";
    let artifact = crate::compile(&source(&format!(
        "{}RESP(RESP-X) RESP2(RESP2-X)",
        if rollback { "ROLLBACK " } else { "" }
    )))
    .unwrap();
    bind_artifact(
        &artifact,
        rollback,
        &[CicsOutputName::Resp, CicsOutputName::Resp2],
    );
    let invocation = invocation(case, &artifact, omitted);
    let dataset = DatasetService::open(store.clone(), DatasetLimits::default()).unwrap();
    seed_dataset(&dataset, invocation.idempotency_key.as_str()).unwrap();
    let cics = CicsService::open(
        pilot_inner_host(dataset.clone()).unwrap(),
        store.clone(),
        CicsLimits::default(),
    )
    .unwrap();
    let session = SessionId::new(format!("session-{}", invocation.run_unit_id), 1024).unwrap();
    cics.create_session(&session, 24, 80).unwrap();
    cics.register_run(invocation.clone(), &session, "DEFAULT", "ME01", "S001")
        .unwrap();
    let before = resource_snapshot(store.as_ref());
    let execution = PilotExecution::new(pilot_outer_host(cics).unwrap(), store.clone());
    let output = drive_artifact(&artifact, invocation.clone(), &execution).unwrap();
    let observation = observe(store.as_ref(), &invocation, &output);
    let sample = ExpectedSample {
        case: case.clone(),
        invocation,
        profile,
        omitted,
        artifact_payload_digest: digest(artifact.payload()),
        observed_output: output,
    };
    let value = serde_json::json!({
        "case": case["id"], "profile": profile, "omitted": omitted,
        "artifact": sample.artifact_payload_digest,
        "observation": observation,
        "record": read_record_hex(&dataset).unwrap(),
        "business_before": before, "business_after": resource_snapshot(store.as_ref()),
        "reopen": Value::Null,
    });
    (sample, value)
}

fn capture_participants() -> (Vec<ExpectedSample>, Vec<Value>) {
    let fixtures: Value = serde_json::from_str(PARTICIPANT_FIXTURES).unwrap();
    let directory = std::env::temp_dir().join(format!(
        "mainframe-env-consumption-ledger-{}-{}",
        std::process::id(),
        PROFILE_NONCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).unwrap();
    let result = std::panic::catch_unwind(|| {
        let mut expected = Vec::new();
        let mut observed = Vec::new();
        for case in fixtures["cics_cases"].as_array().unwrap() {
            for omitted in if case["execution_context"] == "local" {
                vec![false, true]
            } else {
                vec![false]
            } {
                let (sample, value) = capture_sample(
                    Arc::new(MemoryStore::new(StoreLimits::default())),
                    case,
                    omitted,
                    "memory",
                );
                expected.push(sample);
                observed.push(value);
                let url = format!(
                    "sqlite://{}?mode=rwc",
                    directory
                        .join(format!("{}-{omitted}.db", case["id"].as_str().unwrap()))
                        .display()
                );
                let store =
                    Arc::new(SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap());
                let (sample, mut value) = capture_sample(store.clone(), case, omitted, "sqlite");
                let before = durable_snapshot(store.as_ref(), &sample.invocation);
                assert_eq!(Arc::strong_count(&store), 1);
                drop(store);
                let reopened = SqliteStateStore::open(&url, 4 * 1024 * 1024, 65_536).unwrap();
                let after = durable_snapshot(&reopened, &sample.invocation);
                value["reopen"] = serde_json::json!([before, after]);
                // Re-observe actual fields after close/open, not an in-memory copy of the old report.
                value["observation"] =
                    observe(&reopened, &sample.invocation, &sample.observed_output);
                value["business_after"] = resource_snapshot(&reopened).into();
                let dataset =
                    DatasetService::open(Arc::new(reopened), DatasetLimits::default()).unwrap();
                value["record"] = read_record_hex(&dataset).unwrap().into();
                expected.push(sample);
                observed.push(value);
            }
        }
        (expected, observed)
    });
    fs::remove_dir_all(directory).unwrap();
    result.unwrap()
}

// Full-spec closure is retained, but unselected subsystems cannot execute or grant any pass.
// Running public COBOL handler admission here would itself start an unrelated fixture campaign.
struct OutsideScope;
impl ConformanceDriver for OutsideScope {
    fn execute(&self, _: &FixtureRef) -> Result<DriverOutput, String> {
        Err("outside this local CICS scenario".into())
    }
}
impl ConformancePredicate for OutsideScope {
    fn evaluate(&self, _: &FixtureRef) -> Result<bool, String> {
        Err("outside this local CICS scenario".into())
    }
}
impl ConformanceObservation for OutsideScope {
    fn evaluate(&self, _: &DriverOutput) -> Result<ObservationCheck, String> {
        Err("outside this local CICS scenario".into())
    }
}

struct CapturedScenario(ScenarioObservationBundle);
impl ConformanceScenarioDriver for CapturedScenario {
    fn execute(&self, scenario: &ScenarioSpec) -> Result<ScenarioObservationBundle, String> {
        if scenario.scenario_id().as_str() != SCENARIO {
            return Err("wrong captured scenario".into());
        }
        Ok(self.0.clone())
    }
}

struct ConsumptionObservation<'a> {
    original: &'a dyn ConformanceObservation,
    baseline: DriverOutput,
    samples: Vec<&'a ExpectedSample>,
    candidate: &'a str,
    spec: &'a str,
}
impl ConformanceObservation for ConsumptionObservation<'_> {
    fn evaluate(&self, output: &DriverOutput) -> Result<ObservationCheck, String> {
        let original = self.original.evaluate(&self.baseline)?;
        let value: Value = serde_json::from_slice(output.bytes()).map_err(|e| e.to_string())?;
        let mut matched = original.matched
            && value["pilot"] == digest(self.baseline.bytes())
            && value["candidate"] == self.candidate
            && value["spec"] == self.spec
            && value["fixture"] == digest(PARTICIPANT_FIXTURES.as_bytes())
            && value["command_contract"] == digest(COMMAND_CONTRACTS.as_bytes())
            && value["participant_contract"] == digest(PARTICIPANT_CONTRACT.as_bytes());
        let actual = value["samples"]
            .as_array()
            .ok_or("missing participant observations")?;
        matched &= actual.len() == self.samples.len();
        let mut differences = Vec::new();
        for (got, sample) in actual.iter().zip(&self.samples) {
            let mut observed = got["observation"].clone();
            let object = observed
                .as_object_mut()
                .ok_or("malformed participant observation")?;
            let resolved = object
                .remove("resolved_tick")
                .ok_or("missing resolution tick")?;
            let deadline = object
                .remove("deadline_tick")
                .ok_or("missing UOW deadline")?;
            let expected = expected_observation(sample);
            let reopen = &got["reopen"];
            let good = observed == expected
                && resolved.as_u64().is_some_and(|v| v > 0)
                && (if expected["uow"].is_null() {
                    deadline.is_null()
                } else {
                    deadline.as_u64().is_some_and(|v| v > 0)
                })
                && got["case"] == sample.case["id"]
                && got["profile"] == sample.profile
                && got["omitted"] == sample.omitted
                && got["artifact"] == sample.artifact_payload_digest
                && got["record"] == hex(&encode("AA11").unwrap())
                && (sample.case["expected"]["uow_state"] != "absent"
                    || got["business_before"] == got["business_after"])
                && (if sample.profile == "sqlite" {
                    reopen
                        .as_array()
                        .is_some_and(|v| v.len() == 2 && v[0].is_string() && v[0] == v[1])
                } else {
                    reopen.is_null()
                });
            matched &= good;
            if !good {
                differences.push(format!(
                    "{}:{}:{} expected={expected} actual={observed}",
                    sample.case["id"], sample.profile, sample.omitted
                ));
            }
        }
        ObservationCheck::new(
            matched,
            format!(
                "{}; {} independently frozen participant observations; orderly SQLite reopen only",
                original.expected,
                self.samples.len()
            ),
            format!("{}; differences={differences:?}", original.actual),
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}

// Each original observation keeps its original pilot comparison and adds the relevant
// independently specified participant boundary. This also respects the existing 16 KiB bound.
fn matches_obligation(obligation: &str, sample: &ExpectedSample) -> bool {
    match obligation {
        "commit-boundary" => sample.case["requested"] == "commit",
        "rollback-boundary" => sample.case["requested"] == "rollback",
        "durable-restart" => sample.profile == "sqlite",
        _ => false,
    }
}

fn run_bound(
    spec: &CompiledSpec,
    export: &Export,
    pilot: &CicsPilotRuntime,
    baseline: &ScenarioObservationBundle,
    samples: &[ExpectedSample],
    values: &[Value],
    corrupt: Option<&str>,
) -> Result<ConformanceRunReport, SpecProblem> {
    let limits = ConformanceLimits::default();
    let outside = OutsideScope;
    let mut drivers = Vec::new();
    let mut observations = Vec::new();
    pilot.bind(spec, &mut drivers, &mut observations, limits)?;
    let selected = spec.scenario(&ScenarioId::new(SCENARIO, limits)?).unwrap();
    let originals = observations.iter().cloned().collect::<BTreeMap<_, _>>();
    let mut wrappers = Vec::new();
    let mut outputs = Vec::new();
    for key in selected.credits() {
        let original_output = &baseline.observations()[key];
        if key.row_id.as_str() == ROW {
            let case = spec.case(key).unwrap();
            let observation = case.expected()[0].clone();
            if !wrappers.iter().any(|(id, _)| id == &observation) {
                wrappers.push((
                    observation.clone(),
                    ConsumptionObservation {
                        original: originals[&observation],
                        baseline: original_output.clone(),
                        samples: samples
                            .iter()
                            .filter(|sample| matches_obligation(key.obligation_id.as_str(), sample))
                            .collect(),
                        candidate: &export.candidate_digest,
                        spec: spec.spec_digest(),
                    },
                ));
            }
            let mut value = serde_json::json!({ "pilot": digest(original_output.bytes()),
                "candidate": export.candidate_digest, "spec": spec.spec_digest(),
                "fixture": digest(PARTICIPANT_FIXTURES.as_bytes()),
                "command_contract": digest(COMMAND_CONTRACTS.as_bytes()),
                "participant_contract": digest(PARTICIPANT_CONTRACT.as_bytes()), "samples": samples.iter().zip(values)
                    .filter(|(sample, _)| matches_obligation(key.obligation_id.as_str(), sample))
                    .map(|(_, value)| value).collect::<Vec<_>>() });
            match corrupt {
                Some("condition") => value["samples"][0]["observation"]["response"][0] = 16.into(),
                Some("context") => {
                    value["samples"][0]["observation"]["context"] = serde_json::json!([
                        "mainframe-env.cics.execution-context@1",
                        hex(b"dpl-without-synconreturn")
                    ])
                }
                Some("effect") => {
                    value["samples"][0]["observation"]["effect"][6] = hex(&[0; 32]).into()
                }
                Some("malformed") => value["samples"][0]["observation"] = Value::Null,
                _ => {}
            }
            outputs.push((
                key.clone(),
                DriverOutput::new(serde_json::to_vec(&value).unwrap(), limits)?,
            ));
        } else {
            outputs.push((key.clone(), original_output.clone()));
        }
    }
    for (id, wrapper) in &wrappers {
        let item = observations.iter_mut().find(|(key, _)| key == id).unwrap();
        item.1 = wrapper as &dyn ConformanceObservation;
    }
    for id in spec.registries().drivers() {
        if !drivers.iter().any(|(key, _)| key == id) {
            drivers.push((id.clone(), &outside));
        }
    }
    for id in spec.registries().observations() {
        if !observations.iter().any(|(key, _)| key == id) {
            observations.push((id.clone(), &outside));
        }
    }
    let predicates = spec
        .registries()
        .predicates()
        .iter()
        .map(|id| (id.clone(), &outside as &dyn ConformancePredicate))
        .collect();
    if corrupt == Some("missing") {
        outputs.pop();
    }
    let captured = CapturedScenario(ScenarioObservationBundle::new(outputs, limits)?);
    let runtime = RuntimeRegistry::new(spec, drivers, predicates, observations, limits)?
        .with_scenario_drivers(
            spec,
            vec![(selected.scenario_id().clone(), &captured)],
            limits,
        )?;
    let context = RunnerContext::new_with_environment_manifest(
        &export.candidate_digest,
        "local",
        digest(ENVIRONMENT.as_bytes()),
        limits,
    )?;
    ConformanceRunner::new(spec, runtime, limits)
        .run(&RunnerSelection::scenario(SCENARIO, limits)?, &context)
}

#[test]
fn syncpoint_consumption_ledger_compiled_route_and_negative_verdicts() {
    let (_, export, spec) = export_input();
    let pilot = cics_pilot_runtime();
    let mut drivers = Vec::new();
    let mut observations = Vec::new();
    let scenario_drivers = pilot
        .bind(
            &spec,
            &mut drivers,
            &mut observations,
            ConformanceLimits::default(),
        )
        .unwrap();
    let scenario = spec
        .scenario(&ScenarioId::new(SCENARIO, ConformanceLimits::default()).unwrap())
        .unwrap();
    // Execute existing compiled file/UOW/recovery route, then six frozen participant cases and
    // omitted-local controls through the same compiler/interpreter/coordinator/provider helpers.
    let baseline = scenario_drivers[0].1.execute(scenario).unwrap();
    let (samples, values) = capture_participants();
    assert_eq!(samples.len(), 16);
    for value in &values {
        println!(
            "SYNCPOINT_PARTICIPANT {}",
            serde_json::to_string(value).unwrap()
        );
    }
    let report = run_bound(&spec, &export, &pilot, &baseline, &samples, &values, None).unwrap();
    let events = report
        .batches
        .iter()
        .flat_map(|b| &b.events)
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 30);
    assert!(events.iter().all(|e| e.verdict == Verdict::Pass));
    for event in events {
        assert_eq!(event.candidate_digest, export.candidate_digest);
        assert_eq!(event.spec_digest, spec.spec_digest());
        assert_eq!(event.catalog_digest, export.catalog_digest);
        assert_eq!(
            event.environment_manifest_digest,
            digest(ENVIRONMENT.as_bytes())
        );
        assert_eq!(event.fixture_or_seed.as_str(), "cics.file-uow.pilot-v1");
        assert!(event.oracle_receipt_digest.is_none());
        let case = spec.case(&event.key).unwrap();
        assert_eq!(case.scenario().unwrap().as_str(), SCENARIO);
        assert!(!case.reviewed_rules().is_empty());
        println!(
            "SYNCPOINT_LEDGER {}",
            String::from_utf8(event.canonical_json().unwrap()).unwrap()
        );
    }
    assert_eq!(
        report.ledger.rows[&OfficialRowId::new(ROW, ConformanceLimits::default()).unwrap()].gates
            [&CoverageGate::Differential]
            .state,
        GateState::Pending
    );
    println!(
        "SYNCPOINT_DERIVED_LEDGER {}",
        String::from_utf8(report.ledger.canonical_json().unwrap()).unwrap()
    );
    for mutant in ["condition", "context", "effect", "malformed"] {
        let report = run_bound(
            &spec,
            &export,
            &pilot,
            &baseline,
            &samples,
            &values,
            Some(mutant),
        )
        .unwrap();
        let events = report
            .batches
            .iter()
            .flat_map(|b| &b.events)
            .collect::<Vec<_>>();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.key.row_id.as_str() == ROW && e.verdict == Verdict::Fail)
                .count(),
            7,
            "{mutant}"
        );
        assert!(
            events
                .iter()
                .filter(|e| e.key.row_id.as_str() != ROW)
                .all(|e| e.verdict == Verdict::Pass)
        );
        println!("SYNCPOINT_MUTANT {mutant} failed=7 unchanged=23");
    }
    assert!(matches!(
        run_bound(
            &spec,
            &export,
            &pilot,
            &baseline,
            &samples,
            &values,
            Some("missing")
        ),
        Err(SpecProblem::InvalidScenario(_))
    ));
}

#[test]
fn syncpoint_consumption_ledger_export_and_binding_fail_closed() {
    let (bytes, export, spec) = export_input();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    for field in [
        "schema_version",
        "candidate_digest",
        "catalog_digest",
        "spec_digest",
    ] {
        let mut wrong = value.clone();
        wrong[field] = "wrong".into();
        assert!(
            decode_export(
                &serde_json::to_vec(&wrong).unwrap(),
                &export.candidate_digest
            )
            .is_err(),
            "{field}"
        );
    }
    for field in ["source_locator", "subsystem", "family", "row_id"] {
        let mut wrong = value.clone();
        wrong["catalog_rows"][0][field] = "altered-valid-token".into();
        assert!(
            decode_export(
                &serde_json::to_vec(&wrong).unwrap(),
                &export.candidate_digest
            )
            .is_err(),
            "catalog {field}"
        );
    }
    for change in [
        "missing-gate",
        "unknown-gate",
        "duplicate-gate",
        "missing-binding",
    ] {
        let mut wrong = value.clone();
        match change {
            "missing-gate" => {
                wrong["catalog_rows"][0]["applicable_gates"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
            }
            "unknown-gate" => wrong["catalog_rows"][0]["applicable_gates"][0] = "unknown".into(),
            "duplicate-gate" => {
                wrong["catalog_rows"][0]["applicable_gates"][0] =
                    wrong["catalog_rows"][0]["applicable_gates"][1].clone()
            }
            "missing-binding" => {
                let cases = wrong["spec_document"]["cases"].as_array_mut().unwrap();
                let at = cases.iter().position(|c| c["row_id"] == ROW).unwrap();
                cases.remove(at);
            }
            _ => unreachable!(),
        }
        assert!(
            decode_export(
                &serde_json::to_vec(&wrong).unwrap(),
                &export.candidate_digest
            )
            .is_err(),
            "{change}"
        );
    }
    assert!(decode_export(b"{}", &export.candidate_digest).is_err());
    // The actual registry rejects missing driver/observer bindings, rather than creating success.
    assert!(
        RuntimeRegistry::new(&spec, vec![], vec![], vec![], ConformanceLimits::default()).is_err()
    );
    let guard = OutsideScope;
    assert!(
        ConformanceDriver::execute(
            &guard,
            &FixtureRef::new("outside", ConformanceLimits::default()).unwrap()
        )
        .is_err()
    );
    assert!(
        ConformanceObservation::evaluate(
            &guard,
            &DriverOutput::new(vec![], ConformanceLimits::default()).unwrap()
        )
        .is_err()
    );
}
