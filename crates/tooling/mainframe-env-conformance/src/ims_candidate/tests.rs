use super::*;
use serde_json::Value;

fn candidate() -> CompiledCandidate {
    let value: Value = serde_json::from_slice(include_bytes!(
        "../../../../../conformance/spec/candidates/ims-db.json"
    ))
    .unwrap();
    let catalog: Value = serde_json::from_slice(include_bytes!(
        "../../../../../conformance/0.2/catalogs/ims.json"
    ))
    .unwrap();
    let rows = catalog["units"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            OfficialCatalogRow::new(
                row["id"].as_str().unwrap(),
                "ims",
                "dli-call-families",
                row["source_locator"].as_str().unwrap(),
                CoverageGate::ALL,
                ConformanceLimits::default(),
            )
            .unwrap()
        })
        .collect();
    CompiledCandidate::compile_json(
        value["spec"]["catalog_digest"].as_str().unwrap(),
        rows,
        &serde_json::to_vec(&value).unwrap(),
        ConformanceLimits::default(),
    )
    .unwrap()
}
fn runtime() -> ImsCandidateRuntime {
    ims_candidate_runtime(include_bytes!(
        "../../../../../conformance/spec/fixtures/ims-db.json"
    ))
    .unwrap()
}

struct Mutant<'a> {
    driver: &'a ImsCandidateRuntime,
    kind: usize,
}
impl ConformanceDriver for Mutant<'_> {
    fn execute(&self, fixture: &FixtureRef) -> Result<DriverOutput, String> {
        let output = self.driver.execute(fixture)?;
        let mut actual: Outcome = serde_json::from_slice(output.bytes()).unwrap();
        match self.kind {
            0 => {
                // Omit the replacement transition.
                actual.state.database[0] = b"A1x".to_vec();
                actual.state.current = Some(b"A1x".to_vec());
            }
            1 => actual.calls[0].status = Some("  ".into()), // Generic success for GP.
            2 | 3 => {
                // Bypass operand validation or authorization.
                actual.calls[0].problem = None;
                actual.calls[0].status = Some("  ".into());
            }
            4 => actual.state.database[0][2] ^= 1, // Forbidden mutation.
            5 => actual.calls[0].segments[0].data[0] ^= 1, // Wrong bytes.
            6 => actual.state.parentage = None,    // Lost parentage.
            7 => actual.state.held = false,        // Omitted hold.
            _ => unreachable!(),
        }
        DriverOutput::new(
            serde_json::to_vec(&actual).unwrap(),
            ConformanceLimits::default(),
        )
        .map_err(|e| e.to_string())
    }
}

#[test]
fn shared_runner_rejects_eight_representative_ims_driver_mutants() {
    let candidate = candidate();
    let runtime = runtime();
    let limits = ConformanceLimits::default();
    let context = RunnerContext::new(
        format!("sha256:{}", "1".repeat(64)),
        "ims-mutant-preparation",
        limits,
    )
    .unwrap();
    let targets = [
        "hold_unique_replace",
        "parent_required",
        "malformed_ssa",
        "denied",
        "replace_without_hold",
        "unique_repeated",
        "parent_sequence",
        "hold_unique_replace",
    ];
    for (kind, target) in targets.iter().enumerate() {
        let selection = RunnerSelection::replay(format!("ims.db.{target}.memory"), limits).unwrap();
        let report = runtime
            .prepare_using(
                &candidate,
                &Mutant {
                    driver: &runtime,
                    kind,
                },
                &selection,
                &context,
            )
            .unwrap();
        assert_eq!(report.checks, 1);
        assert_eq!(report.mismatches.len(), 1, "mutant {kind} survived");
    }
}

#[test]
fn ims_driver_rejects_unknown_fixture_and_observation_shape() {
    let runtime = runtime();
    assert!(
        runtime
            .execute(&FixtureRef::new("ims.db.unknown", ConformanceLimits::default()).unwrap())
            .is_err()
    );
    let observation = Observation(runtime.fixtures[0].expected.clone());
    let output = DriverOutput::new(b"{}".to_vec(), ConformanceLimits::default()).unwrap();
    assert!(observation.evaluate(&output).is_err());
}

#[test]
fn ims_fixture_parser_rejects_duplicate_identity_unknown_recipe_and_oversized_bytes() {
    let bytes = include_bytes!("../../../../../conformance/spec/fixtures/ims-db.json");
    let original: Value = serde_json::from_slice(bytes).unwrap();
    let mut duplicate = original.clone();
    duplicate["fixtures"][1]["id"] = duplicate["fixtures"][0]["id"].clone();
    assert!(ims_candidate_runtime(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    let mut unknown = original;
    unknown["fixtures"][0]["trial"] = Value::String("script".into());
    assert!(ims_candidate_runtime(&serde_json::to_vec(&unknown).unwrap()).is_err());
    assert!(ims_candidate_runtime(&vec![b' '; 512 * 1024 + 1]).is_err());
}
