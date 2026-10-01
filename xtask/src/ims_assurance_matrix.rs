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
    "public-database-engine-route",
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
}
