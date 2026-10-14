//! Own the derived data-runtime fixture digests; expectations remain reviewed inputs.
use super::*;

const SPEC_PATH: &str = "conformance/spec/v1/spec.json";
const FIXTURE_PATH: &str =
    "conformance/subsystems/cobol/execution/cobol/data-runtime-fixtures.json";
const PREFIX: &str = "cobol.data-runtime.";

pub(super) fn run(root: &Path, check: bool) -> TaskResult {
    let fixture_path = root.join(FIXTURE_PATH);
    let fixtures = json(&fixture_path)?;
    validate_schema_instance(
        &json(&root.join("conformance/spec/schemas/cobol-data-runtime-fixtures.schema.json"))?,
        &fixtures,
        &fixture_path,
    )?;
    let spec_path = root.join(SPEC_PATH);
    let mut spec = json(&spec_path)?;
    let digest = format!("sha256:{}", file_digest(&fixture_path)?);
    project(&mut spec, &fixtures, &digest)?;
    let rendered = pretty_json(&spec)?;
    if check {
        require(
            fs::read(&spec_path).map_err(|error| error.to_string())? == rendered,
            "COBOL data runtime fixture bindings are stale; run cargo xtask cobol-data-runtime-bindings",
        )
    } else {
        fs::write(&spec_path, rendered).map_err(|error| error.to_string())
    }
}

fn project(spec: &mut Value, fixtures: &Value, digest: &str) -> TaskResult {
    let cases = fixtures["fixtures"]
        .as_array()
        .ok_or("COBOL data fixtures are missing")?;
    let mut expected = BTreeSet::new();
    let mut rows = BTreeSet::new();
    for case in cases {
        let id = case["id"]
            .as_str()
            .ok_or("COBOL data fixture id is missing")?;
        let row = case["row_id"]
            .as_str()
            .ok_or("COBOL data fixture row is missing")?;
        require(
            expected.insert(format!("{PREFIX}{id}")) && rows.insert(row),
            "COBOL data runtime fixture ids or rows are duplicated",
        )?;
    }
    require(
        expected.len() == 17,
        "COBOL data runtime fixture denominator drifted",
    )?;
    let entries = spec["registries"]["fixtures"]
        .as_array_mut()
        .ok_or("fixture registry is missing")?;
    let mut registered = BTreeSet::new();
    for entry in entries {
        let id = entry["id"]
            .as_str()
            .ok_or("registered fixture id is missing")?;
        if id.starts_with(PREFIX) {
            require(
                expected.contains(id) && registered.insert(id.to_string()),
                "COBOL data runtime fixture registry is duplicated or stale",
            )?;
            entry["digest"] = json!(digest);
        }
    }
    require(
        registered == expected,
        "COBOL data runtime fixture registry is incomplete",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> (Value, Value, String) {
        let fixtures = include_bytes!(
            "../../conformance/subsystems/cobol/execution/cobol/data-runtime-fixtures.json"
        );
        let spec = include_str!("../../conformance/spec/v1/spec.json");
        (
            serde_json::from_str(spec).unwrap(),
            serde_json::from_slice(fixtures).unwrap(),
            format!("sha256:{:x}", Sha256::digest(fixtures)),
        )
    }

    #[test]
    fn projection_refreshes_all_17_digests_without_changing_other_contracts() {
        let (mut spec, fixtures, digest) = inputs();
        let before = spec.clone();
        project(&mut spec, &fixtures, &digest).unwrap();
        let mut count = 0;
        for (new, old) in spec["registries"]["fixtures"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(before["registries"]["fixtures"].as_array().unwrap())
        {
            if new["id"].as_str().unwrap().starts_with(PREFIX) {
                assert_eq!(new["digest"], digest);
                new["digest"] = old["digest"].clone();
                count += 1;
            }
        }
        assert_eq!(count, 17);
        assert_eq!(spec, before);
        project(&mut spec, &fixtures, &digest).unwrap();
        let once = spec.clone();
        project(&mut spec, &fixtures, &digest).unwrap();
        assert_eq!(spec, once);
    }

    #[test]
    fn stale_digest_is_repaired_but_incomplete_or_duplicate_registry_is_rejected() {
        let (spec, fixtures, digest) = inputs();
        let position = spec["registries"]["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .position(|entry| entry["id"] == "cobol.data-runtime.usage")
            .unwrap();
        let mut stale = spec.clone();
        stale["registries"]["fixtures"][position]["digest"] =
            json!(format!("sha256:{}", "a".repeat(64)));
        project(&mut stale, &fixtures, &digest).unwrap();
        assert_eq!(stale["registries"]["fixtures"][position]["digest"], digest);
        let mut missing = spec.clone();
        missing["registries"]["fixtures"]
            .as_array_mut()
            .unwrap()
            .remove(position);
        assert!(project(&mut missing, &fixtures, &digest).is_err());
        let mut duplicate = spec.clone();
        let entry = duplicate["registries"]["fixtures"][position].clone();
        duplicate["registries"]["fixtures"]
            .as_array_mut()
            .unwrap()
            .push(entry);
        assert!(project(&mut duplicate, &fixtures, &digest).is_err());
        let mut unknown = spec;
        unknown["registries"]["fixtures"][position]["id"] = json!("cobol.data-runtime.unknown");
        assert!(project(&mut unknown, &fixtures, &digest).is_err());
    }

    #[test]
    fn changed_denominator_and_duplicate_fixture_rows_are_rejected() {
        let (mut spec, mut fixtures, digest) = inputs();
        fixtures["fixtures"][1]["row_id"] = fixtures["fixtures"][0]["row_id"].clone();
        assert!(project(&mut spec, &fixtures, &digest).is_err());
        fixtures["fixtures"].as_array_mut().unwrap().remove(1);
        assert!(project(&mut spec, &fixtures, &digest).is_err());
    }
}
