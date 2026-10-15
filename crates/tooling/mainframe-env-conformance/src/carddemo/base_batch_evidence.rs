//! Additive record-comparison evidence. Legacy attribute/version digests retain
//! their meaning and remain regression-only; no licensed authority is inferred.
use super::*;
use serde_json::{Value, json};

const EVIDENCE_ID: &str = "mainframe-env.carddemo-base-batch-comparison-evidence@1";
const REFERENCE_PATH: &str =
    "conformance/subsystems/jes/oracles/carddemo-financial-reference@1.json";
const REFERENCE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/subsystems/jes/oracles/carddemo-financial-reference@1.json"
));
const REFERENCE_SCHEMA_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/subsystems/jes/schemas/carddemo-financial-reference@1.schema.json"
));
const PRODUCER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/tools/carddemo_financial_reference.py"
));
const SCHEMA_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../conformance/subsystems/jes/schemas/carddemo-base-batch-comparison-evidence@1.schema.json"
));
// Exactly the generation families required by the original base-cycle owner.
const GENERATION_PREFIXES: [&str; 7] = [
    "AWS.M2.CARDDEMO.DALYREJS.G",
    "AWS.M2.CARDDEMO.SYSTRAN.G",
    "AWS.M2.CARDDEMO.TRANSACT.BKUP.G",
    "AWS.M2.CARDDEMO.TRANSACT.COMBINED.G",
    "AWS.M2.CARDDEMO.TRANSACT.DALY.G",
    "AWS.M2.CARDDEMO.TRANREPT.G",
    "AWS.M2.CARDDEMO.TCATBALF.BKUP.G",
];
const NAMES: [&str; 3] = ["posttran-tcatbalf", "tranrept-report", "tranrept-selected"];

/// Derived local assertion counts; never an editable evidence credit field.
#[derive(Debug, Default, Eq, PartialEq, Serialize)]
pub struct CardDemoComparisonCredit {
    /// All correctness leaves in the admitted evidence.
    pub total: usize,
    /// Validated local regression leaves without independent credit.
    pub self_recorded: usize,
    /// Historical v1 leaves with no attribution; these earn no credit.
    pub unattributed: usize,
    /// Exact independently derived record count/hash leaves.
    pub independent_reference: usize,
    /// Always zero: this companion establishes no licensed authority.
    pub licensed: usize,
}

fn problem(detail: impl Into<String>) -> CorpusProblem {
    CorpusProblem::new("carddemo.base_batch.provenance", detail)
}

fn expect(condition: bool, detail: &str) -> Result<(), CorpusProblem> {
    if condition {
        Ok(())
    } else {
        Err(problem(detail))
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn value_digest(value: &Value) -> Result<String, CorpusProblem> {
    serde_json::to_vec(value)
        .map(|bytes| digest(&bytes))
        .map_err(|error| problem(error.to_string()))
}

fn reference() -> Result<Value, CorpusProblem> {
    let manifest: Value =
        serde_json::from_slice(REFERENCE_BYTES).map_err(|error| problem(error.to_string()))?;
    let schema: Value = serde_json::from_slice(REFERENCE_SCHEMA_BYTES)
        .map_err(|error| problem(error.to_string()))?;
    jsonschema::draft202012::new(&schema)
        .map_err(|error| problem(error.to_string()))?
        .validate(&manifest)
        .map_err(|error| problem(error.to_string()))?;
    expect(
        manifest["schema_version"] == "mainframe-env.carddemo-financial-reference@1"
            && manifest["producer"]["path"] == "conformance/tools/carddemo_financial_reference.py"
            && manifest["producer"]["sha256"] == digest(PRODUCER_BYTES)
            && manifest["licensed_credit"] == 0,
        "compiled independent reference identity differs",
    )?;
    expect(
        manifest["source_pins"].as_object().is_some_and(|pins| {
            pins.len() == 36
                && pins.iter().all(|(path, value)| {
                    value.as_str().is_some_and(|digest| {
                        financial_observations::source_pin_matches(path, digest)
                    })
                })
        }),
        "independent manifest source pins differ from the original owner",
    )?;
    Ok(manifest)
}

pub(super) fn comparison_name(phase: &str, dataset: &str) -> Option<&'static str> {
    match (phase, dataset) {
        ("POSTTRAN", "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS") => Some("posttran-tcatbalf"),
        ("TRANREPT", "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00") => Some("tranrept-selected"),
        ("TRANREPT", "AWS.M2.CARDDEMO.TRANREPT.G0001V00") => Some("tranrept-report"),
        _ => None,
    }
}

/// Only the complete original FinancialChecks owner constructs this value.
/// It is consumed after both checked drains and exact checked cleanup.
pub(super) struct FinancialProof(BTreeMap<String, Value>);

impl FinancialProof {
    pub(super) fn complete(comparisons: BTreeMap<String, Value>) -> Result<Self, CorpusProblem> {
        expect(
            comparisons.keys().map(String::as_str).eq(NAMES),
            "complete owner is missing an independent record comparison",
        )?;
        let manifest = reference()?;
        for (name, actual) in &comparisons {
            let expected = &manifest["comparisons"][name];
            expect(
                actual["dataset"] == expected["dataset"]
                    && actual["records"] == expected["records"]
                    && actual["record_u32_framed_sha256"] == expected["record_u32_framed_sha256"]
                    && actual["version"].as_u64().is_some_and(|value| value != 0)
                    && actual["attributes"] == expected["attributes"],
                "completed owner comparison differs from independent record reference",
            )?;
        }
        Ok(Self(comparisons))
    }
}

fn leaves(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if prefix == "/receipt"
                    && matches!(key.as_str(), "schema_version" | "corpus_commit")
                {
                    continue;
                }
                let key = key.replace('~', "~0").replace('/', "~1");
                leaves(value, &format!("{prefix}/{key}"), out);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                leaves(value, &format!("{prefix}/{index}"), out);
            }
        }
        _ => {
            out.insert(prefix.into(), value.clone());
        }
    }
}

fn payload_leaves(payload: &Value) -> BTreeMap<String, Value> {
    let mut result = BTreeMap::new();
    leaves(&payload["receipt"], "/receipt", &mut result);
    leaves(&payload["comparisons"], "/comparisons", &mut result);
    leaves(&payload["lifecycle"], "/lifecycle", &mut result);
    result
}

fn independent_locator(path: &str) -> Option<String> {
    for name in NAMES {
        for field in ["records", "record_u32_framed_sha256"] {
            if path == format!("/comparisons/{name}/{field}") {
                return Some(path.into());
            }
        }
    }
    None
}

fn evidence(
    receipt: &CardDemoBaseBatchReceipt,
    proof: FinancialProof,
) -> Result<Value, CorpusProblem> {
    let payload = json!({
        "receipt": receipt,
        "comparisons": proof.0,
        "lifecycle": {"both_checked_drains": true, "checked_owned_root_removed": true}
    });
    let payload_digest = value_digest(&payload)?;
    let reference_digest = digest(REFERENCE_BYTES);
    let provenance = payload_leaves(&payload)
        .keys()
        .map(|path| {
            if let Some(locator) = independent_locator(path) {
                json!({"assertion_path": path, "source": "independent-reference",
                "source_id": REFERENCE_PATH, "source_digest": reference_digest,
                "source_locator": locator})
            } else {
                json!({"assertion_path": path, "source": "self-recorded",
                "source_id": EVIDENCE_ID, "source_digest": payload_digest,
                "source_locator": path})
            }
        })
        .collect::<Vec<_>>();
    let value = json!({"schema_version": EVIDENCE_ID, "target_subsystem": "jes.execution",
        "payload": payload, "payload_sha256": payload_digest,
        "expected_value_provenance": provenance});
    validate_carddemo_base_batch_evidence(&value)?;
    Ok(value)
}

/// Read an additive current companion or a legacy v1 receipt. Missing historical
/// attribution is readable and earns zero credit. Summary fields are derived.
pub fn validate_carddemo_base_batch_evidence(
    value: &Value,
) -> Result<CardDemoComparisonCredit, CorpusProblem> {
    if value["schema_version"] == "mainframe-env.carddemo-base-batch-receipt@1"
        || value["schema_version"] == "mainframe-env.carddemo-base-batch-version-evidence@1"
    {
        let receipt = if value.get("receipt").is_some() {
            &value["receipt"]
        } else {
            value
        };
        let mut historical = BTreeMap::new();
        leaves(receipt, "/receipt", &mut historical);
        return Ok(CardDemoComparisonCredit {
            total: historical.len(),
            unattributed: historical.len(),
            ..Default::default()
        });
    }
    let schema: Value =
        serde_json::from_slice(SCHEMA_BYTES).map_err(|error| problem(error.to_string()))?;
    let validator =
        jsonschema::draft202012::new(&schema).map_err(|error| problem(error.to_string()))?;
    validator
        .validate(value)
        .map_err(|error| problem(error.to_string()))?;
    expect(
        value.as_object().is_some_and(|fields| fields.len() == 5)
            && value["schema_version"] == EVIDENCE_ID
            && value["target_subsystem"] == "jes.execution",
        "comparison evidence header or editable credit is invalid",
    )?;
    let payload = &value["payload"];
    let dataset_digests = payload["receipt"]["dataset_sha256"]
        .as_object()
        .ok_or_else(|| problem("legacy dataset digest map is missing"))?;
    expect(
        GENERATION_PREFIXES
            .iter()
            .all(|prefix| dataset_digests.keys().any(|name| name.starts_with(prefix))),
        "original required generation digest is missing",
    )?;
    let manifest = reference()?;
    expect(
        payload.as_object().is_some_and(|fields| fields.len() == 3)
            && payload["receipt"]["schema_version"]
                == "mainframe-env.carddemo-base-batch-receipt@1"
            && payload["receipt"]["status"] == "pass"
            && payload["receipt"]["journeys_passed"] == 3
            && payload["receipt"]["corpus_commit"] == manifest["corpus_commit"]
            && payload["receipt"]["tranrept_selected_records"]
                == payload["comparisons"]["tranrept-selected"]["records"]
            && payload["receipt"]["tranrept_report_records"]
                == payload["comparisons"]["tranrept-report"]["records"]
            && payload["lifecycle"]
                == json!({"both_checked_drains": true, "checked_owned_root_removed": true}),
        "comparison evidence requires the complete original owner and checked cleanup",
    )?;
    let comparisons: BTreeMap<String, Value> =
        serde_json::from_value(payload["comparisons"].clone())
            .map_err(|error| problem(error.to_string()))?;
    FinancialProof::complete(comparisons)?;
    let payload_digest = value_digest(payload)?;
    expect(
        value["payload_sha256"] == payload_digest,
        "comparison payload digest differs",
    )?;
    let expected = payload_leaves(payload);
    let entries = value["expected_value_provenance"]
        .as_array()
        .ok_or_else(|| problem("comparison provenance is missing"))?;
    let mut seen = BTreeSet::new();
    let mut summary = CardDemoComparisonCredit {
        total: expected.len(),
        ..Default::default()
    };
    for entry in entries {
        let path = entry["assertion_path"]
            .as_str()
            .ok_or_else(|| problem("assertion path is missing"))?;
        expect(
            entry.as_object().is_some_and(|fields| fields.len() == 5)
                && expected.contains_key(path)
                && seen.insert(path),
            "unknown, duplicate or editable provenance assertion",
        )?;
        match entry["source"].as_str() {
            Some("self-recorded") => {
                expect(
                    entry["source_id"] == EVIDENCE_ID
                        && entry["source_digest"] == payload_digest
                        && entry["source_locator"] == path,
                    "self-recorded comparison binding differs",
                )?;
                summary.self_recorded += 1;
            }
            Some("independent-reference") => {
                let locator = independent_locator(path)
                    .ok_or_else(|| problem("assertion has no independent record authority"))?;
                expect(
                    entry["source_id"] == REFERENCE_PATH
                        && entry["source_digest"] == digest(REFERENCE_BYTES)
                        && entry["source_locator"] == locator
                        && manifest.pointer(&locator) == expected.get(path),
                    "independent reference identity, locator or value differs",
                )?;
                summary.independent_reference += 1;
            }
            _ => {
                return Err(problem(
                    "source has no supported independent or licensed authority",
                ));
            }
        }
    }
    expect(
        seen.len() == expected.len(),
        "correctness leaves have missing provenance",
    )?;
    Ok(summary)
}

/// Execute the existing complete owner once and emit its consumed comparison
/// proof. A failed job, drain, reopen or cleanup returns no companion.
pub fn verify_carddemo_base_batch_evidence_from_env(
    inventory_path: &Path,
) -> Result<Value, CorpusProblem> {
    let (receipt, _, _, proof) = verify_carddemo_base_batch_with_operator(inventory_path)?;
    evidence(&receipt, proof)
}
pub(super) fn verify_carddemo_base_batch_with_operator(
    inventory_path: &Path,
) -> Result<
    (
        CardDemoBaseBatchReceipt,
        RouteObservations,
        operator_observations::BatchProof,
        base_batch_evidence::FinancialProof,
    ),
    CorpusProblem,
> {
    let (batch, operator_proof) = verify_carddemo_batch_programs_observed(inventory_path)?;
    let corpus_dir = PathBuf::from(env::var_os(CORPUS_ENV).ok_or_else(|| {
        CorpusProblem::new(
            "carddemo.corpus.environment_missing",
            "CARDDEMO_CORPUS_DIR is required",
        )
    })?);
    let bundles = explicit_carddemo_bundles(&corpus_dir)?
        .into_iter()
        .filter(|(path, _)| path.starts_with("app/cbl/"))
        .map(|(path, bundle)| {
            let name = Path::new(&path)
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    CorpusProblem::new(
                        "carddemo.base_batch.path_invalid",
                        "program path is invalid",
                    )
                })?
                .to_ascii_uppercase();
            Ok((name, (path, bundle)))
        })
        .collect::<Result<BTreeMap<_, _>, CorpusProblem>>()?;
    let needed = batch
        .named_programs
        .iter()
        .chain(batch.called_programs.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    let definitions = compile_carddemo_batch_definitions(&bundles, &needed)?;
    let online = carddemo_base_online_definition(&corpus_dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CorpusProblem::new("carddemo.base_batch.runtime", error.to_string()))?;
    let (receipt, observations, financial_proof) = runtime.block_on(exercise_base_batch_routes(
        &corpus_dir,
        batch.corpus_commit,
        online,
        definitions,
    ))?;
    Ok((receipt, observations, operator_proof, financial_proof))
}

#[cfg(test)]
#[path = "base_batch_evidence_tests.rs"]
mod tests;
