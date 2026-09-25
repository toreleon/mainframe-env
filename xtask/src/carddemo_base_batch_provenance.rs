use crate::{TaskResult, canonical_evidence_digest, json, require, validate_schema_instance};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

const V1_PATH: &str = "conformance/0.8/evidence/carddemo-base-batch.json";
const V2_PATH: &str = "conformance/0.8/evidence/carddemo-base-batch@2.json";
const V1_SCHEMA: &str = "conformance/0.8/schemas/carddemo-base-batch-evidence.schema.json";
const V2_SCHEMA: &str = "conformance/0.8/schemas/carddemo-base-batch-evidence@2.schema.json";

#[derive(Debug, Default)]
pub(super) struct CreditSummary {
    pub total: usize,
    pub self_recorded: usize,
    pub unattributed: usize,
    pub licensed_pending: usize,
    pub conformance: usize,
}

impl CreditSummary {
    pub fn print(&self) {
        println!(
            "conformance credit: {} of {} assertions ({} self-recorded, {} unattributed, {} licensed pending)",
            self.conformance,
            self.total,
            self.self_recorded,
            self.unattributed,
            self.licensed_pending
        );
    }
}

fn correctness_paths(receipt: &Value) -> TaskResult<BTreeSet<String>> {
    let fields = receipt
        .as_object()
        .ok_or("CardDemo base-batch receipt is malformed")?;
    let mut paths = BTreeSet::new();
    for (field, value) in fields {
        if matches!(field.as_str(), "schema_version" | "corpus_commit") {
            continue;
        }
        if matches!(field.as_str(), "dataset_sha256" | "spool_sha256") {
            let digests = value
                .as_object()
                .ok_or_else(|| format!("{field} is malformed"))?;
            for name in digests.keys() {
                let escaped = name.replace('~', "~0").replace('/', "~1");
                paths.insert(format!("/{field}/{escaped}"));
            }
        } else {
            require(
                !value.is_object() && !value.is_array(),
                &format!("{field} is not a correctness leaf"),
            )?;
            paths.insert(format!("/{field}"));
        }
    }
    Ok(paths)
}

pub(super) fn validate_carddemo_provenance(v2: &Value, v1: &Value) -> TaskResult<CreditSummary> {
    require(
        v2["schema_version"] == "mainframe-env.carddemo-base-batch-version-evidence@2"
            && v2["target_version"] == "0.8.0"
            && v2["supersedes"] == V1_PATH
            && v2["historical_receipt_rewritten"] == false,
        "0.8 CardDemo v2 evidence header is invalid",
    )?;
    let v1_receipt = v1["receipt"]
        .as_object()
        .ok_or("0.8 CardDemo v1 receipt is malformed")?;
    let v1_digest = canonical_evidence_digest(v1_receipt)?;
    require(
        v1["evidence_digest"].as_str() == Some(&v1_digest),
        "0.8 CardDemo v1 evidence digest differs",
    )?;
    let receipt = &v2["receipt"];
    let v2_receipt = receipt
        .as_object()
        .ok_or("0.8 CardDemo v2 receipt is malformed")?;
    let digest = canonical_evidence_digest(v2_receipt)?;
    require(
        v2["evidence_digest"].as_str() == Some(&digest),
        "0.8 CardDemo v2 evidence digest differs",
    )?;
    let expected_paths = correctness_paths(receipt)?;
    let entries = v2["expected_value_provenance"]
        .as_array()
        .ok_or("expected_value_provenance is missing")?;
    let mut observed = BTreeSet::new();
    let mut summary = CreditSummary {
        total: expected_paths.len(),
        ..CreditSummary::default()
    };
    for entry in entries {
        let path = entry["assertion_path"]
            .as_str()
            .ok_or("provenance assertion_path is missing")?;
        require(
            expected_paths.contains(path),
            &format!("unexpected provenance for {path}"),
        )?;
        require(
            observed.insert(path.to_string()),
            &format!("duplicate provenance for {path}"),
        )?;
        let source = entry["source"]
            .as_str()
            .ok_or_else(|| format!("missing source for {path}"))?;
        match source {
            "self-recorded" => {
                require(
                    entry["source_id"] == format!("{V1_PATH}#{v1_digest}")
                        && entry["source_digest"] == v1_digest
                        && entry["source_locator"] == format!("/receipt{path}")
                        && v1.pointer(&format!("/receipt{path}")) == receipt.pointer(path),
                    &format!("self-recorded source does not match v1 for {path}"),
                )?;
                summary.self_recorded += 1;
            }
            "independent-reference" | "customer-captured" => summary.conformance += 1,
            "licensed-ibm" => summary.licensed_pending += 1,
            _ => return Err(format!("unknown provenance source {source} for {path}")),
        }
    }
    if let Some(path) = expected_paths.difference(&observed).next() {
        return Err(format!("missing provenance for {path}"));
    }
    Ok(summary)
}

pub(super) fn read_carddemo_evidence(
    root: &Path,
    historical_receipt: &Map<String, Value>,
) -> TaskResult<(Value, CreditSummary)> {
    let v1_path = root.join(V1_PATH);
    let v2_path = root.join(V2_PATH);
    if !v1_path.is_file() {
        require(
            !v2_path.is_file(),
            "CardDemo v2 evidence requires the v1 source artifact",
        )?;
        let receipt = Value::Object(historical_receipt.clone());
        let total = correctness_paths(&receipt)?.len();
        return Ok((
            receipt,
            CreditSummary {
                total,
                unattributed: total,
                ..CreditSummary::default()
            },
        ));
    }
    let v1 = json(&v1_path)?;
    validate_schema_instance(&json(&root.join(V1_SCHEMA))?, &v1, &v1_path)?;
    let v1_receipt = v1["receipt"]
        .as_object()
        .ok_or("0.8 CardDemo v1 receipt is malformed")?;
    let v1_digest = canonical_evidence_digest(v1_receipt)?;
    require(
        v1["evidence_digest"].as_str() == Some(&v1_digest),
        "0.8 CardDemo v1 evidence digest differs",
    )?;
    if !v2_path.is_file() {
        let receipt = Value::Object(v1_receipt.clone());
        let total = correctness_paths(&receipt)?.len();
        return Ok((
            receipt,
            CreditSummary {
                total,
                unattributed: total,
                ..CreditSummary::default()
            },
        ));
    }
    let v2 = json(&v2_path)?;
    validate_schema_instance(&json(&root.join(V2_SCHEMA))?, &v2, &v2_path)?;
    let summary = validate_carddemo_provenance(&v2, &v1)?;
    Ok((v2["receipt"].clone(), summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> (Value, Value) {
        let receipt = json!({
            "schema_version": "mainframe-env.carddemo-base-batch-receipt@1",
            "status": "pass", "corpus_commit": "0000000000000000000000000000000000000000",
            "business_date": "2022-07-06", "journeys_passed": 3,
            "initialization_jobs": 12, "operational_jobs": 9, "cics_file_controls": 2,
            "internal_submissions": 1, "warm_restart_controls": 1,
            "rollback_controls": 1, "cancellation_controls": 1,
            "tranrept_selected_records": 312, "tranrept_report_records": 519,
            "dataset_sha256": {"A/B~C": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
            "spool_sha256": {"JOB": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
            "journey_shape_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        });
        let digest = crate::canonical_evidence_digest(receipt.as_object().unwrap()).unwrap();
        let v1 = json!({
            "schema_version": "mainframe-env.carddemo-base-batch-version-evidence@1",
            "target_version": "0.8.0",
            "supersedes": "conformance/0.1.1/evidence/issues/CD-023.json",
            "historical_receipt_rewritten": false,
            "transition_justification": ["fixture"],
            "evidence_digest": digest,
            "receipt": receipt
        });
        let paths = [
            "/status",
            "/business_date",
            "/journeys_passed",
            "/initialization_jobs",
            "/operational_jobs",
            "/cics_file_controls",
            "/internal_submissions",
            "/warm_restart_controls",
            "/rollback_controls",
            "/cancellation_controls",
            "/tranrept_selected_records",
            "/tranrept_report_records",
            "/dataset_sha256/A~1B~0C",
            "/spool_sha256/JOB",
            "/journey_shape_sha256",
        ];
        let provenance: Vec<Value> = paths
            .iter()
            .map(|path| {
                json!({
                    "assertion_path": path,
                    "source": "self-recorded",
                    "source_id": format!("{V1_PATH}#{digest}"),
                    "source_digest": digest,
                    "source_locator": format!("/receipt{path}")
                })
            })
            .collect();
        let v2 = json!({
            "schema_version": "mainframe-env.carddemo-base-batch-version-evidence@2",
            "target_version": "0.8.0",
            "supersedes": V1_PATH,
            "historical_receipt_rewritten": false,
            "transition_justification": ["fixture"],
            "evidence_digest": digest,
            "receipt": receipt,
            "expected_value_provenance": provenance
        });
        (v1, v2)
    }

    #[test]
    fn carddemo_provenance_all_self_recorded_is_regression_only() {
        let (v1, v2) = fixture();
        let summary = validate_carddemo_provenance(&v2, &v1).unwrap();
        assert_eq!(summary.total, 15);
        assert_eq!(summary.self_recorded, 15);
        assert_eq!(summary.conformance, 0);
    }

    #[test]
    fn carddemo_provenance_missing_digest_is_precise_failure() {
        let (v1, mut v2) = fixture();
        v2["expected_value_provenance"]
            .as_array_mut()
            .unwrap()
            .retain(|entry| entry["assertion_path"] != "/dataset_sha256/A~1B~0C");
        let error = validate_carddemo_provenance(&v2, &v1).unwrap_err();
        assert!(
            error.contains("missing provenance for /dataset_sha256/A~1B~0C"),
            "{error}"
        );
    }

    #[test]
    fn carddemo_provenance_unknown_source_fails() {
        let (v1, mut v2) = fixture();
        v2["expected_value_provenance"][0]["source"] = json!("unspecified");
        assert!(validate_carddemo_provenance(&v2, &v1).is_err());
    }

    #[test]
    fn carddemo_provenance_editable_credit_is_schema_invalid() {
        let (_, mut v2) = fixture();
        v2["expected_value_provenance"][0]["credit"] = json!(1);
        let root = crate::repository_root().unwrap();
        let schema = crate::json(
            &root.join("conformance/0.8/schemas/carddemo-base-batch-evidence@2.schema.json"),
        )
        .unwrap();
        assert!(
            crate::validate_schema_instance(&schema, &v2, std::path::Path::new("fixture-v2.json"))
                .is_err()
        );
    }

    #[test]
    fn carddemo_provenance_reader_keeps_v1_unattributed_and_prefers_v2() {
        let (v1, v2) = fixture();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "carddemo-provenance-{}-{nonce}",
            std::process::id()
        ));
        let repository = crate::repository_root().unwrap();
        for relative in [V1_SCHEMA, V2_SCHEMA] {
            let target = root.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(repository.join(relative), target).unwrap();
        }
        let v1_path = root.join(V1_PATH);
        fs::create_dir_all(v1_path.parent().unwrap()).unwrap();
        fs::write(&v1_path, serde_json::to_vec(&v1).unwrap()).unwrap();
        let historical = v1["receipt"].as_object().unwrap();
        let (_, old_credit) = read_carddemo_evidence(&root, historical).unwrap();
        assert_eq!(old_credit.unattributed, 15);
        assert_eq!(old_credit.conformance, 0);
        fs::write(root.join(V2_PATH), serde_json::to_vec(&v2).unwrap()).unwrap();
        let (expected, new_credit) = read_carddemo_evidence(&root, historical).unwrap();
        assert_eq!(expected, v2["receipt"]);
        assert_eq!(new_credit.self_recorded, 15);
        assert_eq!(new_credit.conformance, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn carddemo_provenance_checked_in_v2_covers_every_pin() {
        let root = crate::repository_root().unwrap();
        let v1 = crate::json(&root.join(V1_PATH)).unwrap();
        let v2 = crate::json(&root.join(V2_PATH)).unwrap();
        let summary = validate_carddemo_provenance(&v2, &v1).unwrap();
        assert_eq!(summary.total, 90);
        assert_eq!(summary.self_recorded, summary.total);
        assert_eq!(summary.conformance, 0);
    }
}
