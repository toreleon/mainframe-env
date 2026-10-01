use super::*;

const FOUNDATIONS: [&str; 6] = ["host", "database", "tm", "recovery", "metadata", "utility"];
const PROPERTIES: [&str; 8] = [
    "saf",
    "malformed-limit",
    "cas-conflict",
    "replay",
    "reopen-corruption",
    "scale",
    "compatibility-rollback",
    "unknown-outcome",
];
const PENDING: [&str; 4] = [
    "carddemo-corpus-package-route",
    "full-25-family-gates",
    "licensed-ims-15.6-differential",
    "mixed-resource-syncpoint",
];

pub(super) fn check(root: &Path) -> TaskResult {
    let matrix_path = root.join("conformance/0.14/ims/assurance-matrix.json");
    let schema_path = root.join("conformance/0.14/schemas/ims-assurance-matrix.schema.json");
    let matrix = json(&matrix_path)?;
    validate_schema_instance(&json(&schema_path)?, &matrix, &matrix_path)?;

    let catalog = json(&root.join("conformance/0.2/catalogs/ims.json"))?;
    let catalog_rows = catalog["units"]
        .as_array()
        .ok_or("IMS catalog lacks units")?
        .iter()
        .flat_map(|unit| unit["rows"].as_array().into_iter().flatten())
        .filter_map(|row| row["id"].as_str())
        .collect::<BTreeSet<_>>();

    check_handler_closure(root, &matrix, &catalog)?;

    let mut seen_ids = BTreeSet::new();
    let mut seen_bindings = BTreeSet::new();
    let mut foundations = BTreeSet::new();
    let mut properties = BTreeSet::new();
    let mut backends = BTreeSet::new();
    for case in matrix["cases"].as_array().ok_or("IMS matrix lacks cases")? {
        let id = case["id"].as_str().ok_or("IMS matrix case lacks ID")?;
        let foundation = case["foundation"]
            .as_str()
            .ok_or("IMS case lacks foundation")?;
        let property = case["property"].as_str().ok_or("IMS case lacks property")?;
        let backend = case["backend"].as_str().ok_or("IMS case lacks backend")?;
        let path = case["test_file"]
            .as_str()
            .ok_or("IMS case lacks test file")?;
        let name = case["test_name"]
            .as_str()
            .ok_or("IMS case lacks test name")?;
        require(
            seen_ids.insert(id),
            &format!("duplicate IMS assurance case {id}"),
        )?;
        require(
            seen_bindings.insert((foundation, property, path, name)),
            &format!("duplicate IMS assurance binding {id}"),
        )?;
        foundations.insert(foundation);
        properties.insert(property);
        backends.insert(backend);
        check_test_binding(root, path, name)?;

        let scope = case["source_scope"]
            .as_str()
            .ok_or("IMS case lacks source scope")?;
        let topic = case["source_topic"]
            .as_str()
            .ok_or("IMS case lacks source topic")?;
        check_source_binding(root, scope, topic)?;
        for row in case["catalog_rows"]
            .as_array()
            .ok_or("IMS case lacks catalog rows")?
        {
            let row = row.as_str().ok_or("IMS catalog row must be a string")?;
            require(
                catalog_rows.contains(row),
                &format!("IMS assurance case {id} names unknown catalog row {row}"),
            )?;
        }
    }
    require(
        foundations == FOUNDATIONS.into_iter().collect(),
        "IMS assurance matrix omits a foundation",
    )?;
    require(
        properties == PROPERTIES.into_iter().collect(),
        "IMS assurance matrix omits a required property",
    )?;
    require(
        backends.contains("sqlite") && backends.contains("memory"),
        "IMS assurance matrix lacks Memory or SQLite backend evidence",
    )?;
    let pending = matrix["pending"]
        .as_array()
        .ok_or("IMS matrix lacks pending list")?
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect::<BTreeSet<_>>();
    require(
        pending == PENDING.into_iter().collect(),
        "IMS assurance matrix must retain all four explicit pending outcomes",
    )?;
    Ok(())
}

fn check_handler_closure(root: &Path, matrix: &Value, catalog: &Value) -> TaskResult {
    let applicability = json(&root.join("conformance/0.14/ims/call-applicability-rules.json"))?;
    let families = applicability["families"]
        .as_array()
        .ok_or("IMS applicability families are missing")?;
    let rows = catalog["units"][0]["rows"]
        .as_array()
        .ok_or("IMS official rows are missing")?;
    let bindings = matrix["bindings"]
        .as_array()
        .ok_or("IMS handler bindings are missing")?;
    let mut seen = BTreeSet::new();
    for binding in bindings {
        let row_id = binding["row_id"]
            .as_str()
            .ok_or("IMS binding row is missing")?;
        require(
            seen.insert(row_id),
            &format!("duplicate IMS handler binding {row_id}"),
        )?;
        let row = rows
            .iter()
            .find(|row| row["id"] == row_id)
            .ok_or_else(|| format!("stale IMS handler row {row_id}"))?;
        require(
            row["source_locator"] == binding["source_locator"],
            &format!("stale IMS source locator {row_id}"),
        )?;
        let ordinal = row_id
            .rsplit(':')
            .next()
            .unwrap_or("")
            .parse::<u64>()
            .map_err(|_| format!("invalid IMS row ordinal {row_id}"))?;
        let family = families
            .iter()
            .find(|family| family[0].as_u64() == Some(ordinal))
            .ok_or_else(|| format!("missing IMS applicability family {row_id}"))?;
        require(
            binding["applicability_profiles"] == family[1],
            &format!("stale IMS applicability profiles {row_id}"),
        )?;

        let kind = binding["handler"]["kind"]
            .as_str()
            .ok_or("IMS handler kind is missing")?;
        let variants = binding["handler"]["variants"]
            .as_array()
            .ok_or("IMS handler variants are missing")?;
        let source = match kind {
            "ims-operation" => root.join("crates/contracts/mainframe-env-host-api/src/request.rs"),
            "ims-system-call" => {
                root.join("crates/contracts/mainframe-env-host-api/src/ims_system.rs")
            }
            "recovery-method" => {
                root.join("crates/providers/mainframe-env-ims/src/recovery/runtime.rs")
            }
            "applicability-only" => {
                root.join("crates/contracts/mainframe-env-host-api/src/ims_applicability.rs")
            }
            _ => return Err(format!("name-dispatch or unknown IMS handler kind {kind}")),
        };
        let code = fs::read_to_string(&source)
            .map_err(|error| format!("IMS handler source {}: {error}", source.display()))?;
        require(
            !code.contains("CARDDEMO") && !code.contains("PAUTSUM0") && !code.contains("DBPAUTP0"),
            &format!(
                "application-name dispatch in IMS handler source {}",
                source.display()
            ),
        )?;
        for variant in variants {
            let variant = variant
                .as_str()
                .ok_or("IMS handler variant is not a string")?;
            let needle = match kind {
                "ims-operation" => format!("    {variant},"),
                "ims-system-call" => format!("    {variant}"),
                "recovery-method" => format!("pub fn {variant}"),
                _ => format!("pub fn {variant}("),
            };
            require(
                code.contains(&needle),
                &format!("non-executable IMS handler binding {row_id}: {kind}::{variant}"),
            )?;
        }
        require(
            (kind == "applicability-only") == (binding["credit_state"] == "pending-execution"),
            &format!("IMS handler credit state is stale {row_id}"),
        )?;
        let gates = binding["local_gates"]
            .as_array()
            .ok_or("IMS local gates are missing")?;
        let mut gate_ids = BTreeSet::new();
        let mut has_applicability = false;
        let mut has_runtime = false;
        for gate in gates {
            let path = gate["file"].as_str().ok_or("IMS gate file is missing")?;
            let name = gate["test"].as_str().ok_or("IMS gate test is missing")?;
            let gate_kind = gate["gate"].as_str().ok_or("IMS gate kind is missing")?;
            require(
                gate_ids.insert((path, name, gate_kind)),
                &format!("duplicate IMS local gate {row_id}"),
            )?;
            check_test_binding(root, path, name)?;
            has_applicability |= gate_kind == "applicability";
            has_runtime |= gate_kind == "local-regression";
        }
        require(
            has_applicability && (kind == "applicability-only" || has_runtime),
            &format!("non-executable IMS local gate closure {row_id}"),
        )?;
        require(
            binding["coverage_credit"] == 0,
            &format!("IMS binding claims unearned coverage credit {row_id}"),
        )?;
    }
    require(
        seen == rows.iter().filter_map(|row| row["id"].as_str()).collect(),
        "IMS handler closure omits official rows",
    )
}

fn check_test_binding(root: &Path, relative: &str, name: &str) -> TaskResult {
    let path = Path::new(relative);
    require(
        !path.is_absolute()
            && !path.components().any(|part| {
                matches!(
                    part,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            }),
        &format!("unsafe IMS test path {relative}"),
    )?;
    let source = fs::read_to_string(root.join(path))
        .map_err(|error| format!("IMS matrix test {relative}: {error}"))?;
    let target = format!("fn {name}(");
    let lines = source.lines().collect::<Vec<_>>();
    require(
        lines.iter().enumerate().any(|(index, line)| {
            line.trim_start().starts_with(&target)
                && lines[index.saturating_sub(3)..index]
                    .iter()
                    .any(|prior| prior.trim() == "#[test]")
                && !lines[index.saturating_sub(3)..index]
                    .iter()
                    .any(|prior| prior.trim_start().starts_with("#[ignore"))
        }),
        &format!("IMS matrix binding is not an executable #[test]: {relative}::{name}"),
    )
}

fn check_source_binding(root: &Path, scope: &str, topic: &str) -> TaskResult {
    let filename = match scope {
        "ims-programming-contracts" => "ims-programming-contracts-topics.json",
        "ims-database-contracts" => "ims-database-contracts-topics.json",
        "ims-tm-contracts" => "ims-tm-contracts-topics.json",
        "ims-metadata-contracts" => "ims-metadata-contracts-topics.json",
        "ims-recovery-utilities-contracts" => "ims-recovery-utilities-contracts-topics.json",
        _ => return Err(format!("unknown IMS source scope {scope}")),
    };
    let manifest = json(&root.join("conformance/0.14/manifests").join(filename))?;
    require(
        manifest["product"] == "SSEPH2_15.6.0"
            && manifest["coverage_credit"] == 0
            && manifest["topics"]
                .as_array()
                .is_some_and(|topics| topics.iter().any(|pin| pin["topic_path"] == topic)),
        &format!("IMS assurance topic is not pinned in {scope}: {topic}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binding_rejects_missing_or_non_test_function() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        assert!(check_test_binding(root, "xtask/src/ims_assurance_matrix.rs", "check").is_err());
        assert!(check_test_binding(root, "../outside.rs", "anything").is_err());
    }

    #[test]
    fn matrix_keeps_pending_outcomes_and_refuses_credit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        check(root).unwrap();
        let schema_path = root.join("conformance/0.14/schemas/ims-assurance-matrix.schema.json");
        let matrix_path = root.join("conformance/0.14/ims/assurance-matrix.json");
        let mut matrix = json(&matrix_path).unwrap();
        matrix["coverage_credit"] = json!(1);
        assert!(
            validate_schema_instance(&json(&schema_path).unwrap(), &matrix, &matrix_path).is_err()
        );
        assert!(
            check_source_binding(root, "ims-tm-contracts", "SSEPH2_15.6.0/unpinned.htm").is_err()
        );
    }

    #[test]
    fn handler_closure_rejects_missing_duplicate_stale_name_dispatch_and_non_executable() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let matrix = json(&root.join("conformance/0.14/ims/assurance-matrix.json")).unwrap();
        let catalog = json(&root.join("conformance/0.2/catalogs/ims.json")).unwrap();
        check_handler_closure(root, &matrix, &catalog).unwrap();
        let mut changed = matrix.clone();
        changed["bindings"].as_array_mut().unwrap().pop();
        assert!(check_handler_closure(root, &changed, &catalog).is_err());
        let mut changed = matrix.clone();
        let duplicate = changed["bindings"][0].clone();
        changed["bindings"].as_array_mut().unwrap()[1] = duplicate;
        assert!(check_handler_closure(root, &changed, &catalog).is_err());
        let mut changed = matrix.clone();
        changed["bindings"][0]["source_locator"] = json!("stale");
        assert!(check_handler_closure(root, &changed, &catalog).is_err());
        let mut changed = matrix.clone();
        changed["bindings"][0]["handler"]["kind"] = json!("application-name-dispatch");
        assert!(check_handler_closure(root, &changed, &catalog).is_err());
        let mut changed = matrix.clone();
        changed["bindings"][3]["handler"]["variants"] = json!(["MissingHandler"]);
        assert!(check_handler_closure(root, &changed, &catalog).is_err());
        let mut changed = matrix;
        changed["bindings"][3]["local_gates"][1]["test"] = json!("missing_test");
        assert!(check_handler_closure(root, &changed, &catalog).is_err());
    }
}
