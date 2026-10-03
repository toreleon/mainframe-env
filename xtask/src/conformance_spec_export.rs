//! Read-only export of the same validated effective document used by the runner.

use super::*;

fn inputs(root: &Path) -> TaskResult<(String, Vec<OfficialCatalogRow>, Value)> {
    let index_path = root.join("conformance/0.2/catalogs/index.json");
    let catalog_digest = format!("sha256:{}", file_digest(&index_path)?);
    let spec_path = root.join("conformance/spec/v1/spec.json");
    let mut spec_value = json(&spec_path)?;
    augment_ams_spec(root, &mut spec_value)?;
    augment_docs_driven_pilots(root, &mut spec_value)?;
    Ok((catalog_digest, official_catalog_rows(root)?, spec_value))
}

fn compile(
    catalog_digest: &str,
    rows: Vec<OfficialCatalogRow>,
    document: &Value,
) -> TaskResult<CompiledSpec> {
    let bytes = serde_json::to_vec(document).map_err(|error| error.to_string())?;
    CompiledSpec::compile_json(catalog_digest, rows, &bytes, ConformanceLimits::default())
        .map_err(|problem| problem.to_string())
}

pub(super) fn compile_shared_spec(root: &Path) -> TaskResult<CompiledSpec> {
    let (catalog_digest, rows, document) = inputs(root)?;
    compile(&catalog_digest, rows, &document)
}

fn bundle(root: &Path) -> TaskResult<Value> {
    let (catalog_digest, rows, document) = inputs(root)?;
    // Validate before emitting anything; catalog and document share one input read.
    let spec = compile(&catalog_digest, rows.clone(), &document)?;
    let catalog_rows = rows
        .iter()
        .map(|row| {
            json!({
                "row_id": row.row_id().as_str(),
                "subsystem": row.subsystem(),
                "family": row.family(),
                "source_locator": row.source_locator(),
                "applicable_gates": row.applicable_gates().iter().map(|gate| gate.slug()).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema_version": "mainframe-env.conformance-spec-export@1",
        "candidate_digest": candidate_digest(root)?,
        "catalog_digest": catalog_digest,
        "spec_digest": spec.spec_digest(),
        "spec_document": document,
        "catalog_rows": catalog_rows,
        "execution_credit": 0,
        "licensed_credit": 0,
    }))
}

pub(super) fn run(root: &Path) -> TaskResult {
    let value = bundle(root)?;
    println!(
        "{}",
        serde_json::to_string(&value).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mainframe_env_coverage::ScenarioId;

    #[test]
    fn export_roundtrips_the_effective_runner_spec_and_existing_cics_pilot() {
        let root = repository_root().unwrap();
        let value = bundle(&root).unwrap();
        let rows = value["catalog_rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                OfficialCatalogRow::new(
                    row["row_id"].as_str().unwrap(),
                    row["subsystem"].as_str().unwrap(),
                    row["family"].as_str().unwrap(),
                    row["source_locator"].as_str().unwrap(),
                    row["applicable_gates"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|slug| {
                            CoverageGate::ALL
                                .into_iter()
                                .find(|gate| Some(gate.slug()) == slug.as_str())
                                .unwrap()
                        }),
                    ConformanceLimits::default(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let compiled = compile(
            value["catalog_digest"].as_str().unwrap(),
            rows,
            &value["spec_document"],
        )
        .unwrap();
        assert_eq!(
            compiled.spec_digest(),
            value["spec_digest"].as_str().unwrap()
        );
        assert_eq!(
            compiled.spec_digest(),
            compile_shared_spec(&root).unwrap().spec_digest()
        );
        let scenario =
            ScenarioId::new("cics.file-uow.local", ConformanceLimits::default()).unwrap();
        assert!(compiled.scenario(&scenario).is_some());
        let cases = compiled
            .cases()
            .filter(|case| case.scenario() == Some(&scenario))
            .collect::<Vec<_>>();
        assert_eq!(cases.len(), 30);
        assert_eq!(
            cases
                .iter()
                .map(|case| case.key().row_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "ibm-cics-ts-6x-2026-08-31:api-commands:0156",
                "ibm-cics-ts-6x-2026-08-31:api-commands:0181",
                "ibm-cics-ts-6x-2026-08-31:api-commands:0218",
            ])
        );
        assert_eq!(value["execution_credit"], 0);
        assert_eq!(value["licensed_credit"], 0);
    }
}
