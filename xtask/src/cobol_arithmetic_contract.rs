//! Independently reviewed arithmetic expectations; never infer these from candidate output.
use super::*;

pub(super) fn check(fixture: &Value) -> TaskResult {
    let cases = fixture["cases"]
        .as_array()
        .ok_or("COBOL arithmetic cases are missing")?;
    let row_id = "ibm-enterprise-cobol-6.5-2026-05-31:procedure-statements:0002";
    let expected_contracts = BTreeMap::from([
        (
            "receiver-local-size-error",
            json!(["3639310a", [2], [2], ["GOOD-X", "SMALL-X"]]),
        ),
        (
            "receiver-overflow-without-handler",
            json!(["36310a", [2], [2], ["GOOD-X", "SMALL-X"]]),
        ),
        (
            "corresponding-relative-qualifiers",
            json!(["31310a32300a", [2], [1], ["DST-G.MATCH-X"]]),
        ),
        (
            "corresponding-no-pair-no-op",
            json!(["33340a", [2], [0], []]),
        ),
        (
            "corresponding-numeric-edited-eligible",
            json!(["31310a2b32320a", [2], [2], ["DST-G.GOOD-X", "DST-G.EDIT-X"]]),
        ),
    ]);
    require(
        fixture["schema_version"] == "mainframe-env.cobol-arithmetic-pilot-fixtures@1"
            && fixture["fixture_id"] == "cobol.typed-arithmetic.issue-140-141-v1"
            && fixture["row_id"] == row_id
            && cases.len() == expected_contracts.len()
            && cases.iter().all(|case| {
                case["program_source"]
                    .as_str()
                    .is_some_and(|source| source.contains("ADD"))
                    && expected_contracts
                        .get(case["case_id"].as_str().unwrap_or_default())
                        .is_some_and(|expected| {
                            *expected
                                == json!([
                                    case["expected_output_hex"],
                                    case["expected_typed_plan"]["operation_majors"],
                                    case["expected_typed_plan"]["assignment_counts"],
                                    case["expected_typed_plan"]["assignment_targets"],
                                ])
                        })
                    && case["expected_typed_plan"]["semantic_origins"] == json!(["cobol.add@1"])
                    && case["expected_typed_plan"]["execution_policies"]
                        == json!(["cobol-extend-v2/cobol-numeric-v1/captured-operands-receiver-local-v1/cobol-size-error-v1"])
            })
            && fixture["expectation_authority"]["external_control"]["status"] == "not-run"
            && fixture["expectation_authority"]["external_control"]["coverage_credit"] == 0
            && fixture["expectation_authority"]["external_control"]["licensed_credit"] == 0
            && fixture["comparison_policy"]["normalizable_fields"]
                .as_array()
                .is_some_and(Vec::is_empty)
,
        "COBOL arithmetic independent exact-byte or typed-plan contracts drifted",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../conformance/subsystems/cics/application/cobol/arithmetic-fixtures.json"
        ))
        .unwrap()
    }

    #[test]
    fn reviewed_extend_contract_retains_independent_bytes_and_receiver_plans() {
        check(&fixture()).unwrap();
    }

    #[test]
    fn independent_contract_rejects_output_receiver_and_policy_drift() {
        for (field, wrong) in [
            ("operation_majors", json!([1])),
            ("assignment_counts", json!([1])),
            ("assignment_targets", json!(["SMALL-X", "GOOD-X"])),
            ("semantic_origins", json!(["cobol.subtract@1"])),
            (
                "execution_policies",
                json!([
                    "decimal34-v1/cobol-numeric-v1/captured-operands-receiver-local-v1/cobol-size-error-v1"
                ]),
            ),
            (
                "execution_policies",
                json!([
                    "cobol-compat-v2/cobol-numeric-v1/captured-operands-receiver-local-v1/cobol-size-error-v1"
                ]),
            ),
        ] {
            let mut wrong_fixture = fixture();
            wrong_fixture["cases"][0]["expected_typed_plan"][field] = wrong;
            assert!(check(&wrong_fixture).is_err(), "accepted drift in {field}");
        }
        let mut wrong_output = fixture();
        wrong_output["cases"][0]["expected_output_hex"] = json!("3439310a");
        assert!(check(&wrong_output).is_err());
        let mut missing_cases = fixture();
        missing_cases["cases"] = json!([]);
        assert!(check(&missing_cases).is_err());
    }
}
