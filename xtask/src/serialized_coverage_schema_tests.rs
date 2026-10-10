//! Synthetic legacy-wire regressions; these tests grant no official coverage credit.

use super::*;
use mainframe_env_coverage::{
    COVERAGE_EVIDENCE_CONTRACT, COVERAGE_ROW_CONTRACT, CoverageLimits, CoverageProblem,
    CoverageRow, CoverageSnapshot, CoverageStore, EvidenceInput, EvidenceOutcome, EvidenceRecord,
    OracleReceipt,
};

const ROW_ID: &str = "synthetic:serialized:row";
const BASELINE: &str = "synthetic-serialized-baseline";
const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn schema(name: &str) -> (PathBuf, Value) {
    let path = repository_root()
        .expect("repository root")
        .join("conformance/subsystems/coverage/schemas")
        .join(name);
    let value = json(&path).expect("committed schema");
    (path, value)
}

fn evidence() -> Value {
    json!({
        "schema_version": "mainframe-env.coverage-evidence@1",
        "identity": DIGEST,
        "row_id": ROW_ID,
        "gate": "differential",
        "sequence": 1,
        "outcome": "fail",
        "producer": "synthetic.schema.regression",
        "artifact_digest": DIGEST,
        "source_identity": "synthetic:pinned-source",
        "oracle": null
    })
}

fn row() -> Value {
    json!({
        "schema_version": "mainframe-env.coverage-row@1",
        "baseline_id": BASELINE,
        "unit_id": "synthetic-unit",
        "row_id": ROW_ID,
        "applicable_gates": ["recognized"],
        "evidence_refs": {},
        "derived_state": "pending"
    })
}

fn malformed_ledger() -> Value {
    json!({
        "schema_version": "mainframe-env.coverage-ledger@1",
        "generation": 1,
        "catalog_index": "conformance/subsystems/coverage/catalogs/index.json",
        "catalog_index_sha256": DIGEST,
        "evidence_records": [],
        "baselines": vec![json!({}); 9],
        "official_compatibility_numerator": 0,
        "generated_catalog_credit": 0
    })
}

fn typed_evidence(gate: CoverageGate, outcome: EvidenceOutcome) -> EvidenceRecord {
    EvidenceRecord::new(
        EvidenceInput {
            row_id: ROW_ID.into(),
            gate,
            sequence: 1,
            outcome,
            producer: "synthetic.schema.regression".into(),
            artifact_digest: DIGEST.into(),
            source_identity: "synthetic:pinned-source".into(),
            oracle: (gate == CoverageGate::Differential && outcome == EvidenceOutcome::Pass).then(
                || OracleReceipt {
                    environment_identity: "synthetic:oracle-environment".into(),
                    product_identity: "synthetic:oracle-product".into(),
                    source_digest: DIGEST.into(),
                },
            ),
        },
        CoverageLimits::default(),
    )
    .expect("synthetic owned evidence")
}

#[test]
fn differential_pass_with_null_oracle_is_rejected() {
    let (path, schema) = schema("coverage-evidence.schema.json");
    let mut invalid = evidence();
    invalid["outcome"] = json!("pass");
    assert!(
        validate_schema_instance(&schema, &invalid, &path).is_err(),
        "differential pass cannot omit the oracle"
    );
}

#[test]
fn differential_failure_without_oracle_remains_valid() {
    let (path, schema) = schema("coverage-evidence.schema.json");
    validate_schema_instance(&schema, &evidence(), &path).expect("failure needs no oracle");
    assert_eq!(
        typed_evidence(CoverageGate::Differential, EvidenceOutcome::Fail).outcome(),
        EvidenceOutcome::Fail
    );
}

#[test]
fn nondifferential_pass_without_oracle_remains_valid() {
    let (path, schema) = schema("coverage-evidence.schema.json");
    let mut valid = evidence();
    valid["gate"] = json!("recognized");
    valid["outcome"] = json!("pass");
    validate_schema_instance(&schema, &valid, &path).expect("ordinary pass needs no oracle");
}

#[test]
fn complete_row_without_any_evidence_is_rejected() {
    let (path, schema) = schema("coverage-row.schema.json");
    let mut invalid = row();
    invalid["derived_state"] = json!("complete");
    assert!(
        validate_schema_instance(&schema, &invalid, &path).is_err(),
        "completion requires evidence"
    );
}

#[test]
fn arbitrary_evidence_gate_key_is_rejected() {
    let (path, schema) = schema("coverage-row.schema.json");
    let mut invalid = row();
    invalid["evidence_refs"] = json!({"invented_gate": [DIGEST]});
    assert!(
        validate_schema_instance(&schema, &invalid, &path).is_err(),
        "only the six frozen gate keys are admitted"
    );
}

#[test]
fn malformed_evidence_reference_is_rejected() {
    let (path, schema) = schema("coverage-row.schema.json");
    let mut invalid = row();
    invalid["evidence_refs"] = json!({"recognized": ["not-a-content-address"]});
    assert!(validate_schema_instance(&schema, &invalid, &path).is_err());
}

#[test]
fn historical_pending_rows_keep_empty_and_explicit_empty_histories() {
    let (path, schema) = schema("coverage-row.schema.json");
    let mut valid = row();
    validate_schema_instance(&schema, &valid, &path).expect("absent history remains pending");
    valid["evidence_refs"] = json!({"recognized": []});
    validate_schema_instance(&schema, &valid, &path).expect("empty history remains pending");
}

#[test]
fn ledger_baseline_items_are_validated() {
    let (path, schema) = schema("coverage-ledger.schema.json");
    let validator = compile_draft_2020_12_schema(&schema, &path).expect("reviewed validator");
    for item in [json!({}), json!(null), json!(7)] {
        let mut invalid = malformed_ledger();
        invalid["baselines"][0] = item;
        assert!(
            validator.iter_errors(&invalid).any(|error| error
                .instance_path()
                .to_string()
                .starts_with("/baselines/0")),
            "malformed baseline item must itself be rejected"
        );
    }
}

#[test]
fn ledger_evidence_items_are_validated() {
    let (path, schema) = schema("coverage-ledger.schema.json");
    let validator = compile_draft_2020_12_schema(&schema, &path).expect("reviewed validator");
    for item in [json!({}), json!(null), json!("invalid-evidence")] {
        let mut invalid = malformed_ledger();
        invalid["evidence_records"] = json!([item]);
        assert!(
            validator.iter_errors(&invalid).any(|error| {
                error
                    .instance_path()
                    .to_string()
                    .starts_with("/evidence_records/0")
            }),
            "other malformed arrays cannot mask missing evidence-item validation"
        );
    }
}

#[test]
fn row_contract_has_a_schema_binding() {
    assert_eq!(
        schema_for_0_2_artifact(COVERAGE_ROW_CONTRACT),
        Some("coverage-row.schema.json")
    );
}

#[test]
fn evidence_contract_has_a_schema_binding() {
    assert_eq!(
        schema_for_0_2_artifact(COVERAGE_EVIDENCE_CONTRACT),
        Some("coverage-evidence.schema.json")
    );
}

#[test]
fn fully_sourced_complete_row_passes_schemas_and_owned_store() {
    let limits = CoverageLimits::default();
    let mut owned = CoverageRow::new(
        BASELINE,
        "synthetic-unit",
        ROW_ID,
        CoverageGate::ALL,
        limits,
    )
    .expect("owned row");
    let mut refs = serde_json::Map::new();
    let mut store = CoverageStore::default();
    let (evidence_path, evidence_schema) = schema("coverage-evidence.schema.json");
    for gate in CoverageGate::ALL {
        let record = typed_evidence(gate, EvidenceOutcome::Pass);
        let mut wire = evidence();
        wire["identity"] = json!(record.identity());
        wire["gate"] = json!(gate.slug());
        wire["outcome"] = json!("pass");
        if gate == CoverageGate::Differential {
            wire["oracle"] = json!({
                "environment_identity": "synthetic:oracle-environment",
                "product_identity": "synthetic:oracle-product",
                "source_digest": DIGEST
            });
        }
        validate_schema_instance(&evidence_schema, &wire, &evidence_path)
            .expect("fully sourced synthetic evidence");
        refs.insert(gate.slug().into(), json!([record.identity()]));
        store
            .append_evidence(record.clone(), limits)
            .expect("retain evidence");
        owned.record(record).expect("same-row applicable evidence");
    }
    let mut wire = row();
    wire["applicable_gates"] = json!(CoverageGate::ALL.map(CoverageGate::slug));
    wire["evidence_refs"] = Value::Object(refs);
    wire["derived_state"] = json!("complete");
    let (row_path, row_schema) = schema("coverage-row.schema.json");
    validate_schema_instance(&row_schema, &wire, &row_path).expect("fully sourced complete row");
    assert!(owned.is_complete());
    let snapshot = CoverageSnapshot::new(BASELINE, DIGEST, 1, 1, vec![owned], limits)
        .expect("one-row denominator");
    for gate in CoverageGate::ALL {
        assert_eq!(snapshot.count(gate).numerator, 1);
        assert_eq!(snapshot.count(gate).denominator, 1);
    }
    store
        .commit(snapshot, limits)
        .expect("all referenced evidence retained");
    assert_eq!(store.latest(BASELINE).expect("snapshot").complete_rows(), 1);
}

#[test]
fn joins_and_completion_remain_owned_by_typed_coverage() {
    let limits = CoverageLimits::default();
    let mut owned = CoverageRow::new(
        BASELINE,
        "synthetic-unit",
        ROW_ID,
        [CoverageGate::Recognized],
        limits,
    )
    .expect("owned row");
    assert!(!owned.is_complete(), "no evidence means pending");
    let mut wrong_row = CoverageRow::new(
        BASELINE,
        "synthetic-unit",
        "synthetic:other:row",
        [CoverageGate::Recognized],
        limits,
    )
    .expect("other row");
    assert_eq!(
        wrong_row.record(typed_evidence(
            CoverageGate::Recognized,
            EvidenceOutcome::Pass
        )),
        Err(CoverageProblem::WrongRow)
    );
    assert_eq!(
        owned.record(typed_evidence(
            CoverageGate::Differential,
            EvidenceOutcome::Fail
        )),
        Err(CoverageProblem::GateNotApplicable)
    );
    assert_eq!(
        CoverageSnapshot::new(BASELINE, DIGEST, 1, 2, vec![owned.clone()], limits),
        Err(CoverageProblem::DenominatorMismatch)
    );
    owned
        .record(typed_evidence(
            CoverageGate::Recognized,
            EvidenceOutcome::Pass,
        ))
        .expect("owned evidence");
    let snapshot = CoverageSnapshot::new(BASELINE, DIGEST, 1, 1, vec![owned], limits)
        .expect("matching denominator");
    let mut store = CoverageStore::default();
    assert_eq!(
        store.commit(snapshot, limits),
        Err(CoverageProblem::MissingEvidence)
    );
    assert!(
        store.latest(BASELINE).is_none(),
        "refusal preserves selection"
    );
}

mod projection {
    use super::*;

    fn fixture() -> (PathBuf, Value, Value) {
        let root = repository_root().expect("root");
        let index_path = root.join("conformance/subsystems/coverage/catalogs/index.json");
        let index = json(&index_path).expect("current index");
        let baselines = index["baselines"].as_array().expect("indexed baselines");
        let summaries = baselines
            .iter()
            .map(|baseline| {
                let mut gates = serde_json::Map::new();
                for gate in CoverageGate::ALL {
                    gates.insert(
                        gate.slug().into(),
                        json!({
                            "numerator": 0, "denominator": baseline["mandatory_rows"]
                        }),
                    );
                }
                json!({
                    "id": baseline["id"], "catalog_sha256": baseline["catalog_sha256"],
                    "mandatory_rows": baseline["mandatory_rows"], "complete_rows": 0,
                    "gates": gates
                })
            })
            .collect::<Vec<_>>();
        let ledger = json!({
            "schema_version": "mainframe-env.coverage-ledger@1", "generation": 1,
            "catalog_index": "conformance/subsystems/coverage/catalogs/index.json",
            "catalog_index_sha256": format!("sha256:{}", file_digest(&index_path).unwrap()),
            "evidence_records": [], "baselines": summaries,
            "official_compatibility_numerator": 0, "generated_catalog_credit": 0
        });
        let catalog = json(&root.join(baselines[0]["catalog"].as_str().unwrap())).unwrap();
        let mut row = row();
        row["baseline_id"] = baselines[0]["id"].clone();
        row["unit_id"] = catalog["units"][0]["id"].clone();
        row["row_id"] = catalog["units"][0]["rows"][0]["id"].clone();
        row["applicable_gates"] = json!(CoverageGate::ALL.map(CoverageGate::slug));
        (root, ledger, row)
    }

    fn wire(row: &Value, gate: CoverageGate, outcome: EvidenceOutcome, sequence: u64) -> Value {
        let mut input = EvidenceInput {
            row_id: row["row_id"].as_str().unwrap().into(),
            gate,
            sequence,
            outcome,
            producer: "synthetic.schema.regression".into(),
            artifact_digest: DIGEST.into(),
            source_identity: "synthetic:pinned-source".into(),
            oracle: None,
        };
        let mut value = evidence();
        value["row_id"] = row["row_id"].clone();
        value["gate"] = json!(gate.slug());
        value["sequence"] = json!(sequence);
        value["outcome"] = json!(if outcome == EvidenceOutcome::Pass {
            "pass"
        } else {
            "fail"
        });
        if gate == CoverageGate::Differential && outcome == EvidenceOutcome::Pass {
            input.oracle = Some(OracleReceipt {
                environment_identity: "synthetic:oracle-environment".into(),
                product_identity: "synthetic:oracle-product".into(),
                source_digest: DIGEST.into(),
            });
            value["oracle"] = json!({
                "environment_identity": "synthetic:oracle-environment",
                "product_identity": "synthetic:oracle-product", "source_digest": DIGEST
            });
        }
        value["identity"] = json!(
            EvidenceRecord::new(input, CoverageLimits::default())
                .unwrap()
                .identity()
        );
        value
    }

    fn validate(root: &Path, values: &[Value]) -> TaskResult {
        let artifacts = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                (
                    PathBuf::from(format!("synthetic-{index}.json")),
                    value.clone(),
                )
            })
            .collect::<Vec<_>>();
        coverage_projection::validate_values(root, &artifacts)
    }

    fn schema_valid(value: &Value) {
        let name = schema_for_0_2_artifact(value["schema_version"].as_str().unwrap()).unwrap();
        let (path, schema) = schema(name);
        validate_schema_instance(&schema, value, &path).expect("schema admission is separate");
    }

    #[test]
    fn current_empty_legacy_summary_and_pending_histories_are_admitted() {
        let (root, ledger, mut row) = fixture();
        validate(&root, &[ledger.clone(), row.clone()]).expect("zero-credit legacy summary");
        row["evidence_refs"] = json!({"recognized": []});
        validate(&root, &[ledger, row]).expect("explicit empty pending history");
    }

    #[test]
    fn embedded_differential_failure_without_oracle_is_admitted() {
        let (root, mut ledger, mut row) = fixture();
        let record = wire(&row, CoverageGate::Differential, EvidenceOutcome::Fail, 1);
        row["evidence_refs"] = json!({"differential": [record["identity"]]});
        row["derived_state"] = json!("failed");
        ledger["evidence_records"] = json!([record]);
        validate(&root, &[ledger, row]).expect("failure does not require licensed oracle metadata");
    }

    #[test]
    fn fully_sourced_row_and_derived_legacy_counts_are_admitted() {
        let (root, mut ledger, mut row) = fixture();
        let mut refs = serde_json::Map::new();
        let records = CoverageGate::ALL
            .into_iter()
            .map(|gate| {
                let record = wire(&row, gate, EvidenceOutcome::Pass, 1);
                refs.insert(gate.slug().into(), json!([record["identity"]]));
                ledger["baselines"][0]["gates"][gate.slug()]["numerator"] = json!(1);
                record
            })
            .collect::<Vec<_>>();
        row["evidence_refs"] = Value::Object(refs);
        row["derived_state"] = json!("complete");
        ledger["evidence_records"] = json!(records);
        ledger["baselines"][0]["complete_rows"] = json!(1);
        ledger["official_compatibility_numerator"] = json!(1);
        validate(&root, &[ledger, row]).expect("one complete synthetic sourced row");
    }

    #[test]
    fn schema_valid_tampered_content_identity_is_rejected() {
        let (root, _, row) = fixture();
        let mut record = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Pass, 1);
        record["producer"] = json!("different.synthetic.producer");
        schema_valid(&record);
        assert!(
            validate(&root, &[record])
                .unwrap_err()
                .contains("content identity")
        );
    }

    #[test]
    fn unknown_official_evidence_row_is_rejected() {
        let (root, _, mut row) = fixture();
        row["row_id"] = json!("synthetic:unknown:official:row");
        let record = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Fail, 1);
        schema_valid(&record);
        assert!(
            validate(&root, &[record])
                .unwrap_err()
                .contains("unknown official evidence row")
        );
    }

    #[test]
    fn catalog_row_membership_unit_baseline_and_applicability_are_enforced() {
        let (root, _, row) = fixture();
        for (field, value) in [
            ("row_id", json!("synthetic:unknown:official:row")),
            ("unit_id", json!("synthetic-wrong-unit")),
            ("baseline_id", json!("synthetic-wrong-baseline")),
            ("applicable_gates", json!(["recognized"])),
        ] {
            let mut invalid = row.clone();
            invalid[field] = value;
            schema_valid(&invalid);
            assert!(
                validate(&root, &[invalid]).is_err(),
                "descriptor mutation {field}"
            );
        }
    }

    #[test]
    fn unresolved_wrong_row_and_wrong_gate_references_are_rejected() {
        let (root, _, mut row) = fixture();
        row["evidence_refs"] = json!({"recognized": [DIGEST]});
        schema_valid(&row);
        assert!(
            validate(&root, &[row.clone()])
                .unwrap_err()
                .contains("unresolved")
        );
        let record = wire(&row, CoverageGate::Executed, EvidenceOutcome::Pass, 1);
        row["evidence_refs"] = json!({"recognized": [record["identity"]]});
        assert!(
            validate(&root, &[row.clone(), record])
                .unwrap_err()
                .contains("gate mismatch")
        );
        let mut other = row.clone();
        other["row_id"] = json!("synthetic:other:official:row");
        let record = wire(&other, CoverageGate::Recognized, EvidenceOutcome::Pass, 1);
        let projected =
            coverage_projection::project_evidence(&record, Path::new("synthetic.json")).unwrap();
        row["evidence_refs"] = json!({"recognized": [record["identity"]]});
        let evidence = BTreeMap::from([(projected.identity().to_string(), projected)]);
        assert!(
            coverage_projection::project_row(&row, &evidence, Path::new("synthetic.json"))
                .unwrap_err()
                .contains("WrongRow")
        );
    }

    #[test]
    fn schema_valid_complete_with_failed_latest_evidence_is_rejected() {
        let (root, _, mut row) = fixture();
        let mut refs = serde_json::Map::new();
        let records = CoverageGate::ALL
            .into_iter()
            .map(|gate| {
                let outcome = if gate == CoverageGate::Differential {
                    EvidenceOutcome::Fail
                } else {
                    EvidenceOutcome::Pass
                };
                let record = wire(&row, gate, outcome, 1);
                refs.insert(gate.slug().into(), json!([record["identity"]]));
                record
            })
            .collect::<Vec<_>>();
        row["evidence_refs"] = Value::Object(refs);
        row["derived_state"] = json!("complete");
        schema_valid(&row);
        let mut values = records;
        values.push(row);
        assert!(
            validate(&root, &values)
                .unwrap_err()
                .contains("derived state mismatch")
        );
    }

    #[test]
    fn schema_valid_reordered_history_is_rejected() {
        let (root, _, mut row) = fixture();
        let older = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Pass, 1);
        let newer = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Fail, 2);
        row["evidence_refs"] = json!({"recognized": [newer["identity"], older["identity"]]});
        row["derived_state"] = json!("failed");
        schema_valid(&row);
        assert!(
            validate(&root, &[row, older, newer])
                .unwrap_err()
                .contains("StaleEvidence")
        );
    }

    #[test]
    fn schema_valid_ledger_catalog_and_derived_counts_are_not_trusted() {
        let (root, ledger, _) = fixture();
        for (pointer, replacement) in [
            ("/catalog_index_sha256", json!(DIGEST)),
            ("/baselines/0/id", json!("synthetic-unknown-baseline")),
            ("/baselines/0/catalog_sha256", json!(DIGEST)),
            ("/baselines/0/mandatory_rows", json!(1)),
            ("/baselines/0/complete_rows", json!(1)),
            ("/baselines/0/gates/recognized/numerator", json!(1)),
            ("/baselines/0/gates/recognized/denominator", json!(1)),
            ("/official_compatibility_numerator", json!(1)),
        ] {
            let mut invalid = ledger.clone();
            *invalid.pointer_mut(pointer).unwrap() = replacement;
            schema_valid(&invalid);
            assert!(
                validate(&root, &[invalid]).is_err(),
                "untrusted claim {pointer}"
            );
        }
        let mut duplicate = ledger.clone();
        duplicate["baselines"][1] = duplicate["baselines"][0].clone();
        schema_valid(&duplicate);
        assert!(
            validate(&root, &[duplicate])
                .unwrap_err()
                .contains("duplicate coverage baseline")
        );
    }

    #[test]
    fn unknown_legacy_summary_and_gate_fields_are_rejected() {
        let (_, ledger, _) = fixture();
        let (path, schema) = schema("coverage-ledger.schema.json");
        for pointer in [
            "/baselines/0/rows",
            "/baselines/0/gates/fake",
            "/baselines/0/gates/recognized/extra",
        ] {
            let mut invalid = ledger.clone();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            invalid
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), json!([]));
            assert!(
                validate_schema_instance(&schema, &invalid, &path).is_err(),
                "unknown field {pointer}"
            );
        }
    }

    #[test]
    fn ledger_generations_cannot_discard_retained_failure_and_can_append_recovery() {
        let (root, mut first, row) = fixture();
        let pass = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Pass, 1);
        let fail = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Fail, 2);
        first["evidence_records"] = json!([pass, fail]);
        let mut truncated = first.clone();
        truncated["generation"] = json!(2);
        truncated["evidence_records"].as_array_mut().unwrap().pop();
        truncated["baselines"][0]["gates"]["recognized"]["numerator"] = json!(1);
        schema_valid(&truncated);
        assert!(validate(&root, &[first.clone(), truncated]).is_err());
        let mut recovered = first.clone();
        recovered["generation"] = json!(2);
        recovered["evidence_records"]
            .as_array_mut()
            .unwrap()
            .push(wire(
                &row,
                CoverageGate::Recognized,
                EvidenceOutcome::Pass,
                3,
            ));
        recovered["baselines"][0]["gates"]["recognized"]["numerator"] = json!(1);
        validate(&root, &[recovered, first])
            .expect("retained history and sorted ledger generation");
    }

    #[test]
    fn complete_rows_require_every_applicable_nonempty_history() {
        let (_, _, mut row) = fixture();
        row["derived_state"] = json!("complete");
        let (path, schema) = schema("coverage-row.schema.json");
        for refs in [json!({"recognized": []}), json!({"recognized": [DIGEST]})] {
            row["evidence_refs"] = refs;
            assert!(validate_schema_instance(&schema, &row, &path).is_err());
        }
    }

    #[test]
    fn supplied_evidence_cannot_conflict_at_a_retained_sequence() {
        let (root, _, row) = fixture();
        let pass = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Pass, 1);
        let fail = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Fail, 1);
        schema_valid(&pass);
        schema_valid(&fail);
        assert!(
            validate(&root, &[pass, fail]).is_err(),
            "immutable key cannot be replaced"
        );
    }

    #[test]
    fn supplied_evidence_failure_cannot_be_omitted_from_row_history() {
        let (root, _, mut row) = fixture();
        let mut refs = serde_json::Map::new();
        let mut records = CoverageGate::ALL
            .into_iter()
            .map(|gate| {
                let record = wire(&row, gate, EvidenceOutcome::Pass, 1);
                refs.insert(gate.slug().into(), json!([record["identity"]]));
                record
            })
            .collect::<Vec<_>>();
        row["evidence_refs"] = Value::Object(refs);
        row["derived_state"] = json!("complete");
        records.push(wire(
            &row,
            CoverageGate::Recognized,
            EvidenceOutcome::Fail,
            2,
        ));
        schema_valid(&row);
        records.push(row);
        assert!(
            validate(&root, &records).is_err(),
            "retained failure cannot disappear"
        );
    }

    #[test]
    fn largest_owned_sequence_keeps_its_valid_serialized_form() {
        let (root, _, row) = fixture();
        let record = wire(
            &row,
            CoverageGate::Recognized,
            EvidenceOutcome::Pass,
            u64::MAX,
        );
        schema_valid(&record);
        validate(&root, &[record]).expect("valid u64 sequence boundary");
    }

    #[test]
    fn schema_valid_unbounded_or_control_text_is_refused_by_the_owned_projection() {
        let (root, _, row) = fixture();
        let record = wire(&row, CoverageGate::Recognized, EvidenceOutcome::Pass, 1);
        for text in ["x".repeat(513), "synthetic\ncontrol".into()] {
            let mut invalid = record.clone();
            invalid["source_identity"] = json!(text);
            schema_valid(&invalid);
            assert!(validate(&root, &[invalid]).is_err());
        }
    }
}
