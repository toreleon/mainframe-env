//! Frozen coverage wire bindings and projections through the existing typed owners.

use super::*;
use mainframe_env_coverage::{
    COVERAGE_EVIDENCE_CONTRACT, COVERAGE_LEDGER_CONTRACT, COVERAGE_ROW_CONTRACT, CoverageLimits,
    CoverageRow, CoverageSnapshot, CoverageStore, EvidenceInput, EvidenceOutcome, EvidenceRecord,
    OracleReceipt,
};

const EVIDENCE_SCHEMA_ID: &str = "https://mainframe-env.invalid/schemas/coverage-evidence@1";
const LEDGER_SCHEMA_ID: &str = "https://mainframe-env.invalid/schemas/coverage-ledger@1";
const PACKAGE_SCHEMA_ID: &str = "https://mainframe-env.invalid/schemas/application-package@2";
const IMS_SCHEMA_ID: &str = "https://mainframe-env.invalid/schemas/ims-metadata@1";

struct RefuseSchemaRetrieval;

impl jsonschema::Retrieve for RefuseSchemaRetrieval {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("external schema retrieval is forbidden: {uri}").into())
    }
}

pub(super) fn compile_schema(schema: &Value, path: &Path) -> TaskResult<jsonschema::Validator> {
    jsonschema::draft202012::meta::validate(schema)
        .map_err(|error| format!("{} is not valid Draft 2020-12: {error}", path.display()))?;
    let options = jsonschema::draft202012::options().offline();
    let resources: &[(&str, &str)] = match schema["$id"].as_str() {
        Some(LEDGER_SCHEMA_ID) => &[(
            EVIDENCE_SCHEMA_ID,
            "conformance/subsystems/coverage/schemas/coverage-evidence.schema.json",
        )],
        Some(PACKAGE_SCHEMA_ID) => &[(
            IMS_SCHEMA_ID,
            "conformance/subsystems/ims/schemas/ims-metadata.schema.json",
        )],
        _ => &[],
    };
    if !resources.is_empty() {
        // Resolve only these owned references locally, with no network retriever.
        let root = repository_root()?;
        let mut registry = jsonschema::Registry::new().retriever(RefuseSchemaRetrieval);
        for (id, relative) in resources {
            let resource = json(&root.join(relative))?;
            require(
                resource["$id"].as_str() == Some(id),
                "local schema identity differs",
            )?;
            jsonschema::draft202012::meta::validate(&resource)
                .map_err(|error| format!("{relative} is invalid: {error}"))?;
            registry = registry
                .add(*id, resource)
                .map_err(|error| error.to_string())?;
        }
        let registry = registry.prepare().map_err(|error| error.to_string())?;
        return options
            .with_registry(&registry)
            .build(schema)
            .map_err(|error| format!("{} did not compile: {error}", path.display()));
    }
    options
        .build(schema)
        .map_err(|error| format!("{} did not compile: {error}", path.display()))
}

struct Baseline {
    digest: String,
    denominator: usize,
}

struct Catalog {
    index_digest: String,
    baselines: BTreeMap<String, Baseline>,
    rows: BTreeMap<String, CoverageRow>,
}

fn catalog(root: &Path) -> TaskResult<Catalog> {
    let path = root.join("conformance/subsystems/coverage/catalogs/index.json");
    let index = json(&path)?;
    // This owner verifies catalog bytes against the current index and supplies
    // official membership, family and applicability without publication fetching.
    let official = official_catalog_rows(root)?;
    let mut baselines = BTreeMap::new();
    let mut subsystems = BTreeMap::new();
    for value in array(&index, "baselines", &path)? {
        let id = identity(value, "id", &path)?;
        require(
            subsystems
                .insert(text(value, "subsystem", &path)?.to_string(), id.clone())
                .is_none(),
            "duplicate coverage catalog subsystem",
        )?;
        require(
            baselines
                .insert(
                    id,
                    Baseline {
                        digest: text(value, "catalog_sha256", &path)?.to_string(),
                        denominator: usize_value(value, "mandatory_rows", &path)?,
                    },
                )
                .is_none(),
            "duplicate coverage baseline",
        )?;
    }
    let limits = CoverageLimits::default();
    require(
        official.len() <= limits.max_rows,
        "coverage row limit exceeded",
    )?;
    let mut rows = BTreeMap::new();
    for row in official {
        let baseline = subsystems
            .get(row.subsystem())
            .ok_or("official row has no indexed baseline")?;
        let owned = CoverageRow::new(
            baseline,
            row.family(),
            row.row_id().as_str(),
            row.applicable_gates().iter().copied(),
            limits,
        )
        .map_err(|problem| problem.to_string())?;
        require(
            rows.insert(owned.row_id().to_string(), owned).is_none(),
            "duplicate official coverage row",
        )?;
    }
    Ok(Catalog {
        index_digest: format!("sha256:{}", file_digest(&path)?),
        baselines,
        rows,
    })
}

fn identity(value: &Value, key: &str, path: &Path) -> TaskResult<String> {
    let value = text(value, key, path)?;
    require(
        value.len() <= CoverageLimits::default().max_identity_bytes,
        "coverage identity byte limit exceeded",
    )?;
    Ok(value.to_string())
}

fn usize_value(value: &Value, key: &str, path: &Path) -> TaskResult<usize> {
    usize::try_from(u64_value(value, key, path)?).map_err(|error| error.to_string())
}

fn u64_value(value: &Value, key: &str, path: &Path) -> TaskResult<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| format!("{} {key} is not a bounded unsigned integer", path.display()))
}

fn gate(value: &str) -> TaskResult<CoverageGate> {
    CoverageGate::ALL
        .into_iter()
        .find(|gate| gate.slug() == value)
        .ok_or_else(|| format!("unknown coverage gate {value}"))
}

pub(super) fn project_evidence(value: &Value, path: &Path) -> TaskResult<EvidenceRecord> {
    let oracle = match &value["oracle"] {
        Value::Null => None,
        oracle => Some(OracleReceipt {
            environment_identity: identity(oracle, "environment_identity", path)?,
            product_identity: identity(oracle, "product_identity", path)?,
            source_digest: identity(oracle, "source_digest", path)?,
        }),
    };
    let outcome = match text(value, "outcome", path)? {
        "pass" => EvidenceOutcome::Pass,
        "fail" => EvidenceOutcome::Fail,
        other => return Err(format!("invalid evidence outcome {other}")),
    };
    let record = EvidenceRecord::new(
        EvidenceInput {
            row_id: identity(value, "row_id", path)?,
            gate: gate(text(value, "gate", path)?)?,
            sequence: u64_value(value, "sequence", path)?,
            outcome,
            producer: identity(value, "producer", path)?,
            artifact_digest: identity(value, "artifact_digest", path)?,
            source_identity: identity(value, "source_identity", path)?,
            oracle,
        },
        CoverageLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    require(
        record.identity() == text(value, "identity", path)?,
        "coverage evidence content identity mismatch",
    )?;
    Ok(record)
}

pub(super) fn project_row(
    value: &Value,
    evidence: &BTreeMap<String, EvidenceRecord>,
    path: &Path,
) -> TaskResult<CoverageRow> {
    let applicable = array(value, "applicable_gates", path)?
        .iter()
        .map(|value| gate(value.as_str().ok_or("coverage gate is not text")?))
        .collect::<TaskResult<Vec<_>>>()?;
    let mut row = CoverageRow::new(
        identity(value, "baseline_id", path)?,
        identity(value, "unit_id", path)?,
        identity(value, "row_id", path)?,
        applicable,
        CoverageLimits::default(),
    )
    .map_err(|problem| problem.to_string())?;
    let mut retained = 0_usize;
    for (name, history) in object(&value["evidence_refs"], path)? {
        let selected_gate = gate(name)?;
        require(
            row.gate_state(selected_gate) != GateState::NotApplicable,
            "coverage history names a non-applicable gate",
        )?;
        let history = history
            .as_array()
            .ok_or("coverage history is not an array")?;
        retained = retained
            .checked_add(history.len())
            .ok_or("history count overflow")?;
        require(
            retained <= CoverageLimits::default().max_evidence_records,
            "coverage history limit exceeded",
        )?;
        let mut seen = BTreeSet::new();
        for reference in history {
            let reference = reference.as_str().ok_or("evidence reference is not text")?;
            require(
                seen.insert(reference),
                "duplicate coverage history reference",
            )?;
            let record = evidence
                .get(reference)
                .ok_or("unresolved coverage evidence reference")?;
            require(
                record.gate() == selected_gate,
                "coverage evidence gate mismatch",
            )?;
            row.record(record.clone())
                .map_err(|problem| problem.to_string())?;
        }
    }
    let derived = if row.is_complete() {
        "complete"
    } else if CoverageGate::ALL
        .into_iter()
        .any(|gate| row.gate_state(gate) == GateState::Failed)
    {
        "failed"
    } else {
        "pending"
    };
    require(
        text(value, "derived_state", path)? == derived,
        "coverage row derived state mismatch",
    )?;
    Ok(row)
}

pub(super) fn validate_artifacts(root: &Path, paths: &[PathBuf]) -> TaskResult {
    let mut values = Vec::new();
    for path in paths {
        let value = json(path)?;
        if matches!(
            value["schema_version"].as_str(),
            Some(COVERAGE_ROW_CONTRACT | COVERAGE_EVIDENCE_CONTRACT | COVERAGE_LEDGER_CONTRACT)
        ) {
            values.push((path.clone(), value));
        }
    }
    validate_values(root, &values)
}

pub(super) fn validate_values(root: &Path, values: &[(PathBuf, Value)]) -> TaskResult {
    if values.is_empty() {
        return Ok(());
    }
    let limits = CoverageLimits::default();
    let catalog = catalog(root)?;
    let mut evidence = BTreeMap::new();
    let mut ledgers = Vec::new();
    let mut row_count = 0_usize;
    let mut record_count = 0_usize;
    for (path, value) in values {
        let version = text(value, "schema_version", path)?;
        let name = schema_for_0_2_artifact(version).ok_or("unknown coverage wire identity")?;
        let schema = root
            .join("conformance/subsystems/coverage/schemas")
            .join(name);
        validate_schema_instance(&json(&schema)?, value, path)?;
        match version {
            COVERAGE_LEDGER_CONTRACT => {
                require(
                    ledgers.len() < limits.max_generations_per_baseline,
                    "ledger limit exceeded",
                )?;
                ledgers.push((path, value));
                for item in array(value, "evidence_records", path)? {
                    retain_evidence(item, path, &catalog, &mut evidence, &mut record_count)?;
                }
            }
            COVERAGE_EVIDENCE_CONTRACT => {
                retain_evidence(value, path, &catalog, &mut evidence, &mut record_count)?;
            }
            COVERAGE_ROW_CONTRACT => {
                row_count += 1;
                require(row_count <= limits.max_rows, "row artifact limit exceeded")?;
            }
            _ => return Err("not a coverage row, evidence or ledger".into()),
        }
    }
    // Standalone row projections describe the supplied current evidence set.
    // Historical ledger generations below are reconstructed separately, so
    // later observations do not rewrite their earlier derived states.
    let mut current_rows = catalog.rows.clone();
    let mut current_store = CoverageStore::default();
    let mut current_records = evidence.values().collect::<Vec<_>>();
    current_records.sort_by(|left, right| {
        (left.row_id(), left.gate(), left.sequence()).cmp(&(
            right.row_id(),
            right.gate(),
            right.sequence(),
        ))
    });
    for record in current_records {
        current_store
            .append_evidence(record.clone(), limits)
            .map_err(|problem| problem.to_string())?;
        current_rows
            .get_mut(record.row_id())
            .ok_or("unknown official evidence row")?
            .record(record.clone())
            .map_err(|problem| problem.to_string())?;
    }
    let mut selected_baselines = BTreeSet::new();
    let mut declared_rows = BTreeMap::new();
    for (path, value) in values {
        if value["schema_version"] == COVERAGE_ROW_CONTRACT {
            let row = project_row(value, &evidence, path)?;
            let official = catalog
                .rows
                .get(row.row_id())
                .ok_or("unknown official coverage row")?;
            require(
                official.baseline_id() == row.baseline_id()
                    && official.unit_id() == row.unit_id()
                    && CoverageGate::ALL.into_iter().all(|gate| {
                        (official.gate_state(gate) == GateState::NotApplicable)
                            == (row.gate_state(gate) == GateState::NotApplicable)
                    }),
                "coverage row catalog descriptor mismatch",
            )?;
            selected_baselines.insert(row.baseline_id().to_string());
            if let Some(existing) = declared_rows.insert(row.row_id().to_string(), row.clone()) {
                require(
                    existing == row,
                    "conflicting current coverage row projections",
                )?;
            }
            current_rows.insert(row.row_id().to_string(), row);
        }
    }
    for id in selected_baselines {
        let baseline = &catalog.baselines[&id];
        let selected = current_rows
            .values()
            .filter(|row| row.baseline_id() == id)
            .cloned()
            .collect();
        let snapshot = CoverageSnapshot::new(
            id,
            &baseline.digest,
            1,
            baseline.denominator,
            selected,
            limits,
        )
        .map_err(|problem| problem.to_string())?;
        current_store
            .commit(snapshot, limits)
            .map_err(|problem| problem.to_string())?;
    }
    ledgers.sort_by_key(|(_, value)| value["generation"].as_u64());
    let mut store = CoverageStore::default();
    for (path, value) in ledgers {
        validate_ledger(&catalog, value, path, &mut store)?;
    }
    Ok(())
}

fn retain_evidence(
    value: &Value,
    path: &Path,
    catalog: &Catalog,
    evidence: &mut BTreeMap<String, EvidenceRecord>,
    count: &mut usize,
) -> TaskResult {
    *count = count.checked_add(1).ok_or("evidence count overflow")?;
    require(
        *count <= CoverageLimits::default().max_evidence_records,
        "evidence limit exceeded",
    )?;
    let record = project_evidence(value, path)?;
    require(
        catalog.rows.contains_key(record.row_id()),
        "unknown official evidence row",
    )?;
    if let Some(existing) = evidence.insert(record.identity().to_string(), record.clone()) {
        require(existing == record, "conflicting coverage evidence identity")?;
    }
    Ok(())
}

fn validate_ledger(
    catalog: &Catalog,
    value: &Value,
    path: &Path,
    store: &mut CoverageStore,
) -> TaskResult {
    require(
        text(value, "catalog_index_sha256", path)? == catalog.index_digest,
        "coverage ledger catalog index mismatch",
    )?;
    let limits = CoverageLimits::default();
    let mut rows = catalog.rows.clone();
    let mut records = array(value, "evidence_records", path)?
        .iter()
        .map(|item| project_evidence(item, path))
        .collect::<TaskResult<Vec<_>>>()?;
    records.sort_by(|left, right| {
        (left.row_id(), left.gate(), left.sequence()).cmp(&(
            right.row_id(),
            right.gate(),
            right.sequence(),
        ))
    });
    for record in records {
        let row = rows
            .get_mut(record.row_id())
            .ok_or("unknown official evidence row")?;
        store
            .append_evidence(record.clone(), limits)
            .map_err(|problem| problem.to_string())?;
        row.record(record).map_err(|problem| problem.to_string())?;
    }
    let mut seen = BTreeSet::new();
    let mut complete = 0_usize;
    for summary in array(value, "baselines", path)? {
        let id = text(summary, "id", path)?;
        require(seen.insert(id), "duplicate coverage baseline summary")?;
        let baseline = catalog
            .baselines
            .get(id)
            .ok_or("unknown coverage baseline summary")?;
        require(
            text(summary, "catalog_sha256", path)? == baseline.digest
                && usize_value(summary, "mandatory_rows", path)? == baseline.denominator,
            "coverage baseline catalog or denominator mismatch",
        )?;
        let selected = rows
            .values()
            .filter(|row| row.baseline_id() == id)
            .cloned()
            .collect();
        let snapshot = CoverageSnapshot::new(
            id,
            &baseline.digest,
            u64_value(value, "generation", path)?,
            baseline.denominator,
            selected,
            limits,
        )
        .map_err(|problem| problem.to_string())?;
        require(
            usize_value(summary, "complete_rows", path)? == snapshot.complete_rows(),
            "coverage baseline complete-row count mismatch",
        )?;
        complete = complete
            .checked_add(snapshot.complete_rows())
            .ok_or("complete count overflow")?;
        for gate in CoverageGate::ALL {
            let count = snapshot.count(gate);
            let claimed = &summary["gates"][gate.slug()];
            require(
                usize_value(claimed, "numerator", path)? == count.numerator
                    && usize_value(claimed, "denominator", path)? == count.denominator,
                "coverage derived gate count mismatch",
            )?;
        }
        store
            .commit(snapshot, limits)
            .map_err(|problem| problem.to_string())?;
    }
    require(
        seen.len() == catalog.baselines.len(),
        "coverage baseline set mismatch",
    )?;
    require(
        usize_value(value, "official_compatibility_numerator", path)? == complete,
        "coverage official compatibility numerator mismatch",
    )
}
