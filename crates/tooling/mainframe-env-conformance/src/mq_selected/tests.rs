use super::*;
#[test]
fn selected_memory_sqlite_match_independent_transcript() {
    for name in ["mq.memory.local-v1", "mq.sqlite.local-v1"] {
        let report = product::run(name).unwrap();
        let output = DriverOutput::new(
            serde_json::to_vec(&report).unwrap(),
            ConformanceLimits::default(),
        )
        .unwrap();
        let observation = if name == "mq.memory.local-v1" {
            &MEMORY_OBSERVATION
        } else {
            &SQLITE_OBSERVATION
        };
        let check = observation.evaluate(&output).unwrap();
        assert!(check.matched, "{}", check.actual);
    }
}
#[test]
fn harness_rejects_missing_generic_saf_mutation_and_byte_evidence() {
    let report = product::run("mq.memory.local-v1").unwrap();
    let evaluate = |r: &Report| {
        MEMORY_OBSERVATION
            .evaluate(
                &DriverOutput::new(serde_json::to_vec(r).unwrap(), ConformanceLimits::default())
                    .unwrap(),
            )
            .unwrap()
            .matched
    };
    assert!(evaluate(&report));
    for field in ["fixture", "expectation", "setup"] {
        let mut r = report.clone();
        match field {
            "fixture" => r.fixture_digest = digest(b"stale actual input"),
            "expectation" => r.expectation_digest = digest(b"stale expected output"),
            _ => r.setup_digest = digest(b"stale setup"),
        }
        assert!(!evaluate(&r), "{field} identity");
    }
    let mut r = report.clone();
    r.saf.observations.remove(10);
    // The exact resource-name set is unchanged, but a repeated check is absent.
    assert_eq!(
        report
            .saf
            .observations
            .iter()
            .map(|o| (&o.class, &o.resource, &o.intent))
            .collect::<std::collections::BTreeSet<_>>(),
        r.saf
            .observations
            .iter()
            .map(|o| (&o.class, &o.resource, &o.intent))
            .collect::<std::collections::BTreeSet<_>>(),
    );
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.observations[2].decision = "Unauthorized".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.observations[2].phase = "replay".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.observations[2].principal = "OTHER".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.observations[2].sequence += 1;
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.run_unit = "foreign-run".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.invocation_key = "foreign-invocation".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.denied_saf.observations.last_mut().unwrap().decision = "allow".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.fixture = "mq.sqlite.local-v1".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.steps.pop();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.steps[3].kind = "success".into();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.saf.observations.clear();
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.forbidden_mutation = true;
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.steps[6].body_hex = Some("00".into());
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.audited_effects = 0;
    assert!(!evaluate(&r));
    let mut r = report.clone();
    r.steps[6].descriptor_fields.as_mut().unwrap()[12] = "00".into();
    assert!(!evaluate(&r));
    let mut value = serde_json::to_value(report.clone()).unwrap();
    value["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("body_hex");
    assert!(
        MEMORY_OBSERVATION
            .evaluate(
                &DriverOutput::new(
                    serde_json::to_vec(&value).unwrap(),
                    ConformanceLimits::default()
                )
                .unwrap()
            )
            .is_err()
    );
    let mut value = serde_json::to_value(report).unwrap();
    value.as_object_mut().unwrap().remove("provider_receipts");
    assert!(
        MEMORY_OBSERVATION
            .evaluate(
                &DriverOutput::new(
                    serde_json::to_vec(&value).unwrap(),
                    ConformanceLimits::default()
                )
                .unwrap()
            )
            .is_err()
    );
}

#[test]
fn registered_fixture_binds_actual_emission_source_not_expectation() {
    let limits = ConformanceLimits::default();
    let catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../conformance/0.2/catalogs/mq.json"
    ))
    .unwrap();
    let rows = catalog["units"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            OfficialCatalogRow::new(
                r["id"].as_str().unwrap(),
                "mq",
                "mqi-calls-unique",
                r["source_locator"].as_str().unwrap(),
                CoverageGate::ALL,
                limits,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut document: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../conformance/spec/v1/spec.json")).unwrap();
    for key in ["rows", "obligations", "cases"] {
        document[key]
            .as_array_mut()
            .unwrap()
            .retain(|r| r["row_id"].as_str().unwrap().starts_with("ibm-mq-"));
    }
    document["scenarios"] = serde_json::json!([]);
    let bind = |document: &serde_json::Value| {
        let spec = CompiledSpec::compile_json(
            document["catalog_digest"].as_str().unwrap(),
            rows.clone(),
            &serde_json::to_vec(document).unwrap(),
            limits,
        )
        .unwrap();
        let mut drivers = Vec::new();
        let mut observations = Vec::new();
        let result = bind_mq_selected(&spec, &mut drivers, &mut observations, limits);
        if result.is_err() {
            assert!(drivers.is_empty() && observations.is_empty());
        }
        result
    };
    assert_ne!(digest(INPUT), digest(EXPECTED.as_bytes()));
    assert!(bind(&document).is_ok());
    for f in document["registries"]["fixtures"].as_array_mut().unwrap() {
        if f["id"].as_str().unwrap().starts_with("mq.") {
            f["digest"] = serde_json::json!(digest(EXPECTED.as_bytes()));
        }
    }
    assert!(bind(&document).is_err());
}
