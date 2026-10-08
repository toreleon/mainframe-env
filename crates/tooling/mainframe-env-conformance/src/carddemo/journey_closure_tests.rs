//! Focused tests for the private CardDemo full-receipt closure helper.
//! Synthetic observations exercise harness admission only, with no product credit.
use super::*;
use serde_json::{Value, json};

fn manifest() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../conformance/profiles/carddemo/workloads/carddemo-journeys.json"
    )))
    .unwrap()
}

// Test input for a complete closure. Product runners must obtain each token
// from an executed assertion; they must never enumerate this manifest as output.
fn complete_observations(manifest: &Value) -> Vec<(String, Vec<String>)> {
    manifest["journeys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|journey| {
            (
                journey["id"].as_str().unwrap().to_owned(),
                journey["observations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|observation| observation.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn journey_closure_complete_observations_use_manifest_denominator() {
    let manifest = manifest();
    let observed = complete_observations(&manifest);
    assert_eq!(
        close_carddemo_journeys(&manifest, &observed).unwrap(),
        manifest["journeys"].as_array().unwrap().len()
    );
}

#[test]
fn journey_closure_planned_manifest_alone_earns_no_execution() {
    let manifest = manifest();
    assert_eq!(manifest["status"], "planned_not_executed");
    assert!(close_carddemo_journeys(&manifest, &[]).is_err());
}

#[test]
fn journey_closure_omitted_journey_refuses_even_when_other_helpers_pass() {
    let manifest = manifest();
    let complete = complete_observations(&manifest);
    for index in 0..complete.len() {
        let mut observed = complete.clone();
        let removed = observed.remove(index);
        assert!(
            close_carddemo_journeys(&manifest, &observed).is_err(),
            "omitted {} still earned full closure",
            removed.0
        );
    }
}

#[test]
fn journey_closure_duplicate_cannot_replace_omitted_journey() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed[1] = observed[0].clone();
    assert_eq!(
        observed.len(),
        manifest["journeys"].as_array().unwrap().len()
    );
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_identical_duplicate_refuses() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed.push(observed[0].clone());
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_unknown_journey_refuses() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed[0].0 = "CD.J99".into();
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_missing_any_mandatory_assertion_refuses() {
    let manifest = manifest();
    let complete = complete_observations(&manifest);
    for (journey_index, (journey, observations)) in complete.iter().enumerate() {
        for observation_index in 0..observations.len() {
            let mut observed = complete.clone();
            let missing = observed[journey_index].1.remove(observation_index);
            assert!(
                close_carddemo_journeys(&manifest, &observed).is_err(),
                "missing {journey}/{missing} still earned closure"
            );
        }
    }
}

#[test]
fn journey_closure_skipped_route_with_empty_observations_refuses() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed
        .iter_mut()
        .find(|item| item.0 == "CD.J06")
        .unwrap()
        .1
        .clear();
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_one_missing_assertion_cannot_hide_behind_success_count() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    let transaction = observed.iter_mut().find(|item| item.0 == "CD.J06").unwrap();
    transaction
        .1
        .retain(|observation| observation != "duplicate condition");
    assert_eq!(
        observed.len(),
        manifest["journeys"].as_array().unwrap().len()
    );
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_duplicate_observation_cannot_replace_missing_assertion() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed[0].1[1] = observed[0].1[0].clone();
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_unknown_observation_refuses() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed[0].1.push("unregistered generic success".into());
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_observation_from_another_journey_refuses() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed[0].1 = observed[1].1.clone();
    assert!(close_carddemo_journeys(&manifest, &observed).is_err());
}

#[test]
fn journey_closure_order_does_not_change_exact_membership() {
    let manifest = manifest();
    let mut observed = complete_observations(&manifest);
    observed.reverse();
    for (_, observations) in &mut observed {
        observations.reverse();
    }
    assert_eq!(
        close_carddemo_journeys(&manifest, &observed).unwrap(),
        manifest["journeys"].as_array().unwrap().len()
    );
}

#[test]
fn journey_closure_small_manifest_does_not_report_literal_twenty() {
    let manifest = json!({
        "schema_version": "mainframe-env.carddemo-journeys@1",
        "status": "planned_not_executed",
        "journeys": [{
            "id": "CD.J01", "profile": "carddemo-base-online",
            "name": "harness-only subset", "observations": ["package identity"]
        }]
    });
    assert_eq!(
        close_carddemo_journeys(
            &manifest,
            &[("CD.J01".into(), vec!["package identity".into()])]
        )
        .unwrap(),
        1
    );
}

fn issue_matrix() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../conformance/profiles/carddemo/inventory/carddemo-gap-matrix.json"
    )))
    .unwrap()
}

fn required_issues(matrix: &Value) -> Vec<&Value> {
    // The manager's surviving denominator excludes aggregate certification CD-027.
    (1..=26)
        .map(|number| {
            let id = format!("CD-{number:03}");
            matrix["issues"]
                .as_array()
                .unwrap()
                .iter()
                .find(|issue| issue["id"] == id)
                .unwrap()
        })
        .collect()
}

fn complete_issue_observations(matrix: &Value) -> Vec<(String, Vec<String>)> {
    required_issues(matrix)
        .into_iter()
        .map(|issue| {
            (
                issue["id"].as_str().unwrap().to_owned(),
                issue["acceptance"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|acceptance| acceptance.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn issue_closure_uses_surviving_matrix_denominator_without_cd027() {
    let matrix = issue_matrix();
    let observed = complete_issue_observations(&matrix);
    assert!(matrix["issues"].as_array().unwrap().len() > observed.len());
    assert_eq!(
        close_carddemo_issues(&matrix, &observed).unwrap(),
        required_issues(&matrix).len()
    );
}

#[test]
fn issue_closure_open_matrix_alone_earns_no_execution() {
    let matrix = issue_matrix();
    assert_eq!(matrix["status"], "open");
    assert!(close_carddemo_issues(&matrix, &[]).is_err());
}

#[test]
fn issue_closure_missing_issue_refuses() {
    let matrix = issue_matrix();
    let complete = complete_issue_observations(&matrix);
    for index in 0..complete.len() {
        let mut observed = complete.clone();
        let missing = observed.remove(index);
        assert!(
            close_carddemo_issues(&matrix, &observed).is_err(),
            "missing {} still earned issue closure",
            missing.0
        );
    }
}

#[test]
fn issue_closure_duplicate_cannot_replace_dependency() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    observed[0] = observed[1].clone();
    assert_eq!(observed.len(), required_issues(&matrix).len());
    assert!(close_carddemo_issues(&matrix, &observed).is_err());
}

#[test]
fn issue_closure_identical_duplicate_refuses() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    observed.push(observed[0].clone());
    assert!(close_carddemo_issues(&matrix, &observed).is_err());
}

#[test]
fn issue_closure_unknown_issue_refuses() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    observed[0].0 = "CD-999".into();
    assert!(close_carddemo_issues(&matrix, &observed).is_err());
}

#[test]
fn issue_closure_cd027_cannot_replace_surviving_issue() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    let aggregate = matrix["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["id"] == "CD-027")
        .unwrap();
    observed[25] = (
        "CD-027".into(),
        aggregate["acceptance"]
            .as_array()
            .unwrap()
            .iter()
            .map(|acceptance| acceptance.as_str().unwrap().to_owned())
            .collect(),
    );
    assert!(close_carddemo_issues(&matrix, &observed).is_err());
}

#[test]
fn issue_closure_missing_any_acceptance_refuses() {
    let matrix = issue_matrix();
    let complete = complete_issue_observations(&matrix);
    for (issue_index, (issue, acceptance)) in complete.iter().enumerate() {
        for acceptance_index in 0..acceptance.len() {
            let mut observed = complete.clone();
            let missing = observed[issue_index].1.remove(acceptance_index);
            assert!(
                close_carddemo_issues(&matrix, &observed).is_err(),
                "missing {issue}/{missing} still earned issue closure"
            );
        }
    }
}

#[test]
fn issue_closure_duplicate_acceptance_cannot_replace_missing_check() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    observed[0].1[1] = observed[0].1[0].clone();
    assert!(close_carddemo_issues(&matrix, &observed).is_err());
}

#[test]
fn issue_closure_unknown_acceptance_refuses() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    observed[0].1.push("static mapping is enough".into());
    assert!(close_carddemo_issues(&matrix, &observed).is_err());
}

#[test]
fn issue_closure_order_does_not_change_exact_membership() {
    let matrix = issue_matrix();
    let mut observed = complete_issue_observations(&matrix);
    observed.reverse();
    for (_, acceptance) in &mut observed {
        acceptance.reverse();
    }
    assert_eq!(
        close_carddemo_issues(&matrix, &observed).unwrap(),
        required_issues(&matrix).len()
    );
}

#[test]
fn journey_closure_duplicate_expected_identity_refuses() {
    let mut manifest = manifest();
    let duplicate = manifest["journeys"][0].clone();
    manifest["journeys"].as_array_mut().unwrap().push(duplicate);
    assert!(close_carddemo_journeys(&manifest, &complete_observations(&manifest)).is_err());
}

#[test]
fn journey_closure_malformed_authority_fields_refuse() {
    for (field, value) in [
        (
            "schema_version",
            json!("mainframe-env.carddemo-journeys@99"),
        ),
        ("status", json!("pass")),
        ("journeys", json!(null)),
    ] {
        let mut authority = manifest();
        let observed = complete_observations(&authority);
        authority[field] = value;
        assert!(close_carddemo_journeys(&authority, &observed).is_err());
    }
    for (field, value) in [
        ("id", json!("CD.J99")),
        ("profile", json!(false)),
        ("name", json!("")),
        ("observations", json!([])),
        ("observations", json!([42])),
    ] {
        let mut authority = manifest();
        let observed = complete_observations(&authority);
        authority["journeys"][0][field] = value;
        assert!(close_carddemo_journeys(&authority, &observed).is_err());
    }
}

#[test]
fn journey_closure_duplicate_expected_requirement_refuses() {
    let mut authority = manifest();
    let observed = complete_observations(&authority);
    authority["journeys"][0]["observations"][1] =
        authority["journeys"][0]["observations"][0].clone();
    assert!(close_carddemo_journeys(&authority, &observed).is_err());
}

#[test]
fn journey_closure_expected_row_bound_refuses_before_membership() {
    let mut authority = manifest();
    let row = authority["journeys"][0].clone();
    authority["journeys"] = json!(vec![row; journey_closure::MAX_ROWS + 1]);
    let problem = close_carddemo_journeys(&authority, &[]).unwrap_err();
    assert!(problem.detail.contains("row count bound"));
}

#[test]
fn journey_closure_expected_requirement_bound_refuses_before_duplicates() {
    let mut authority = manifest();
    authority["journeys"][0]["observations"] = json!(vec![
        "independent expectation";
        journey_closure::MAX_REQUIREMENTS + 1
    ]);
    let problem = close_carddemo_journeys(&authority, &[]).unwrap_err();
    assert!(problem.detail.contains("requirement count bound"));
}

#[test]
fn journey_closure_expected_text_boundary_and_excess() {
    let mut authority = manifest();
    authority["journeys"][0]["name"] = json!("x".repeat(journey_closure::MAX_TEXT_BYTES));
    assert!(close_carddemo_journeys(&authority, &complete_observations(&authority)).is_ok());
    authority["journeys"][0]["name"] = json!("x".repeat(journey_closure::MAX_TEXT_BYTES + 1));
    assert!(close_carddemo_journeys(&authority, &complete_observations(&authority)).is_err());
}

#[test]
fn journey_closure_expected_aggregate_text_excess_refuses() {
    let mut authority = manifest();
    for row in authority["journeys"].as_array_mut().unwrap() {
        row["name"] = json!("n".repeat(journey_closure::MAX_TEXT_BYTES));
        row["profile"] = json!("p".repeat(journey_closure::MAX_TEXT_BYTES));
    }
    let problem = close_carddemo_journeys(&authority, &[]).unwrap_err();
    assert!(problem.detail.contains("aggregate"));
}

#[test]
fn journey_closure_observed_bounds_refuse() {
    let authority = manifest();
    let mut observed = complete_observations(&authority);
    observed[0].1[0] = "x".repeat(journey_closure::MAX_TEXT_BYTES + 1);
    assert!(close_carddemo_journeys(&authority, &observed).is_err());
    let mut observed = complete_observations(&authority);
    observed[0].1 = vec!["expected".into(); journey_closure::MAX_REQUIREMENTS + 1];
    assert!(close_carddemo_journeys(&authority, &observed).is_err());
    let observed = vec![("CD.J01".into(), vec!["actual".into()]); journey_closure::MAX_ROWS + 1];
    let problem = close_carddemo_journeys(&authority, &observed).unwrap_err();
    assert!(problem.detail.contains("row count bound"));
}

#[test]
fn issue_closure_unknown_self_duplicate_and_aggregate_dependencies_refuse() {
    for dependencies in [
        json!(["CD-999"]),
        json!(["CD-002"]),
        json!(["CD-001", "CD-001"]),
        json!(["CD-027"]),
        json!(false),
    ] {
        let mut authority = issue_matrix();
        let observed = complete_issue_observations(&authority);
        authority["issues"][1]["depends_on"] = dependencies;
        assert!(close_carddemo_issues(&authority, &observed).is_err());
    }
}

#[test]
fn issue_closure_cyclic_dependencies_refuse() {
    let mut authority = issue_matrix();
    let observed = complete_issue_observations(&authority);
    authority["issues"][0]["depends_on"] = json!(["CD-002"]);
    assert!(close_carddemo_issues(&authority, &observed).is_err());
}

#[test]
fn issue_closure_smaller_expected_issue_selection_refuses() {
    let mut authority = issue_matrix();
    let mut observed = complete_issue_observations(&authority);
    authority["issues"].as_array_mut().unwrap().remove(25);
    observed.remove(25);
    assert!(close_carddemo_issues(&authority, &observed).is_err());
}

#[test]
fn issue_closure_duplicate_expected_identity_and_acceptance_refuse() {
    let mut authority = issue_matrix();
    let observed = complete_issue_observations(&authority);
    authority["issues"][1]["id"] = json!("CD-001");
    assert!(close_carddemo_issues(&authority, &observed).is_err());
    let mut authority = issue_matrix();
    authority["issues"][0]["acceptance"][1] = authority["issues"][0]["acceptance"][0].clone();
    assert!(close_carddemo_issues(&authority, &observed).is_err());
}

#[test]
fn issue_closure_invalid_aggregate_metadata_still_refuses() {
    let mut authority = issue_matrix();
    let observed = complete_issue_observations(&authority);
    authority["issues"][26]["acceptance"] = json!([]);
    assert!(close_carddemo_issues(&authority, &observed).is_err());
}

#[test]
fn issue_closure_malformed_version_status_and_requirements_refuse() {
    for (field, value) in [
        (
            "schema_version",
            json!("mainframe-env.carddemo-gap-matrix@99"),
        ),
        ("status", json!("pass")),
        ("commit_policy", json!(null)),
    ] {
        let mut authority = issue_matrix();
        let observed = complete_issue_observations(&authority);
        authority[field] = value;
        assert!(close_carddemo_issues(&authority, &observed).is_err());
    }
    let mut authority = issue_matrix();
    let observed = complete_issue_observations(&authority);
    authority["issues"][0]["acceptance"] = json!([true]);
    assert!(close_carddemo_issues(&authority, &observed).is_err());
}

fn comparison_problem() -> CorpusProblem {
    CorpusProblem::new("test.independent_expectation", "observed bytes differ")
}

#[test]
fn journey_closure_compare_produces_token_only_on_matching_actual_bytes() {
    let mut observed = RouteObservations::default();
    let actual: &[u8] = b"actual public-route bytes";
    let expected = b"actual public-route bytes";
    observed
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "program artifacts",
            actual != expected,
            || panic!("passing comparison evaluated its lazy error"),
        )
        .unwrap();
    assert_eq!(
        observed.journeys(),
        [("CD.J01".into(), vec!["program artifacts".into()])]
    );
    let before = observed.journeys().to_vec();
    let problem = observed
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "map catalog",
            actual != b"independently wrong expected bytes",
            || Ok(comparison_problem()),
        )
        .unwrap_err();
    assert_eq!(problem, comparison_problem());
    assert_eq!(observed.journeys(), before);
}

#[test]
fn journey_closure_skipping_compare_leaves_no_token() {
    let authority = manifest();
    let observed = RouteObservations::default();
    assert!(observed.journeys().is_empty());
    assert!(close_carddemo_journeys(&authority, observed.journeys()).is_err());
}

#[test]
fn issue_closure_compare_keeps_issue_and_journey_tokens_separate() {
    let mut observed = RouteObservations::default();
    let actual = b"selected route bytes";
    let expected = b"selected route bytes";
    observed
        .compare(
            journey_closure::AuthorityKind::Issue,
            "CD-001",
            "test-only independent acceptance",
            actual != expected,
            || Ok(comparison_problem()),
        )
        .unwrap();
    assert!(observed.journeys().is_empty());
    assert_eq!(observed.issues().len(), 1);
    assert!(close_carddemo_issues(&issue_matrix(), observed.issues()).is_err());
}

#[test]
fn journey_closure_duplicate_compare_refuses_without_mutation() {
    let mut observed = RouteObservations::default();
    observed
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "program artifacts",
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    let before = observed.journeys().to_vec();
    assert!(
        observed
            .compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J01",
                "program artifacts",
                false,
                || Ok(comparison_problem()),
            )
            .is_err()
    );
    assert_eq!(observed.journeys(), before);
}

struct AuthorityFixture(std::path::PathBuf);

impl AuthorityFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "carddemo-closure-authority-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("workloads")).unwrap();
        std::fs::create_dir_all(root.join("inventory")).unwrap();
        std::fs::write(root.join("inventory/carddemo-corpus.json"), b"{}").unwrap();
        std::fs::write(
            root.join("workloads/carddemo-journeys.json"),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../conformance/profiles/carddemo/workloads/carddemo-journeys.json"
            )),
        )
        .unwrap();
        std::fs::write(
            root.join("inventory/carddemo-gap-matrix.json"),
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../conformance/profiles/carddemo/inventory/carddemo-gap-matrix.json"
            )),
        )
        .unwrap();
        Self(root)
    }

    fn inventory(&self) -> std::path::PathBuf {
        self.0.join("inventory/carddemo-corpus.json")
    }
}

impl Drop for AuthorityFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn journey_closure_runtime_loads_both_exact_owning_inputs_but_no_static_credit() {
    let fixture = AuthorityFixture::new();
    let authority = journey_closure::ClosureAuthority::load(&fixture.inventory()).unwrap();
    assert!(authority.finish(&RouteObservations::default()).is_err());
}

#[test]
fn journey_closure_runtime_cannot_admit_smaller_or_modified_manifest() {
    let fixture = AuthorityFixture::new();
    let mut authority = manifest();
    authority["journeys"].as_array_mut().unwrap().pop();
    std::fs::write(
        fixture.0.join("workloads/carddemo-journeys.json"),
        serde_json::to_vec(&authority).unwrap(),
    )
    .unwrap();
    assert!(journey_closure::ClosureAuthority::load(&fixture.inventory()).is_err());
}

#[test]
fn issue_closure_runtime_cannot_admit_smaller_or_modified_matrix() {
    let fixture = AuthorityFixture::new();
    let mut authority = issue_matrix();
    authority["issues"].as_array_mut().unwrap().remove(25);
    std::fs::write(
        fixture.0.join("inventory/carddemo-gap-matrix.json"),
        serde_json::to_vec(&authority).unwrap(),
    )
    .unwrap();
    assert!(journey_closure::ClosureAuthority::load(&fixture.inventory()).is_err());
}

#[test]
fn journey_closure_runtime_missing_or_oversized_authority_refuses() {
    let fixture = AuthorityFixture::new();
    std::fs::write(
        fixture.0.join("workloads/carddemo-journeys.json"),
        vec![b'x'; journey_closure::MAX_AUTHORITY_BYTES + 1],
    )
    .unwrap();
    assert!(journey_closure::ClosureAuthority::load(&fixture.inventory()).is_err());
    std::fs::remove_file(fixture.0.join("workloads/carddemo-journeys.json")).unwrap();
    assert!(journey_closure::ClosureAuthority::load(&fixture.inventory()).is_err());
}

#[test]
fn journey_closure_runtime_requires_real_inventory_location() {
    let fixture = AuthorityFixture::new();
    let nonexistent = fixture.0.join("unrelated/carddemo-corpus.json");
    assert!(journey_closure::ClosureAuthority::load(&nonexistent).is_err());
    let wrong_name = fixture.0.join("inventory/caller-selected.json");
    std::fs::write(&wrong_name, b"{}").unwrap();
    assert!(journey_closure::ClosureAuthority::load(&wrong_name).is_err());
}

#[test]
fn journey_closure_observation_merge_preserves_partial_comparisons() {
    let mut observed = RouteObservations::default();
    observed
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "program artifacts",
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    let mut later = RouteObservations::default();
    later
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "map catalog",
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    observed.extend(later).unwrap();
    assert_eq!(
        observed.journeys(),
        [(
            "CD.J01".into(),
            vec!["program artifacts".into(), "map catalog".into()]
        )]
    );
    assert!(close_carddemo_journeys(&manifest(), observed.journeys()).is_err());
}

#[test]
fn issue_closure_merge_refusal_preserves_both_observation_families() {
    let mut observed = RouteObservations::default();
    observed
        .compare(
            journey_closure::AuthorityKind::Issue,
            "CD-001",
            "test-only acceptance",
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    let before = observed.issues().to_vec();
    let mut later = RouteObservations::default();
    later
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            "program artifacts",
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    later
        .compare(
            journey_closure::AuthorityKind::Issue,
            "CD-001",
            "test-only acceptance",
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    assert!(observed.extend(later).is_err());
    assert!(observed.journeys().is_empty());
    assert_eq!(observed.issues(), before);
}

#[test]
fn journey_closure_comparison_text_boundary_and_excess_do_not_partially_record() {
    let mut observed = RouteObservations::default();
    let accepted = "x".repeat(journey_closure::MAX_TEXT_BYTES);
    observed
        .compare(
            journey_closure::AuthorityKind::Journey,
            "CD.J01",
            &accepted,
            false,
            || Ok(comparison_problem()),
        )
        .unwrap();
    let before = observed.journeys().to_vec();
    let excess = "x".repeat(journey_closure::MAX_TEXT_BYTES + 1);
    assert!(
        observed
            .compare(
                journey_closure::AuthorityKind::Journey,
                "CD.J01",
                &excess,
                false,
                || Ok(comparison_problem()),
            )
            .is_err()
    );
    assert_eq!(observed.journeys(), before);
}
