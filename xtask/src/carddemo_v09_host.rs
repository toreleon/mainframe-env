//! Candidate-bound CardDemo host comparison for the bounded 0.9 preservation slice.

use super::*;

const COMPARISON_PATH: &str = "conformance/0.9/evidence/carddemo-host-preservation.json";
const CORPUS_INVENTORY: &str = "conformance/0.1.1/inventory/carddemo-corpus.json";

pub(super) fn check(root: &Path) -> TaskResult {
    let candidate_commit = command_text(root, "git", &["rev-parse", "HEAD"])?;
    let candidate_tree = command_text(root, "git", &["rev-parse", "HEAD^{tree}"])?;
    require(
        command_text(
            root,
            "git",
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )?
        .is_empty(),
        "CardDemo 0.9 host comparison requires a clean candidate",
    )?;

    let comparison = json(&root.join(COMPARISON_PATH))?;
    require(
        comparison["schema_version"] == "mainframe-env.carddemo-v09-host-comparison@1"
            && comparison["target_version"] == "0.9.0"
            && comparison["status"] == "comparison-only"
            && comparison["historical_receipt_rewritten"] == false
            && comparison["excluded_credit"]
                == json!([
                    "CardDemo-full",
                    "licensed-differential",
                    "0.9.0-final-candidate"
                ]),
        "CardDemo 0.9 host comparison header changed",
    )?;
    let source_commit = required_text(&comparison["comparison_source"], "commit")?;
    let source_tree = required_text(&comparison["comparison_source"], "tree")?;
    require(
        command_text(
            root,
            "git",
            &["rev-parse", &format!("{source_commit}^{{tree}}")],
        )? == source_tree
            && comparison["comparison_source"]["role"]
                == "bounded-host-observation-not-final-candidate-evidence",
        "CardDemo 0.9 comparison source identity changed",
    )?;

    let mut historical = BTreeMap::new();
    for issue in ["CD-008", "CD-010", "CD-019", "CD-024"] {
        let relative = format!("conformance/0.1.1/evidence/issues/{issue}.json");
        let path = root.join(&relative);
        require(
            file_digest(&path)?
                == required_text(&comparison["historical_evidence_file_sha256"], issue)?,
            &format!("{issue} historical evidence bytes changed"),
        )?;
        let evidence = json(&path)?;
        require(
            evidence["issue"] == issue
                && evidence["status"] == "pass"
                && evidence["derived"] == true,
            &format!("{issue} historical evidence header changed"),
        )?;
        historical.insert(issue, evidence);
    }
    for (issue, receipt_key) in [
        ("CD-008", "host_receipt"),
        ("CD-010", "resource_receipt"),
        ("CD-019", "base_online_receipt"),
        ("CD-024", "db2_receipt"),
    ] {
        let evidence = &historical[issue];
        let receipt = evidence[receipt_key]
            .as_object()
            .ok_or_else(|| format!("{issue} historical receipt is malformed"))?;
        require(
            evidence["evidence_digest"] == canonical_evidence_digest(receipt)?,
            &format!("{issue} historical receipt digest changed"),
        )?;
    }
    let cd008 = &historical["CD-008"];
    let completion = find_completion_commit(
        root,
        "Lower CardDemo embedded host operations",
        &[
            ("CardDemo-Issue", "CD-008=pass"),
            ("Evidence-Digest", required_text(cd008, "evidence_digest")?),
            ("Target-Product", "0.1.1"),
        ],
    )?;
    verify_commit_bound_live_file(
        root,
        &completion,
        "conformance/0.1.1/evidence/issues/CD-008.json",
        &root.join("conformance/0.1.1/evidence/issues/CD-008.json"),
    )?;
    require(
        historical["CD-024"]["db2_receipt"]["sql_include_expansions"] == 9
            && historical["CD-024"]["db2_receipt"]["sql_operations"]["DECLARE"] == 2
            && historical["CD-024"]["db2_receipt"]["sql_operations"]["SELECT"] == 3
            && comparison["sql_delta"]["cause_commit"]
                == "3d1a55b1719e150042467e381fc5f8f609510dd6"
            && comparison["sql_delta"]["historical_operations"] == 20
            && comparison["sql_delta"]["comparison_operations"] == 22
            && comparison["sql_delta"]["opcode_changes"]
                == json!({
                    "SQL.INCLUDE": {"historical": 1, "comparison": 0},
                    "SQL.DECLARE": {"historical": 0, "comparison": 2},
                    "SQL.SELECT": {"historical": 2, "comparison": 3},
                }),
        "CD-024 Db2 preprocessing explanation changed",
    )?;

    let inventory = root.join(CORPUS_INVENTORY);
    let host = verify_carddemo_host_operands_from_env(&inventory)
        .map_err(|problem| problem.to_string())?;
    let host = serde_json::to_value(&host).map_err(|error| error.to_string())?;
    require(
        canonical_evidence_digest(
            comparison["expected_host_receipt"]
                .as_object()
                .ok_or("0.9 comparison host receipt is malformed")?,
        )? == comparison["expected_host_receipt_sha256"],
        "0.9 comparison host digest changed",
    )?;
    compare_host_transition(
        &cd008["host_receipt"],
        &comparison["expected_host_receipt"],
        &host,
    )?;
    require(
        host["corpus_commit"] == comparison["corpus"]["commit"],
        "CardDemo corpus commit differs from the 0.9 comparison",
    )?;

    let resources =
        verify_carddemo_resources_from_env(&inventory).map_err(|problem| problem.to_string())?;
    let resources = serde_json::to_value(&resources).map_err(|error| error.to_string())?;
    require(
        resources == historical["CD-010"]["resource_receipt"],
        "CardDemo transaction, map, or resource definition differs from CD-010",
    )?;
    let online =
        verify_carddemo_base_online_from_env(&inventory).map_err(|problem| problem.to_string())?;
    let online = serde_json::to_value(&online).map_err(|error| error.to_string())?;
    require(
        online == historical["CD-019"]["base_online_receipt"],
        "CardDemo transaction path or installed map differs from CD-019",
    )?;

    let bounded_receipt = json!({
        "schema_version": "mainframe-env.carddemo-v09-host-candidate@1",
        "status": "bounded-pass",
        "candidate_commit": candidate_commit,
        "candidate_tree": candidate_tree,
        "comparison_source_commit": source_commit,
        "comparison_source_tree": source_tree,
        "corpus_commit": host["corpus_commit"],
        "corpus_tree": comparison["corpus"]["tree"],
        "historical_cd008_completion": completion,
        "programs_checked": host["programs_checked"],
        "cics_operations": host["cics_operations"],
        "host_shape_sha256": host["host_shape_sha256"],
        "resource_sha256": resources["resource_sha256"],
        "online_journey_shape_sha256": online["journey_shape_sha256"],
        "online_journeys": online["journeys_passed"],
        "sql_delta": comparison["sql_delta"],
        "excluded_credit": comparison["excluded_credit"],
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&bounded_receipt).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn required_text<'a>(value: &'a Value, name: &str) -> TaskResult<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("CardDemo 0.9 comparison field {name} is missing"))
}

fn compare_host_transition(historical: &Value, expected: &Value, current: &Value) -> TaskResult {
    require(
        current == expected,
        "0.9 CardDemo host token receipt differs from comparison",
    )?;
    for field in [
        "schema_version",
        "status",
        "corpus_commit",
        "programs_checked",
        "cics_operations",
        "dli_operations",
        "mq_calls",
        "typed_oracle_cases",
    ] {
        require(
            historical[field] == current[field],
            &format!("CardDemo host preservation changed {field}"),
        )?;
    }
    require(
        historical["sql_operations"] == 20 && current["sql_operations"] == 22,
        "CardDemo SQL operation delta differs from CD-008 to 0.9",
    )?;
    let old = historical["opcodes"]
        .as_object()
        .ok_or("CD-008 host opcodes are malformed")?;
    let now = current["opcodes"]
        .as_object()
        .ok_or("0.9 host opcodes are malformed")?;
    let keys = old.keys().chain(now.keys()).collect::<BTreeSet<_>>();
    for key in keys {
        let before = old.get(key).and_then(Value::as_u64).unwrap_or(0);
        let after = now.get(key).and_then(Value::as_u64).unwrap_or(0);
        let prescribed = match key.as_str() {
            "SQL.INCLUDE" => Some((1, 0)),
            "SQL.DECLARE" => Some((0, 2)),
            "SQL.SELECT" => Some((2, 3)),
            _ => None,
        };
        require(
            prescribed.map_or(before == after, |pair| pair == (before, after)),
            &format!("CardDemo host opcode {key} changed outside the SQL comparison"),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_delta_cannot_mask_cics_or_other_sql_drift() {
        let comparison: Value = serde_json::from_str(include_str!(
            "../../conformance/0.9/evidence/carddemo-host-preservation.json"
        ))
        .unwrap();
        let expected = &comparison["expected_host_receipt"];
        let mut historical = json!(expected);
        historical["sql_operations"] = json!(20);
        let opcodes = historical["opcodes"].as_object_mut().unwrap();
        opcodes.remove("SQL.DECLARE");
        opcodes.insert("SQL.INCLUDE".into(), json!(1));
        opcodes.insert("SQL.SELECT".into(), json!(2));
        assert!(compare_host_transition(&historical, expected, expected).is_ok());

        let mut cics_drift = historical.clone();
        cics_drift["opcodes"]["CICS.READ"] = json!(28);
        assert!(compare_host_transition(&cics_drift, expected, expected).is_err());
        let mut sql_drift = historical.clone();
        sql_drift["opcodes"]["SQL.DELETE"] = json!(4);
        assert!(compare_host_transition(&sql_drift, expected, expected).is_err());
        let mut current_drift = json!(expected);
        current_drift["typed_operands"] = json!(0);
        assert!(compare_host_transition(&historical, expected, &current_drift).is_err());
    }
}
