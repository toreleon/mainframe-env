use super::*;

fn fixture() -> Value {
    let manifest = reference().unwrap();
    let comparisons = manifest["comparisons"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, expected)| {
            let mut value = expected.clone();
            value["version"] = json!(7);
            (name.clone(), value)
        })
        .collect();
    let schema: Value = serde_json::from_slice(SCHEMA_BYTES).unwrap();
    let receipt_properties =
        &schema["properties"]["payload"]["properties"]["receipt"]["properties"];
    let required_digests = |field: &str, byte: char| {
        receipt_properties[field]["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| (key.as_str().unwrap().into(), byte.to_string().repeat(64)))
            .collect::<BTreeMap<String, String>>()
    };
    let mut dataset_sha256 = required_digests("dataset_sha256", 'a');
    dataset_sha256.insert("A/B~C".into(), "a".repeat(64));
    for prefix in GENERATION_PREFIXES {
        dataset_sha256.insert(format!("{prefix}0001V00"), "a".repeat(64));
    }
    let receipt = CardDemoBaseBatchReceipt {
        schema_version: "mainframe-env.carddemo-base-batch-receipt@1".into(),
        status: "pass".into(),
        corpus_commit: manifest["corpus_commit"].as_str().unwrap().into(),
        business_date: "2022-07-06".into(),
        journeys_passed: 3,
        initialization_jobs: 12,
        operational_jobs: 9,
        cics_file_controls: 2,
        internal_submissions: 1,
        warm_restart_controls: 1,
        rollback_controls: 1,
        cancellation_controls: 1,
        tranrept_selected_records: 312,
        tranrept_report_records: 519,
        dataset_sha256,
        spool_sha256: required_digests("spool_sha256", 'b'),
        journey_shape_sha256: "c".repeat(64),
    };
    evidence(&receipt, FinancialProof::complete(comparisons).unwrap()).unwrap()
}

fn entry<'a>(value: &'a mut Value, path: &str) -> &'a mut Value {
    value["expected_value_provenance"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["assertion_path"] == path)
        .unwrap()
}

fn rehash(value: &mut Value) {
    let leaves = payload_leaves(&value["payload"]);
    value["expected_value_provenance"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| {
            entry["assertion_path"]
                .as_str()
                .is_some_and(|path| leaves.contains_key(path))
        });
    let updated = value_digest(&value["payload"]).unwrap();
    value["payload_sha256"] = json!(updated);
    for entry in value["expected_value_provenance"].as_array_mut().unwrap() {
        if entry["source"] == "self-recorded" {
            entry["source_digest"] = json!(updated);
        }
    }
}

#[test]
fn record_comparison_credit_is_derived_and_legacy_digests_stay_self_recorded() {
    let value = fixture();
    let summary = validate_carddemo_base_batch_evidence(&value).unwrap();
    assert_eq!(summary.independent_reference, 6);
    assert_eq!(summary.licensed, 0);
    assert_eq!(summary.unattributed, 0);
    assert_eq!(summary.total, summary.self_recorded + 6);
    for entry in value["expected_value_provenance"].as_array().unwrap() {
        let path = entry["assertion_path"].as_str().unwrap();
        if path.starts_with("/receipt/")
            || path.ends_with("/version")
            || path.contains("/attributes/")
        {
            assert_eq!(entry["source"], "self-recorded");
        }
    }
}

#[test]
fn legacy_bare_and_historical_v1_remain_readable_without_credit() {
    let value = fixture();
    let bare = value["payload"]["receipt"].clone();
    let historical = json!({"schema_version": "mainframe-env.carddemo-base-batch-version-evidence@1", "receipt": bare});
    for value in [bare, historical] {
        let summary = validate_carddemo_base_batch_evidence(&value).unwrap();
        assert!(summary.total > 0);
        assert_eq!(summary.unattributed, summary.total);
        assert_eq!(summary.independent_reference, 0);
        assert_eq!(summary.licensed, 0);
    }
}

#[test]
fn missing_duplicate_and_unknown_paths_are_refused() {
    for mode in 0..3 {
        let mut value = fixture();
        let entries = value["expected_value_provenance"].as_array_mut().unwrap();
        match mode {
            0 => {
                entries.pop();
            }
            1 => {
                entries.push(entries[0].clone());
            }
            _ => {
                entries[0]["assertion_path"] = json!("/receipt/unoffered");
            }
        }
        assert!(validate_carddemo_base_batch_evidence(&value).is_err());
    }
}

#[test]
fn independent_source_digest_locator_and_value_are_exact() {
    for (field, changed) in [
        ("source_id", json!("different-reference.json")),
        ("source_digest", json!("0".repeat(64))),
        (
            "source_locator",
            json!("/comparisons/tranrept-report/records"),
        ),
        ("source", json!("licensed-ibm")),
    ] {
        let mut value = fixture();
        entry(&mut value, "/comparisons/posttran-tcatbalf/records")[field] = changed;
        assert!(
            validate_carddemo_base_batch_evidence(&value).is_err(),
            "{field}"
        );
    }
    for (field, wrong) in [
        ("records", json!(99)),
        ("record_u32_framed_sha256", json!("0".repeat(64))),
    ] {
        let mut value = fixture();
        value["payload"]["comparisons"]["posttran-tcatbalf"][field] = wrong;
        rehash(&mut value);
        assert!(
            validate_carddemo_base_batch_evidence(&value).is_err(),
            "{field}"
        );
    }
}

#[test]
fn record_authority_cannot_promote_legacy_framing_or_observed_metadata() {
    for path in [
        "/receipt/dataset_sha256/A~1B~0C",
        "/comparisons/posttran-tcatbalf/version",
        "/comparisons/posttran-tcatbalf/attributes/ccsid",
    ] {
        let mut value = fixture();
        let entry = entry(&mut value, path);
        entry["source"] = json!("independent-reference");
        entry["source_id"] = json!(REFERENCE_PATH);
        entry["source_digest"] = json!(digest(REFERENCE_BYTES));
        entry["source_locator"] = json!("/comparisons/posttran-tcatbalf/records");
        assert!(
            validate_carddemo_base_batch_evidence(&value).is_err(),
            "{path}"
        );
    }
}

#[test]
fn incomplete_owner_cleanup_and_wrong_dataset_produce_no_valid_companion() {
    assert!(FinancialProof::complete(BTreeMap::new()).is_err());
    for path in [
        "/lifecycle/both_checked_drains",
        "/lifecycle/checked_owned_root_removed",
    ] {
        let mut value = fixture();
        *value["payload"].pointer_mut(path).unwrap() = json!(false);
        rehash(&mut value);
        assert!(validate_carddemo_base_batch_evidence(&value).is_err());
    }
    let mut value = fixture();
    value["payload"]["comparisons"]["posttran-tcatbalf"]["attributes"]["logical_record_length"] =
        json!(51);
    rehash(&mut value);
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
    let mut value = fixture();
    value["payload"]["comparisons"]["posttran-tcatbalf"]["dataset"] =
        json!("AWS.M2.CARDDEMO.UNOFFERED");
    rehash(&mut value);
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
}

#[test]
fn editable_credit_and_unknown_source_never_grant_credit() {
    let mut value = fixture();
    value["credit"] = json!(999);
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
    let mut value = fixture();
    entry(&mut value, "/receipt/status")["credit"] = json!(1);
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
    let mut value = fixture();
    entry(&mut value, "/receipt/status")["source"] = json!("customer-captured");
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
}

#[test]
fn incomplete_original_receipt_is_refused_after_full_provenance_rebinding() {
    for field in [
        "initialization_jobs",
        "operational_jobs",
        "cics_file_controls",
        "internal_submissions",
        "warm_restart_controls",
        "rollback_controls",
        "cancellation_controls",
    ] {
        let mut value = fixture();
        value["payload"]["receipt"][field] = json!(0);
        rehash(&mut value);
        assert!(
            validate_carddemo_base_batch_evidence(&value).is_err(),
            "{field}"
        );
    }
    for field in ["dataset_sha256", "spool_sha256"] {
        let mut value = fixture();
        value["payload"]["receipt"][field] = json!({});
        rehash(&mut value);
        assert!(
            validate_carddemo_base_batch_evidence(&value).is_err(),
            "{field}"
        );
    }
    let mut value = fixture();
    value["payload"]["receipt"]["business_date"] = json!("wrong-date");
    rehash(&mut value);
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
    let mut value = fixture();
    value["payload"]["receipt"]["dataset_sha256"]
        .as_object_mut()
        .unwrap()
        .remove("AWS.M2.CARDDEMO.SYSTRAN.G0001V00");
    rehash(&mut value);
    assert!(validate_carddemo_base_batch_evidence(&value).is_err());
}
