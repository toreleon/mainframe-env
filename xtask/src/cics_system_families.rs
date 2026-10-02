//! Source-bound private family contracts, with no runtime or verdict authority.
use crate::{TaskResult, array, json, require, text, validate_schema_instance};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SCHEMA: &str = "conformance/0.10/schemas/cics-system-family-contract.schema.json";
const DIRECTORY: &str = "conformance/0.10/cics/families";
const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024;
const FAMILIES: [&str; 12] = [
    "spi-program",
    "fepi-pool",
    "spi-file",
    "fepi-resources",
    "fepi-pool-list",
    "spi-csd-definition",
    "spi-csd-browse",
    "spi-monitoring-control",
    "spi-region-lifecycle",
    "fepi-session-data",
    "spi-web-resources",
    "spi-queue-storage",
];

fn rows(family: &str) -> TaskResult<(&'static str, &'static [&'static str])> {
    match family {
        "spi-program" => Ok(("spi", &["0026", "0084", "0155", "0241"])),
        "fepi-pool" => Ok(("fepi", &["0001", "0007", "0009", "0018", "0021", "0034"])),
        "spi-file" => Ok(("spi", &["0012", "0072", "0127", "0224"])),
        "fepi-resources" => Ok((
            "fepi",
            &[
                "0008", "0010", "0011", "0017", "0019", "0020", "0022", "0023", "0024", "0032",
                "0033", "0036", "0037",
            ],
        )),
        "fepi-pool-list" => Ok(("fepi", &["0035"])),
        "spi-csd-definition" => Ok((
            "spi",
            &[
                "0037", "0038", "0040", "0041", "0042", "0053", "0054", "0055", "0056", "0060",
                "0061",
            ],
        )),
        "spi-csd-browse" => Ok((
            "spi",
            &[
                "0039", "0043", "0044", "0045", "0046", "0047", "0048", "0049", "0050", "0051",
                "0052", "0057", "0058", "0059",
            ],
        )),
        "spi-monitoring-control" => Ok((
            "spi",
            &[
                "0002", "0095", "0116", "0139", "0148", "0159", "0164", "0173", "0174", "0175",
                "0177", "0195", "0218", "0234", "0238", "0243", "0244", "0253", "0254", "0255",
                "0257",
            ],
        )),
        "spi-region-lifecycle" => Ok((
            "spi",
            &[
                "0004", "0065", "0096", "0100", "0101", "0113", "0157", "0160", "0161", "0163",
                "0165", "0166", "0183", "0184", "0185", "0186", "0199", "0202", "0205", "0208",
                "0215", "0245", "0246", "0261", "0262",
            ],
        )),
        "fepi-session-data" => Ok((
            "fepi",
            &[
                "0002", "0003", "0004", "0005", "0006", "0012", "0013", "0014", "0015", "0016",
                "0025", "0026", "0027", "0028", "0029", "0030", "0031", "0038", "0039",
            ],
        )),
        "spi-web-resources" => Ok((
            "spi",
            &[
                "0003", "0009", "0023", "0035", "0036", "0063", "0070", "0081", "0091", "0092",
                "0097", "0114", "0150", "0187", "0190", "0191", "0193", "0198", "0206", "0216",
                "0239", "0263", "0266", "0267", "0269",
            ],
        )),
        "spi-queue-storage" => Ok((
            "spi",
            &[
                "0011", "0014", "0017", "0029", "0033", "0071", "0074", "0075", "0086", "0090",
                "0107", "0117", "0118", "0132", "0133", "0134", "0162", "0170", "0171", "0179",
                "0180", "0181", "0182", "0219", "0228", "0229", "0250", "0251", "0259", "0260",
            ],
        )),
        _ => Err(format!("unknown CICS system family {family}")),
    }
}

pub(crate) fn check(root: &Path, selected: Option<&str>) -> TaskResult {
    let families = selected.map_or_else(|| FAMILIES.to_vec(), |family| vec![family]);
    let mut commands = 0;
    for family in &families {
        rows(family)?;
        let path = root.join(format!("{DIRECTORY}/{family}.json"));
        require(
            path.is_file(),
            &format!("family contract missing: {}", path.display()),
        )?;
        commands += check_file(root, family, &path)?;
    }
    println!(
        "CICS system contracts: {} families, {commands} commands; runtime/coverage credit 0",
        families.len()
    );
    Ok(())
}

/// The general schema gate checks every present contract without asserting that
/// unimplemented families have completed. The explicit family gate requires its file.
pub(crate) fn check_present(root: &Path) -> TaskResult {
    let directory = root.join(DIRECTORY);
    if !directory.exists() {
        return Ok(());
    }
    for item in fs::read_dir(&directory).map_err(|error| error.to_string())? {
        let path = item.map_err(|error| error.to_string())?.path();
        require(
            path.is_file(),
            "CICS family contract directory contains a non-file",
        )?;
        let family = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("invalid CICS family filename")?;
        require(
            path.extension().is_some_and(|value| value == "json"),
            "unexpected CICS family artifact extension",
        )?;
        rows(family)?;
        check_file(root, family, &path)?;
    }
    Ok(())
}

fn check_file(root: &Path, family: &str, path: &Path) -> TaskResult<usize> {
    let bytes = fs::metadata(path).map_err(|error| error.to_string())?.len();
    require(
        bytes <= MAX_ARTIFACT_BYTES,
        "CICS family artifact exceeds its byte bound",
    )?;
    validate(root, family, &json(path)?, path)
}

fn names<'a>(value: &'a Value, path: &Path) -> TaskResult<Vec<&'a str>> {
    value
        .as_array()
        .ok_or_else(|| format!("{}: expected name list", path.display()))?
        .iter()
        .map(|name| {
            name.as_str()
                .ok_or_else(|| "expected option name".to_string())
        })
        .collect()
}

fn validate(root: &Path, family: &str, artifact: &Value, path: &Path) -> TaskResult<usize> {
    validate_schema_instance(&json(&root.join(SCHEMA))?, artifact, path)?;
    require(
        text(artifact, "family", path)? == family,
        "CICS family filename/identity mismatch",
    )?;
    let (interface, expected) = rows(family)?;
    let map_path = root.join(format!(
        "conformance/0.10/cics/{interface}-command-source-map.json"
    ));
    let map = json(&map_path)?;
    let mappings = array(&map, "rows", &map_path)?;
    let manifest_path = root.join(format!(
        "conformance/0.10/manifests/cics-{interface}-command-topics.json"
    ));
    let manifest = json(&manifest_path)?;
    let baseline = text(&manifest, "baseline_id", &manifest_path)?;
    let topics = array(&manifest, "topics", &manifest_path)?;
    let commands = array(artifact, "commands", path)?;
    require(
        commands.len() == expected.len(),
        "CICS family command count mismatch",
    )?;
    let unit = if interface == "spi" {
        "spi-commands-unique"
    } else {
        "fepi-commands"
    };
    let expected_rows = expected
        .iter()
        .map(|row| format!("ibm-cics-ts-6x-2026-08-31:{unit}:{row}"))
        .collect::<Vec<_>>();
    let actual_rows = commands
        .iter()
        .map(|command| text(command, "official_row", path))
        .collect::<TaskResult<Vec<_>>>()?;
    require(
        actual_rows
            .iter()
            .copied()
            .eq(expected_rows.iter().map(String::as_str)),
        "CICS family row identity/order mismatch",
    )?;
    let mut operations = BTreeSet::new();
    for command in commands {
        let row = text(command, "official_row", path)?;
        let mapping = mappings
            .iter()
            .find(|mapping| mapping["official_row"].as_str() == Some(row))
            .ok_or("CICS family row has no source mapping")?;
        require(
            mapping["state"] == "mapped",
            "CICS family source mapping is unresolved",
        )?;
        require(
            command["label"] == mapping["label"],
            "CICS family label mismatch",
        )?;
        require(
            operations.insert(text(command, "operation_id", path)?),
            "duplicate CICS family operation identity",
        )?;
        let source = &command["source"];
        require(
            source["topic_path"] == mapping["topic"]["topic_path"]
                && source["sha256"] == mapping["topic"]["sha256"]
                && source["baseline"].as_str() == Some(baseline),
            "CICS family source locator/hash/baseline mismatch",
        )?;
        let pinned = topics
            .iter()
            .find(|topic| topic["topic_path"] == source["topic_path"])
            .ok_or("CICS family topic is not pinned")?;
        require(
            source["sha256"].as_str()
                == Some(&format!(
                    "sha256:{}",
                    text(pinned, "sha256", &manifest_path)?
                )),
            "CICS family source pin mismatch",
        )?;
        let grammar = &command["grammar"];
        let known = validate_grammar(grammar, path)?;
        if grammar.get("forms").is_some() {
            let forms = array(grammar, "forms", path)?;
            let ids = forms
                .iter()
                .map(|form| text(form, "id", path))
                .collect::<TaskResult<Vec<_>>>()?;
            require(
                ids.windows(2).all(|pair| pair[0] < pair[1]),
                "CICS family forms must have unique sorted IDs",
            )?;
            for form in forms {
                let form_grammar = &form["grammar"];
                let form_known = validate_grammar(form_grammar, path)?;
                require(
                    form_known.is_subset(&known),
                    "CICS form declares an option outside the parent source union",
                )?;
                let required = names(&form_grammar["required"], path)?;
                let selectors = names(&form["selector_options"], path)?;
                require(
                    selectors.windows(2).all(|pair| pair[0] < pair[1])
                        && selectors
                            .iter()
                            .all(|option| form_known.contains(option) && required.contains(option)),
                    "CICS form selectors must be sorted declared required options",
                )?;
            }
        }
        let obligations = array(command, "obligations", path)?;
        require(
            !obligations.is_empty(),
            "CICS family has no independent obligation candidates",
        )?;
        let mut obligation_ids = BTreeSet::new();
        let mut case_ids = BTreeSet::new();
        for obligation in obligations {
            require(
                obligation_ids.insert(text(obligation, "id", path)?),
                "duplicate CICS family obligation candidate",
            )?;
            require(
                !array(obligation, "gates", path)?.is_empty(),
                "CICS family obligation has no gate candidates",
            )?;
            let cases = array(obligation, "cases", path)?;
            require(
                !cases.is_empty(),
                "CICS family obligation has no independent case candidates",
            )?;
            for case in cases {
                require(
                    case_ids.insert(text(case, "id", path)?),
                    "duplicate CICS family case candidate",
                )?;
            }
        }
    }
    Ok(commands.len())
}

fn validate_grammar<'a>(grammar: &'a Value, path: &Path) -> TaskResult<BTreeSet<&'a str>> {
    let options = array(grammar, "options", path)?;
    let option_names = options
        .iter()
        .map(|option| text(option, "name", path))
        .collect::<TaskResult<Vec<_>>>()?;
    require(
        !option_names.is_empty() && option_names.windows(2).all(|pair| pair[0] < pair[1]),
        "CICS family options must be nonempty, unique and sorted",
    )?;
    let known = option_names.into_iter().collect::<BTreeSet<_>>();
    let references = |list: &Value| -> TaskResult {
        require(
            names(list, path)?.iter().all(|name| known.contains(name)),
            "CICS family constraint names an undeclared option",
        )
    };
    references(&grammar["required"])?;
    for group in array(grammar, "exclusive", path)? {
        require(
            names(group, path)?.len() >= 2,
            "CICS family exclusion group must contain at least two options",
        )?;
        references(group)?;
    }
    if grammar.get("alternative_groups").is_some() {
        let mut groups = BTreeSet::new();
        for group in array(grammar, "alternative_groups", path)? {
            references(&group["members"])?;
            let mut members = names(&group["members"], path)?;
            members.sort_unstable();
            require(
                groups.insert(members),
                "duplicate CICS family alternative group",
            )?;
        }
    }
    let mut dependency_heads = BTreeSet::new();
    for dependency in array(grammar, "dependencies", path)? {
        let head = text(dependency, "option", path)?;
        let targets = names(&dependency["requires"], path)?;
        require(
            known.contains(head)
                && dependency_heads.insert(head)
                && !targets.is_empty()
                && !targets.contains(&head),
            "invalid or duplicate CICS family dependency",
        )?;
        references(&dependency["requires"])?;
    }
    Ok(known)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    // These synthetic facts test contract validation, never IBM behavior.
    fn fixture(family: &str) -> Value {
        let root = root();
        let (interface, ids) = rows(family).unwrap();
        let map = json(&root.join(format!(
            "conformance/0.10/cics/{interface}-command-source-map.json"
        )))
        .unwrap();
        let manifest = json(&root.join(format!(
            "conformance/0.10/manifests/cics-{interface}-command-topics.json"
        )))
        .unwrap();
        let unit = if interface == "spi" {
            "spi-commands-unique"
        } else {
            "fepi-commands"
        };
        let commands = ids.iter().map(|id| {
            let row = format!("ibm-cics-ts-6x-2026-08-31:{unit}:{id}");
            let source = map["rows"].as_array().unwrap().iter().find(|value| value["official_row"] == row).unwrap();
            json!({"official_row":row,"label":source["label"],"operation_id":format!("fixture-{id}"),
                "source":{"topic_path":source["topic"]["topic_path"],"sha256":source["topic"]["sha256"],"baseline":manifest["baseline_id"],"lines":[1]},
                "grammar":{"options":[{"name":"NAME","value_shape":"value","direction":"input","source_max_value_bytes":8,"source_lines":[1]}],"required":["NAME"],"exclusive":[],"dependencies":[]},
                "responses":[],"lifecycle":{"resource_authority":"synthetic validation fixture","preconditions":[],"mutations":[],"implicit_syncpoint":"unresolved","rollback":"unresolved","recovery_obligations":[],"lock_order":[]},
                "authorization":{"intent":"unresolved","resource_class":null,"resource_pattern":null,"audit_obligations":[]},
                "obligations":[{"id":"synthetic.shape","gates":["validated"],"cases":[{"id":"synthetic.case","input":{"NAME":"A"},"expected":"contract-only shape accepted, no semantic claim","source_lines":[1]}]}],"gaps":["Synthetic fixture has no semantic authority"]})
        }).collect::<Vec<_>>();
        json!({"schema_version":"mainframe-env.cics-system-family@1","target_version":"0.10.0","family":family,"runtime_binding":"private-unregistered","commands":commands})
    }

    fn valid(family: &str, value: &Value) -> TaskResult<usize> {
        validate(&root(), family, value, Path::new("synthetic-family.json"))
    }

    #[test]
    fn source_cohorts_preserve_distinct_rows_and_bounded_artifacts() {
        let mut identities = BTreeSet::new();
        for family in FAMILIES {
            let (interface, ids) = rows(family).unwrap();
            assert!(!ids.is_empty() && ids.len() <= 32);
            for id in ids {
                assert!(
                    identities.insert((interface, *id)),
                    "cohort duplicates {interface}:{id}"
                );
            }
        }
        assert_eq!(identities.len(), 173);
        assert_eq!(
            identities
                .iter()
                .filter(|(interface, _)| *interface == "spi")
                .count(),
            134
        );
        assert_eq!(
            identities
                .iter()
                .filter(|(interface, _)| *interface == "fepi")
                .count(),
            39
        );
    }

    #[test]
    fn declared_cohorts_validate_without_semantic_claims() {
        for (family, count) in [
            ("spi-program", 4),
            ("fepi-pool", 6),
            ("spi-file", 4),
            ("fepi-resources", 13),
            ("fepi-pool-list", 1),
            ("spi-csd-definition", 11),
            ("spi-csd-browse", 14),
            ("spi-monitoring-control", 21),
            ("spi-region-lifecycle", 25),
            ("fepi-session-data", 19),
            ("spi-web-resources", 25),
            ("spi-queue-storage", 30),
        ] {
            assert_eq!(valid(family, &fixture(family)).unwrap(), count);
        }
    }

    #[test]
    fn foreign_missing_duplicate_and_reordered_rows_fail() {
        for mode in 0..5 {
            let mut value = fixture("spi-program");
            let commands = value["commands"].as_array_mut().unwrap();
            match mode {
                0 => {
                    commands.pop();
                }
                1 => {
                    commands[1] = commands[0].clone();
                }
                2 => commands.swap(0, 1),
                3 => commands[0]["official_row"] = json!("foreign:0001"),
                _ => {
                    commands.push(commands[0].clone());
                }
            }
            assert!(valid("spi-program", &value).is_err(), "mode {mode}");
        }
    }

    #[test]
    fn changed_source_locator_hash_baseline_and_label_fail() {
        for field in ["topic_path", "sha256", "baseline", "label"] {
            let mut value = fixture("spi-program");
            if field == "label" {
                value["commands"][0][field] = json!("FOREIGN");
            } else {
                value["commands"][0]["source"][field] = json!(if field == "sha256" {
                    format!("sha256:{}", "0".repeat(64))
                } else {
                    "foreign".into()
                });
            }
            assert!(valid("spi-program", &value).is_err(), "field {field}");
        }
    }

    #[test]
    fn undeclared_constraint_heads_targets_and_self_dependency_fail() {
        for mode in 0..5 {
            let mut value = fixture("spi-program");
            let grammar = &mut value["commands"][0]["grammar"];
            match mode {
                0 => grammar["required"] = json!(["FOREIGN"]),
                1 => grammar["exclusive"] = json!([["NAME", "FOREIGN"]]),
                2 => grammar["dependencies"] = json!([{"option":"FOREIGN","requires":["NAME"]}]),
                3 => grammar["dependencies"] = json!([{"option":"NAME","requires":["FOREIGN"]}]),
                _ => grammar["dependencies"] = json!([{"option":"NAME","requires":["NAME"]}]),
            }
            assert!(valid("spi-program", &value).is_err(), "mode {mode}");
        }
    }

    #[test]
    fn duplicate_operations_options_and_cases_fail() {
        for mode in 0..3 {
            let mut value = fixture("spi-program");
            match mode {
                0 => {
                    value["commands"][1]["operation_id"] =
                        value["commands"][0]["operation_id"].clone()
                }
                1 => {
                    let option = value["commands"][0]["grammar"]["options"][0].clone();
                    value["commands"][0]["grammar"]["options"]
                        .as_array_mut()
                        .unwrap()
                        .push(option);
                }
                _ => {
                    let case = value["commands"][0]["obligations"][0]["cases"][0].clone();
                    value["commands"][0]["obligations"][0]["cases"]
                        .as_array_mut()
                        .unwrap()
                        .push(case);
                }
            }
            assert!(valid("spi-program", &value).is_err(), "mode {mode}");
        }
    }

    #[test]
    fn public_runtime_claims_and_unexpected_fields_fail_schema() {
        for mode in 0..3 {
            let mut value = fixture("spi-program");
            match mode {
                0 => value["runtime_binding"] = json!("advertised"),
                1 => value["coverage_credit"] = json!(4),
                _ => value["commands"][0]["grammar"]["generic_success"] = json!(true),
            }
            assert!(valid("spi-program", &value).is_err(), "mode {mode}");
        }
    }

    #[test]
    fn empty_case_sets_and_wrong_family_fail() {
        let mut value = fixture("spi-program");
        value["commands"][0]["obligations"][0]["cases"] = json!([]);
        assert!(valid("spi-program", &value).is_err());
        assert!(valid("fepi-pool", &fixture("spi-program")).is_err());
        assert!(rows("foreign").is_err());
    }

    #[test]
    fn byte_bound_and_missing_family_fail_without_credit() {
        let root = root();
        let directory = std::env::temp_dir().join(format!(
            "cics-family-bound-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("oversized.json");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_ARTIFACT_BYTES + 1).unwrap();
        assert!(check_file(&root, "spi-program", &path).is_err());
        assert!(check(&directory, Some("spi-program")).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    fn form_fixture() -> Value {
        let mut value = fixture("spi-program");
        let grammar = value["commands"][0]["grammar"].clone();
        value["commands"][0]["grammar"]["forms"] = json!([{"id":"named", "selector_options":["NAME"], "grammar":grammar, "source_lines":[1]}]);
        value
    }

    #[test]
    fn bounded_forms_reuse_common_constraints_and_keep_legacy_inputs() {
        assert_eq!(valid("spi-program", &fixture("spi-program")).unwrap(), 4);
        let mut value = form_fixture();
        assert_eq!(valid("spi-program", &value).unwrap(), 4);
        value["commands"][0]["grammar"]["forms"][0]["grammar"]["options"][0]["direction"] =
            json!("output");
        assert_eq!(valid("spi-program", &value).unwrap(), 4);
    }

    #[test]
    fn malformed_or_admitting_forms_fail_closed() {
        for mutation in 0..14 {
            let mut value = form_fixture();
            let forms = &mut value["commands"][0]["grammar"]["forms"];
            match mutation {
                0 => {
                    let copy = forms[0].clone();
                    forms.as_array_mut().unwrap().push(copy);
                }
                1 => forms[0]["selector_options"] = json!(["OTHER"]),
                2 => forms[0]["grammar"]["required"] = json!([]),
                3 => forms[0]["grammar"]["options"][0]["name"] = json!("OTHER"),
                4 => {
                    forms[0]["grammar"]["dependencies"] =
                        json!([{"option":"NAME", "requires":["MISSING"]}])
                }
                5 => {
                    let copy = forms[0]["grammar"]["options"][0].clone();
                    forms[0]["grammar"]["options"]
                        .as_array_mut()
                        .unwrap()
                        .push(copy);
                }
                6 => forms[0]["grammar"]["forms"] = json!([]),
                7 => forms[0]["constraint_status"] = json!("resolved"),
                8 => {
                    let copy = forms[0].clone();
                    *forms = json!(
                        (0..17)
                            .map(|i| {
                                let mut form = copy.clone();
                                form["id"] = json!(format!("named-{i:02}"));
                                form
                            })
                            .collect::<Vec<_>>()
                    );
                }
                9 => forms[0]["source_lines"] = json!([]),
                10 => {
                    let mut copy = forms[0].clone();
                    forms[0]["id"] = json!("z");
                    copy["id"] = json!("a");
                    forms.as_array_mut().unwrap().push(copy);
                }
                11 => forms[0]["selector_options"] = json!(["NAME", "NAME"]),
                12 => forms[0]["selector_options"] = json!([1]),
                13 => forms[0]["grammar"]["options"] = json!([]),
                _ => unreachable!(),
            }
            assert!(
                valid("spi-program", &value).is_err(),
                "form mutation {mutation}"
            );
        }
    }

    fn alternative_fixture() -> Value {
        let mut value = fixture("spi-program");
        let grammar = &mut value["commands"][0]["grammar"];
        let mut other = grammar["options"][0].clone();
        other["name"] = json!("OTHER");
        grammar["options"].as_array_mut().unwrap().push(other);
        grammar["required"] = json!([]);
        grammar["alternative_groups"] = json!([{"members":["NAME","OTHER"],"required":true}]);
        value
    }

    #[test]
    fn required_optional_and_absent_alternatives_validate_without_promotion() {
        let mut value = alternative_fixture();
        assert_eq!(valid("spi-program", &value).unwrap(), 4);
        value["commands"][0]["grammar"]["alternative_groups"][0]["required"] = json!(false);
        assert_eq!(valid("spi-program", &value).unwrap(), 4);
        assert_eq!(valid("spi-program", &fixture("spi-program")).unwrap(), 4);
    }

    #[test]
    fn dangling_small_duplicate_or_nonboolean_alternatives_fail() {
        for mode in 0..6 {
            let mut value = alternative_fixture();
            let groups = &mut value["commands"][0]["grammar"]["alternative_groups"];
            match mode {
                0 => groups[0]["members"] = json!(["NAME", "FOREIGN"]),
                1 => groups[0]["members"] = json!(["NAME"]),
                2 => groups[0]["members"] = json!(["NAME", "NAME"]),
                3 => groups[0]["required"] = json!("true"),
                4 => groups
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"members":["OTHER","NAME"],"required":true})),
                _ => groups
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"members":["NAME","OTHER"],"required":false})),
            }
            assert!(valid("spi-program", &value).is_err(), "mode {mode}");
        }
    }
}
