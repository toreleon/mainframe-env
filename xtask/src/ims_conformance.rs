//! IMS bindings remain draft until human maintainer acceptance.
use super::*;
use mainframe_env_conformance::{ImsCandidateRuntime, ims_candidate_runtime};
use mainframe_env_coverage::{CandidateSource, CompiledCandidate};

const CANDIDATE: &str = "conformance/spec/candidates/ims-db.json";
const FIXTURES: &str = "conformance/spec/fixtures/ims-db.json";

fn load(root: &Path) -> TaskResult<(CompiledCandidate, ImsCandidateRuntime)> {
    let path = root.join(CANDIDATE);
    let candidate_value = json(&path)?;
    validate_schema_instance(
        &json(&root.join("conformance/spec/schemas/conformance-candidate.schema.json"))?,
        &candidate_value,
        &path,
    )?;
    validate_schema_instance(
        &json(&root.join("conformance/spec/schemas/conformance-spec.schema.json"))?,
        &candidate_value["spec"],
        &path,
    )?;
    let fixture_path = root.join(FIXTURES);
    validate_schema_instance(
        &json(&root.join("conformance/spec/schemas/ims-db-fixtures.schema.json"))?,
        &json(&fixture_path)?,
        &fixture_path,
    )?;
    let fixture_value = json(&fixture_path)?;
    validate_schema_instance(
        &json(&root.join("conformance/0.14/schemas/ims-metadata.schema.json"))?,
        &fixture_value["metadata"],
        &fixture_path,
    )?;
    let digest = format!(
        "sha256:{}",
        file_digest(&root.join("conformance/0.2/catalogs/index.json"))?
    );
    let candidate = CompiledCandidate::compile_json(
        &digest,
        official_catalog_rows(root)?,
        &fs::read(&path).map_err(|e| e.to_string())?,
        ConformanceLimits::default(),
    )
    .map_err(|e| e.to_string())?;
    let runtime = ims_candidate_runtime(&fs::read(&fixture_path).map_err(|e| e.to_string())?)?;
    let actual_fixture_digest = format!("sha256:{}", file_digest(&fixture_path)?);
    require(
        candidate
            .registries()
            .fixtures()
            .values()
            .all(|digest| digest == &actual_fixture_digest),
        "IMS independent fixture digest is stale",
    )?;
    let declared = candidate
        .registries()
        .fixtures()
        .keys()
        .map(FixtureRef::as_str)
        .collect::<BTreeSet<_>>();
    require(
        declared == runtime.fixture_ids().collect(),
        "IMS fixture registry closure is incomplete",
    )?;
    for case in candidate.cases() {
        require(
            case.driver().as_str() == "ims.db.public-host"
                && case.expected().len() == 1
                && case.expected()[0].as_str() == format!("observe.{}", case.input()),
            "IMS candidate observation binding is not fixture-specific",
        )?;
    }
    for rule in candidate.rules() {
        for source in &rule.sources {
            validate_source(root, source)?;
        }
    }
    Ok((candidate, runtime))
}

fn validate_source(root: &Path, source: &CandidateSource) -> TaskResult {
    require(
        matches!(
            source.manifest.as_str(),
            "conformance/0.14/manifests/ims-programming-contracts-topics.json"
                | "conformance/0.14/manifests/ims-database-contracts-topics.json"
                | "conformance/0.14/manifests/ims-recovery-utilities-contracts-topics.json"
                | "conformance/0.14/manifests/ims-status-explanations-topics.json"
        ),
        "IMS candidate source is outside the bounded pinned manifests",
    )?;
    let manifest_path = root.join(&source.manifest);
    let manifest = json(&manifest_path)?;
    require(
        manifest["baseline_id"].as_str() == Some(&source.baseline)
            && array(&manifest, "topics", &manifest_path)?
                .iter()
                .any(|topic| {
                    topic["topic_path"].as_str() == Some(&source.topic_path)
                        && topic["sha256"].as_str() == Some(&source.sha256)
                }),
        "IMS candidate source identity is unpinned or mismatched",
    )?;
    Ok(())
}

pub(super) fn check(root: &Path, accepted: &CompiledSpec) -> TaskResult {
    let (candidate, _) = load(root)?;
    require(
        !accepted
            .rows()
            .any(|row| row.row_id().as_str().starts_with("ibm-ims-")),
        "IMS candidate preparation cannot change the accepted registry",
    )?;
    println!(
        "ims-candidate rules={} bindings={} rows={} review=pending-maintainer official-credit=0",
        candidate.rules().len(),
        candidate.cases().count(),
        candidate.missing_classes().len()
    );
    Ok(())
}

pub(super) fn run(root: &Path, args: &ConformanceArgs) -> TaskResult {
    require(
        args.prepare_candidates,
        "IMS official conformance remains pending HUMAN maintainer acceptance; use --prepare-candidates for zero-credit preparation",
    )?;
    let (candidate, runtime) = load(root)?;
    let limits = ConformanceLimits::default();
    let (gate, local) = parse_focused_gate(args.gate.as_deref())?;
    require(
        gate != Some(CoverageGate::Differential),
        "licensed IMS preparation is excluded",
    )?;
    let selection = if let Some(replay) = &args.replay {
        RunnerSelection::replay(replay, limits).map_err(|e| e.to_string())?
    } else {
        make_focused_selection("ims", gate, local, args.shard, limits)?
    };
    let context = RunnerContext::new(repository_digest(root)?, "ims-candidate-local", limits)
        .map_err(|e| e.to_string())?;
    let report = runtime.prepare(&candidate, &selection, &context)?;
    let mutants = runtime.mutation_checks()?;
    println!(
        "ims-candidate-preparation checks={} mismatches={} observation-mutants-rejected={} official-credit=0/25 licensed-credit=0/25 review=pending-maintainer",
        report.checks,
        report.mismatches.len(),
        mutants
    );
    require(
        report.checks > 0,
        "IMS preparation selection executed zero checks",
    )?;
    require(
        report.mismatches.is_empty(),
        &format!("IMS candidate mismatches: {:?}", report.mismatches),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .into()
    }
    fn compile(value: &Value) -> Result<CompiledCandidate, SpecProblem> {
        let root = root();
        CompiledCandidate::compile_json(
            &format!(
                "sha256:{}",
                file_digest(&root.join("conformance/0.2/catalogs/index.json")).unwrap()
            ),
            official_catalog_rows(&root).unwrap(),
            &serde_json::to_vec(value).unwrap(),
            ConformanceLimits::default(),
        )
    }
    #[test]
    fn status_supplement_source_identity_is_bound_without_acceptance() {
        let root = root();
        let mut source = CandidateSource {
            baseline: "ibm-ims-15.6-status-explanations-2026-09-11".into(),
            manifest: "conformance/0.14/manifests/ims-status-explanations-topics.json".into(),
            topic_path: "SSEPH2_15.6.0/com.ibm.ims156.doc.mc/msgs/dj.htm".into(),
            sha256: "61eb932d71cf9d009e0e6652d24840945ba51ed6a3bc98e676dac54164e18c82".into(),
            anchor: "plain_text:4-8;current hold is required".into(),
        };
        assert!(validate_source(&root, &source).is_ok());
        source.sha256 = "0".repeat(64);
        assert!(validate_source(&root, &source).is_err());
        source.sha256 = "2cdd75d8c8b15e6ca28deecbc28b458fe57cb7bedaf72ac48b24cf5ba825201d".into();
        assert!(validate_source(&root, &source).is_err());
        source.topic_path = "SSEPH2_15.6.0/com.ibm.ims156.doc.mc/msgs/da.htm".into();
        assert!(validate_source(&root, &source).is_ok());
        source.baseline = "unreviewed-baseline".into();
        assert!(validate_source(&root, &source).is_err());
        source.manifest = "conformance/0.14/manifests/unregistered.json".into();
        assert!(validate_source(&root, &source).is_err());
    }

    #[test]
    fn candidate_compiler_accepts_drafts_without_promoting_accepted_rows() {
        let root = root();
        let accepted = compile_shared_spec(&root).unwrap();
        check(&root, &accepted).unwrap();
        let (candidate, _) = load(&root).unwrap();
        assert_eq!(candidate.cases().count(), 42);
        assert_eq!(candidate.missing_classes().len(), 4);
        assert!(candidate.registries().reviewed_rules().is_empty());
    }
    #[test]
    fn candidate_compiler_rejects_self_approval_credit_or_reviewed_artifacts() {
        let original = json(&root().join(CANDIDATE)).unwrap();
        for (field, value) in [
            ("review_status", json!("accepted")),
            ("coverage_credit", json!(1)),
            (
                "review_authority",
                json!({"kind":"maintainer","approval":"fake"}),
            ),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            assert!(compile(&changed).is_err(), "{field}");
        }
        let mut changed = original;
        changed["spec"]["registries"]["reviewed_rules"] = json!([{"id":"fake",
            "digest":format!("sha256:{}", "1".repeat(64))}]);
        assert!(compile(&changed).is_err());
    }
    #[test]
    fn candidate_compiler_rejects_missing_unknown_or_duplicate_references() {
        let original = json(&root().join(CANDIDATE)).unwrap();
        let mut omitted = original.clone();
        omitted["rules"].as_array_mut().unwrap().remove(0);
        assert!(compile(&omitted).is_err());
        let mut unknown = original.clone();
        unknown["rules"][0]["bindings"][0]["obligation_id"] = json!("unknown");
        assert!(compile(&unknown).is_err());
        let mut duplicate = original.clone();
        duplicate["missing_classes"][1] = duplicate["missing_classes"][0].clone();
        assert!(compile(&duplicate).is_err());
        let mut stale = original;
        stale["spec"]["cases"][0]["input"] = json!("unknown.fixture");
        assert!(compile(&stale).is_err());
    }
    #[test]
    fn ims_candidate_shared_runner_executes_nonzero_public_driver_checks() {
        let (candidate, runtime) = load(&root()).unwrap();
        let limits = ConformanceLimits::default();
        let context = RunnerContext::new(
            format!("sha256:{}", "1".repeat(64)),
            "unit-preparation",
            limits,
        )
        .unwrap();
        let report = runtime
            .prepare(
                &candidate,
                &RunnerSelection::local("ims", None, limits).unwrap(),
                &context,
            )
            .unwrap();
        assert_eq!(report.checks, 42);
        assert!(report.mismatches.is_empty(), "{:#?}", report.mismatches);
        assert_eq!(runtime.mutation_checks().unwrap(), 7);
    }
    #[test]
    fn ims_candidate_official_selector_stays_uncredited() {
        let args = ConformanceArgs {
            subsystem: Some("ims".into()),
            gate: Some("local".into()),
            shard: None,
            replay: None,
            prepare_candidates: false,
            check: true,
        };
        assert!(
            run(&root(), &args)
                .unwrap_err()
                .contains("HUMAN maintainer")
        );
    }
}
